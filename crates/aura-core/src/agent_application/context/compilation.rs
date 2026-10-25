use super::*;
use crate::agent_application::compaction::{Compacted, CompactionRequest};

/// Compaction passes one preparation may make. A history larger than one summary request
/// can carry is compacted oldest first over several passes.
const MAX_COMPACTION_PASSES: usize = 8;

pub(super) struct CompilationInput<'a> {
    pub run_id: AgentRunId,
    pub turn: &'a TurnTask,
    pub step: &'a ModelStep,
    pub state: serde_json::Value,
    pub input: ContextPreparationInput,
    pub snapshot: &'a ModelExecutionSnapshot,
    pub endpoint: &'a ModelEndpointConfig,
    pub window_trial: &'a Option<model::context_window::WindowCorrection>,
    pub force_rebuild: bool,
    pub cancel: &'a ModelCancellationToken,
    pub estimators: ContextEstimatorRegistry,
    /// The tools the step always loads, which a compaction on its prefix sends too.
    pub fixed_tool_ids: &'a [aura_domain_common::ToolId],
    /// The screenshots the request may carry, attached anew on each pass.
    pub screenshots: &'a super::preparation::screenshots::Screenshots,
}

impl AgentApplication {
    pub(super) async fn compile_context(
        self: &Arc<Self>,
        pass: CompilationInput<'_>,
    ) -> anyhow::Result<Option<(Box<PreparedContextDraft>, serde_json::Value)>> {
        let CompilationInput {
            run_id,
            turn,
            step,
            mut state,
            mut input,
            snapshot,
            endpoint,
            window_trial,
            force_rebuild,
            cancel,
            estimators,
            fixed_tool_ids,
            screenshots,
        } = pass;
        let mut passes = 0;
        // Compacting every sealed group is the last thing tried before the user is asked.
        let mut everything_sealed = false;
        // Once a compaction
        // starts, by the user or because the request no longer fits, it goes on until all but the
        // recent history is summarized, not only until the request fits again.
        let mut deep = false;
        // What this preparation gave back, for the line the conversation shows -
        // the request's estimate when it first did not fit, results cleared, history compacted.
        let mut reduction: Option<PromptInspectorReductionView> = None;
        let mut prepared = loop {
            // Only the screenshots whose results this pass still carries as they were.
            screenshots.attach(&mut input);
            // Boundary: the manual-compaction request read, one bounded section per pass.
            let manual = {
                let journal = Arc::clone(&self.execution.journal);
                self.store
                    .run_critical(
                        StoreStep::run(run_id, "agent.context.manual_compaction"),
                        StoreAccess::Read,
                        move || {
                            Ok(aura_persistence_v2::context::manual_compaction::requested(
                                journal.lock()?.database.connection(),
                                run_id,
                            )?)
                        },
                    )
                    .await?
            };
            // `over` is the request that does not fit, with its reasons; `None` when it fits.
            // `prefix` is what that request is made of, for a compaction asked on it.
            let (budget, non_history_tokens, over, prefix) =
                match ContextPreparationCompiler::new(&estimators).compile(input.clone())? {
                    ContextPreparationCompilationOutcome::Prepared(prepared) => {
                        if manual.is_none() && !deep && (!force_rebuild || passes > 0) {
                            if let Some(plan) = window_rebuild::no_progress(
                                window_trial.as_ref(),
                                &prepared.context.snapshot,
                            ) {
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
                        self.require_remediation(run_id, step.model_step_id, plan, budget).await?;
                        return Ok(None);
                    }
                };
            reduction.get_or_insert(PromptInspectorReductionView {
                input_tokens_before: budget.estimated_input_tokens,
                cleared_tool_results: 0,
                compacted: false,
            });
            if passes >= MAX_COMPACTION_PASSES {
                // A deep compaction that has made the request fit stops here; it is sent.
                if over.is_none() && manual.is_none() {
                    deep = false;
                    continue;
                }
                // Compaction no longer makes the request smaller; the user decides.
                let Some((plan, budget)) = over else {
                    anyhow::bail!("context.compaction_no_progress")
                };
                self.require_remediation(run_id, step.model_step_id, plan, budget).await?;
                return Ok(None);
            }
            let compacted = self
                .compact(
                    &input,
                    snapshot,
                    endpoint,
                    CompactionRequest {
                        budget: &budget,
                        non_history_tokens,
                        manual: manual.as_ref(),
                        everything_sealed,
                        prefix: Some(&prefix),
                        fixed_tool_ids,
                    },
                    cancel,
                )
                .await;
            let error = match compacted {
                Ok(Compacted::Done { whole }) => {
                    if let Some(reduced) = reduction.as_mut() {
                        reduced.compacted = true;
                    }
                    // A pass that took a part of the history is followed by another.
                    deep = !whole;
                    None
                }
                // Nothing left to
                // compact never ends the turn. A request that fits is sent as it is; one that does
                // not first gives up the recent history it kept, then waits for the user's decision
                // with its reasons.
                Ok(Compacted::NothingEligible) => {
                    passes += 1;
                    let Some((_, over_budget)) = &over else {
                        // All but the recent history is summarized: the request is sent.
                        deep = false;
                        continue;
                    };
                    // Each batch of old
                    // results cleared changes the request in its middle, so the provider's cache
                    // is lost for all that follows; they go only when compaction cannot make room.
                    let cleared = super::clearing::clear_next(&mut input, over_budget, true)?;
                    if cleared > 0 {
                        if let Some(reduced) = reduction.as_mut() {
                            reduced.cleared_tool_results = reduced
                                .cleared_tool_results
                                .saturating_add(u32::try_from(cleared).unwrap_or(u32::MAX));
                        }
                        continue;
                    }
                    match over {
                        None => continue,
                        Some(_) if !everything_sealed => {
                            everything_sealed = true;
                            continue;
                        }
                        Some((plan, budget)) => {
                            self.require_remediation(run_id, step.model_step_id, plan, budget)
                                .await?;
                            return Ok(None);
                        }
                    }
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
            super::compaction::apply(self, &mut input).await?;
            passes += 1;
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
            for fragment in input.section_inputs.iter_mut().flat_map(|s| &mut s.fragments) {
                if matches!(&fragment.source_ref,ContextSourceRef::RuntimeState{state_kind,..} if state_kind=="agent.control_state.v1")
                {
                    fragment.content = serde_json::to_string(&prompt_state(&state))?;
                }
            }
        };
        prepared.prompt_inspector.reduction =
            reduction.filter(|reduced| reduced.compacted || reduced.cleared_tool_results > 0);
        Ok(Some((prepared, state)))
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
