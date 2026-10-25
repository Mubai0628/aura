use super::super::AgentExecution;
use crate::agent_store::{StoreAccess, StoreStep};
use anyhow::Result;
use aura_domain_common::ToolCallId;

impl AgentExecution {
    /// The actual file section owns its managed lifetime even after its caller's wait expires.
    pub(crate) async fn download_file_section<T, F>(
        &self,
        call: ToolCallId,
        step: StoreStep,
        work: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let lifetime = self.active_calls.retain_worker(call)?;
        self.store
            .run_critical(step, StoreAccess::Write, move || {
                let _lifetime = lifetime;
                work()
            })
            .await
    }
}
