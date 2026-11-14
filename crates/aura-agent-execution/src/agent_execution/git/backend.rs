//! Git writes are explicitly Native; the read broker retains its independent enforcement floor.
use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;

pub(in crate::agent_execution) const NATIVE_ID: &str = "agent-native-git-v1";
pub(in crate::agent_execution) fn is_write(action: &str) -> bool {
    matches!(action, "git.commit" | "git.branch" | "git.sync" | "git.push")
}

pub(in crate::agent_execution) fn native_descriptor(
    runtime: RuntimeInstanceId,
) -> BackendDescriptor {
    let mut value = descriptor(runtime);
    value.id = BackendId(NATIVE_ID.into());
    value.class = BackendClass::Native;
    value.capabilities = [
        ResolvedCapability::GitWrite,
        ResolvedCapability::GitRemoteRead,
        ResolvedCapability::GitRemoteWrite,
        ResolvedCapability::GitDestructive,
    ]
    .into();
    // Only the kill-on-close creation-time Job and the adapter's output quota are
    // OS-enforced here. Hooks, signing, filters and LFS retain the user's authority;
    // no filesystem, credential or network isolation is claimed for these writes.
    value.guarantees =
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into();
    value.protocol_fingerprint = canonical_sha256(&"aura.agent-native-git.v1").expect("literal");
    value
}

pub(super) struct NativeGitBackend {
    pub descriptor: BackendDescriptor,
    pub port: broker::Port,
}
impl ExecutionBackend for NativeGitBackend {
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
                    reason: "git.unsupported_operation: Git 写入通道不接受这次调用（它与准备时的调用不一致，或不是 Git 写入），没有执行任何 Git 命令。请重新调用；仍然失败时请告诉用户。".into(),
                });
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| BackendError::PreparationRejected {
                reason: "git.invalid_preparation: Aura 没能记录这次 Git 写入的准备结果，没有执行任何 Git 命令。请重新调用；仍然失败时请告诉用户。".into(),
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
                    reason: "git.preparation_changed: 准备之后仓库、Git 程序、相关配置或远端目标已经改变，这次 Git 写入没有执行，仓库没有改变。请先用 git.status 核对，再重新调用。".into(),
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
