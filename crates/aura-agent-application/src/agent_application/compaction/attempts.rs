//! The attempts of one compaction: the first on the request the step would have sent when it has
//! room for the summary, the isolated request otherwise, and after a failed one what the
//! [`ladder`](super::ladder) chooses.
use super::ladder::{Failure, Rung};
use super::*;
use aura_context_runtime::UnusableAnswer;

/// Why an answer that could not be used is asked again, said at the end of the request sent again.
fn unusable_note(why: UnusableAnswer) -> &'static str {
    match why {
        UnusableAnswer::ToolCalls => {
            "Your previous answer called tools instead of giving the summary, so it could not be used; no tool runs during a compaction request. Write the summary now as one paragraph between <summary> and </summary>, within the length given above, and call no tool."
        }
        UnusableAnswer::Other => {
            "Your previous answer could not be used as the summary: it was cut off, refused, empty, longer than the length given above, or had no complete summary between <summary> and </summary>. Write the summary now as one paragraph between <summary> and </summary>, within that length, and call no tool."
        }
    }
}

/// The closing question of a request asked again after an answer that could not be used: what
/// it asked, then why the answer could not be used. Everything before it is the request already
/// sent, so the provider's cache still serves it.
fn noted(ask: Option<String>, why: UnusableAnswer) -> String {
    let note = unusable_note(why);
    match ask {
        Some(ask) if ask.ends_with(note) => ask,
        Some(ask) => format!("{ask}\n\n{note}"),
        None => note.to_owned(),
    }
}

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
                if let Some((estimated_input_tokens, safety_reserve_tokens)) =
                    self.isolated_estimate
                {
                    append_core_log(
                        "context.compaction.isolated_budget",
                        serde_json::json!({
                            "compaction": self.record.id,
                            "attempt_ordinal": ordinal,
                            "estimated_input_tokens": estimated_input_tokens,
                            "safety_reserve_tokens": safety_reserve_tokens,
                        }),
                    );
                }
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
        let mut failed = failed.clone();
        if let (Rung::Resend, FailedCompactionAttempt::UnusableAnswer { why }) = (rung, how) {
            let request = &mut failed.logical_request;
            request.closing_question = Some(noted(request.closing_question.take(), why));
        }
        attempts.next = Some((rung, Box::new(failed)));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_asked_again_is_told_why_after_what_was_asked() {
        let ask = "Summarize the history above.".to_owned();
        let once = noted(Some(ask.clone()), UnusableAnswer::ToolCalls);
        assert!(once.starts_with(&ask) && once.contains("called tools instead"), "{once}");
        assert_eq!(noted(Some(once.clone()), UnusableAnswer::ToolCalls), once, "said once");
        let isolated = noted(None, UnusableAnswer::Other);
        assert!(isolated.starts_with("Your previous answer could not be used"), "{isolated}");
        assert!(!isolated.contains('\n') && !isolated.ends_with('\n'));
    }
}
