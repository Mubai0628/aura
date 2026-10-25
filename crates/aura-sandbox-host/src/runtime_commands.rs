// Long-lived command loop. Execution is delegated to the reviewed
// `SandboxExecutor`, whose native launcher creates a new kill-on-close Job for
// every process. Because this Host is already in Core's Component Job, Windows
// forms the required nested Execution Job hierarchy.

use std::{collections::BTreeMap, sync::mpsc};

use anyhow::{Context, Result, bail};
use aura_domain_common::{ContractVersion, ErrorCategory, ErrorCode};
use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
use aura_ipc::{
    JSON_RPC_VERSION, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    ToolRuntimeSecretFrameV1, WireFrame, method, read_frame, write_frame,
};
use aura_permission_domain::{
    AuthorizedToolSandboxExecutionV1, CancelSandboxExecutionInput, ExternalProcessLaunchPlan,
    NonSecretToolRuntimeFactsV1, SandboxExecutionPlan, SandboxExecutionState, SandboxHostEvent,
    ToolCredentialPlacementV1, ToolPreparedRuntimeKindV1,
};
use aura_persistence::PersistenceCoordinator;
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_sandbox::{
    CancellationRegistry, SandboxError, SandboxExecutionOutcome, SandboxExecutor,
    SandboxExecutorOptions, SandboxProcessInputV1, sandbox_plan_for_external_process,
};
use aura_tool_contract::StdioProtocolFrameV1;
use aura_tool_runtime::{ToolStdioHostInputV1, encode_stdio_frame_bytes};
use aura_windows_platform::NativeSecretEnvironmentEntry;

struct ActiveToolStdioInputV1 {
    sandbox_execution_id: aura_domain_common::SandboxExecutionId,
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    next_sequence: u64,
    execute_sent: bool,
    cancel_sent: bool,
    sender: mpsc::Sender<SandboxProcessInputV1>,
}

enum ActiveStdioBootstrapV1 {
    Tool(aura_permission_domain::AuraStdioDispatchBootstrapV1),
}

impl ActiveToolStdioInputV1 {
    fn accept(&mut self, input: ToolStdioHostInputV1) -> Result<(), ()> {
        input.validate().map_err(|_| ())?;
        if input.sandbox_execution_id != self.sandbox_execution_id
            || input.tool_execution_attempt_id != self.tool_execution_attempt_id
            || input.frame.tool_call_id() != self.tool_call_id
            || input.frame.sequence() != self.next_sequence
        {
            return Err(());
        }
        match &input.frame {
            StdioProtocolFrameV1::Execute { .. } if !self.execute_sent => {}
            StdioProtocolFrameV1::Cancel { .. } if self.execute_sent && !self.cancel_sent => {}
            _ => return Err(()),
        }
        let bytes = encode_stdio_frame_bytes(&input.frame).map_err(|_| ())?;
        self.sender.send(SandboxProcessInputV1::Write(bytes)).map_err(|_| ())?;
        match input.frame {
            StdioProtocolFrameV1::Execute { .. } => self.execute_sent = true,
            StdioProtocolFrameV1::Cancel { .. } => self.cancel_sent = true,
            _ => return Err(()),
        }
        self.next_sequence = self.next_sequence.checked_add(1).ok_or(())?;
        Ok(())
    }
}
use serde::Serialize;
use serde_json::{Value, json};
use tokio::net::windows::named_pipe::NamedPipeClient;

