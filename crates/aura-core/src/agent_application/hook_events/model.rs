//! `model.after_call`.
use super::*;

impl AgentApplication {
    /// `model.after_call`, for a step that led to tools: a Hook may change what the following
    /// steps see of this reply. The recorded reply itself never changes.
    pub(in crate::agent_application) async fn after_model_call(
        &self,
        run_id: AgentRunId,
        step: ModelStepId,
    ) -> anyhow::Result<()> {
        if !self.hooks_registered() || !self.hook_runner()?.hears(HookEvent::ModelAfterCall) {
            return Ok(());
        }
        let Some(text) = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.model_text"), move |db| {
                hook_store::step_text_awaiting_hooks(db, step)
            })
            .await?
        else {
            return Ok(());
        };
        let turn = self.turn(run_id).await?;
        let payload = serde_json::json!({"model_step_id": step, "text": text});
        let outcome =
            self.run_hooks(self.hook_call(HookEvent::ModelAfterCall, &turn, payload)).await?;
        let kept = serde_json::json!({
            "text": outcome.transformed.then(|| outcome.payload["text"].clone()),
        });
        let subject = step.to_string();
        self.store
            .write(StoreStep::run(run_id, "agent.hooks.model_text_decision"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    hook_store::save_decision(
                        tx,
                        &subject,
                        HookEvent::ModelAfterCall,
                        &kept,
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?;
        Ok(())
    }
}
