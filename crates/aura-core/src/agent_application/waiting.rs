//! Authenticated user answers and cancellation, separate from model execution.
//!
//! Entries that ask a run to stop share **one** monotonic budget computed once per entry: the
//! deadline is not reset for a later step or a second run. A cancellation request, the run's
//! real end and its persisted state are three different facts, and the answers here report the
//! one they really observed.
use super::*;
use crate::agent_store::StoreStep;
use crate::agent_supervision::{RUN_STOP_BUDGET, RunStage};
use aura_execution_contract::*;
use aura_persistence_v2::agent::{approval, query, session};

impl AgentApplication {
    /// One budget for a whole entry: never reset per descendant or per stage.
    #[must_use]
    pub(super) fn stop_deadline() -> Instant {
        Instant::now() + RUN_STOP_BUDGET
    }

    /// Waits for the run's **real** end inside an absolute deadline. Cancels nothing.
    pub(super) async fn idle_within(
        &self,
        run: AgentRunId,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        let outcome = self.supervisor.wait_ended(run, deadline).await;
        if outcome.real_end {
            return Ok(());
        }
        anyhow::bail!(
            "agent.stop_pending: run {} is still alive in stage {} after {} ms",
            outcome.run,
            outcome.stage,
            outcome.waited_ms
        )
    }

    /// The same wait on its own budget.
    #[allow(dead_code)] // the regressions call it; every product entry threads one deadline
    pub(super) async fn idle(&self, run: AgentRunId) -> anyhow::Result<()> {
        self.idle_within(run, Self::stop_deadline()).await
    }

    /// Asks one run to stop and waits for its real end inside the deadline already in force.
    pub(super) async fn stop_run_within(
        self: &Arc<Self>,
        run: AgentRunId,
        deadline: Instant,
    ) -> anyhow::Result<()> {
        let outcome = self.supervisor.stop_within(run, deadline).await;
        if let Some(residue) = &outcome.residue {
            self.note_run_diagnostic(
                run,
                "stopping",
                "agent.stop_pending",
                format!(
                    "the run is still alive in stage {} after {} ms; observed={} (budget {} ms)",
                    residue.stage,
                    outcome.waited_ms,
                    residue.observed,
                    residue.observation_budget_ms
                ),
            );
        }
        if outcome.real_end {
            return Ok(());
        }
        anyhow::bail!(
            "agent.stop_pending: run {} is still alive in stage {} after {} ms",
            run,
            outcome.stage,
            outcome.waited_ms
        )
    }

    pub async fn answer(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentQueryAnswerRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        let deadline = Self::stop_deadline();
        let request =
            AgentRunRequest { session_id: input.session_id, agent_run_id: input.agent_run_id };
        self.status(&request).await?;
        let answered_input = input.clone();
        let answered = self
            .store
            .read(StoreStep::operation("agent.query.answered"), move |db| {
                query::answered(db, command, &answered_input)
            })
            .await?;
        if answered {
            return Ok(self.status(&request).await?);
        }
        self.idle_within(input.agent_run_id, deadline).await?;
        for answer in &input.answers {
            if let QueryAnswerValue::FileOrDirectory { file_reference_id } = answer.value {
                anyhow::ensure!(
                    self.execution
                        .persistence
                        .resolve_file_reference(input.session_id, file_reference_id)?
                        .is_some(),
                    "agent.query_file_scope"
                );
            }
        }
        let stored = input.clone();
        self.store
            .write(StoreStep::operation("agent.query.answer"), move |journal| {
                journal
                    .database
                    .unit_of_work(|tx| query::answer(tx, command, &stored, UtcTimestamp::now()))
            })
            .await?;
        self.launch(input.agent_run_id)?;
        Ok(self.status(&request).await?)
    }

    /// The user touched the waiting question (clicked it, chose, typed), so
    /// its countdown starts again. The run's timer reads the later deadline when it next wakes.
    pub async fn touch_query(
        self: &Arc<Self>,
        input: AgentQueryTouchRequest,
    ) -> anyhow::Result<AgentQueryTouchView> {
        let stored = input.clone();
        let deadline_at = self
            .store
            .write(StoreStep::operation("agent.query.touch"), move |journal| {
                journal.database.unit_of_work(|tx| query::touch(tx, &stored, UtcTimestamp::now()))
            })
            .await?;
        Ok(AgentQueryTouchView { query_id: input.query_id, deadline_at })
    }

    pub async fn approval(
        &self,
        input: ExecutionApprovalRequest,
    ) -> anyhow::Result<ExecutionApprovalView> {
        self.store
            .read(StoreStep::operation("agent.approval.view"), move |db| approval::view(db, &input))
            .await
            .map_err(Into::into)
    }

