//! A native ConPTY executor with the same policy, claim and result owner as Shell.
use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;
use aura_domain_common::jcs::canonical_sha256;

pub(super) const BACKEND_ID: &str = "agent-terminal-v1";
struct TerminalBackend {
    descriptor: BackendDescriptor,
    port: broker::Port,
    sensitive: Option<(u64, tokio::sync::watch::Receiver<bool>)>,
}
#[cfg(any(test, feature = "test-hooks"))]
pub fn backend(
    service: Arc<AgentExecution>,
    call: &CallContext,
    commands: Arc<crate::command_execution::CommandExecution>,
) -> anyhow::Result<Arc<dyn ExecutionBackend>> {
    backend_with_policy(service, call, commands, None)
}
pub(super) fn backend_with_policy(
    service: Arc<AgentExecution>,
    call: &CallContext,
    commands: Arc<crate::command_execution::CommandExecution>,
    sensitive: Option<(u64, tokio::sync::watch::Receiver<bool>)>,
) -> anyhow::Result<Arc<dyn ExecutionBackend>> {
    let mut descriptor = if matches!(
        call.operation.content.action_kind.as_str(),
        "terminal.open" | "terminal.write"
    ) {
        let mut value = commands.candidates_for(call.snapshot.mode)[0].descriptor();
        value.id = BackendId(BACKEND_ID.into());
        value
    } else {
        broker::descriptor(service.runtime)
    };
    descriptor.protocol_fingerprint = canonical_sha256(&"aura.agent-terminal.v1")?;
    Ok(Arc::new(TerminalBackend {
        descriptor,
        port: broker::Port { service, call: call.clone(), commands: Some(commands) },
        sensitive,
    }))
}
impl ExecutionBackend for TerminalBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if request.operation.validate().is_err()
                || !self.descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(preparation("terminal.unsupported_operation"));
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| preparation("terminal.invalid_preparation"))
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request();
            if request.plan.validate().is_err()
                || request.plan.prepared.backend != self.descriptor
                || self.port.inspect(&request.operation).await? != request.plan.prepared.preparation
            {
                return Err(rejected("terminal.preparation_changed"));
            }
            self.port.claim(&dispatch).await?;
            let invoked =
                if request.operation.content.action_kind == "terminal.request_sensitive_input" {
                    let (timeout_ms, cancellation) = self
                        .sensitive
                        .as_ref()
                        .ok_or_else(|| rejected("terminal.sensitive.policy_missing"))?;
                    let service = self.port.service.clone();
                    let call = self.port.call.clone();
                    let commands = self
                        .port
                        .commands
                        .clone()
                        .ok_or_else(|| rejected("terminal.controller_missing"))?;
                    let lease =
                        service.persistence.permission_start_lease(call.snapshot.session_id).await;
                    let lifetime = service
                        .active_calls
                        .retain_worker(call.validated.call.tool_call_id)
                        .map_err(rejected)?;
                    let result = invoke_with_sensitive(
                        service,
                        call,
                        request.clone(),
                        commands,
                        lease,
                        Some(crate::command_execution::SensitiveWait {
                            timeout_ms: *timeout_ms,
                            cancellation: cancellation.clone(),
                        }),
                    )
                    .await;
                    drop(lifetime);
                    result.map_err(|_| rejected("terminal.sensitive.invocation_failed"))
                } else {
                    self.port.invoke(request).await
                };
            invoked.map_err(|_| BackendError::DispatchUnknown {
                attempt: request.binding.attempt.clone(),
            })?;
            Ok(ExecutionHandle {
                attempt: request.binding.attempt.clone(),
                dispatch_fingerprint: canonical_sha256(&request.binding).map_err(|_| {
                    BackendError::DispatchUnknown { attempt: request.binding.attempt.clone() }
                })?,
            })
        })
    }
    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        self.port.cancel(handle)
    }
    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        self.port.reconcile(attempt)
    }
}
pub(super) async fn invoke(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    commands: Arc<crate::command_execution::CommandExecution>,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> anyhow::Result<()> {
    invoke_with_sensitive(service, call, request, commands, start_lease, None).await
}
async fn invoke_with_sensitive(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    commands: Arc<crate::command_execution::CommandExecution>,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
    sensitive: Option<crate::command_execution::SensitiveWait>,
) -> anyhow::Result<()> {
    let verifier = service.clone();
    let verified = call.clone();
    let operation = request.operation.operation_id;
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "terminal.verify_start"),
            StoreAccess::Read,
            move || {
                verifier.verify_source(&verified)?;
                Ok(aura_persistence_v2::agent::checkpoint::verify_start_operation(
                    verifier.journal.lock()?.database.connection(),
                    operation,
                )?)
            },
        )
        .await?;
    let mut side_effect = SideEffectStateV1::NoEffect;
    let output = commands
        .agent_terminal_call(&call, &request, &mut side_effect, start_lease, sensitive)
        .await;
    let success = output.is_ok();
    let evidence = BrokerEvidence {
        action_kind: request.operation.content.action_kind.clone(),
        request_fingerprint: request.operation.fingerprint.0.clone(),
        side_effect,
        before: request.plan.prepared.preparation["domain"].clone(),
        after: serde_json::Value::Null,
        domain_journal: serde_json::json!({"schema":"agent-terminal-effect-v1","output":output.as_ref().ok(),"error":output.as_ref().err().map(ToString::to_string)}),
    };
    let owner = service.clone();
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "terminal.result"),
            StoreAccess::Write,
            move || {
                Ok(owner.journal.lock()?.database.unit_of_work(|tx| {
                    aura_persistence_v2::execution::broker_result::record(
                        tx,
                        &request,
                        &evidence,
                        success,
                        UtcTimestamp::now(),
                    )
                })?)
            },
        )
        .await?;
    Ok(())
}
