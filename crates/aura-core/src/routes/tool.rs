//! tool route adapter; domain services retain authorization and transaction ownership.
use crate::*;
use aura_domain_common::{SessionId, WorkspaceId};

/// Commands whose committed effect changes what the Registry contains: what is
/// enabled, configured or pinned. After one of them, the scopes that are still in
/// use get a fresh head so the next ModelStep plans against the new facts.
fn registry_affecting(method: &str) -> bool {
    matches!(
        method,
        "tools.tool.set_enabled"
            | "tools.instance.update_configuration"
            | "tools.instance.bind_credential"
            | "tools.instance.clear_credential"
            | "tools.pin.workspace.set"
            | "tools.pin.workspace.clear"
            | "tools.pin.session.set"
            | "tools.pin.session.clear"
    )
}

/// Which scopes a registry-affecting command can still affect.
///
/// Install/trust/enable/configuration changes the facts every scope layers on top
/// of, so every current scope is reached. A pin is narrower: a Workspace pin belongs
/// to the Workspace authority its Sessions inherit, a Session pin only to that
/// Session (and the Child Agents below it). Reading the scope out of the request is
/// what keeps an unrelated active Session's head untouched.
fn registry_change_impact(
    method: &str,
    params: &serde_json::Value,
) -> Result<p07_registry::ToolRegistryChangeImpactV1, String> {
    use p07_registry::ToolRegistryChangeImpactV1;
    match method {
        "tools.pin.workspace.set" | "tools.pin.workspace.clear" => {
            serde_json::from_value::<WorkspaceId>(params["workspace_id"].clone())
                .map(ToolRegistryChangeImpactV1::Workspace)
                .map_err(|error| format!("workspace_id: {error}"))
        }
        "tools.pin.session.set" | "tools.pin.session.clear" => {
            serde_json::from_value::<SessionId>(params["session_id"].clone())
                .map(ToolRegistryChangeImpactV1::Session)
                .map_err(|error| format!("session_id: {error}"))
        }
        _ => Ok(ToolRegistryChangeImpactV1::EveryCurrentScope),
    }
}

/// Rebuilds the Registry head of the still-current scopes a committed change
/// affects.
///
/// The command's own response is already sent, so this never rewrites a committed
/// outcome; a failure is reported as a Core event with the affected scopes and the
/// stage that failed, so a stale head cannot pass as a successful refresh, and the
/// next change-driven refresh retries the same scopes. A request whose scope cannot
/// be read is reported and refreshes nothing: falling back to "every scope" would
/// silently widen a narrow, authorized change.
/// The bounded, path-free name of a registry scope kind.
///
/// A log line or an error message must not carry a scope's raw identity
/// (workspace/session ids). The *kind* is what a reader needs to know which class
/// of scope failed, and it is stable across runs.
fn scope_kind(scope: &aura_tool_contract::ToolRegistryScopeV1) -> &'static str {
    match scope {
        aura_tool_contract::ToolRegistryScopeV1::Workspace { .. } => "workspace",
        aura_tool_contract::ToolRegistryScopeV1::Session { .. } => "session",
        aura_tool_contract::ToolRegistryScopeV1::ChildAgent { .. } => "child_agent",
    }
}

/// Which stage of a registry refresh failed, and why.
#[derive(Debug)]
struct RegistryRefreshFailure {
    /// The bounded stage name: `impact`, `settings`, `enumerate` or `scopes`.
    stage: &'static str,
    /// The stable code the caller reports and the user can act on.
    code: &'static str,
    /// The real reason, for the log only — never for the response.
    reason: String,
}

