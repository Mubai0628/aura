//! Package programs: the trusted, terminal-like Native exception. The program runs with the user's identity in a creation-time Job,
//! honestly described; only Job control and bounded output are claimed.
use super::super::*;
use super::installed_tool_for;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;
use aura_domain_common::jcs::canonical_sha256;
use aura_package_service::{failures::code_of, package_program};
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
                    reason: "package.unsupported_operation: 运行工具包程序的后端不能执行这次调用，程序没有运行。可以重新调用。".into(),
                });
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| BackendError::PreparationRejected {
                reason: "package.invalid_preparation: Aura 没能准备这次程序调用，程序没有运行。可以重新调用。".into(),
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
                    reason: "package.preparation_changed: 这个工具包在调用准备好之后变了（停用、撤销信任、卸载或升级），程序没有运行。可以重新调用。".into(),
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
        .ok_or_else(|| {
            anyhow::anyhow!(
                "package.call_not_active: 这次工具包调用在程序启动之前已经结束（回合停止了或调用被取消），程序没有运行。"
            )
        })?
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
        // A program not seen to stop is kept under this call: its locks wait for it.
        let _kept = aura_backend_windows_user::lingering::enter(
            request.binding.attempt.operation_id.to_bytes(),
        );
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

const CLEANUP_UNCONFIRMED: &str = "package.cleanup_unconfirmed: 程序是否已经完全停止还没能确认，结果不明，Aura 不会自动重试。程序可能已经做了改动；再运行之前先确认它的效果。";
const PROGRAM_CREDENTIALS: &str = "package.program_credentials_unsupported: 这个工具用不了：运行程序的工具不能使用 Aura 保存的凭据。请告诉用户这个工具包的凭据设置有误。";

/// The fixed, model-safe failure text for a program outcome: what happened to the run, whether
/// it may have changed something, and what to check. No stderr, path or raw error reaches the
/// model.
pub(super) fn failure_text(reason: Reason, timeout_ms: u64) -> String {
    let base = match reason {
        Reason::Completed => "package.completed: 程序运行完成。",
        Reason::CompletedResultUnchecked => {
            "package.result_unchecked: 程序运行完了，但 Aura 没能检查它的结果，结果没有采用。程序可能已经做了改动；再运行之前先确认。"
        }
        Reason::InvalidResult => {
            "package.invalid_result: 程序运行完了，但输出不符合工具声明的结果格式，结果没有采用。程序可能已经做了改动；如果反复出现，告诉用户这个工具包可能需要更新。"
        }
        Reason::InvalidProtocol => {
            "package.invalid_protocol: 程序没有按 Aura 的工具协议回应，结果没有采用。程序可能已经做了改动；如果反复出现，告诉用户这个工具包可能需要更新。"
        }
        Reason::InvalidArtifact => {
            "package.invalid_artifact: 程序交出的文件不符合工具的声明，没有采用。程序可能已经做了改动；如果反复出现，告诉用户这个工具包可能需要更新。"
        }
        Reason::Cancelled => {
            "package.cancelled: 调用被取消，程序已经停止。它在停止前可能已经做了一部分改动。"
        }
        Reason::TimedOut => {
            return format!(
                "package.timed_out: 程序没有在 {} 秒内完成，已经被停止。它在停止前可能已经做了一部分改动；再运行之前先确认。",
                timeout_ms.div_ceil(1000)
            );
        }
        Reason::OutputLimit => {
            "package.output_limit: 程序的输出超过了这次调用允许的上限，已经被停止，结果没有采用。它在停止前可能已经做了一部分改动；可以换更小的输入再调用。"
        }
        Reason::ProcessFailed => {
            "package.process_failed: 程序运行失败（以失败状态退出，或没能正常运行），结果没有采用。它在失败前可能已经做了一部分改动；再运行之前先确认。"
        }
        Reason::TransportUnconfirmed => {
            "package.transport_unconfirmed: Aura 在程序报告结果之前和它断了联系，结果不明。程序可能已经做了改动；再运行之前先确认它的效果。"
        }
        Reason::CleanupUnconfirmed => CLEANUP_UNCONFIRMED,
        Reason::CredentialReflected => {
            "package.credential_reflected: 程序的输出里出现了凭据，为保护凭据，输出没有交给模型。程序可能已经做了改动；如果反复出现，告诉用户这个工具包有问题。"
        }
        Reason::ProgramCredentialUnsupported => PROGRAM_CREDENTIALS,
    };
    base.to_owned()
}

/// The line for the most telling code a failed start carries (bare from the program runner, or
/// with the package service's own sentence), when Core has words for the model.
pub(super) fn start_failure_text(chain: &[String]) -> Option<&'static str> {
    const LINES: &[(&str, &str)] = &[
        (
            "package.execution_report_unconfirmed",
            "package.execution_report_unconfirmed: 程序已经启动，但 Aura 没能拿到它的运行结果，结果不明。程序可能已经做了改动；再运行之前先确认它的效果。",
        ),
        ("package.cleanup_unconfirmed", CLEANUP_UNCONFIRMED),
        (
            "package.arguments_invalid",
            "package.arguments_invalid: 参数不符合这个工具的输入格式，程序没有运行。请按工具说明改好参数再调用。",
        ),
        (
            "package.configuration_required",
            "package.configuration_required: 这个工具需要的设置还没有填，程序没有运行。请用户在工具包页填好这个工具的设置。",
        ),
        (
            "package.configuration_invalid",
            "package.configuration_invalid: 这个工具的设置不符合它的要求，程序没有运行。请用户在工具包页改好这个工具的设置。",
        ),
        (
            "package.workspace_required",
            "package.workspace_required: 这个工具要在工作区里运行，现在没有可用的工作区，程序没有运行。",
        ),
        ("package.program_credentials_unsupported", PROGRAM_CREDENTIALS),
    ];
    LINES
        .iter()
        .find(|(code, _)| chain.iter().any(|text| code_of(text) == *code))
        .map(|(_, line)| *line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed run says what happened to it and whether it may have changed something; the
    /// unchecked result is no longer called complete.
    #[test]
    fn a_failed_program_says_what_happened() {
        let unchecked = failure_text(Reason::CompletedResultUnchecked, 0);
        assert!(unchecked.starts_with("package.result_unchecked: "), "{unchecked}");
        assert!(failure_text(Reason::TimedOut, 30_000).contains("30 秒"));
        assert!(failure_text(Reason::TransportUnconfirmed, 0).contains("可能已经做了改动"));
        let chain =
            ["package.preparation_failed".to_owned(), "package.arguments_invalid".to_owned()];
        let line = start_failure_text(&chain).expect("known");
        assert!(line.starts_with("package.arguments_invalid: ") && line.contains("没有运行"));
        assert_eq!(start_failure_text(&["package.preparation_failed".to_owned()]), None);
        // The package service now says its own sentence after the code; the model still gets
        // Core's line for that code.
        let said = [aura_package_service::failures::ARGUMENTS_INVALID.to_owned()];
        assert_eq!(start_failure_text(&said), Some(line));
    }
}
