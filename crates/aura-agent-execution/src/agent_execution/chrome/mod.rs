//! The user's own Chrome, in tabs shared with the conversation. Core resolves and authorizes each
//! call, sends Aura's extension one command for the tab through Chrome control, and records what
//! comes back. `chrome.tabs` reads Core's own record of the shares and runs as task state.
mod invoke;
mod resolve;
#[cfg(test)]
mod tests;

use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;
use aura_domain_common::jcs::canonical_sha256;
pub(in crate::agent_execution) use invoke::invoke;
pub use resolve::inspect_before_admission;
pub(in crate::agent_execution) use resolve::{admission_feedback, tabs_result};

pub(super) const BACKEND_ID: &str = "agent-chrome-v1";

/// The backend a Chrome call runs on: Core's broker, carrying the Chrome capabilities.
pub(super) struct ChromeBackend {
    descriptor: BackendDescriptor,
    port: broker::Port,
}

pub(super) fn backend(
    service: Arc<AgentExecution>,
    call: &CallContext,
) -> anyhow::Result<Arc<dyn ExecutionBackend>> {
    let mut descriptor = broker::descriptor(service.runtime);
    descriptor.id = BackendId(BACKEND_ID.into());
    descriptor.protocol_fingerprint = canonical_sha256(&"aura.agent-chrome.v1")?;
    descriptor.capabilities = [
        ResolvedCapability::ChromeRead,
        ResolvedCapability::ChromeInteraction,
        ResolvedCapability::ChromeSensitiveAction,
    ]
    .into();
    Ok(Arc::new(ChromeBackend {
        descriptor,
        port: broker::Port { service, call: call.clone(), commands: None },
    }))
}

impl ExecutionBackend for ChromeBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if request.operation.validate().is_err()
                || !self.descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(preparation(
                    "chrome.unsupported_operation: 这次调用要的能力超出了 Chrome 控制能做的范围，没有开始，什么都没改动。",
                ));
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| {
                preparation(
                    "chrome.invalid_preparation: Aura 没能为这次 Chrome 调用做好准备，没有开始，什么都没改动；可以再调用一次。",
                )
            })
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request();
            if request.plan.validate().is_err()
                || request.plan.prepared.backend != self.descriptor
                || self.port.inspect(&request.operation).await? != request.plan.prepared.preparation
            {
                return Err(rejected(
                    "chrome.preparation_changed: 这次 Chrome 调用准备好之后、开始之前，会话的权限或运行状态变了，所以没有开始，什么都没改动；还需要的话再调用一次。",
                ));
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
