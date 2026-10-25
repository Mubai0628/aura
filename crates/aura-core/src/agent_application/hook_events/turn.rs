//! `session.started`, `session.resumed`, `turn.before_start` and `turn.completed`.
use super::*;

impl AgentApplication {
    /// `session.started` or `session.resumed` (once per session in this process), then
    /// `turn.before_start` (once per turn, decision kept). True when a Hook blocked the turn.
    pub(in crate::agent_application) async fn before_turn(
        &self,
        run_id: AgentRunId,
    ) -> anyhow::Result<bool> {
        if !self.hooks_registered() {
            return Ok(false);
        }
        let runner = self.hook_runner()?;
        let turn = self.turn(run_id).await?;
        let first_here =
            self.hook_sessions.lock().is_ok_and(|mut seen| seen.insert(turn.session_id));
        if first_here {
            let (session, current) = (turn.session_id, turn.turn_task_id);
            let resumed = self
                .store
                .read(StoreStep::run(run_id, "agent.hooks.session"), move |db| {
                    hook_store::session_had_earlier_turns(db, session, current)
                })
                .await?;
            let event = if resumed { HookEvent::SessionResumed } else { HookEvent::SessionStarted };
            if runner.hears(event) {
                self.run_hooks(self.hook_call(event, &turn, serde_json::json!({}))).await?;
            }
        }
        let subject = turn.turn_task_id.to_string();
        let lookup = subject.clone();
        let kept = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.turn_start"), move |db| {
                hook_store::decision(db, &lookup, HookEvent::TurnBeforeStart)
            })
            .await?;
        if let Some(kept) = kept {
            return Ok(kept["kind"] == "blocked");
        }
        if !runner.hears(HookEvent::TurnBeforeStart) {
            return Ok(false);
        }
        let payload = serde_json::json!({
            "mode": turn.mode,
            "turn_task_id": turn.turn_task_id,
        });
        let outcome =
            self.run_hooks(self.hook_call(HookEvent::TurnBeforeStart, &turn, payload)).await?;
        let decision = match &outcome.block {
            Some(block) => {
                serde_json::json!({"kind": "blocked", "hook": block.hook_name, "reason": block.reason})
            }
            None => serde_json::json!({"kind": "continue"}),
        };
        self.store
            .write(StoreStep::run(run_id, "agent.hooks.turn_start_decision"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    hook_store::save_decision(
                        tx,
                        &subject,
                        HookEvent::TurnBeforeStart,
                        &decision,
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?;
        Ok(outcome.block.is_some())
    }

    /// `turn.completed`, once per completed turn: a notification with the final report.
    pub(in crate::agent_application) async fn after_turn(
        &self,
        run_id: AgentRunId,
    ) -> anyhow::Result<()> {
        if !self.hooks_registered() {
            return Ok(());
        }
        let turn = self.turn(run_id).await?;
        let TurnTaskState::Completed { final_report } = &turn.state else { return Ok(()) };
        let runner = self.hook_runner()?;
        if !runner.hears(HookEvent::TurnCompleted) {
            return Ok(());
        }
        let subject = turn.turn_task_id.to_string();
        let lookup = subject.clone();
        let sent = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.turn_completed"), move |db| {
                hook_store::decision(db, &lookup, HookEvent::TurnCompleted)
            })
            .await?;
        if sent.is_some() {
            return Ok(());
        }
        // Kept first, so a restart never sends the same completion twice.
        self.store
            .write(StoreStep::run(run_id, "agent.hooks.turn_completed_sent"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    hook_store::save_decision(
                        tx,
                        &subject,
                        HookEvent::TurnCompleted,
                        &serde_json::json!({"kind": "sent"}),
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?;
        let payload = serde_json::json!({ "final_report": final_report });
        self.run_hooks(self.hook_call(HookEvent::TurnCompleted, &turn, payload)).await?;
        Ok(())
    }
}
