//! Continuing a turn a restart of Core cut off: by itself when Main says Core restarted while Aura
//! stayed open, or when the user presses 继续. Either way the turn that continues it says "继续",
//! as the user would type it, ahead of whatever is queued. Nothing the cut-off turn left
//! unfinished is replayed: its history already says which calls did not run and which results
//! are unknown, and the model reads that as it reads any history.
use super::*;
use aura_core_base::append_core_log;
use aura_core_broker::agent_store::StoreStep;
use aura_persistence_v2::agent::restart_continuation::{self as record, Decision};

/// What the continuing turn says, as the user would.
const CONTINUE: &str = "继续";

/// A "继续" for a cut-off turn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Continuation {
    after: TurnTaskId,
    /// Set when Aura continues by itself: why Core restarted, and when the new Core started.
    automatic: Option<(CoreRestartCause, UtcTimestamp)>,
}

impl Continuation {
    /// Inside the transaction that creates `turn`. Aura's own continuation is recorded, and the
    /// queue the restart paused follows it as it would have followed the cut-off turn; after the
    /// user's 继续 the queue stays paused until the user resumes it.
    pub(super) fn record(
        self,
        tx: &aura_persistence_core::Transaction<'_>,
        turn: &TurnTask,
        at: UtcTimestamp,
    ) -> aura_persistence_core::StoreResult<()> {
        let Some((cause, restarted_at)) = self.automatic else {
            return Ok(());
        };
        record::record(tx, turn.turn_task_id, self.after, cause, restarted_at, at)?;
        aura_persistence_v2::agent::inbox::resume(tx, turn.session_id, at)
    }
}

impl AgentApplication {
    /// Continues the turns this process's start cut off, once, after Main said Core restarted
    /// while Aura stayed open (`cause`). A task already continued twice in the last half hour, or
    /// cut off again within two minutes of being continued, is left for the user's 继续.
    pub async fn continue_after_restart(
        self: &Arc<Self>,
        cause: CoreRestartCause,
    ) -> Vec<(TurnTaskId, Decision)> {
        let interrupted = self
            .startup_interrupted
            .lock()
            .map(|mut turns| std::mem::take(&mut *turns))
            .unwrap_or_default();
        let restarted_at = UtcTimestamp::now();
        let mut decisions = Vec::with_capacity(interrupted.len());
        for cut in interrupted {
            let turn = cut.turn_task_id;
            let decided = self
                .store
                .read(StoreStep::operation("agent.continue.decide"), move |db| {
                    record::decide(db, turn, restarted_at)
                })
                .await;
            let decision = match decided {
                Ok(decision) => decision,
                Err(error) => {
                    note_continuation(turn, cause, "decide_failed", Some(&error.to_string()));
                    continue;
                }
            };
            let outcome = if decision == Decision::Continue {
                let automatic = Some((cause, restarted_at));
                match self
                    .start_continuation(CommandInvocationId::new(), cut.session_id, turn, automatic)
                    .await
                {
                    Ok(_) => "continued",
                    Err(error) => {
                        note_continuation(turn, cause, "start_failed", Some(&error.to_string()));
                        decisions.push((turn, decision));
                        continue;
                    }
                }
            } else {
                match decision {
                    Decision::LimitReached => "limit_reached",
                    Decision::RestartLoop => "restart_loop",
                    _ => "moved",
                }
            };
            note_continuation(turn, cause, outcome, None);
            decisions.push((turn, decision));
        }
        decisions
    }

    /// The user's 继续 for a turn a restart cut off. The queue stays paused.
    pub async fn continue_turn(
        self: &Arc<Self>,
        command: CommandInvocationId,
        request: AgentContinueRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        let turn = request.turn_task_id;
        let session = request.session_id;
        let current = self
            .store
            .read(StoreStep::operation("agent.continue.check"), move |db| {
                Ok(turns::load(db, turn)?.session_id == session && record::cut_off(db, turn)?)
            })
            .await?;
        anyhow::ensure!(
            current,
            "agent.continue_stale: 这一轮已经继续过或会话有了新的进展，请刷新后再看"
        );
        self.start_continuation(command, session, turn, None).await
    }

    /// Starts the "继续" turn with the session's model, or the cut-off run's when the session
    /// has none, and the thinking level the cut-off turn used.
    async fn start_continuation(
        self: &Arc<Self>,
        command: CommandInvocationId,
        session_id: SessionId,
        after: TurnTaskId,
        automatic: Option<(CoreRestartCause, UtcTimestamp)>,
    ) -> anyhow::Result<AgentRunProjection> {
        let configured = self
            .store
            .read(StoreStep::operation("agent.continue.configuration"), move |db| {
                store::configuration(db, turns::load(db, after)?.agent_run_id)
            })
            .await?;
        let model_profile_id = self
            .execution
            .persistence
            .session_model_binding(session_id)?
            .map_or(configured.request.model_profile_id, |binding| binding.model_profile_id);
        let request = AgentStartRequest {
            session_id,
            model_profile_id,
            text: CONTINUE.to_owned(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: configured.request.reasoning_effort,
            attachments: Vec::new(),
        };
        let continuation = Continuation { after, automatic };
        self.start_purpose(
            command,
            request,
            None,
            None,
            AgentTaskPurpose::Conversation,
            None,
            Some(continuation),
        )
        .await
    }
}

fn note_continuation(
    turn: TurnTaskId,
    cause: CoreRestartCause,
    outcome: &str,
    error: Option<&str>,
) {
    append_core_log(
        "auto_continue",
        serde_json::json!({
            "turn": turn.to_string(),
            "cause": cause,
            "outcome": outcome,
            "error": error.map(|text| text.chars().take(200).collect::<String>()),
        }),
    );
}
