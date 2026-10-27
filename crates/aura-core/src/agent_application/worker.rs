//! Each wake-up reloads the last committed boundary, rather than restarting a
//! model response or manufacturing new user input.
//!
//! [`AgentStore`] owns the journal sections routed through it and their eventual results.
//! The run's task itself is owned by
//! [`crate::agent_supervision::RunSupervisor`]: the guard below is the single place its active
//! entry is released, and it runs on a normal return, on an `Err`, on a panic, on an abort and
//! when the runtime is dropped.
use super::*;
use crate::agent_store::StoreStep;
use crate::agent_supervision::{CatchUnwind, RunStage, RunStageCell, TaskEnd};
use aura_persistence_v2::agent::{approval, continuation, work};

/// One failed model step, kept typed instead of flattened into a message.
///
/// The transport already classifies the failure and — for a refused Provider
/// output — carries bounded structured evidence. Formatting that into a single
/// `anyhow` string loses the message and every identity the UI needs, so the
/// failure travels as a value the application layer can read field by field.
#[derive(Debug, thiserror::Error)]
#[error("{summary}")]
pub(super) struct ModelStepFailure {
    summary: String,
    pub diagnostic: Option<ProviderBindingViolationDiagnostic>,
}

impl ModelStepFailure {
    fn new(
        failure: &PersistedModelFailure,
        diagnostic: Option<ProviderBindingViolationDiagnostic>,
    ) -> Self {
        let mut summary = format!(
            "agent.provider_failed: {} ({:?}, HTTP {:?})",
            failure.failure.code, failure.failure.category, failure.failure.provider_status
        );
        // Only the bounded, Aura-built summary is appended: never
        // `failure.failure.message`, which for an HTTP failure is Provider text
        // that could contain a response body.
        if let Some(diagnostic) = &diagnostic {
            summary.push_str(": ");
            summary.push_str(&diagnostic.summary());
        }
        Self { summary, diagnostic }
    }
}

/// A panic inside the real run task, as a bounded fact. The payload's own text is never
/// persisted - only that a panic happened, and where.
#[derive(Debug, thiserror::Error)]
#[error("agent.run_panicked: the run task unwound while it was {stage}")]
pub(super) struct RunPanicFact {
    stage: &'static str,
}

impl AgentApplication {
    /// Admits one run and starts its single real task.
    pub(super) fn launch(self: &Arc<Self>, run: AgentRunId) -> anyhow::Result<()> {
        // Admission first: a second launch of the same run is refused here, so one run can
        // never have two tasks or two generations.
        let Some(admission) = self.supervisor.admit(run) else {
            return Ok(());
        };
        let generation = admission.generation;
        let model = admission.signals().model.clone();
        let receive = admission.tool_receiver();
        let stage = admission.stage_cell();
        let app = self.clone();
        // Own teardown before spawning: an abort or runtime drop can discard the future before
        // its first poll, so constructing the guard inside that future would leak admission.
        let mut guard = admission.guard(app.supervisor.clone());
        let task = tokio::spawn(async move {
            guard.mark_started();
            stage.set(RunStage::Starting);
            // The one place a panic becomes a value instead of a dead task.
            let outcome =
                CatchUnwind::new(app.drive(run, model.clone(), receive, stage.clone())).await;
            match outcome {
                Ok(Ok(())) => guard.finished(TaskEnd::Completed),
                Ok(Err(error)) => {
                    guard.finished(TaskEnd::Failed);
                    stage.set(RunStage::Recording);
                    app.persist_drive_failure(run, generation, &error, model.is_cancelled()).await;
                }
                Err(payload) => {
                    // The payload's own content is never persisted, and it is dropped before any
                    // await so the run's task future stays `Send`.
                    let kind = payload_kind(payload.as_ref());
                    drop(payload);
                    guard.finished(TaskEnd::Panicked);
                    stage.set(RunStage::Recording);
                    app.persist_run_panic(run, generation, kind).await;
                }
            }
            let turn = app.turn(run).await;
            // The tabs shared for this turn only stop being shared with it.
            if let (Ok(turn), Some(chrome)) = (&turn, app.execution.chrome.get())
                && turn.state.terminal()
            {
                chrome.turn_ended(turn.session_id);
            }
            if let Ok(turn) = turn
                && matches!(turn.state, TurnTaskState::Completed { .. })
                && let Err(error) = app.schedule_inbox(turn.session_id).await
            {
                // Input is already durable. Failure to start a later task is
                // not failure of the completed task and must not replay it.
                app.note_run_diagnostic(
                    run,
                    "handover",
                    "agent.inbox_schedule_failed",
                    error.to_string(),
                );
            }
            if let Err(error) = app.schedule_memory(run).await {
                crate::append_core_log(
                    "memory.maintenance_schedule_failed",
                    serde_json::json!({"run":run,"error":error.to_string()}),
                );
            }
            // A durable wait remains visible even if the expiry timer cannot start.
            let _ = app.schedule_expiry(run).await;
            drop(guard);
        });
        if !self.supervisor.install(run, generation, task) {
            self.note_run_diagnostic(
                run,
                "admission",
                "agent.task_not_installed",
                "the admitted run's task could not be installed",
            );
        }
        Ok(())
    }

