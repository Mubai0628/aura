//! Open a model step once and rebuild its draft when inputs change before commit.
use super::*;

pub(in crate::agent_application) struct Prepared {
    pub request: provider::Request,
    pub profile: ModelProfileConfig,
}

pub(super) struct StepPreparation {
    pub step: ModelStep,
    pub session_id: SessionId,
    pub profile: ModelProfileId,
    pub reasoning: Option<aura_agent_domain::TurnReasoningEffort>,
    pub speed_mode: Option<aura_agent_domain::TurnSpeedMode>,
}

pub(super) enum PreparedStep {
    Ready(Box<Prepared>),
    Waiting,
    DraftChanged,
}

impl AgentApplication {
    pub(in crate::agent_application) async fn prepare(
        self: &Arc<Self>,
        run_id: AgentRunId,
        profile: ModelProfileId,
        reasoning: Option<aura_agent_domain::TurnReasoningEffort>,
        speed_mode: Option<aura_agent_domain::TurnSpeedMode>,
        continuation: AgentContinuation,
        cancel: &ModelCancellationToken,
    ) -> anyhow::Result<Option<Prepared>> {
        self.refresh_prompt(run_id).await?;
        let session_id = self.turn(run_id).await?.session_id;
        self.wait_for_branch_copy(run_id, session_id, cancel).await?;
        let step = self
            .store
            .write(StoreStep::run(run_id, "agent.context.prepare_step"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    aura_persistence_v2::agent::lifecycle::prepare_step(
                        tx,
                        run_id,
                        continuation,
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?;
        let draft = StepPreparation { step, session_id, profile, reasoning, speed_mode };
        loop {
            anyhow::ensure!(!cancel.is_cancelled(), "agent.context_cancelled");
            match Box::pin(self.prepare_existing_step(&draft, cancel)).await? {
                PreparedStep::Ready(prepared) => return Ok(Some(*prepared)),
                PreparedStep::Waiting => return Ok(None),
                PreparedStep::DraftChanged => {}
            }
        }
    }
}