impl RegistryRefreshFailure {
    /// The refusal as an IPC terminal envelope.
    ///
    /// The code is this failure's own stable code and the category is
    /// `Unavailable`, so a caller can tell "the switch is committed but not yet in
    /// force, retry" apart from a generic internal error. The `reason` is
    /// deliberately **not** placed in the message: it is the log's, and a response
    /// must not carry a scope's raw identity.
    fn envelope(&self, correlation_id: CorrelationId) -> IpcErrorEnvelope {
        IpcErrorEnvelope {
            code: ErrorCode::new(self.code).expect("static registry refresh code is valid"),
            category: ErrorCategory::Unavailable,
            message: format!(
                "The tool configuration change is committed but its registry authority could not be \
                 refreshed ({}, stage {}). Calls planned against the previous configuration are not \
                 guaranteed to be refused until the refresh succeeds; retry the operation.",
                self.code, self.stage
            ),
            retryable: true,
            correlation_id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        }
    }

    /// The real reason, for the log.
    ///
    /// Kept separate from [`Self::envelope`] on purpose: the response carries a
    /// bounded code, and the raw reason — which may name a scope — stays in the
    /// log where it already lives.
    fn log_reason(&self) -> &str {
        &self.reason
    }
}

/// Refreshes the scope heads a committed change affects, reporting whether the
/// change is really in force.
///
/// Returns `None` when the refresh succeeded or when the method affects no
/// registry scope. Returns `Some(failure)` when the authority other calls are
/// checked against could not be brought up to the committed change — the caller
/// must not report success in that case.
///
/// A request whose scope cannot be read is reported and refreshes nothing:
/// falling back to "every scope" would silently widen a narrow, authorized
/// change.
fn refresh_registry_after_change(
    services: &RuntimeServices,
    method: &str,
    params: &serde_json::Value,
    correlation_id: CorrelationId,
) -> Option<RegistryRefreshFailure> {
    let impact = match registry_change_impact(method, params) {
        Ok(impact) => impact,
        Err(reason) => {
            append_core_log(
                "tool_registry_refresh_failed",
                json!({ "stage": "impact", "method": method, "reason": reason }),
            );
            return Some(RegistryRefreshFailure {
                stage: "impact",
                code: "tool.registry.scope_unresolved",
                reason,
            });
        }
    };
    let settings = match services._settings.tool_settings_snapshot() {
        Ok(settings) => settings,
        Err(error) => {
            append_core_log(
                "tool_registry_refresh_failed",
                json!({ "stage": "settings", "reason": error.to_string() }),
            );
            return Some(RegistryRefreshFailure {
                stage: "settings",
                code: config_degradation::config_fault_code(&error),
                reason: error.to_string(),
            });
        }
    };
    match p07_registry::ToolRegistryControlServiceV1.refresh_current_scopes(
        &services.persistence,
        settings,
        impact,
        correlation_id,
    ) {
        Ok(report) if report.is_clean() => {
            append_core_log(
                "tool_registry_refreshed",
                json!({
                    "method": method,
                    "refreshed": report.refreshed.len(),
                    "untouched": report.untouched,
                    "skipped_unaffected": report.skipped_unaffected,
                }),
            );
            None
        }
        Ok(report) => {
            // The scope is a typed identity, not a Display string; it is named by
            // its kind so the reason stays bounded and path-free.
            let reason = report
                .failures
                .iter()
                .map(|failure| format!("{}:{}", scope_kind(&failure.scope), failure.stage))
                .collect::<Vec<_>>()
                .join(",");
            append_core_log(
                "tool_registry_refresh_failed",
                json!({
                    "method": method,
                    "refreshed": report.refreshed.len(),
                    "untouched": report.untouched,
                    "skipped_unaffected": report.skipped_unaffected,
                    "failures": report
                        .failures
                        .iter()
                        .map(|failure| json!({
                            "scope": failure.scope,
                            "stage": failure.stage,
                            "reason": failure.reason,
                        }))
                        .collect::<Vec<_>>(),
                }),
            );
            Some(RegistryRefreshFailure {
                stage: "scopes",
                code: "tool.registry.refresh_incomplete",
                reason,
            })
        }
        Err(error) => {
            append_core_log(
                "tool_registry_refresh_failed",
                json!({ "stage": "enumerate", "reason": error.to_string() }),
            );
            Some(RegistryRefreshFailure {
                stage: "enumerate",
                code: "tool.registry.enumerate_failed",
                reason: error.to_string(),
            })
        }
    }
}