    /// Records a panic of the run's own task, keeping the side effect's terminal state unknown
    /// rather than claiming a failure it cannot prove.
    async fn persist_run_panic(
        &self,
        run: AgentRunId,
        generation: RunGeneration,
        payload_kind: &'static str,
    ) {
        let stage = self.supervisor.stage_of(run).map_or("unknown", RunStage::as_str);
        let fact = RunPanicFact { stage };
        self.note_run_diagnostic(
            run,
            "run_task",
            "agent.run_panicked",
            format!("{fact} (payload kind: {payload_kind})"),
        );
        self.persist_drive_failure(run, generation, &anyhow::Error::new(fact), false).await;
    }

    async fn drive(
        self: &Arc<Self>,
        run_id: AgentRunId,
        cancel: ModelCancellationToken,
        tool_cancel: watch::Receiver<bool>,
        stage: RunStageCell,
    ) -> anyhow::Result<()> {
        // S04-C probe: one injected panic **inside the real run body**, so the teardown path it
        // exercises is the production one. Compiled out of every product build.
        #[cfg(test)]
        if self.probe_drive_panic.swap(false, std::sync::atomic::Ordering::AcqRel) {
            panic!("S04-C probe: injected panic inside the real run body");
        }
        let config = self
            .store
            .read(StoreStep::run(run_id, "agent.drive.configuration"), move |db| {
                store::configuration(db, run_id)
            })
            .await?;
        loop {
            stage.set(RunStage::Preparing);
            if self.turn(run_id).await?.state.terminal() {
                self.after_turn(run_id).await?;
                return Ok(());
            }
            let turn = self.turn(run_id).await?;
            let turn_task_id = turn.turn_task_id;
            let stop = self
                .store
                .read(StoreStep::run(run_id, "agent.drive.turn_guard"), move |db| {
                    aura_persistence_v2::agent::guard::blocked(db, turn_task_id)
                })
                .await?;
            if let Some(reason) = stop {
                let code = match reason {
                    TurnGuardStopReason::ProtocolNotCompleted => "agent.protocol_not_completed",
                    TurnGuardStopReason::DeterministicCorrectionExhausted => {
                        "agent.deterministic_correction_exhausted"
                    }
                };
                self.interrupt(run_id, code, false).await?;
                return Ok(());
            }
            let cancellation_requested = self
                .store
                .read(StoreStep::run(run_id, "agent.drive.cancellation_requested"), move |db| {
                    store::cancellation_requested(db, run_id)
                })
                .await?;
            let run = self
                .store
                .read(StoreStep::run(run_id, "agent.drive.run_state"), move |db| {
                    turns::load_run(db, run_id)
                })
                .await?;
            if cancel.is_cancelled() || cancellation_requested {
                // Shutdown and an explicit user stop are different sources and are not
                // conflated. Only an explicit stop or a capability revocation is durable - it
                // wrote the cancellation request the read above returned. Shutdown only fires the
                // in-memory token, so `cancel.is_cancelled()` alone proves nothing about what the
                // user asked for.
                //
                // Shutdown still stops **this generation's execution**: the loop ends and the task
                // really exits. What it must not do is rewrite a wait that is already persisted and
                // still actionable into a user cancellation. That rewrite is what advanced the run
                // revision past the remediation record's, so a restart's legitimate
                // `agent.window.resolve` at the original revision met a revision conflict.
                let committed_wait = matches!(
                    run.state,
                    AgentRunState::AwaitingApproval { .. }
                        | AgentRunState::AwaitingQuery { .. }
                        | AgentRunState::AwaitingContextRemediation { .. }
                );
                if cancellation_requested || !committed_wait {
                    self.interrupt(run_id, "agent.cancelled", true).await?;
                } else {
                    self.note_run_diagnostic(
                        run_id,
                        "teardown",
                        "agent.shutdown_left_committed_wait",
                        "shutdown ended this generation's task without cancelling the persisted \
                         wait, which stays actionable at its original revision",
                    );
                }
                return Ok(());
            }
            match run.state {
                AgentRunState::PreparingContext => {
                    if self.before_turn(run_id).await? {
                        self.interrupt(run_id, "hooks.turn_blocked", false).await?;
                        return Ok(());
                    }
                    match self.review_completion(run_id).await? {
                        Some(super::hook_events::CompletionReview::Completed) => continue,
                        Some(super::hook_events::CompletionReview::Stop) => {
                            self.interrupt(run_id, "hooks.completion_blocked", false).await?;
                            return Ok(());
                        }
                        None => {}
                    }
                    let now = UtcTimestamp::now();
                    self.store
                        .write(
                            StoreStep::run(run_id, "agent.drive.input_consumption"),
                            move |journal| {
                                journal.database.unit_of_work(|tx| {
                                    aura_persistence_v2::agent::input_consumption::additions(
                                        tx, run_id, now,
                                    )
                                })
                            },
                        )
                        .await?;
                    let turn = self.turn(run_id).await?;
                    if turn.lineage_resolution_required {
                        let turn_owned = turn.clone();
                        let now = UtcTimestamp::now();
                        self.store
                            .write(StoreStep::run(run_id, "agent.drive.lineage"), move |journal| {
                                journal.database.unit_of_work(|tx| {
                                    aura_persistence_v2::agent::lifecycle::ensure_current_lineage(
                                        tx,
                                        &turn_owned,
                                        now,
                                    )
                                })
                            })
                            .await?;
                    }
                    if turn.mode == AgentTaskMode::Goal {
                        let turn_task_id = turn.turn_task_id;
                        let goal = self
                            .store
                            .read(StoreStep::run(run_id, "agent.drive.goal_budget"), move |db| {
                                work::goal(db, turn_task_id)
                            })
                            .await?;
                        anyhow::ensure!(!goal.budget_exhausted(), "agent.goal_budget_exhausted");
                    }
                    let next = self
                        .store
                        .read(StoreStep::run(run_id, "agent.drive.continuation"), move |db| {
                            continuation::next(db, run_id)
                        })
                        .await?;
                    let binding =
                        self.execution.persistence.session_model_binding_state(turn.session_id)?;
                    let profile = match binding.binding {
                        Some(value) => value.model_profile_id,
                        // An explicit start can seed an otherwise never-bound
                        // session. A cleared binding cannot reuse that choice.
                        None if binding.revision.value() == 0 => config.request.model_profile_id,
                        None => anyhow::bail!("agent.model_binding_required"),
                    };
                    let chosen = config.request.reasoning_effort;
                    let Some(prepared) =
                        self.prepare(run_id, profile, chosen, next, &cancel).await?
                    else {
                        return Ok(());
                    };
                    let snapshot = prepared.request.snapshot.clone();
                    stage.set(RunStage::Provider);
                    match provider::execute(self, prepared.request, &cancel).await? {
                        TransportRunTerminal::Cancelled { .. } => {
                            self.interrupt(run_id, "agent.cancelled", true).await?;
                            return Ok(());
                        }
                        TransportRunTerminal::Failed { failure, diagnostic, .. } => {
                            if failure.failure.category
                                == ModelFailureCategory::ContextWindowExceeded
                                && failure.failure.partial_output_ref.is_none()
                                && failure.failure.provider_error_ref.is_some()
                            {
                                self.reject_context_window(
                                    run_id,
                                    &snapshot,
                                    &prepared.profile,
                                    &failure,
                                )
                                .await?;
                                continue;
                            }
                            // A failure before any answer goes at once to the provider's
                            // next backup connection this streak has not tried.
                            if super::connections::switches(&failure.failure)
                                && self
                                    .untried_connection(
                                        run_id,
                                        &prepared.profile,
                                        snapshot.endpoint_id,
                                    )
                                    .await?
                            {
                                let failed = (snapshot.clone(), failure.clone());
                                self.store
                                    .write(
                                        StoreStep::run(run_id, "agent.drive.backup_connection"),
                                        move |journal| {
                                            journal.database.unit_of_work(|tx| {
                                                aura_persistence_v2::model::transient_retry::retry(
                                                    tx,
                                                    &failed.0,
                                                    &failed.1,
                                                    UtcTimestamp::now(),
                                                )
                                            })
                                        },
                                    )
                                    .await?;
                                continue;
                            }
                            // A step the provider failed for a passing reason is asked
                            // again as a new step, a bounded number of times in a row.
                            if super::transient::passing(&failure.failure) {
                                let streak = self
                                    .store
                                    .read(
                                        StoreStep::run(run_id, "agent.drive.transient_streak"),
                                        move |db| {
                                            aura_persistence_v2::model::transient_retry::streak(
                                                db, run_id,
                                            )
                                        },
                                    )
                                    .await?;
                                if streak < super::transient::MAX_RETRIES {
                                    let failed = (snapshot.clone(), failure.clone());
                                    self.store
                                        .write(
                                            StoreStep::run(run_id, "agent.drive.transient_retry"),
                                            move |journal| {
                                                journal.database.unit_of_work(|tx| {
                                                    aura_persistence_v2::model::transient_retry::retry(
                                                        tx,
                                                        &failed.0,
                                                        &failed.1,
                                                        UtcTimestamp::now(),
                                                    )
                                                })
                                            },
                                        )
                                        .await?;
                                    super::transient::wait(streak, &cancel).await;
                                    continue;
                                }
                            }
                            return Err(anyhow::Error::new(ModelStepFailure::new(
                                &failure, diagnostic,
                            )));
                        }
                        TransportRunTerminal::Completed { .. } => {
                            self.complete_window_trial(run_id, &snapshot).await?;
                            self.after_model_call(run_id, snapshot.model_step_id).await?;
                        }
                    }
                }
                AgentRunState::ToolsRunning { tool_batch_id: batch_id } => {
                    stage.set(RunStage::Tools);
                    let steps = self
                        .store
                        .read(StoreStep::run(run_id, "agent.drive.model_steps"), move |db| {
                            aura_persistence_v2::agent::model_context::steps(db, run_id)
                        })
                        .await?;
                    let step =
                        steps.last().ok_or_else(|| anyhow::anyhow!("agent.model_step_missing"))?;
                    let model_step_id = step.model_step_id;
                    let tools = self
                        .store
                        .read(StoreStep::run(run_id, "agent.drive.step_tools"), move |db| {
                            store::step_tool_settings(db, model_step_id)
                        })
                        .await?;
                    // Admission is one contiguous synchronous section of its own; it is
                    // handed to the execution service, which runs it whole on the one bounded Agent
                    // Store owner instead of on this reactive task.
                    self.decide_tool_calls(run_id, model_step_id).await?;
                    self.inspect_chrome_calls(run_id, model_step_id).await?;
                    let batch =
                        Arc::clone(&self.execution).admit_output(model_step_id, tools).await?;
                    anyhow::ensure!(batch.batch.tool_batch_id == batch_id, "agent.batch_changed");
                    match self
                        .execution
                        .execute_batch(batch, self.commands.clone(), tool_cancel.clone())
                        .await?
                    {
                        crate::agent_execution::BatchOutcome::Completed(group) => {
                            self.review_tool_results(run_id, batch_id).await?;
                            let now = UtcTimestamp::now();
                            self.store
                                .write(
                                    StoreStep::run(run_id, "agent.drive.after_batch"),
                                    move |journal| {
                                        journal.database.unit_of_work(|tx| {
                                            store::after_batch(tx, run_id, batch_id, &group, now)
                                        })
                                    },
                                )
                                .await?;
                        }
                        crate::agent_execution::BatchOutcome::Unknown { attempts } => {
                            // A typed fact, not a formatted message: the batch and the real
                            // Attempt identities are what a later reader needs, and the run must
                            // never re-dispatch them. Flattening this into an `anyhow` string
                            // would leave the class readable only by parsing text.
                            return Err(anyhow::Error::new(
                                crate::agent_application::fault::ToolOutcomeUnknown {
                                    run: run_id,
                                    batch: batch_id,
                                    attempts,
                                },
                            ));
                        }
                        crate::agent_execution::BatchOutcome::AwaitingApproval {
                            call_id,
                            proposal,
                            backend,
                        } => {
                            let at = UtcTimestamp::now();
                            let expires =
                                UtcTimestamp::from_value(at.value() + time::Duration::minutes(10));
                            self.store
                                .write(
                                    StoreStep::run(run_id, "agent.drive.approval_pause"),
                                    move |journal| {
                                        journal.database.unit_of_work(|tx| {
                                            approval::pause(
                                                tx, run_id, batch_id, call_id, *proposal, *backend,
                                                at, expires,
                                            )
                                        })
                                    },
                                )
                                .await?;
                            return Ok(());
                        }
                    }
                }
                AgentRunState::AwaitingQuery { .. }
                | AgentRunState::AwaitingApproval { .. }
                | AgentRunState::AwaitingContextRemediation { .. } => {
                    stage.set(RunStage::Waiting);
                    return Ok(());
                }
                _ => anyhow::bail!("agent.unexpected_active_state"),
            }
        }
    }

    /// The one interruption step, so every stop path writes the same fact the same way.
    pub(super) async fn interrupt(
        &self,
        run_id: AgentRunId,
        reason: &'static str,
        cancelled: bool,
    ) -> Result<(), StoreError> {
        let at = UtcTimestamp::now();
        self.store
            .write(StoreStep::run(run_id, "agent.interrupt"), move |journal| {
                journal
                    .database
                    .unit_of_work(|tx| store::interrupt(tx, run_id, reason, cancelled, at))
            })
            .await
            .map(|_| ())
    }
}

/// The kind of a panic payload, without its content: a persisted diagnostic must never carry
/// whatever text the panicking code happened to hold.
fn payload_kind(payload: &(dyn std::any::Any + Send)) -> &'static str {
    if payload.downcast_ref::<&'static str>().is_some() {
        "&str"
    } else if payload.downcast_ref::<String>().is_some() {
        "String"
    } else {
        "opaque"
    }
}