pub async fn serve(mut pipe: NamedPipeClient, persistence: PersistenceCoordinator) -> Result<()> {
    let cancellation = CancellationRegistry::default();
    let executor = SandboxExecutor::new(
        persistence.clone(),
        cancellation.clone(),
        SandboxExecutorOptions::default(),
    );
    loop {
        let request = read_request(&mut pipe).await?;
        match request.method.as_str() {
            method::SIDECAR_PING => {
                write_success(&mut pipe, request.id, json!({ "ready": true })).await?;
            }
            method::SIDECAR_SHUTDOWN => {
                write_success(&mut pipe, request.id, json!({ "stopping": true })).await?;
                return Ok(());
            }
            method::SANDBOX_CANCEL => {
                let input: CancelSandboxExecutionInput = serde_json::from_value(request.params)
                    .context("invalid idle sandbox.cancel params")?;
                let cancelled = cancellation.cancel(input.cancellation_id)?;
                write_success(&mut pipe, request.id, json!({ "cancelled": cancelled })).await?;
            }
            method::SANDBOX_EXECUTE => {
                let plan: SandboxExecutionPlan = serde_json::from_value(request.params)
                    .context("invalid sandbox.execute plan")?;
                if execute_one(
                    &mut pipe,
                    &executor,
                    &cancellation,
                    &persistence,
                    request.id,
                    plan,
                    None,
                    None,
                    None,
                    None,
                )
                .await?
                {
                    return Ok(());
                }
            }
            method::SANDBOX_EXECUTE_EXTERNAL => {
                let external: ExternalProcessLaunchPlan = serde_json::from_value(request.params)
                    .context("invalid sandbox.execute_external plan")?;
                let prepared = persistence
                    .external_trust(&external.source)
                    .map_err(SandboxError::Persistence)
                    .and_then(|trust| trust.ok_or(SandboxError::ExternalSourceNotTrusted))
                    .and_then(|trust| sandbox_plan_for_external_process(&external, &trust));
                match prepared {
                    Ok(plan) => {
                        persistence.record_external_process_execution_audit(
                            &external,
                            SandboxExecutionState::Planned,
                            None,
                            None,
                            None,
                            external.identity.correlation_id,
                        )?;
                        if execute_one(
                            &mut pipe,
                            &executor,
                            &cancellation,
                            &persistence,
                            request.id,
                            plan,
                            Some(external),
                            None,
                            None,
                            None,
                        )
                        .await?
                        {
                            return Ok(());
                        }
                    }
                    Err(error) => write_sandbox_error(&mut pipe, request.id, &error).await?,
                }
            }
            method::SANDBOX_EXECUTE_AUTHORIZED_TOOL => {
                let authorized: AuthorizedToolSandboxExecutionV1 =
                    match serde_json::from_value(request.params) {
                        Ok(value) => value,
                        Err(_) => {
                            write_failure(
                                &mut pipe,
                                request.id,
                                "sandbox.tool_authorization_invalid",
                                "Sandbox Host rejected an invalid Tool authorization binding",
                            )
                            .await?;
                            continue;
                        }
                    };
                if aura_sandbox::plan_hash(&authorized.plan).ok().as_ref()
                    != Some(&authorized.plan.plan_hash)
                {
                    write_failure(
                        &mut pipe,
                        request.id,
                        "sandbox.tool_authorization_invalid",
                        "Sandbox Host rejected a Tool launch whose plan hash changed",
                    )
                    .await?;
                    continue;
                }
                let plan = authorized.plan.clone();
                let stdio_bootstrap = match authorized.aura_stdio_dispatch_bootstrap() {
                    Ok(value) => value,
                    Err(_) => {
                        write_failure(
                            &mut pipe,
                            request.id,
                            "sandbox.tool_authorization_invalid",
                            "Sandbox Host rejected an invalid Aura Stdio bootstrap binding",
                        )
                        .await?;
                        continue;
                    }
                };
                let secret_environment = if authorized.operation.runtime.credential().is_some() {
                    let frame = read_frame(&mut pipe).await?;
                    let WireFrame::ToolRuntimeSecret(secret) = frame else {
                        write_failure(
                            &mut pipe,
                            request.id,
                            "sandbox.tool_authorization_invalid",
                            "Sandbox Host rejected a missing Tool Credential binding",
                        )
                        .await?;
                        continue;
                    };
                    match materialize_secret_environment(&authorized, secret) {
                        Ok(value) => Some(value),
                        Err(()) => {
                            write_failure(
                                &mut pipe,
                                request.id,
                                "sandbox.tool_authorization_invalid",
                                "Sandbox Host rejected a changed Tool Credential binding",
                            )
                            .await?;
                            continue;
                        }
                    }
                } else {
                    None
                };
                let stdio_input = match &authorized.operation.runtime {
                    NonSecretToolRuntimeFactsV1::Sandbox { facts }
                        if facts.runtime_kind == ToolPreparedRuntimeKindV1::AuraStdio =>
                    {
                        let (sender, receiver) = mpsc::channel();
                        Some((
                            ActiveToolStdioInputV1 {
                                sandbox_execution_id: authorized.binding.sandbox_execution_id,
                                tool_call_id: authorized.binding.tool_call_id,
                                tool_execution_attempt_id: authorized
                                    .binding
                                    .tool_execution_attempt_id,
                                next_sequence: 1,
                                execute_sent: false,
                                cancel_sent: false,
                                sender,
                            },
                            receiver,
                        ))
                    }
                    _ => None,
                };
                match persistence.consume_authorized_tool_sandbox_dispatch_v1(
                    authorized,
                    aura_domain_common::UtcTimestamp::now(),
                ) {
                    Ok(_consumed_binding) => {
                        if execute_one(
                            &mut pipe,
                            &executor,
                            &cancellation,
                            &persistence,
                            request.id,
                            plan,
                            None,
                            stdio_bootstrap.map(ActiveStdioBootstrapV1::Tool),
                            secret_environment,
                            stdio_input,
                        )
                        .await?
                        {
                            return Ok(());
                        }
                    }
                    Err(_) => {
                        write_failure(
                            &mut pipe,
                            request.id,
                            "sandbox.tool_authorization_unavailable",
                            "Sandbox Host rejected a missing, changed, revoked, cancelled, or consumed Tool dispatch binding",
                        )
                        .await?;
                    }
                }
            }
            // Retired P07 Tool Conformance in the sandbox; the request is unsupported.
            _ => {
                write_failure(
                    &mut pipe,
                    request.id,
                    "sandbox.unsupported_request",
                    "Sandbox Host rejected an unsupported method",
                )
                .await?;
            }
        }
    }
}

