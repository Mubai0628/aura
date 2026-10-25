//! Live managed jobs remain addressable after the foreground Tool has returned.
use crate::controller::{
    ActiveMap, PortSection, ScopeFacts, WindowsUserCorePort, WindowsUserCorePortSection,
};
use crate::{NativeApprovedBackend, WindowsUserBackend};
use anyhow::Result;
use aura_domain_common::{SecurityOperationId, SessionId};
use aura_execution_contract::DispatchRequest;
use std::sync::Arc;

fn operations(active: &ActiveMap, session: SessionId) -> Result<Vec<SecurityOperationId>> {
    Ok(active
        .lock()
        .map_err(|_| anyhow::anyhow!("runner.active_lock"))?
        .values()
        .filter(|entry| entry.session == session)
        .map(|entry| entry.handle.attempt.operation_id)
        .collect())
}

impl WindowsUserBackend {
    pub fn managed_operations(&self, session: SessionId) -> Result<Vec<SecurityOperationId>> {
        operations(&self.active, session)
    }
}

impl NativeApprovedBackend {
    pub fn managed_operations(&self, session: SessionId) -> Result<Vec<SecurityOperationId>> {
        operations(&self.active, session)
    }
}

pub(crate) async fn prepared_scope(
    port: Arc<dyn WindowsUserCorePort>,
    request: &DispatchRequest,
    name: &'static str,
) -> Result<ScopeFacts> {
    let owned = request.clone();
    let reader = port.clone();
    port.section(
        PortSection::read(name, request.operation.content.identity.agent_run_id),
        Box::new(move || reader.prepared_scope(&owned)),
    )
    .await
}
