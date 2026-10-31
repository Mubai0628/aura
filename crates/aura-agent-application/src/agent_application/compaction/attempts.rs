//! The attempts of one compaction: the first on the request the step would have sent when it has
//! room for the summary, the isolated request otherwise, and after a failed one what the
//! [`ladder`](super::ladder) chooses.
use super::ladder::{Failure, Rung};
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
                let next = self
                    .attempts
                    .lock()
                    .map_err(|_| anyhow::anyhow!("context.compaction_attempts_poisoned"))?
                    .next
                    .take();
                match next {
                    Some((Rung::Resend, failed)) => {
                        return self.ask_again(&failed, ordinal, None, None);
                    }
                    Some((Rung::AnswerTools, failed)) => {
                        return self.answer_tools(&failed, ordinal).await;
                    }
                    Some((Rung::Isolated, _)) => {}
                    None => {
                        if let Some(prefix) = &self.prefix {
                            let mut snapshot = self.template.clone();
                            snapshot.id = ModelExecutionSnapshotId::new();
                            snapshot.model_step_id = ModelStepId::new();
                            snapshot.context_snapshot_id = ContextProjectionSnapshotId::new();
                            snapshot.created_at = UtcTimestamp::now();
                            return self.prefix_attempt(prefix, snapshot, ordinal);
                        }
                    }
                }
                let mut snapshot = self.template.clone();
                snapshot.id = ModelExecutionSnapshotId::new();
                snapshot.model_step_id = ModelStepId::new();
                snapshot.context_snapshot_id = ContextProjectionSnapshotId::new();
                snapshot.created_at = UtcTimestamp::now();
                snapshot.historical_tool_call_wire_bindings.clear();
                snapshot.request_options.effective_max_output_tokens = self.max_output;
                let logical = LogicalModelRequest {
                    fixed_tool_ids: vec![],
                    model_step_id: snapshot.model_step_id,
                    execution_snapshot_id: snapshot.id,
                    context_snapshot_id: snapshot.context_snapshot_id,
                    instruction_envelope: self.instructions.clone(),
                    conversation_items: vec![],
                    tool_schema_set: aura_tool_contract::ToolSchemaSetSnapshotV1::new(
                        self.registry.clone(),
                        vec![],
                    )?,
                    attachments: vec![],
                    provider_continuation: None,
                    output_request: ModelOutputRequest {
                        format: ModelOutputFormat::Text,
                        effective_max_output_tokens: self.max_output,
                    },
                    closing_question: None,
                    closing_exchange: Vec::new(),
                };
                let manifest = build_model_egress_manifest(
                    snapshot.context_snapshot_id,
                    vec![ModelEgressSourceDraft {
                        source_ref: ContextSourceRef::Compaction {
                            context_snapshot_id: self.record.id,
                        },
                        source_kind: ModelEgressSourceKind::Compaction,
                        data_class: aura_security_contract::NetworkDataClass::WorkspaceSource,
                        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                        content_hash: self.instructions.instruction_hash.clone(),
                        byte_length: self.instructions.canonical_text.len() as u64,
                    }],
                )?;
                Ok(PreparedCompactionModelAttempt {
                    attempt_ordinal: ordinal,
                    logical_request: logical,
                    execution_snapshot: snapshot,
                    egress_manifest: manifest,
                    shape: CompactionRequestShape::Isolated,
                })
            }
            .await;
            built.map_err(|e| CompactionExecutionFailure {
                code: e.to_string(),
                retryable: false,
                side_effect_unknown: false,
            })
        })
    }

    fn asks_again(
        &self,
        failed: &PreparedCompactionModelAttempt,
        how: FailedCompactionAttempt,
    ) -> bool {
        let on_prefix = failed.shape == CompactionRequestShape::SessionPrefix;
        let Ok(mut attempts) = self.attempts.lock() else {
            return false;
        };
        let failure = Failure { how, on_prefix };
        let Some(rung) = attempts.ladder.next(failure, self.isolated, self.manual) else {
            return false;
        };
        attempts.next = Some((rung, Box::new(failed.clone())));
        true
    }
}