// This private host boundary binds the exact pipe, executor, cancellation, persistence,
// secret, input, and session facts used for one authenticated runtime dispatch.
#[allow(clippy::too_many_arguments)]
async fn execute_one(
    pipe: &mut NamedPipeClient,
    executor: &SandboxExecutor,
    cancellation: &CancellationRegistry,
    persistence: &PersistenceCoordinator,
    execution_request_id: aura_domain_common::CorrelationId,
    plan: SandboxExecutionPlan,
    external_audit: Option<ExternalProcessLaunchPlan>,
    stdio_bootstrap: Option<ActiveStdioBootstrapV1>,
    secret_environment: Option<NativeSecretEnvironmentEntry>,
    stdio_input: Option<(ActiveToolStdioInputV1, mpsc::Receiver<SandboxProcessInputV1>)>,
) -> Result<bool> {
    let cancellation_id = plan.cancellation_id;
    let executor = executor.clone();
    let (mut active_stdio, stdin_commands) = match stdio_input {
        Some((active, receiver)) => (Some(active), Some(receiver)),
        None => (None, None),
    };
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut execution = tokio::task::spawn_blocking(move || match stdio_bootstrap {
        Some(ActiveStdioBootstrapV1::Tool(bootstrap)) => executor.execute_with_runtime_io(
            plan,
            Some(bootstrap),
            secret_environment,
            stdin_commands,
            |event| {
                let _ = events_tx.send(event);
            },
        ),
        None => executor.execute_with_runtime_io(
            plan,
            None,
            secret_environment,
            stdin_commands,
            |event| {
                let _ = events_tx.send(event);
            },
        ),
    });
    let mut shutdown_request = None;
    loop {
        tokio::select! {
            Some(event) = events_rx.recv() => {
                write_event(pipe, &event).await?;
            }
            outcome = &mut execution => {
                while let Ok(event) = events_rx.try_recv() {
                    write_event(pipe, &event).await?;
                }
                match outcome.context("Sandbox execution worker panicked")? {
                    Ok(outcome) => {
                        if let Some(external) = external_audit.as_ref() {
                            persistence.record_external_process_execution_audit(
                                external,
                                outcome.state,
                                outcome.process_id,
                                outcome.exit_code,
                                None,
                                external.identity.correlation_id,
                            )?;
                        }
                        write_success(
                            pipe,
                            execution_request_id,
                            execution_outcome_value(&outcome),
                        ).await?
                    }
                    Err(error) => {
                        if let Some(external) = external_audit.as_ref() {
                            persistence.record_external_process_execution_audit(
                                external,
                                SandboxExecutionState::PolicyViolation,
                                None,
                                None,
                                Some(sandbox_error_code(&error).to_owned()),
                                external.identity.correlation_id,
                            )?;
                        }
                        write_sandbox_error(pipe, execution_request_id, &error).await?
                    }
                }
                if let Some(shutdown_id) = shutdown_request {
                    write_success(pipe, shutdown_id, json!({ "stopping": true })).await?;
                    return Ok(true);
                }
                return Ok(false);
            }
            inbound = read_request(pipe) => {
                let request = inbound?;
                match request.method.as_str() {
                    method::SANDBOX_CANCEL => {
                        let input: CancelSandboxExecutionInput = serde_json::from_value(request.params)
                            .context("invalid sandbox.cancel params")?;
                        let cancelled = cancellation.cancel(input.cancellation_id)?;
                        write_success(pipe, request.id, json!({ "cancelled": cancelled })).await?;
                    }
                    method::SIDECAR_PING => {
                        write_success(pipe, request.id, json!({ "ready": true, "busy": true })).await?;
                    }
                    method::SIDECAR_SHUTDOWN => {
                        let _ = cancellation.cancel(cancellation_id)?;
                        if shutdown_request.replace(request.id).is_some() {
                            write_failure(
                                pipe,
                                request.id,
                                "sandbox.shutdown_already_pending",
                                "Sandbox Host shutdown is already pending",
                            ).await?;
                        }
                    }
                    method::SANDBOX_TOOL_STDIO_INPUT => {
                        let accepted = match (
                            serde_json::from_value::<ToolStdioHostInputV1>(request.params),
                            active_stdio.as_mut(),
                        ) {
                            (Ok(input), Some(active)) => active.accept(input).is_ok(),
                            _ => false,
                        };
                        if accepted {
                            write_success(pipe, request.id, json!({ "accepted": true })).await?;
                        } else {
                            write_failure(
                                pipe,
                                request.id,
                                "sandbox.tool_stdio_input_invalid",
                                "Sandbox Host rejected invalid or unavailable Aura Stdio input",
                            )
                            .await?;
                        }
                    }
                    method::SANDBOX_EXECUTE => {
                        write_failure(
                            pipe,
                            request.id,
                            "sandbox.host_busy",
                            "Sandbox Host is already executing a plan",
                        ).await?;
                    }
                    method::SANDBOX_EXECUTE_EXTERNAL => {
                        write_failure(
                            pipe,
                            request.id,
                            "sandbox.host_busy",
                            "Sandbox Host is already executing a plan",
                        ).await?;
                    }
                    method::SANDBOX_EXECUTE_AUTHORIZED_TOOL => {
                        write_failure(
                            pipe,
                            request.id,
                            "sandbox.host_busy",
                            "Sandbox Host is already executing its single supported Tool process",
                        ).await?;
                    }
                    method::SANDBOX_EXECUTE_AUTHORIZED_TOOL_CONFORMANCE => {
                        write_failure(
                            pipe,
                            request.id,
                            "sandbox.host_busy",
                            "Sandbox Host is already executing its single supported Tool Conformance process",
                        ).await?;
                    }
                    _ => {
                        write_failure(
                            pipe,
                            request.id,
                            "sandbox.unsupported_request",
                            "Sandbox Host rejected an unsupported method",
                        ).await?;
                    }
                }
            }
        }
    }
}

