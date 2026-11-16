//! Settles a failed worker separately from model steps and tool execution.
use super::*;
use std::time::Duration;

pub(super) const SETTLEMENT_PENDING: &str = "agent.failure_settlement_pending";

impl AgentApplication {
    /// The stop and its cause are committed together. A write the caller can no longer wait
    /// for keeps its receipt; the caller does not launch another write in its place.
    pub async fn persist_drive_failure(
        &self,
        run: AgentRunId,
        generation: RunGeneration,
        error: &anyhow::Error,
        cancelled: bool,
    ) {
        let classified = fault::StepFault::of(error);
        let stored: String = classified.stored_text().chars().take(500).collect();
        let diagnostic = classified.structured();
        let mut payload = serde_json::json!({"error": stored, "stage": classified.stage()});
        if let Some(diagnostic) = &diagnostic {
            payload["diagnostic"] = diagnostic.clone();
        }
        // Only the code goes to the log. The error may include paths or content.
        append_core_log(
            "agent.run_failure",
            serde_json::json!({
                "agent_run_id": run,
                "code": stored.split([':', ' ']).next().unwrap_or_default(),
                "cancelled": cancelled,
            }),
        );

        for attempt in 0..2 {
            if self.supervisor.generation_of(run).is_some_and(|current| current != generation) {
                self.note_run_diagnostic(
                    run,
                    "recording",
                    "agent.stale_generation",
                    "a late failure from an older run generation was not recorded",
                );
                return;
            }
            match self
                .record_failure_once(run, payload.clone(), diagnostic.clone(), cancelled)
                .await
            {
                Ok(true) => {
                    self.store.diagnostics().clear(run);
                    return;
                }
                Ok(false) => {
                    self.note_run_diagnostic(
                        run,
                        "recording",
                        "agent.shutdown_left_for_restart",
                        "shutdown ended this generation's task without recording its failure",
                    );
                    return;
                }
                Err(error) => {
                    let code = if error.not_started() {
                        "agent.failure_record_not_started"
                    } else {
                        error.code()
                    };
                    self.note_run_diagnostic(run, "recording", code, error.to_string());
                    self.store.diagnostics().record(
                        run,
                        "recording",
                        SETTLEMENT_PENDING,
                        "the run's terminal state is not confirmed",
                    );
                    if attempt > 0 || matches!(error, StoreError::Unknown { .. }) {
                        return;
                    }
                    // Every edit to the DB rolled back, or the write did not start at all.
                    // Retry only this transaction, which checks the live run again.
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn record_failure_once(
        &self,
        run: AgentRunId,
        payload: serde_json::Value,
        diagnostic: Option<serde_json::Value>,
        cancelled: bool,
    ) -> Result<bool, StoreError> {
        let (recorded, _) = self
            .store
            .write_retained(StoreStep::run(run, "agent.drive_failure_record"), move |journal| {
                let at = UtcTimestamp::now();
                journal.database.unit_of_work(|tx| {
                    // A stop without a user's cancellation is Core shutdown. Startup recovery
                    // owns the stale run and any unresolved side effects.
                    if cancelled && !store::cancellation_requested(tx, run)? {
                        return Ok(false);
                    }
                    aura_persistence_core::event::append(
                        tx,
                        &aura_persistence_core::event::EventDraft {
                            id: EventId::new(),
                            owner: &run.to_string(),
                            kind: "agent.application_error",
                            version: 1,
                            payload: &payload,
                            at,
                        },
                        "agent",
                    )?;
                    if !can_settle(&turns::load_run(tx, run)?.state) {
                        return Ok(true);
                    }
                    store::interrupt_with_diagnostic(
                        tx,
                        run,
                        "agent.execution_interrupted",
                        cancelled,
                        at,
                        diagnostic.as_ref(),
                    )?;
                    Ok(true)
                })
            })
            .await?;
        Ok(recorded)
    }

    pub(super) fn project_pending_failure(&self, projection: &mut AgentSessionProjection) {
        let Some(active) =
            projection.active.as_ref().filter(|active| can_settle(&active.run.state))
        else {
            return;
        };
        let run = active.run.agent_run_id;
        if self.store.diagnostics().of(run).iter().any(|entry| entry.code == SETTLEMENT_PENDING)
            && let Some(turn) =
                projection.turns.iter_mut().find(|turn| turn.turn.agent_run_id == run)
        {
            turn.diagnostic = Some(SETTLEMENT_PENDING.into());
        }
    }
}

fn can_settle(state: &AgentRunState) -> bool {
    matches!(
        state,
        AgentRunState::PreparingContext
            | AgentRunState::ModelRunning { .. }
            | AgentRunState::ToolsRunning { .. }
    )
}
