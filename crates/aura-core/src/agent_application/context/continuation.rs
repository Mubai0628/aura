use super::*;

impl AgentApplication {
    pub(super) async fn continuation_facts(
        self: &Arc<Self>,
        run_id: AgentRunId,
        step: &ModelStep,
        turn: &TurnTask,
        history: &model_context::ModelHistory,
        sections: &mut Vec<ContextSectionInput>,
    ) -> anyhow::Result<(
        ContextPreparationTriggerV1,
        Option<ToolContinuationFactsV1>,
        Option<CoreContinuationFactsV1>,
    )> {
        Ok(match &step.continuation {
            AgentContinuation::Initial { user_event_id }
            | AgentContinuation::QueryAnswered { user_event_id, .. } => {
                let users = {
                    // Boundary: one bounded read of the triggering user events.
                    let journal = Arc::clone(&self.execution.journal);
                    let turn_task_id = turn.turn_task_id;
                    let session_id = turn.session_id;
                    let initial = matches!(step.continuation, AgentContinuation::Initial { .. });
                    let user_event_id = *user_event_id;
                    self.store
                        .run_critical(
                            StoreStep::run(run_id, "agent.context.user_events"),
                            StoreAccess::Read,
                            move || {
                                let journal = journal.lock()?;
                                let db = journal.database.connection();
                                let mut ids = vec![user_event_id];
                                if initial {
                                    ids.extend(
                                        aura_persistence_v2::agent::inbox::consumed(
                                            db,
                                            turn_task_id,
                                        )?
                                        .into_iter()
                                        .flat_map(|g| g.user_event_ids)
                                        .filter(|id| *id != user_event_id),
                                    );
                                }
                                Ok(ids
                                    .into_iter()
                                    .map(|id| session::user_message(db, session_id, id))
                                    .collect::<Result<Vec<_>, _>>()?)
                            },
                        )
                        .await?
                };
                let ids = users.iter().map(|u| u.event_id).collect::<Vec<_>>();
                sections.push(section(
                    ContextSectionKind::CurrentTurnWorkingSet,
                    users
                        .into_iter()
                        .map(|user| ContextFragment {
                            source_ref: ContextSourceRef::Event { event_id: user.event_id },
                            source_kind: ModelEgressSourceKind::UserEvent,
                            data_class: NetworkDataClass::UserFileContent,
                            authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
                            content: user.text,
                        })
                        .collect(),
                    ContextCompressibility::NeverCompress,
                ));
                (
                    if ids.len() == 1 {
                        ContextPreparationTriggerV1::UserSubmission {
                            user_event_id: *user_event_id,
                        }
                    } else {
                        ContextPreparationTriggerV1::UserBatch { user_event_ids: ids }
                    },
                    None,
                    None,
                )
            }
            AgentContinuation::ToolBatch { tool_batch_id, history_group_hash } => {
                let group = history
                    .groups
                    .iter()
                    .find(|g| g.tool_batch_id == *tool_batch_id)
                    .ok_or_else(|| anyhow::anyhow!("agent.history_group_missing"))?
                    .clone();
                anyhow::ensure!(
                    group.group_ref()?.group_hash == *history_group_hash,
                    "agent.history_group_changed"
                );
                let accepted_additions = {
                    // Boundary: one bounded read that binds the accepted additions to the
                    // same journal state the anchor was computed against.
                    let journal = Arc::clone(&self.execution.journal);
                    let turn = turn.clone();
                    let items = history.items.clone();
                    let assistant_event_id = group.assistant_response_event_id;
                    let calls = group.calls.len();
                    self.store
                        .run_critical(
                            StoreStep::run(run_id, "agent.context.accepted_additions"),
                            StoreAccess::Read,
                            move || {
                                let journal = journal.lock()?;
                                let db = journal.database.connection();
                                addition_suffix(
                                    db,
                                    &turn,
                                    &items,
                                    history_anchor(&items, assistant_event_id, calls)?,
                                )
                            },
                        )
                        .await?
                };
                (
                    ContextPreparationTriggerV1::ToolContinuation {
                        originating_user_event_id: turn.root_user_event_id,
                        previous_model_step_id: group.model_step_id,
                        assistant_response_event_id: group.assistant_response_event_id,
                        tool_batch_id: *tool_batch_id,
                        history_atomic_group_ref: group.group_ref()?,
                        provider_continuation: None,
                    },
                    Some(ToolContinuationFactsV1 {
                        previous_model_step_legally_terminal: true,
                        assistant_response_durable: true,
                        tool_batch_terminal: true,
                        durable_result_tool_call_ids: group
                            .calls
                            .iter()
                            .map(|c| c.tool_call_id)
                            .collect(),
                        history_atomic_group: group,
                        verified_provider_continuation: None,
                        accepted_additions,
                    }),
                    None,
                )
            }
            AgentContinuation::CompletionReminder { previous_model_step_id } => {
                // Boundary: the reminder arm's three journal reads were three direct locks on
                // the reactive task; each is now one bounded section over the same statements.
                let previous_model_step_id = *previous_model_step_id;
                let old = {
                    let journal = Arc::clone(&self.execution.journal);
                    self.store
                        .run_critical(
                            StoreStep::run(run_id, "agent.context.previous_step"),
                            StoreAccess::Read,
                            move || {
                                Ok(turns::step(
                                    journal.lock()?.database.connection(),
                                    previous_model_step_id,
                                )?)
                            },
                        )
                        .await?
                };
                let ModelStepState::Completed { assistant_event_id, tool_batch_id: None, .. } =
                    old.state
                else {
                    anyhow::bail!("agent.continuation_invalid");
                };
                let hash = jcs::canonical_sha256(&old)?;
                // A response with no visible text and no Tool call is absent from
                // the model-visible history, so the continuation must not expect
                // an item that was never emitted.
                let output = {
                    let journal = Arc::clone(&self.execution.journal);
                    self.store
                        .run_critical(
                            StoreStep::run(run_id, "agent.context.previous_output"),
                            StoreAccess::Read,
                            move || {
                                Ok(aura_persistence_v2::agent::history::output(
                                    journal.lock()?.database.connection(),
                                    previous_model_step_id,
                                )?)
                            },
                        )
                        .await?
                };
                let previous_response_visible =
                    !aura_persistence_v2::agent::history::visible_text(&output).is_empty()
                        || !output.tool_calls.is_empty();
                let accepted_additions = {
                    let journal = Arc::clone(&self.execution.journal);
                    let turn = turn.clone();
                    let items = history.items.clone();
                    self.store
                        .run_critical(
                            StoreStep::run(run_id, "agent.context.continuation_additions"),
                            StoreAccess::Read,
                            move || {
                                let journal = journal.lock()?;
                                let db = journal.database.connection();
                                let after = if previous_response_visible {
                                    history_anchor(&items, assistant_event_id, 0)?
                                } else {
                                    trailing_addition_index(
                                        &items,
                                        &consumed_addition_orders(db, &turn)?,
                                    )
                                };
                                addition_suffix(db, &turn, &items, after)
                            },
                        )
                        .await?
                };
                (
                    ContextPreparationTriggerV1::CoreContinuation {
                        originating_user_event_id: turn.root_user_event_id,
                        previous_model_step_id,
                        assistant_response_event_id: assistant_event_id,
                        checkpoint_hash: hash.clone(),
                    },
                    None,
                    Some(CoreContinuationFactsV1 {
                        session_id: turn.session_id,
                        turn_task_id: turn.turn_task_id,
                        agent_run_id: run_id,
                        previous_model_step_id,
                        assistant_response_event_id: assistant_event_id,
                        checkpoint_hash: hash,
                        previous_response_visible,
                        accepted_additions,
                    }),
                )
            }
            _ => anyhow::bail!("agent.continuation_requires_checkpoint"),
        })
    }
}