fn materialize_secret_environment(
    authorized: &AuthorizedToolSandboxExecutionV1,
    frame: ToolRuntimeSecretFrameV1,
) -> Result<NativeSecretEnvironmentEntry, ()> {
    let credential = authorized.operation.runtime.credential().ok_or(())?;
    let ToolCredentialPlacementV1::Environment { environment_variable } = &credential.placement
    else {
        return Err(());
    };
    if frame.sandbox_execution_id() != authorized.binding.sandbox_execution_id
        || frame.tool_execution_attempt_id() != authorized.binding.tool_execution_attempt_id
        || frame.credential_id() != credential.credential_id
        || frame.credential_revision() != credential.credential_revision
        || frame.slot_key() != &credential.slot_key
        || frame.environment_variable() != environment_variable
        || frame.runtime_binding_hash() != &authorized.binding.runtime_binding_hash
    {
        return Err(());
    }
    NativeSecretEnvironmentEntry::from_utf8_secret(
        environment_variable.as_str().to_owned(),
        frame.into_secret(),
    )
    .map_err(|_| ())
}

async fn read_request(pipe: &mut NamedPipeClient) -> Result<JsonRpcRequest> {
    let WireFrame::Control(payload) = read_frame(pipe).await? else {
        bail!("Sandbox Host control pipe does not accept binary frames");
    };
    let JsonRpcMessage::Request(request) = serde_json::from_slice(&payload)? else {
        bail!("Sandbox Host received an unsolicited JSON-RPC message");
    };
    if request.jsonrpc != JSON_RPC_VERSION {
        bail!("unsupported JSON-RPC version");
    }
    Ok(request)
}

