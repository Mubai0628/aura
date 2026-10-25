//! `context.before_compact` and `context.after_compact`.
use super::*;

impl AgentApplication {
    /// `context.before_compact`: Hooks may only add what the summary must keep. The answer is
    /// kept per compaction, so a retried compaction asks for the same focus.
    pub(in crate::agent_application) async fn compaction_focus(
        &self,
        run_id: AgentRunId,
        compaction: &str,
        manual: bool,
    ) -> anyhow::Result<Vec<String>> {
        if !self.hooks_registered() || !self.hook_runner()?.hears(HookEvent::ContextBeforeCompact) {
            return Ok(Vec::new());
        }
        let subject = compaction.to_owned();
        let lookup = subject.clone();
        let kept = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.compaction_focus"), move |db| {
                hook_store::decision(db, &lookup, HookEvent::ContextBeforeCompact)
            })
            .await?;
        let decision = match kept {
            Some(kept) => kept,
            None => {
                let turn = self.turn(run_id).await?;
                let payload = serde_json::json!({"focus": [], "manual": manual});
                let outcome = self
                    .run_hooks(self.hook_call(HookEvent::ContextBeforeCompact, &turn, payload))
                    .await?;
                let decision = serde_json::json!({"focus": outcome.payload["focus"]});
                let kept = decision.clone();
                self.store
                    .write(
                        StoreStep::run(run_id, "agent.hooks.compaction_focus_kept"),
                        move |journal| {
                            journal.database.unit_of_work(|tx| {
                                hook_store::save_decision(
                                    tx,
                                    &subject,
                                    HookEvent::ContextBeforeCompact,
                                    &kept,
                                    UtcTimestamp::now(),
                                )
                            })
                        },
                    )
                    .await?;
                decision
            }
        };
        Ok(decision["focus"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|focus| focus.as_str().map(str::to_owned))
            .collect())
    }

    /// `context.after_compact`: a notification once the compaction is committed.
    pub(in crate::agent_application) async fn after_compaction(
        &self,
        run_id: AgentRunId,
        compaction: &str,
    ) -> anyhow::Result<()> {
        if !self.hooks_registered() || !self.hook_runner()?.hears(HookEvent::ContextAfterCompact) {
            return Ok(());
        }
        let turn = self.turn(run_id).await?;
        let payload = serde_json::json!({"compaction": compaction});
        self.run_hooks(self.hook_call(HookEvent::ContextAfterCompact, &turn, payload)).await?;
        Ok(())
    }
}
