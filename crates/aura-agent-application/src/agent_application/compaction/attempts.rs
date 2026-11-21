use super::*;

impl CompactionAttemptFactory for Execution {
    fn prepare_attempt<'a>(
        &'a self,
        _: &'a ContextCompactionOperationSpec,
        ordinal: u8,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<PreparedCompactionModelAttempt, CompactionExecutionFailure>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let built: anyhow::Result<_> = async {
                anyhow::ensure!(!self.cancel.is_cancelled(), "context.compaction_cancelled");
                anyhow::ensure!(
                    dependencies::current(
                        &self.app,
                        &self.record.background_sources,
                        &self.source_access
                    )
                    .await,
                    "context.compaction_sources_changed"
                );
                let turn = self.app.turn_blocking(self.record.agent_run_id).await?;
                if turn.mode == AgentTaskMode::Goal {
                    // Boundary: the Goal budget guard is one bounded read; the section is
                    // entered from the compaction runtime's own awaitable callback.
                    let journal = Arc::clone(&self.app.execution.journal);
                    let turn_task_id = turn.turn_task_id;
                    let run_id = self.record.agent_run_id;
                    self.app
                        .store
                        .run_critical(
                            StoreStep::run(run_id, "agent.compaction.goal_budget"),
                            StoreAccess::Read,
                            move || {
                                anyhow::ensure!(
                                    !aura_persistence_v2::agent::work::goal(
                                        journal.lock()?.database.connection(),
                                        turn_task_id
                                    )?
                                    .budget_exhausted(),
                                    "agent.goal_budget_exhausted"
                                );
                                Ok(())
                            },
                        )
                        .await?;
                }
                let mut snapshot = self.template.clone();
                snapshot.id = ModelExecutionSnapshotId::new();
                snapshot.model_step_id = ModelStepId::new();
                snapshot.context_snapshot_id = ContextProjectionSnapshotId::new();
                snapshot.created_at = UtcTimestamp::now();
                self.prefix_attempt(&self.prefix, snapshot, ordinal)
            }
            .await;
            built.map_err(|error| CompactionExecutionFailure {
                code: error.to_string(),
                retryable: false,
                side_effect_unknown: false,
            })
        })
    }
}