async fn write_event(pipe: &mut NamedPipeClient, event: &SandboxHostEvent) -> Result<()> {
    let notification = JsonRpcNotification {
        jsonrpc: JSON_RPC_VERSION.to_owned(),
        method: method::SANDBOX_EVENT.to_owned(),
        params: serde_json::to_value(event)?,
    };
    write_control(pipe, &notification).await
}

async fn write_success(
    pipe: &mut NamedPipeClient,
    id: aura_domain_common::CorrelationId,
    value: Value,
) -> Result<()> {
    write_control(pipe, &JsonRpcResponse::success(id, value)).await
}

async fn write_failure(
    pipe: &mut NamedPipeClient,
    id: aura_domain_common::CorrelationId,
    code: &str,
    message: &str,
) -> Result<()> {
    let envelope = IpcErrorEnvelope {
        code: ErrorCode::new(code)?,
        category: ErrorCategory::Permission,
        message: message.to_owned(),
        retryable: false,
        correlation_id: id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::from([(
            "protocol_version".to_owned(),
            serde_json::to_value(ContractVersion::CURRENT)?,
        )]),
    };
    write_control(pipe, &JsonRpcResponse::failure(id, envelope)).await
}

async fn write_sandbox_error(
    pipe: &mut NamedPipeClient,
    id: aura_domain_common::CorrelationId,
    error: &SandboxError,
) -> Result<()> {
    let envelope = sandbox_error_envelope(id, error)?;
    write_control(pipe, &JsonRpcResponse::failure(id, envelope)).await
}

