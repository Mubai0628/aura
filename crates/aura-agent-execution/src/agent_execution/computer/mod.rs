//! Programs on the user's own desktop that the user allowed. Core resolves and authorizes each
//! call, sends Aura's desktop host the one command the call was frozen into, and records what
//! comes back. `computer.apps` reads the listing taken before admission and runs as task state.
mod desktop_shell;
mod invoke;
mod listing;
mod resolve;
mod routes;
mod routing;
mod safety;
#[cfg(test)]
mod tests;
mod text;

use super::*;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_core::BrokerPort;
use aura_desktop_domain::HostCommand;
use aura_domain_common::jcs::canonical_sha256;
pub(in crate::agent_execution) use invoke::invoke;
pub(in crate::agent_execution) use listing::apps_result;
pub use listing::prepare_before_admission;
pub(in crate::agent_execution) use resolve::admission_feedback;
pub use routes::dispatch as route;

pub(super) const BACKEND_ID: &str = "agent-computer-v1";

/// A call as Core sends it: one host command, the screenshot an act or an input takes afterwards
/// when the model asked for one, and an act's refs for its steps' elements.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frozen {
    #[serde(default, skip_serializing_if = "aura_desktop_domain::ComputerMode::is_foreground")]
    pub mode: aura_desktop_domain::ComputerMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<String>,
    pub command: HostCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<HostCommand>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

/// The backend a computer call runs on: Core's broker, carrying the desktop capabilities.
pub(super) struct ComputerBackend {
    descriptor: BackendDescriptor,
    port: broker::Port,
}

pub(super) fn backend(
    service: Arc<AgentExecution>,
    call: &CallContext,
) -> anyhow::Result<Arc<dyn ExecutionBackend>> {
    let mut descriptor = broker::descriptor(service.runtime);
    descriptor.id = BackendId(BACKEND_ID.into());
    descriptor.protocol_fingerprint = canonical_sha256(&"aura.agent-computer.v1")?;
    descriptor.capabilities =
        [ResolvedCapability::DesktopRead, ResolvedCapability::DesktopInput].into();
    Ok(Arc::new(ComputerBackend {
        descriptor,
        port: broker::Port { service, call: call.clone(), commands: None },
    }))
}

impl ExecutionBackend for ComputerBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        Box::pin(async move {
            if request.operation.validate().is_err()
                || !self.descriptor.supports_operation(&request.requirement, &request.operation)
            {
                return Err(preparation("computer.unsupported_operation"));
            }
            let facts = self.port.inspect(&request.operation).await?;
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: self.descriptor(),
                preparation: facts,
            })
            .map_err(|_| preparation("computer.invalid_preparation"))
        })
    }
    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        Box::pin(async move {
            let request = dispatch.request();
            if request.plan.validate().is_err()
                || request.plan.prepared.backend != self.descriptor
                || self.port.inspect(&request.operation).await? != request.plan.prepared.preparation
            {
                return Err(rejected("computer.preparation_changed"));
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