pub(crate) async fn dispatch(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
) -> Result<IpcRequestTerminal> {
    match request.method.as_str() {
        "tools.credential.list"
        | "tools.credential.origins"
        | "tools.credential.outcome"
        | "tools.credential.clear"
        | "tools.credential.recover" => {
            let result =
                crate::tool_credential_service::commands::dispatch(state, request, None).await;
            let (response, terminal) = match result {
                Ok(value) => {
                    (JsonRpcResponse::success(request.id, value), IpcRequestTerminal::Succeeded)
                }
                Err(error) => {
                    (JsonRpcResponse::failure(request.id, error), IpcRequestTerminal::Failed)
                }
            };
            send_recorded_response(state, queue, request, response).await?;
            Ok(terminal)
        }

        "broker.tools" => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                return send_projection_unavailable(queue, request.id).await;
            };
            let (response, terminal) = match broker::dispatch_tools(state, services, request).await
            {
                Ok(value) => {
                    (JsonRpcResponse::success(request.id, value), IpcRequestTerminal::Succeeded)
                }
                Err(error) => {
                    (JsonRpcResponse::failure(request.id, error), IpcRequestTerminal::Failed)
                }
            };
            send_recorded_response(state, queue, request, response).await?;
            Ok(terminal)
        }
        method::TOOL_ACTIVITY_PAGE => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                return send_projection_unavailable(queue, request.id).await;
            };
            match p07_control::ToolReadControlServiceV1.activity_page(
                &services.persistence,
                &request.params,
                request.id,
            ) {
                Ok(page) => {
                    send_success(queue, request.id, serde_json::to_value(page)?).await?;
                    Ok(IpcRequestTerminal::Succeeded)
                }
                Err(error) => {
                    send_json(
                        queue,
                        QueuePriority::P0,
                        &JsonRpcResponse::failure(request.id, error),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Failed)
                }
            }
        }
        method::TOOL_CREDENTIAL_CANDIDATES => {
            let StartupMode::Ready(services) = &state.startup_mode else {
                return send_projection_unavailable(queue, request.id).await;
            };
            let authenticated_owner = services
                .persistence
                .current_tool_principal()
                .context("resolve authenticated Tool profile principal")?;
            match p07_control::ToolReadControlServiceV1.credential_candidates(
                &services.persistence,
                &request.params,
                authenticated_owner,
                request.id,
            ) {
                Ok(projection) => {
                    send_success(queue, request.id, serde_json::to_value(projection)?).await?;
                    Ok(IpcRequestTerminal::Succeeded)
                }
                Err(error) => {
                    send_json(
                        queue,
                        QueuePriority::P0,
                        &JsonRpcResponse::failure(request.id, error),
                    )
                    .await?;
                    Ok(IpcRequestTerminal::Failed)
                }
            }
        }
        method_name
            if p07_commands::ToolMutationControlServiceV1::handles_main_command(method_name) =>
        {
            let StartupMode::Ready(services) = &state.startup_mode else {
                let error = IpcErrorEnvelope {
                    code: ErrorCode::new("core.recovery_required")?,
                    category: ErrorCategory::Unavailable,
                    message: "Aura 的数据需要恢复，工具设置暂不可用。".to_owned(),
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
            let command_invocation_id = request
                .command_invocation_id
                .context("authenticated Tool mutation has no CommandInvocationId")?;
            let authenticated_channel = authenticated_configuration_channel_identity(state)?;
            let authenticated_identity_hash =
                match services.p04()?.authorize_authenticated_main_tool_mutation(
                    method_name,
                    &request.params,
                    command_invocation_id,
                    request.id,
                    authenticated_channel,
                ) {
                    Ok(identity_hash) => identity_hash,
                    Err(error) => {
                        send_recorded_response(
                            state,
                            queue,
                            request,
                            JsonRpcResponse::failure(request.id, error),
                        )
                        .await?;
                        return Ok(IpcRequestTerminal::Failed);
                    }
                };
            let handler_persistence = services.persistence.clone();
            let handler_request = request.clone();
            let handler_method = method_name.to_owned();
            // R1: THIS IS NOT A FILE PUBLISH POINT ANY MORE.
            //
            // An earlier version published a `MainFs` activity from right here. This branch is
            // `ToolMutationControlServiceV1`, i.e. tool settings - a *configuration* operation. It does not write an ordinary workspace file, so
            // reporting it as a Main file write was a false fact, and the ordinary `fs.*`
            // requests that really do write files never reached this line at all.
            //
            // The real Main file activity is published from `broker::dispatch_files`, which is
            // the actual execution boundary for ordinary `fs.*` and Buffer save. Nothing is
            // published here, and no row claims coverage of this entry.
            let call = move || async move {
                p07_commands::ToolMutationControlServiceV1.dispatch_main_command(
                    &handler_persistence,
                    &handler_method,
                    &handler_request.params,
                    command_invocation_id,
                    handler_request.id,
                    authenticated_identity_hash,
                )
            };
            let result = if broker::tool_configuration::handles(method_name) {
                broker::tool_configuration::dispatch(state, services, request, call).await
            } else {
                call().await
            };
            // R1: no activity was published on this path, so none is ended here. See the note
            // above the dispatch: this entry is configuration, and the real Main file publisher
            // lives at `broker::dispatch_files`.
            match result {
                Ok(dispatch) => {
                    // THE HANDOFF BETWEEN "CLOSE CONFIRMED" AND "THE BACKEND
                    // ACTUALLY STARTS" MUST BE EFFECTIVE.
                    //
                    // This block used to answer the settings request **first** and
                    // then refresh the registry. That order has a real hole: from
                    // the moment the user is told the tool is closed, to the moment
                    // the scope head is rebuilt, a call planned against the
                    // superseded snapshot is still the scope's head and can start a
                    // real dispatch against a configuration the user has already
                    // closed.
                    //
                    // The refresh is therefore performed **before** the response is
                    // written. The user's confirmation now means "the tool is closed
                    // *and* the authority other calls are checked against already
                    // reflects that", rather than "the row was written and
                    // something will reconcile it later".
                    //
                    // The refresh is not retried behind the response and never
                    // rewrites a committed outcome: it either ran before the answer
                    // or the answer reports that the close is not yet in force.
                    let mut refresh_failure = None;
                    if registry_affecting(method_name) {
                        refresh_failure = refresh_registry_after_change(
                            services,
                            method_name,
                            &request.params,
                            request.id,
                        );
                    }
                    // The projections are published before the response for the
                    // same reason: a subscriber that reacts to the confirmation
                    // must not observe a projection older than the change it is
                    // being told about.
                    if let Err(error) = services
                        .persistence
                        .publish_tool_projection_changes(&dispatch.projection_changes)
                    {
                        append_core_log(
                            "tool_mutation_projection_publish_failed",
                            json!({
                                "method": method_name,
                                "correlation_id": request.id,
                                "reason": error.to_string(),
                            }),
                        );
                    }
                    // A close that could not be made effective is **not** reported
                    // as a success. The command's own row is committed — that part
                    // really happened — but the user is told the switch is not yet
                    // in force, with a stable code, so retrying is an informed
                    // action rather than a guess.
                    let response = match refresh_failure {
                        None => JsonRpcResponse::success(request.id, dispatch.value),
                        // The refusal carries the failure's own stable code and
                        // `Unavailable` category directly, rather than being
                        // flattened through a generic envelope builder that would
                        // replace the code with an internal-error one.
                        Some(failure) => {
                            // The raw reason goes to the log, where it already
                            // belongs; the response carries only the bounded code.
                            append_core_log(
                                "tool_registry_switch_not_in_force",
                                json!({
                                    "method": method_name,
                                    "correlation_id": request.id,
                                    "code": failure.code,
                                    "stage": failure.stage,
                                    "reason": failure.log_reason(),
                                }),
                            );
                            JsonRpcResponse::failure(request.id, failure.envelope(request.id))
                        }
                    };
                    send_recorded_response(state, queue, request, response).await?;
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

        _ => bail!("registered tool route has no handler"),
    }
}
