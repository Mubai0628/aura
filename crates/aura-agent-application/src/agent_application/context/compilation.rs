use super::*;
use crate::agent_application::compaction::{BackgroundUse, Compacted, CompactionRequest};
use aura_core_base::append_core_log;

pub(super) struct CompilationInput<'a> {
    pub run_id: AgentRunId,
    pub turn: &'a TurnTask,
    pub step: &'a ModelStep,
    pub state: serde_json::Value,
    pub input: ContextPreparationInput,
    pub snapshot: &'a ModelExecutionSnapshot,
    pub closing_question: Option<&'a str>,
    pub endpoint: &'a ModelEndpointConfig,
    pub window_trial: &'a Option<model::context_window::WindowCorrection>,
    pub force_rebuild: bool,
    pub history_read_pressure: bool,
    pub selected_history_bytes: u64,
    pub missing_adopted: std::collections::BTreeSet<EventId>,
    pub cancel: &'a ModelCancellationToken,
    pub estimators: ContextEstimatorRegistry,
    /// The tools the step always loads, which a compaction on its prefix sends too.
    pub fixed_tool_ids: &'a [aura_domain_common::ToolId],
    pub native_prefixes: &'a std::collections::BTreeMap<EventId, ContentHash>,
    /// The screenshots the request may carry, attached anew on each pass.
    pub screenshots: &'a super::preparation::screenshots::Screenshots,
    pub memory_available: bool,
    pub source_access: super::compaction::dependencies::Access,
    pub background: BackgroundUse,
}

pub(super) struct CompiledStep {
    pub prepared: Box<PreparedContextDraft>,
    pub state: serde_json::Value,
    pub referenced_results: Vec<(ToolCallId, String)>,
}