fn sandbox_error_envelope(
    id: aura_domain_common::CorrelationId,
    error: &SandboxError,
) -> Result<IpcErrorEnvelope> {
    let code = sandbox_error_code(error);
    let network_policy = match error {
        SandboxError::NetworkPolicy(policy) => Some(serde_json::to_value(policy)?),
        _ => None,
    };
    let mut details = BTreeMap::from([(
        "protocol_version".to_owned(),
        serde_json::to_value(ContractVersion::CURRENT)?,
    )]);
    if let Some(policy) = network_policy {
        details.insert("network_policy".to_owned(), policy);
    }
    Ok(IpcErrorEnvelope {
        code: ErrorCode::new(code)?,
        category: ErrorCategory::Permission,
        message: error.to_string(),
        retryable: false,
        correlation_id: id,
        terminal: IpcRequestTerminal::Failed,
        details,
    })
}

fn sandbox_error_code(error: &SandboxError) -> &'static str {
    match error {
        SandboxError::NetworkPolicy(policy) => match policy.code {
            aura_permission_domain::SandboxNetworkPolicyErrorCode::NetworkAccessDeclarationRequired => {
                "sandbox.network_access_declaration_required"
            }
            aura_permission_domain::SandboxNetworkPolicyErrorCode::AppContainerNetworkDenied => {
                "sandbox.appcontainer_network_denied"
            }
            aura_permission_domain::SandboxNetworkPolicyErrorCode::NetworkGrantUnavailableInSelectedMode => {
                "sandbox.network_grant_unavailable_in_selected_mode"
            }
        },
        SandboxError::AppContainerProfileAlreadyExists => {
            "sandbox.appcontainer_profile_already_exists"
        }
        SandboxError::ExternalSourceNotTrustBound | SandboxError::ExternalSourceNotTrusted => {
            "sandbox.external_source_not_trusted"
        }
        _ => "sandbox.execution_failed",
    }
}

async fn write_control<T: Serialize>(pipe: &mut NamedPipeClient, value: &T) -> Result<()> {
    write_frame(pipe, &WireFrame::Control(serde_json::to_vec(value)?)).await?;
    Ok(())
}

fn execution_outcome_value(outcome: &SandboxExecutionOutcome) -> Value {
    json!({
        "execution_id": outcome.execution_id,
        "state": outcome.state,
        "process_id": outcome.process_id,
        "exit_code": outcome.exit_code,
        "network_enforcement": outcome.network_enforcement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_permission_domain::{
        NativeSandboxMode, NetworkAccessRequirement, NetworkExecutionMode,
        SandboxNetworkPolicyError, SandboxNetworkPolicyErrorCode,
    };

    #[test]
    fn network_policy_error_stays_machine_readable_across_host_ipc() {
        let error = SandboxError::NetworkPolicy(Box::new(SandboxNetworkPolicyError {
            code: SandboxNetworkPolicyErrorCode::AppContainerNetworkDenied,
            declared_access: NetworkAccessRequirement::Required,
            selected_mode: NetworkExecutionMode::Native {
                mode: NativeSandboxMode::CompatibilityAppContainer,
            },
            required_declaration: NetworkAccessRequirement::Required,
            allowed_networked_modes: vec![NetworkExecutionMode::Native {
                mode: NativeSandboxMode::ObservedNativeNetworked,
            }],
        }));
        let envelope = sandbox_error_envelope(aura_domain_common::CorrelationId::new(), &error)
            .expect("network policy envelope");
        assert_eq!(envelope.code.as_str(), "sandbox.appcontainer_network_denied");
        let policy = envelope.details.get("network_policy").expect("typed policy details");
        assert_eq!(policy["required_declaration"], "required");
        assert_eq!(policy["allowed_networked_modes"][0]["mode"], "observed_native_networked");
    }

    #[test]
    fn execution_outcome_reports_honest_network_enforcement() {
        let outcome = SandboxExecutionOutcome {
            execution_id: aura_domain_common::SandboxExecutionId::new(),
            state: aura_permission_domain::SandboxExecutionState::Exited,
            process_id: Some(42),
            exit_code: Some(0),
            network_enforcement:
                aura_permission_domain::NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
        };
        assert_eq!(
            execution_outcome_value(&outcome)["network_enforcement"],
            "observed_native_not_proxy_enforced"
        );
    }
}