    pub async fn resolve_approval(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: ExecutionApprovalResolutionRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        let deadline = Self::stop_deadline();
        let v = self
            .approval(ExecutionApprovalRequest {
                session_id: input.session_id,
                approval_id: input.approval_id,
            })
            .await?;
        let run = v
            .operation
            .content
            .identity
            .agent_run_id
            .ok_or_else(|| anyhow::anyhow!("agent.approval_run_missing"))?;
        let request = AgentRunRequest { session_id: input.session_id, agent_run_id: run };
        let resolved_input = input.clone();
        let resolved = self
            .store
            .read(StoreStep::run(run, "agent.approval.resolved"), move |db| {
                approval::resolved(db, command, &resolved_input)
            })
            .await?;
        if resolved {
            self.ensure_directory_grant(&v.operation, input.decision, input.approval_id)?;
            self.launch_if_waiting(run).await?;
            return Ok(self.status(&request).await?);
        }
        self.idle_within(run, deadline).await?;
        // The live permission revision and this Core runtime instance are read
        // inside the mode barrier that also orders the resolution, so a consent
        // records the authority that was in force when the user answered — not a
        // snapshot the model could have obtained earlier.
        //
        // Boundary: the whole ordered critical section — the session permission authority
        // first, then the journal read of the recorded snapshot and the approval transaction — is
        // one indivisible section and is now moved **whole** onto the one bounded Store owner.
        // Splitting the authority read from the transaction would break the ordering that makes
        // the recorded revision the one in force at answer time, so it is not split; and the
        // section is no longer run on the reactor worker that happened to call this handler.
        // A refusal (`agent.store_refused`) means the consent provably was not recorded; an
        // unknown answer (`agent.store_unknown`) means it started and its result is owned by the
        // section. Neither is reported as the other, and neither re-issues the approval.
        {
            let persistence = Arc::clone(&self.execution.persistence);
            let journal = Arc::clone(&self.execution.journal);
            let runtime = self.execution.runtime;
            let session_id = input.session_id;
            let resolution = input.clone();
            let permission_snapshot_id = v.operation.content.identity.permission_snapshot_id;
            let file_answer =
                aura_security_contract::file_capabilities_are_consentable(
                    &v.operation.content.capabilities,
                ) && v.operation.content.resources.iter().any(|resource| {
                    aura_security_contract::file_consent_target_of(resource).is_some()
                });
            self.store
                .run_critical(
                    StoreStep::run(run, "agent.approval.resolve"),
                    StoreAccess::Write,
                    move || {
                        persistence
                            .with_session_permission_authority(
                                session_id,
                                |current| -> anyhow::Result<()> {
                                    let mut store = journal.lock()?;
                                    let snapshot = aura_persistence_v2::security::snapshot(
                                        store.database.connection(),
                                        permission_snapshot_id.ok_or_else(|| {
                                            anyhow::anyhow!("agent.approval_snapshot_missing")
                                        })?,
                                    )?
                                    .ok_or_else(|| {
                                        anyhow::anyhow!("agent.approval_snapshot_missing")
                                    })?;
                                    // File answers name verified objects independently of the
                                    // default mode. Dispatch revalidates their live identity.
                                    // Pending process answers still require their prepared snapshot.
                                    anyhow::ensure!(
                                        file_answer
                                            || current.mode == snapshot.mode
                                                && current.revision
                                                    == snapshot.session_permission_revision,
                                        "agent.approval_mode_changed"
                                    );
                                    store.database.unit_of_work(|tx| {
                                        approval::resolve(
                                            tx,
                                            command,
                                            &resolution,
                                            current.revision,
                                            runtime,
                                            UtcTimestamp::now(),
                                        )
                                    })?;
                                    Ok(())
                                },
                            )
                            .map_err(anyhow::Error::from)
                    },
                )
                .await??;
        }
        self.ensure_directory_grant(&v.operation, input.decision, input.approval_id)?;
        self.launch(run)?;
        Ok(self.status(&request).await?)
    }

    /// A session, workspace, or turn approval of an ordinary directory becomes a reusable
    /// grant. Allow-once does not. Protected locations are not external-path resources, so
    /// they stay ask-every-time. The grant is written after the approval commits; a retry
    /// of the same decision is idempotent and starts a run that never launched.
    fn ensure_directory_grant(
        &self,
        operation: &SecuredOperation,
        decision: aura_security_contract::ApprovalDecisionKind,
        approval_id: aura_domain_common::ApprovalRequestId,
    ) -> anyhow::Result<()> {
        let Some(term) = crate::external_grants::directory_grant_term(decision) else {
            return Ok(());
        };
        let identity = &operation.content.identity;
        let (Some(session_id), Some(workspace_id)) = (identity.session_id, identity.workspace_id)
        else {
            anyhow::bail!("agent.directory_grant_identity");
        };
        let turn_task_id = if term == aura_permission_domain::ExternalGrantTerm::Turn {
            Some(
                identity
                    .turn_task_id
                    .ok_or_else(|| anyhow::anyhow!("agent.directory_grant_turn"))?,
            )
        } else {
            None
        };
        let capabilities =
            crate::external_grants::grant_capabilities(&operation.content.capabilities);
        if capabilities.is_empty() {
            return Ok(());
        }
        for resource in &operation.content.resources {
            let aura_security_contract::ResourceDescriptor::ExternalPath {
                canonical_path,
                identity: Some(root_identity),
                directory: true,
                recursive,
                ..
            } = resource
            else {
                continue;
            };
            self.execution.external_grants.issue_approved_directory(
                crate::external_grants::ApprovedDirectoryGrant {
                    session_id,
                    workspace_id,
                    canonical_root: canonical_path.clone(),
                    root_identity: root_identity.clone(),
                    recursive_scope: *recursive,
                    capabilities: capabilities.clone(),
                    term,
                    turn_task_id,
                    expires_at: None,
                },
                approval_id,
                operation.fingerprint.0.hex(),
                identity.correlation_id,
            )?;
        }
        Ok(())
    }

