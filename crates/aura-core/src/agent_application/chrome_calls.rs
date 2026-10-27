//! Before admission, the step's Chrome input actions are looked at in their tab, so admission
//! knows whether one is refused outright or asks the user every time.
use super::*;

impl AgentApplication {
    pub(super) async fn inspect_chrome_calls(
        &self,
        run_id: AgentRunId,
        step_id: ModelStepId,
    ) -> anyhow::Result<()> {
        if !self.execution.chrome.get().is_some_and(|chrome| chrome.connected()) {
            return Ok(());
        }
        let calls = self
            .store
            .read(StoreStep::run(run_id, "agent.chrome.calls"), move |db| {
                aura_persistence_v2::hooks::calls_before_admission(db, step_id)
            })
            .await?
            .into_iter()
            .filter(|call| call.tool_name.starts_with("chrome."))
            .collect::<Vec<_>>();
        if calls.is_empty() {
            return Ok(());
        }
        let session = self.turn(run_id).await?.session_id;
        crate::agent_execution::inspect_chrome_calls(&self.execution, session, calls).await;
        Ok(())
    }
}
