//! The context-window rejection and trial steps, on the bounded Store owner.
//!
//! These steps sit on the run path, so their journal work goes through [`AgentStore`] like
//! every other step of a run: the caller's wait is bounded and the transaction keeps its own
//! lifetime.
use super::*;
use aura_core_broker::agent_store::StoreStep;
use aura_model_service::p05;
use aura_persistence_v2::model::context_window as windows;

/// The v1 registry has no `model.` namespace, so a Provider rejection for
/// context length is reported through the registered `provider.` family.
pub const CONTEXT_WINDOW_ISSUE_CODE: &str = "provider.context_window_exceeded";

impl AgentApplication {
    pub(super) async fn reject_context_window(
        &self,
        run_id: AgentRunId,
        snapshot: &ModelExecutionSnapshot,
        profile: &ModelProfileConfig,
        failure: &PersistedModelFailure,
    ) -> anyhow::Result<()> {
        let snapshot = snapshot.clone();
        let profile = profile.clone();
        let failure = failure.clone();
        let summary = self
            .store
            .read(StoreStep::run(run_id, "agent.window.provider_error"), {
                let snapshot = snapshot.clone();
                let failure = failure.clone();
                move |db| {
                    let body = failure
                        .failure
                        .provider_error_ref
                        .map(|id| {
                            aura_persistence_core::artifact::read(
                                db,
                                id,
                                &snapshot.model_step_id.to_string(),
                            )
                        })
                        .transpose()?;
                    Ok(body)
                }
            })
            .await?;
        let summary = p05::provider_error::summarize_provider_error(
            NormalizedIssueCode::new(CONTEXT_WINDOW_ISSUE_CODE)?,
            failure.failure.provider_status,
            summary.as_deref(),
        );
        self.store
            .write(StoreStep::run(run_id, "agent.window.reject"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    windows::reject(tx, &snapshot, &profile, &failure, summary, UtcTimestamp::now())
                })
            })
            .await?;
        Ok(())
    }

    pub(super) async fn complete_window_trial(
        &self,
        run_id: AgentRunId,
        snapshot: &ModelExecutionSnapshot,
    ) -> anyhow::Result<()> {
        let model_step_id = snapshot.model_step_id;
        let record = self
            .store
            .read(StoreStep::run(run_id, "agent.window.trial"), move |db| {
                let step = turns::step(db, model_step_id)?;
                if !matches!(step.state, ModelStepState::Completed { .. }) {
                    return Err(aura_persistence_core::PersistenceCoreError::Invalid(
                        "model.window_trial_not_completed",
                    ));
                }
                windows::trial(db, model_step_id)
            })
            .await?;
        let Some(record) = record else {
            return Ok(());
        };
        let resolution = if record.candidate_windows.is_empty() {
            None
        } else {
            self.p05.commit_window_trial(&record)?
        };
        let correction_run = record.view.correction.agent_run_id;
        self.store
            .write(StoreStep::run(run_id, "agent.window.finish"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    windows::finish(tx, correction_run, resolution, false, UtcTimestamp::now())
                })
            })
            .await?;
        Ok(())
    }

    pub async fn resolve_context_window(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentContextWindowResolveRequest,
    ) -> anyhow::Result<aura_context_domain::AgentContextRemediationView> {
        let deadline = Self::stop_deadline();
        let scope =
            AgentRunRequest { session_id: input.session_id, agent_run_id: input.agent_run_id };
        self.context_remediation(scope.clone()).await?;
        let resolved_input = input.clone();
        let resolved = self
            .store
            .read(StoreStep::run(input.agent_run_id, "agent.window.resolved"), move |db| {
                windows::resolved(db, command, &resolved_input)
            })
            .await?;
        if resolved {
            return self.context_remediation(scope).await;
        }
        self.idle_within(input.agent_run_id, deadline).await?;
        let stored = input.clone();
        self.store
            .write(StoreStep::run(input.agent_run_id, "agent.window.resolve"), move |journal| {
                journal
                    .database
                    .unit_of_work(|tx| windows::resolve(tx, command, &stored, UtcTimestamp::now()))
            })
            .await?;
        let run_id = input.agent_run_id;
        let run = self
            .store
            .read(StoreStep::run(run_id, "agent.window.run_state"), move |db| {
                turns::load_run(db, run_id)
            })
            .await?;
        if run.state == AgentRunState::PreparingContext {
            self.launch(input.agent_run_id)?;
        }
        self.context_remediation(scope).await
    }
}
