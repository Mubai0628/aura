//! `turn.before_complete`: a held answer, allowed or sent back with guidance.
use super::*;

/// How many times one turn's answer may be sent back by `turn.before_complete` before the
/// turn stops instead of looping.
const MAX_COMPLETION_BLOCKS: usize = 3;
const DELIVERY_REVIEW: &str = "aura.exact_artifact";

/// What `turn.before_complete` decided about a held answer.
pub(in crate::agent_application) enum CompletionReview {
    /// The turn ended with the held answer.
    Completed,
    /// The Hook kept sending it back; the turn stops with its reason on record.
    Stop,
    ArtifactUnverified,
}

impl AgentApplication {
    /// Whether an answer that would end the turn must first be offered to Hooks.
    pub(in crate::agent_application) fn hold_completion(&self) -> bool {
        self.hooks_registered()
            && self.hook_runner().is_ok_and(|runner| runner.hears(HookEvent::TurnBeforeComplete))
    }

    /// `turn.before_complete` for a held answer: allowed, the turn ends; blocked, the turn goes
    /// on with the reason as guidance, at most `MAX_COMPLETION_BLOCKS` times.
    pub(in crate::agent_application) async fn review_completion(
        &self,
        run_id: AgentRunId,
    ) -> anyhow::Result<Option<CompletionReview>> {
        let held = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.held_completion"), move |db| {
                hook_store::held_completion(db, run_id)
            })
            .await?;
        let Some((step, answer)) = held else { return Ok(None) };
        let turn = self.turn(run_id).await?;
        let turn_id = turn.turn_task_id;
        let (earlier, requirements) = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.completion_blocks"), move |db| {
                Ok((
                    hook_store::completion_blocks(db, turn_id)?,
                    aura_persistence_v2::agent::exact_artifacts::requirements(
                        db,
                        &aura_persistence_v2::agent::turns::load(db, turn_id)?,
                    )?,
                ))
            })
            .await?;
        let mismatch =
            self.execution.unverified_delivery_value(turn.workspace_id, requirements).await?;
        let delivery_stop =
            mismatch.is_some() && earlier.iter().any(|(gate, _)| gate == DELIVERY_REVIEW);
        let user_blocks = earlier.iter().filter(|(gate, _)| gate != DELIVERY_REVIEW).count();
        let settled = if let Some(value) = mismatch {
            Some((
                DELIVERY_REVIEW.to_owned(),
                format!(
                    "交付文件「{}」未核对到用户明确要求逐字保留的内容。{}",
                    value.path.as_str(),
                    value.formatting_hint()
                ),
            ))
        } else if self.hooks_registered()
            && self.hook_runner()?.hears(HookEvent::TurnBeforeComplete)
        {
            let payload = serde_json::json!({"final_report": answer, "blocks_so_far": user_blocks});
            self.run_hooks(self.hook_call(HookEvent::TurnBeforeComplete, &turn, payload))
                .await?
                .block
                .map(|block| (block.hook_name, block.reason))
        } else {
            None
        };
        let stop = settled.as_ref().is_some_and(|(gate, _)| gate != DELIVERY_REVIEW)
            && user_blocks + 1 >= MAX_COMPLETION_BLOCKS;
        let completed = self
            .store
            .write(StoreStep::run(run_id, "agent.hooks.completion_settled"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    hook_store::resolve_completion(
                        tx,
                        step,
                        settled.as_ref().map(|(hook, reason)| (hook.as_str(), reason.as_str())),
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?;
        Ok(if delivery_stop {
            Some(CompletionReview::ArtifactUnverified)
        } else if stop {
            Some(CompletionReview::Stop)
        } else if completed {
            Some(CompletionReview::Completed)
        } else {
            None
        })
    }

    /// The newest `turn.before_complete` block of this turn, as guidance for the next step.
    pub(in crate::agent_application) async fn completion_guidance(
        &self,
        run_id: AgentRunId,
        turn: TurnTaskId,
    ) -> anyhow::Result<Option<String>> {
        let blocks = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.completion_guidance"), move |db| {
                hook_store::completion_blocks(db, turn)
            })
            .await?;
        Ok(blocks.last().map(|(hook, reason)| {
            if hook == DELIVERY_REVIEW { return reason.clone(); }
            format!(
                "From Hook {hook} (turn.before_complete): the user's Hook did not accept the answer as complete: {reason}\nContinue the task and address this before you finish."
            )
        }))
    }
}