    async fn launch_if_waiting(self: &Arc<Self>, run: AgentRunId) -> anyhow::Result<()> {
        let state = self
            .store
            .read(StoreStep::run(run, "agent.approval.launch_if_waiting"), move |db| {
                Ok(turns::load_run(db, run)?.state)
            })
            .await?;
        // The admission itself refuses a run that already has a live task, so this only has to
        // decide whether the persisted state is one a task should be running for.
        if matches!(state, AgentRunState::ToolsRunning { .. }) {
            self.launch(run)?;
        }
        Ok(())
    }

    pub async fn cancel(
        self: &Arc<Self>,
        input: &AgentRunRequest,
    ) -> anyhow::Result<AgentRunProjection> {
        // One deadline for the whole entry: the request, the real end and the fallback
        // interruption all share it.
        let deadline = Self::stop_deadline();
        self.status(input).await?;
        let run = input.agent_run_id;
        let at = UtcTimestamp::now();
        self.store
            .write(StoreStep::run(run, "agent.cancel.request"), move |journal| {
                journal.database.unit_of_work(|tx| store::request_cancel(tx, run, at))
            })
            .await?;
        // The cancellation signals are fired by the run's own owner; whether the work really
        // ended is a separate fact, and an end that did not happen inside the shared budget is
        // reported to the caller instead of being passed off as stopped.
        let _ = self.supervisor.request_cancel(run);
        self.stop_run_within(run, deadline).await?;
        if !self.turn(run).await?.state.terminal() {
            self.interrupt(run, "agent.cancelled", true).await?;
        }
        Ok(self.status(input).await?)
    }

    pub async fn control_mode(
        self: &Arc<Self>,
        command: CommandInvocationId,
        input: AgentModeControlRequest,
    ) -> anyhow::Result<AgentModeEpisodeView> {
        let deadline = Self::stop_deadline();
        self.ensure_session(input.session_id).await?;
        let session_id = input.session_id;
        let active_run = self
            .store
            .read(StoreStep::operation("agent.mode_control.active_run"), move |db| {
                session::load(db, session_id)?
                    .active_turn
                    .map(|id| turns::load(db, id).map(|turn| turn.agent_run_id))
                    .transpose()
            })
            .await?;
        let replay = self
            .store
            .read(StoreStep::operation("agent.mode_control.replay"), move |db| {
                aura_persistence_v2::agent::mode::remembered(db, command)
            })
            .await?;
        let action = input.action.clone();
        let stored = input.clone();
        let view = self
            .store
            .write(StoreStep::operation("agent.mode_control"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    aura_persistence_v2::agent::mode::control(
                        tx,
                        command,
                        &stored,
                        UtcTimestamp::now(),
                    )
                })
            })
            .await?;
        if !replay
            && matches!(
                action,
                AgentModeControlAction::Terminate | AgentModeControlAction::PauseGoal
            )
            && let Some(run) = active_run
        {
            let at = UtcTimestamp::now();
            self.store
                .write(StoreStep::run(run, "agent.mode_control.request_cancel"), move |journal| {
                    journal.database.unit_of_work(|tx| store::request_cancel(tx, run, at))
                })
                .await?;
            let _ = self.supervisor.request_cancel(run);
            self.stop_run_within(run, deadline).await?;
        }
        Ok(view)
    }

    /// The shutdown entry a `Drop` can call: fires every cancellation and reports every live run
    /// as residue. It never claims the work ended.
    pub fn shutdown(&self) {
        self.memory_maintenance.stop();
        // The background task is stopped first: a task still doing journal work while the runs
        // are cancelled would keep a worker busy for no reason.
        let stopped_backfill = self.derived_backfill.stop();
        let outcome = self.supervisor.shutdown_now();
        if !outcome.residue.is_empty() {
            crate::append_core_log(
                "agent.shutdown_requested",
                serde_json::json!({
                    "derived_backfill_stopped": stopped_backfill,
                    "residue": outcome.residue.iter().map(|residue| serde_json::json!({
                        "agent_run_id": residue.run.to_string(),
                        "generation": residue.generation,
                        "stage": residue.stage,
                        "observed": residue.observed,
                    })).collect::<Vec<_>>(),
                }),
            );
        }
    }

    /// The stage a run is really in, as a fact a report can name.
    #[allow(dead_code)] // read by the regression tests and the shutdown report
    #[must_use]
    pub(crate) fn run_stage(&self, run: AgentRunId) -> Option<RunStage> {
        self.supervisor.stage_of(run)
    }
}
