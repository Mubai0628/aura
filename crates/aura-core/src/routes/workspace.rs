//! workspace route adapter; domain services retain authorization and transaction ownership.
use crate::*;

/// The typed envelope for a session command the synchronous boundary did not complete.
///
/// A refusal and an unknown answer are deliberately different codes: a refusal proves the
/// write did not run, an unknown answer proves only that it did not answer.
pub(super) fn session_envelope(
    code: &str,
    category: ErrorCategory,
    message: String,
    retryable: bool,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static code"),
        category,
        message,
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

/// The answer for a session write that started and did not finish inside its budget.
///
/// IT IS NOT A FAILURE WITH A DIFFERENT WORD. It carries the contract's own recovery field and
/// the original invocation, so Main's confirmation entry and the Renderer's existing
/// pending-confirmation protection recognise it as "query `runtime.command_outcome`, never
/// resend" while `terminal = failed` stays the request's terminal. It is built by the module
/// that owns the late result, so the envelope and the settlement cannot disagree.
pub(super) fn session_unknown_envelope(
    command_invocation_id: CommandInvocationId,
    correlation_id: CorrelationId,
    waited_ms: u64,
) -> IpcErrorEnvelope {
    crate::command_settlement::outcome_unknown_envelope(
        correlation_id,
        Some(command_invocation_id),
        format!(
            "the session write did not answer inside its budget after {waited_ms} ms; its outcome \
             is unknown and it must not be retried automatically"
        ),
    )
}

/// How a session write that failed is classified for its caller.
///
/// ONE FAILURE IS NOT A FAILURE. `PersistenceError::WriterUnresponsive` is the persistence layer's
/// own structured answer that the write was **admitted** and the writer had not answered inside
/// its budget: the job may still run and may already have committed, and there is no value here to
/// publish for it. Reporting that through the ordinary `broker.dispatch_unavailable` would state,
/// as a definite failure, something that has not been established — so it is answered with the
/// same typed Unknown the rest of this path uses (query the invocation, never resend).
///
/// The decision is made from the error's **type** and from the stage that produced it — this is
/// the business-write step — never from its message. Every other failure keeps the classification
/// it already had, because a definite `WorkspaceNotFound` or `WriterBusy` is a real answer.
pub(super) fn session_write_failure(
    correlation_id: CorrelationId,
    command_invocation_id: CommandInvocationId,
    error: &anyhow::Error,
) -> IpcErrorEnvelope {
    if crate::broker::completion::writer_may_still_answer(error) {
        crate::command_settlement::outcome_unknown_envelope(
            correlation_id,
            Some(command_invocation_id),
            format!(
                "the session write was admitted and the storage writer had not answered: {error}; \
                 its outcome is unknown and it must not be retried automatically"
            ),
        )
    } else if matches!(
        error.downcast_ref::<aura_persistence::PersistenceError>(),
        Some(aura_persistence::PersistenceError::WriterBusy { .. })
    ) {
        session_envelope(
            "persistence.busy",
            ErrorCategory::Unavailable,
            error.to_string(),
            true,
            correlation_id,
        )
    } else {
        broker::error_envelope(correlation_id, error)
    }
}

pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    match request.method.as_str() {
        "session.create" | "session.rename" => {
            // The session write is a real storage write with a real path hash, and it is
            // reached from an async route. It runs on the bounded synchronous boundary, so a
            // slow or unreachable store occupies a boundary worker instead of the reactor
            // that also has to deliver the heartbeat and answer every other connection. The
            // authorization, the command claim and the route decision all happened before
            // this point and are unchanged.
            let outcome: Result<serde_json::Value, IpcErrorEnvelope> = match &state.startup_mode {
                StartupMode::Ready(services) => match request.command_invocation_id {
                    Some(command) => {
                        super::session_write::execute(state, services, request, command).await
                    }
                    None => Err(broker::error_envelope(
                        request.id,
                        anyhow::anyhow!("ipc.command_invocation_id_required"),
                    )),
                },
                StartupMode::RecoveryRequired { .. } => Err(broker::error_envelope(
                    request.id,
                    anyhow::anyhow!("core.recovery_required"),
                )),
            };
            let (terminal, response) = match outcome {
                Ok(value) => {
                    (IpcRequestTerminal::Succeeded, JsonRpcResponse::success(request.id, value))
                }
                Err(error) => {
                    (IpcRequestTerminal::Failed, JsonRpcResponse::failure(request.id, error))
                }
            };
            send_recorded_response(state, queue, request, response).await?;
            Ok(terminal)
        }
        // Archive, restore and delete a conversation.
        method_name if crate::session_lifecycle::METHODS.contains(&method_name) => {
            let outcome = match &state.startup_mode {
                StartupMode::Ready(services) => {
                    crate::session_lifecycle::dispatch(services, request).await
                }
                StartupMode::RecoveryRequired { .. } => {
                    Err(anyhow::anyhow!("core.recovery_required"))
                }
            };
            let (terminal, response) = match outcome {
                Ok(value) => {
                    (IpcRequestTerminal::Succeeded, JsonRpcResponse::success(request.id, value))
                }
                Err(error) => (
                    IpcRequestTerminal::Failed,
                    JsonRpcResponse::failure(request.id, broker::error_envelope(request.id, error)),
                ),
            };
            send_recorded_response(state, queue, request, response).await?;
            Ok(terminal)
        }
        method_name if P03Services::handles_main_command(method_name) => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                let error = IpcErrorEnvelope {
                    code: ErrorCode::new("core.recovery_required")?,
                    category: ErrorCategory::Unavailable,
                    message: "Aura 的数据需要恢复，工作区操作暂不可用。".to_owned(),
                    retryable: false,
                    correlation_id: request.id,
                    terminal: IpcRequestTerminal::Failed,
                    details: BTreeMap::new(),
                };
                send_recorded_response(
                    state,
                    queue,
                    request,
                    JsonRpcResponse::failure(request.id, error),
                )
                .await?;
                return Ok(IpcRequestTerminal::Failed);
            };
            if P04Services::gates_authenticated_main_workspace_mutation(method_name)
                && let Err(error) = services.p04()?.authorize_authenticated_main_workspace_mutation(
                    method_name,
                    &request.params,
                    request.id,
                )
            {
                send_recorded_response(
                    state,
                    queue,
                    request,
                    JsonRpcResponse::failure(request.id, error),
                )
                .await?;
                return Ok(IpcRequestTerminal::Failed);
            }
            let result = if method_name.starts_with("fs.") || method_name == method::BUFFER_SAVE {
                broker::dispatch_files(state, services, request).await
            } else {
                // Workspace/editor lifecycle stays in its owner; file execution
                // has one production entry through Core Broker from R05 onward.
                //
                // The call itself is synchronous - a catalog walk and a file read do
                // not become asynchronous by being awaited - so it runs on the bounded
                // blocking pool rather than on this reactor thread. The handler, its
                // authorization and its route are unchanged; only where the work is
                // allowed to block is.
                // The three outcomes are classified in one place (`P03Dispatch`), so the
                // file route and this route cannot disagree about what a refusal or an
                // unknown answer means.
                match services.p03() {
                    Ok(p03) => p03
                        .dispatch_main_command_bounded(
                            method_name,
                            &request.params,
                            request.id,
                            std::time::Instant::now() + core::time::Duration::from_secs(120),
                        )
                        .await
                        .into_route_result(request.id, request.command_invocation_id),
                    // This route's workspace/editor capability is gone, so
                    // it answers with a stable `Unavailable` envelope rather than
                    // panicking or fabricating a result.
                    Err(_) => Err(crate::MissingCapability::P03WorkspaceFiles.envelope(request.id)),
                }
            };
            match result {
                Ok(value) => {
                    let restored_workspace_id = match method_name {
                        method::WORKSPACE_RESTORE => {
                            serde_json::from_value::<WorkspaceSummary>(value.clone())
                                .ok()
                                .map(|workspace| workspace.workspace_id)
                        }
                        method::WORKSPACE_CREATE => {
                            serde_json::from_value::<WorkspaceCreateResult>(value.clone())
                                .ok()
                                .map(|result| result.workspace.workspace_id)
                        }
                        _ => None,
                    };
                    if let Some(workspace_id) = restored_workspace_id {
                        services.p04()?.on_workspace_restored(workspace_id, request.id);
                    }
                    // A removed Workspace's execution-store rows and memory files go next.
                    if crate::session_lifecycle::removed_workspace(method_name, &value) {
                        crate::session_lifecycle::Purger::of(services).spawn_resume();
                    }
                    send_recorded_response(
                        state,
                        queue,
                        request,
                        JsonRpcResponse::success(request.id, value),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Succeeded)
                }
                Err(error) => {
                    send_recorded_response(
                        state,
                        queue,
                        request,
                        JsonRpcResponse::failure(request.id, error),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Failed)
                }
            }
        }

        _ => bail!("registered workspace route has no handler"),
    }
}
