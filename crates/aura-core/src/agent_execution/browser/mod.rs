//! The built-in browser's tools. Core resolves and authorizes each call, sends the browser
//! host (Electron Main) one command for the session's model page, and records what comes back.
//! The page itself, its input and its site policy live in the host.
mod bridge;
mod invoke;
mod resolve;
#[cfg(test)]
mod tests;

use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;
use aura_domain_common::jcs::canonical_sha256;
pub(crate) use bridge::BrowserBridge;
pub(in crate::agent_execution) use invoke::invoke;
pub(in crate::agent_execution) use resolve::admission_feedback;

pub(super) const BACKEND_ID: &str = "agent-browser-v1";

/// The backend a browser call runs on: Core's broker, carrying the browser capabilities.
pub(super) struct BrowserBackend {
    descriptor: BackendDescriptor,
    port: broker::Port,
}

pub(super) fn backend(
    service: Arc<AgentExecution>,
    call: &CallContext,
) -> anyhow::Result<Arc<dyn ExecutionBackend>> {
    let mut descriptor = broker::descriptor(service.runtime);
    descriptor.id = BackendId(BACKEND_ID.into());
    descriptor.protocol_fingerprint = canonical_sha256(&"aura.agent-browser.v1")?;
    descriptor.capabilities = [
        ResolvedCapability::BrowserRead,
        ResolvedCapability::BrowserInteraction,
        ResolvedCapability::BrowserSensitiveAction,
        ResolvedCapability::WorkspaceFileRead,
    ]
    .into();
    Ok(Arc::new(BrowserBackend {
        descriptor,
        port: broker::Port { service, call: call.clone(), commands: None },
    }))
}

impl ExecutionBackend for BrowserBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if request.operation.validate().is_err()
                || !self.descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(preparation("browser.unsupported_operation"));
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| preparation("browser.invalid_preparation"))
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request();
            if request.plan.validate().is_err()
                || request.plan.prepared.backend != self.descriptor
                || self.port.inspect(&request.operation).await? != request.plan.prepared.preparation
            {
                return Err(rejected("browser.preparation_changed"));
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