impl AgentApplication {
    pub(super) async fn compile_context(
        self: &Arc<Self>,
        pass: CompilationInput<'_>,
    ) -> anyhow::Result<Option<CompiledStep>> {
        let CompilationInput {
            run_id,
            turn,
            step,
            mut state,
            mut input,
            snapshot,
            closing_question,
            endpoint,
            window_trial,
            force_rebuild,
            mut history_read_pressure,
            mut selected_history_bytes,
            mut missing_adopted,
            cancel,
            estimators,
            fixed_tool_ids,
            native_prefixes,
            screenshots,
            memory_available,
            source_access,
            background,
        } = pass;
        let mut attempted = false;
        let mut deferred_images = std::collections::BTreeSet::new();
        // What this preparation gave back, for the line the conversation shows -
        // the request's estimate when it first did not fit, results cleared, history compacted.
        let mut reduction: Option<PromptInspectorReductionView> = None;
        let mut referenced_results = Vec::new();
        let mut prepared = loop {
            // Apply recall after the effective history range is known, including each new
            // compaction pass. Visible sources remain in their original messages.
            self.refresh_recalled_sources(turn, &mut input, memory_available).await;
            // Only the screenshots whose results this pass still carries as they were.
            let unread_pictures = screenshots.attach(&mut input, &deferred_images);
            // Boundary: the manual-compaction request read, one bounded section per pass.
            let record = {
                let journal = Arc::clone(&self.execution.journal);
                self.store
                    .run_critical(
                        StoreStep::run(run_id, "agent.context.manual_compaction"),
                        StoreAccess::Read,
                        move || {
                            Ok(aura_persistence_v2::context::manual_compaction::record(
                                journal.lock()?.database.connection(),
                                run_id,
                            )?)
                        },
                    )
                    .await?
            };
            if let Some(record) = record.as_ref().filter(|record| {
                record.state == aura_context_domain::ManualCompactionRequestState::Completed
            }) {
                let journal = Arc::clone(&self.execution.journal);
                let id = step.model_step_id;
                let finished = self
                    .store
                    .run_critical(
                        StoreStep::run(run_id, "agent.compaction.finish_command"),
                        StoreAccess::Write,
                        move || {
                            Ok(journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_v2::agent::completion::finish_compaction_command(
                                    tx,
                                    id,
                                    UtcTimestamp::now(),
                                )
                            })?)
                        },
                    )
                    .await?;
                if finished {
                    return Ok(None);
                }
                state["manual_compaction_result"] = if record.source_context_snapshot_id.is_some() {
                    "compacted".into()
                } else {
                    "nothing_eligible".into()
                };
            }
            let manual = record.filter(|record| {
                record.state == aura_context_domain::ManualCompactionRequestState::Requested
            });
            // The state as of this pass: a compaction or a clearing may have taken the result
            // that showed the to-do.
            write_control_state(&mut input, &state, &unread_pictures)?;
            // `over` is the request that does not fit, with its reasons; `None` when it fits.
            // `prefix` is what that request is made of, for a compaction asked on it.
            let compiled = ContextPreparationCompiler::new(&estimators)
                .for_request(snapshot, closing_question, fixed_tool_ids, native_prefixes)
                .compile(input.clone())?;
            let overflow = match &compiled {
                ContextPreparationCompilationOutcome::CompactionRequired { budget, .. }
                | ContextPreparationCompilationOutcome::AwaitingRemediation { budget, .. } => {
                    Some(budget)
                }
                ContextPreparationCompilationOutcome::Prepared(_) => None,
            };
            if manual.is_none()
                && let Some(budget) = overflow
            {
                let needed = budget
                    .estimated_input_tokens
                    .saturating_sub(budget.request_input_budget_tokens);
                let shortened = super::new_results::shorten(&mut input, budget, needed)?;
                if !shortened.originals.is_empty() {
                    reduction
                        .get_or_insert(PromptInspectorReductionView {
                            input_tokens_before: budget.estimated_input_tokens,
                            cleared_tool_results: 0,
                            compacted: false,
                            history_on_demand: false,
                            summary_cache: None,
                        })
                        .cleared_tool_results += shortened.originals.len() as u32;
                    referenced_results.extend(shortened.originals);
                    selected_history_bytes =
                        selected_history_bytes.saturating_sub(shortened.bytes_released);
                    if missing_adopted.is_empty()
                        && selected_history_bytes
                            < budget.request_input_budget_tokens.saturating_mul(4)
                    {
                        history_read_pressure = false;
                    }
                    continue;
                }
            }
            let (budget, non_history_tokens, mut over, mut prefix) = match compiled {
                ContextPreparationCompilationOutcome::Prepared(prepared) => {
                    let b = &prepared.context.snapshot.budget;
                    let window = b.runtime_effective_window_tokens;
                    let reserve = trigger_output_reserve(
                        snapshot.request_options.effective_max_output_tokens,
                    );
                    // Leave space for the closing instruction and the next normal tool exchange.
                    let growth = context_growth_reserve(window);
                    let near = b
                        .estimated_input_tokens
                        .saturating_add(b.safety_reserve_tokens)
                        .saturating_add(reserve)
                        .saturating_add(growth)
                        >= window;
                    if manual.is_none()
                        && missing_adopted.is_empty()
                        && (attempted || (!force_rebuild && !near && !history_read_pressure))
                    {
                        if let Some(plan) =
                            recovery::no_progress(window_trial.as_ref(), &prepared.context.snapshot)
                        {
                            if recovery::fit(
                                &mut input,
                                &prepared.context.snapshot.budget,
                                &mut deferred_images,
                            )? {
                                continue;
                            }
                            // Boundary: recording the no-progress remediation is one
                            // bounded write.
                            let journal = Arc::clone(&self.execution.journal);
                            let budget = prepared.context.snapshot.budget.clone();
                            let model_step_id = step.model_step_id;
                            self.store
                                .run_critical(
                                    StoreStep::run(run_id, "agent.context.no_progress"),
                                    StoreAccess::Write,
                                    move || {
                                        Ok(journal.lock()?.database.unit_of_work(|tx| {
                                            context::remediation::require(
                                                tx,
                                                model_step_id,
                                                plan,
                                                budget,
                                                UtcTimestamp::now(),
                                            )
                                        })?)
                                    },
                                )
                                .await?;
                            return Ok(None);
                        }
                        break prepared;
                    }
                    let frozen = &prepared.context.snapshot;
                    let non_history = frozen
                        .sections
                        .iter()
                        .filter(|s| {
                            matches!(
                                s.compressibility,
                                ContextCompressibility::NeverCompress
                                    | ContextCompressibility::UserReducibleOnly
                            )
                        })
                        .try_fold(0u64, |total, s| total.checked_add(s.estimated_tokens))
                        .ok_or_else(|| anyhow::anyhow!("context.budget_overflow"))?;
                    let prefix = Box::new(RequestPrefix::of_prepared(&prepared.context));
                    (frozen.budget.clone(), non_history, None, prefix)
                }
                ContextPreparationCompilationOutcome::CompactionRequired {
                    plan,
                    budget,
                    prefix,
                } => (
                    budget.clone(),
                    plan.overflow.non_compressible_tokens,
                    Some((plan, budget)),
                    prefix,
                ),
                ContextPreparationCompilationOutcome::AwaitingRemediation { plan, budget } => {
                    if recovery::fit(&mut input, &budget, &mut deferred_images)? {
                        continue;
                    }
                    self.require_remediation(run_id, step.model_step_id, plan, budget).await?;
                    return Ok(None);
                }
            };
            reduction.get_or_insert(PromptInspectorReductionView {
                input_tokens_before: budget.estimated_input_tokens,
                cleared_tool_results: 0,
                compacted: false,
                history_on_demand: false,
                summary_cache: None,
            });
            if attempted {
                if let Some((plan, budget)) = over {
                    if recovery::fit(&mut input, &budget, &mut deferred_images)? {
                        continue;
                    }
                    self.require_remediation(run_id, step.model_step_id, plan, budget).await?;
                    return Ok(None);
                }
                anyhow::bail!("context.compaction_no_progress");
            }
            attempted = true;
            prefix.closing_question = closing_question.map(str::to_owned);
            let compacted = Box::pin(self.compact(
                &input,
                snapshot,
                endpoint,
                CompactionRequest {
                    budget: &budget,
                    non_history_tokens,
                    manual: manual.as_ref(),
                    prefix: &prefix,
                    source_access: source_access.clone(),
                    missing_adopted: &missing_adopted,
                    fixed_tool_ids,
                    resolved_attachments: screenshots.resolved(&prefix.attachments)?.into(),
                },
                cancel,
            ))
            .await;
            let error = match compacted {
                Ok(Compacted::Done(stats)) => {
                    if let Some(reduced) = reduction.as_mut() {
                        reduced.compacted = true;
                        reduced.summary_cache = stats;
                    }
                    None
                }
                Ok(Compacted::FellShort { code }) if manual.is_some() => {
                    if code == "context.compaction_window_insufficient"
                        && recovery::fit(&mut input, &budget, &mut deferred_images)?
                    {
                        // Admission failed locally; the command has yet to issue a model request.
                        attempted = false;
                        continue;
                    }
                    Some(anyhow::anyhow!("context.compaction_failed: {code}"))
                }
                outcome @ (Ok(Compacted::FellShort { .. }) | Ok(Compacted::NothingEligible)) => {
                    if !missing_adopted.is_empty() {
                        let plan =
                            recovery::adopted_prefix_unavailable(&budget, non_history_tokens);
                        self.require_remediation(run_id, step.model_step_id, plan, budget).await?;
                        return Ok(None);
                    }
                    if let Ok(Compacted::FellShort { code }) = outcome {
                        if code == "context.compaction_window_insufficient"
                            && let Some((plan, _)) = &mut over
                        {
                            plan.overflow.error_code = match background {
                                BackgroundUse::SourcesUnavailable => {
                                    "context.compaction_window_insufficient_after_summary_invalidated".to_owned()
                                }
                                BackgroundUse::Absent | BackgroundUse::Applied => code.clone(),
                            };
                            plan.actions.retain(|action| action != "compact_sealed_history");
                            plan.overflow
                                .remediation
                                .retain(|action| action != "compact_sealed_history");
                        }
                        append_core_log(
                            "context.compaction_fell_short",
                            serde_json::json!({
                                "agent_run_id": run_id, "model_step_id": step.model_step_id, "code": code
                            }),
                        );
                    }
                    if let Some((plan, budget)) = over {
                        if recovery::fit(&mut input, &budget, &mut deferred_images)? {
                            continue;
                        }
                        self.require_remediation(run_id, step.model_step_id, plan, budget).await?;
                        return Ok(None);
                    }
                    continue;
                }
                Err(error) => Some(error),
            };
            if let Some(error) = error {
                if let Some(manual) = &manual {
                    // Boundary: recording the failed manual compaction is one bounded write.
                    let journal = Arc::clone(&self.execution.journal);
                    let command_invocation_id = manual.command_invocation_id;
                    let cancelled = cancel.is_cancelled();
                    let message = error.to_string();
                    self.store
                        .run_critical(
                            StoreStep::run(run_id, "agent.context.manual_compaction_failed"),
                            StoreAccess::Write,
                            move || {
                                Ok(journal.lock()?.database.unit_of_work(|tx| {
                                    aura_persistence_v2::context::manual_compaction::fail_request(
                                        tx,
                                        command_invocation_id,
                                        cancelled,
                                        &message,
                                        UtcTimestamp::now(),
                                    )
                                })?)
                            },
                        )
                        .await?;
                }
                return Err(error);
            }
            let access = source_access.clone();
            let session = turn.session_id;
            let checkpoint = self
                .store
                .read(StoreStep::run(run_id, "agent.context.compaction_checkpoint"), move |db| {
                    super::compaction::authorized_checkpoint(db, session, run_id, &access)
                })
                .await?;
            if !missing_adopted.is_empty() {
                if !checkpoint.record.as_ref().is_some_and(|record| {
                    missing_adopted.iter().all(|id| record.excluded_event_ids.contains(id))
                }) {
                    let plan = recovery::adopted_prefix_unavailable(&budget, non_history_tokens);
                    self.require_remediation(run_id, step.model_step_id, plan, budget).await?;
                    return Ok(None);
                }
                missing_adopted.clear();
            }
            super::compaction::apply(self, &mut input, checkpoint).await?;
            // Boundary: the post-compaction state refresh. Its reads were eleven direct
            // locks inside the loop body; they are now two bounded sections over the same
            // statements, and the JSON they feed is still built from exactly the same rows.
            let refreshed = {
                let journal = Arc::clone(&self.execution.journal);
                let turn_task_id = turn.turn_task_id;
                let session_id = turn.session_id;
                let session_id_guard = turn.session_id;
                let goal = turn.mode == AgentTaskMode::Goal;
                self.store
                    .run_critical(
                        StoreStep::run(run_id, "agent.context.refresh_after_compaction"),
                        StoreAccess::Read,
                        move || {
                            let journal = journal.lock()?;
                            let db = journal.database.connection();
                            let (revision, hash) = checkpoint::current_state(db, turn_task_id)?;
                            let lineages = serde_json::to_value(
                                session::load(db, session_id)?
                                    .open_lineages
                                    .iter()
                                    .map(|id| lineage::load(db, *id))
                                    .collect::<Result<Vec<_>, _>>()?,
                            )?;
                            let goal = if goal {
                                Some(serde_json::to_value(work::goal(db, turn_task_id)?)?)
                            } else {
                                None
                            };
                            let mode_episode = serde_json::to_value(
                                aura_persistence_v2::agent::mode::view(db, session_id_guard)?,
                            )?;
                            let guard = serde_json::to_value(
                                aura_persistence_v2::agent::guard::load(db, turn_task_id)?,
                            )?;
                            Ok((revision, hash, lineages, goal, mode_episode, guard))
                        },
                    )
                    .await?
            };
            {
                let journal = Arc::clone(&self.execution.journal);
                let turn_task_id = turn.turn_task_id;
                input.inspector.active_task_lineage = self
                    .store
                    .run_critical(
                        StoreStep::run(run_id, "agent.context.active_lineage"),
                        StoreAccess::Read,
                        move || {
                            Ok(aura_persistence_v2::agent::inspector::active_lineage_view(
                                journal.lock()?.database.connection(),
                                turn_task_id,
                            )?)
                        },
                    )
                    .await?;
            }
            state["state_revision"] = serde_json::to_value(refreshed.0)?;
            state["state_hash"] = serde_json::to_value(refreshed.1)?;
            state["lineages"] = refreshed.2;
            if let Some(goal) = refreshed.3 {
                state["goal"] = goal;
            }
            state["mode_episode"] = refreshed.4;
            state["guard"] = refreshed.5;
        };
        if recovery::has_gap(&input) {
            reduction
                .get_or_insert(PromptInspectorReductionView {
                    input_tokens_before: prepared.context.snapshot.budget.estimated_input_tokens,
                    cleared_tool_results: 0,
                    compacted: false,
                    history_on_demand: true,
                    summary_cache: None,
                })
                .history_on_demand = true;
        }
        prepared.prompt_inspector.reduction = reduction.filter(|reduced| {
            reduced.compacted || reduced.cleared_tool_results > 0 || reduced.history_on_demand
        });
        Ok(Some(CompiledStep { prepared, state, referenced_results }))
    }

    /// The request cannot be made to fit: the step waits for the user with its reasons and the
    /// remedies (a larger window, less of the content the user manages).
    async fn require_remediation(
        self: &Arc<Self>,
        run_id: AgentRunId,
        model_step_id: ModelStepId,
        plan: ContextRemediationPlan,
        budget: ContextBudgetSnapshot,
    ) -> anyhow::Result<()> {
        // Boundary: recording the required remediation is one bounded write.
        let journal = Arc::clone(&self.execution.journal);
        self.store
            .run_critical(
                StoreStep::run(run_id, "agent.context.awaiting_remediation"),
                StoreAccess::Write,
                move || {
                    Ok(journal.lock()?.database.unit_of_work(|tx| {
                        context::remediation::require(
                            tx,
                            model_step_id,
                            plan,
                            budget,
                            UtcTimestamp::now(),
                        )
                    })?)
                },
            )
            .await
    }
}
