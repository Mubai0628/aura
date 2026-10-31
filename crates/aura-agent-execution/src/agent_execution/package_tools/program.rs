//! Package programs: the trusted, terminal-like Native exception. The program runs with the user's identity in a creation-time Job,
//! honestly described; only Job control and bounded output are claimed.
use super::super::*;
use super::installed_tool_for;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;
use aura_domain_common::jcs::canonical_sha256;
use aura_package_service::package_program;
use aura_tool_domain::{CanonicalToolPackageToolV1, PackageConformanceReasonV1 as Reason};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

pub(in crate::agent_execution) const NATIVE_ID: &str = "agent-native-package-v1";
/// Each call's output limit; the Job and the manifest's own limits still apply.
pub(super) const OUTPUT_BYTES: usize = 8 * 1024 * 1024;

pub(in crate::agent_execution) fn declares_network(tool: &CanonicalToolPackageToolV1) -> bool {
    package_program::program_declares_network(tool)
}

/// Whether an operation is a package program call in its exact shape.
pub fn is_program(content: &OperationContent) -> bool {
    aura_security_contract::package_program(
        &content.source,
        &content.action_kind,
        &content.capabilities,
        &content.resources,
    )
}

pub fn native_descriptor(runtime: RuntimeInstanceId) -> BackendDescriptor {
    let mut value = super::super::broker::descriptor(runtime);
    value.id = BackendId(NATIVE_ID.into());
    value.class = BackendClass::Native;
    value.capabilities =
        [ResolvedCapability::ProcessExecute, ResolvedCapability::OrdinaryNetwork].into();
    // The program keeps the user's own authority. Only the kill-on-close Job and the
    // output quota are enforced; no filesystem, network or identity isolation.
    value.guarantees =
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into();
    // Cancel ends the whole tree: the Job is kill-on-close and the call closes it.
    value.compatibility = [ExecutionCompatibility::ProcessTreeCancellation].into();
    value.protocol_fingerprint =
        canonical_sha256(&"aura.agent-native-package.v1").expect("literal");
    value
}

pub(in crate::agent_execution) struct PackageProgramBackend {
    pub descriptor: BackendDescriptor,
    pub port: super::super::broker::Port,
}

impl ExecutionBackend for PackageProgramBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if request.operation != self.port.call.operation
                || request.operation.validate().is_err()
                || !self.descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(BackendError::PreparationRejected {
                    reason: "package.unsupported_operation".into(),
                });
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| BackendError::PreparationRejected {
                reason: "package.invalid_preparation".into(),
            })
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request();
            if request.plan.validate().is_err()
                || request.plan.prepared.backend != self.descriptor
                || request.operation != self.port.call.operation
                || self.port.inspect(&request.operation).await? != request.plan.prepared.preparation
            {
                return Err(BackendError::PreparationRejected {
                    reason: "package.preparation_changed".into(),
                });
            }
            self.port.claim(&dispatch).await?;
            self.port.invoke(request).await.map_err(|_| BackendError::DispatchUnknown {
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

/// The preparation facts: the installed version this call runs. A change between
/// preparation and dispatch (disable, revoke, uninstall, upgrade) refuses the call.
pub(in crate::agent_execution) fn inspect(
    service: &AgentExecution,
    call: &CallContext,
) -> anyhow::Result<Value> {
    let store = service.journal.lock()?;
    let tool = installed_tool_for(store.database.connection(), &call.validated.definition)?;
    Ok(json!({
        "package_id": tool.package_id,
        "package_version": tool.version.package_version,
        "package_fingerprint": tool.version.package_fingerprint,
        "tool_key": tool.tool.tool_key,
    }))
}

/// The attempt identity a program sees, derived from the real Core attempt.
fn attempt_identity(attempt: &AttemptRef) -> ToolExecutionAttemptId {
    let digest = aura_domain_common::jcs::CanonicalSha256HashV1::sha256(
        format!("aura.package-attempt.v1\0{}\0{}", attempt.operation_id, attempt.ordinal)
            .as_bytes(),
    );
    let hex = digest.hex();
    let mut bytes = [0_u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap_or(0);
    }
    ToolExecutionAttemptId::from_bytes(bytes)
}

pub(in crate::agent_execution) async fn invoke(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
    commands: Option<Arc<crate::command_execution::CommandExecution>>,
) -> anyhow::Result<()> {
    let lifetime = service.active_calls.retain_worker(call.validated.call.tool_call_id)?;
    let mut cancellation = service
        .active_calls
        .session(call.snapshot.session_id)?
        .into_iter()
        .find(|entry| entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id)
        .ok_or_else(|| anyhow::anyhow!("package.call_not_active"))?
        .cancel
        .subscribe();
    let token = Arc::new(AtomicBool::new(*cancellation.borrow()));
    let cancelled = token.clone();
    let relay = tokio::spawn(async move {
        while cancellation.changed().await.is_ok() {
            if *cancellation.borrow_and_update() {
                cancelled.store(true, Ordering::Release);
                break;
            }
        }
    });
    let _relay = Relay { task: relay, token: token.clone() };
    // The same verified managed PowerShell the Shell uses, held for the whole call.
    let powershell = match commands {
        Some(commands) => commands.managed_runtime().await.ok(),
        None => None,
    };
    let environment = aura_windows_platform::current_user_environment()?;
    tokio::task::spawn_blocking(move || {
        let _lifetime = lifetime;
        super::run::run(
            &service,
            &call,
            &request,
            &token,
            super::run::Inputs { start_lease, powershell, environment },
        )
    })
    .await?
}

struct Relay {
    task: tokio::task::JoinHandle<()>,
    token: Arc<AtomicBool>,
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.token.store(true, Ordering::Release);
        self.task.abort();
    }
}

pub(super) fn attempt_identity_for(request: &DispatchRequest) -> ToolExecutionAttemptId {
    attempt_identity(&request.binding.attempt)
}

/// The fixed, model-safe failure text for a program outcome. No stderr, path or
/// raw error reaches the model.
pub(super) fn failure_text(reason: Reason) -> String {
    let base = match reason {
        Reason::Completed | Reason::CompletedResultUnchecked => "package.completed",
        Reason::InvalidResult => "package.invalid_result: 程序的输出不符合工具声明的结果格式",
        Reason::InvalidProtocol => "package.invalid_protocol: 程序没有按 Aura 工具协议回应",
        Reason::InvalidArtifact => "package.invalid_artifact: 程序的输出文件不符合声明",
        Reason::Cancelled => "package.cancelled: 调用已取消，程序已停止",
        Reason::TimedOut => "package.timed_out: 程序超时，已停止",
        Reason::OutputLimit => "package.output_limit: 程序输出超过上限，已停止",
        Reason::ProcessFailed => "package.process_failed: 程序运行失败",
        Reason::TransportUnconfirmed => "package.transport_unconfirmed: 结果不明",
        Reason::CleanupUnconfirmed => {
            "package.cleanup_unconfirmed: 程序是否完全停止尚未确认，结果不明，不会自动重试"
        }
        Reason::CredentialReflected => "package.credential_reflected: 输出中出现了不该出现的凭据",
        Reason::ProgramCredentialUnsupported => {
            "package.program_credentials_unsupported: 运行程序的工具不能使用 Aura 保存的凭据"
        }
    };
    base.to_owned()
}
