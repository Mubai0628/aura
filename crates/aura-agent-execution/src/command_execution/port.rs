//! Core owns live authority and terminal publication; Runner never opens SQL.
use anyhow::{Result, ensure};
use aura_backend_windows_user::{
    NotStarted, PortFuture, PortSection, ScopeFacts, WindowsUserCorePort,
};
use aura_core_broker::agent_store;
use aura_domain_common::{ArtifactId, UtcTimestamp, jcs::canonical_sha256};
use aura_execution_contract::*;
use aura_security_contract::{PermissionMode, ResourceDescriptor};
use aura_windows_user_contract::{RunnerObservation, RunnerStopReason};
use std::sync::Arc;

pub struct CorePort {
    pub persistence: Arc<aura_persistence::PersistenceCoordinator>,
    pub journal: Arc<crate::broker::BrokerJournal>,
    /// The one bounded owner of Agent Store access.
    ///
    /// This is the **same** `Arc<AgentStore>` the application, execution and web layers
    /// hold - one pool, one admission capacity, one receipt owner for the whole process,
    /// over the same `BrokerJournal` - so it is not a second source of state truth. Every
    /// synchronous section the Shell backend asks for runs on it instead of on the reactor
    /// that polls the backend future.
    pub store: Arc<agent_store::AgentStore>,
    /// The run-information index.
    ///
    /// The Shell backend is the **real executor** for a managed Shell, so it - not the Agent
    /// caller, and not a timer - publishes the managed background's start and end. The Agent-side
    /// call returning `Completed` says nothing about descendants that are still running, which is
    /// exactly the distinction this seam keeps honest.
    pub activity_index: aura_activity_runtime::ActivityIndex,
    pub activity_observer: aura_activity_runtime::ActivityObserverHandle,
    /// R2: what has already been applied for each managed background instance.
    ///
    /// It exists so `sequence` is used for what it is - the ordering of one process's
    /// observations - instead of for minting a new activity identity per update. One real instance
    /// is one key, holding the highest sequence applied and whether a terminal observation has been
    /// seen. A late `Running` after a terminal state is therefore refused rather than resurrecting
    /// a process that really stopped, and a repeated update publishes no second row.
    pub background_observations: Arc<super::background_observations::BackgroundObservations>,
}
impl WindowsUserCorePort for CorePort {
    fn filtered_environment_names(
        &self,
        request: &DispatchRequest,
        names: &[String],
    ) -> Result<()> {
        self.record_filtered_names(request, names)
    }
    fn execution_observer(
        &self,
        request: &DispatchRequest,
    ) -> Option<Arc<dyn aura_backend_windows_user::ShellExecutionObserver>> {
        Some(Arc::new(crate::agent_activity::foreground::ForegroundObservation::new(
            crate::agent_activity::execution::current_file_observer(),
            self.activity_index.clone(),
            Arc::clone(&self.background_observations),
            request.binding.attempt.clone(),
        )))
    }
    /// Boundary: the whole synchronous section runs on the bounded owner.
    ///
    /// The section is handed over whole: its lock order (session permission authority,
    /// then the journal), its indivisibility and its own answer by type are unchanged. What
    /// moves is only **where the wait happens** - off the reactor worker that also has to
    /// deliver the Core heartbeat.
    fn section_erased(
        &self,
        section: PortSection,
        work: Box<dyn FnOnce() -> Result<Box<dyn std::any::Any + Send>> + Send + 'static>,
    ) -> PortFuture<'_, Box<dyn std::any::Any + Send>> {
        let store = Arc::clone(&self.store);
        let access = if section.write {
            agent_store::StoreAccess::Write
        } else {
            agent_store::StoreAccess::Read
        };
        let step = match section.run {
            Some(run) => agent_store::StoreStep::run(run, section.name),
            None => agent_store::StoreStep::operation(section.name),
        };
        Box::pin(async move { store.run_critical(step, access, work).await })
    }
    fn scope(&self, operation: &SecuredOperation) -> Result<ScopeFacts> {
        let identity = &operation.content.identity;
        let session =
            identity.session_id.ok_or_else(|| anyhow::anyhow!("command.session_required"))?;
        let workspace_id =
            identity.workspace_id.ok_or_else(|| anyhow::anyhow!("command.workspace_required"))?;
        let projection = self.persistence.workspace_session_projection()?;
        ensure!(
            projection
                .sessions
                .iter()
                .any(|v| v.session_id == session && v.workspace_id == workspace_id),
            "command.session_workspace"
        );
        let workspace = self.persistence.workspace(workspace_id)?;
        ensure!(
            workspace.state == aura_workspace_domain::WorkspaceState::Online,
            "command.workspace_not_online"
        );
        // External grants require a resolver, never an absolute-path escape.
        ensure!(
            operation.content.resources == vec![ResourceDescriptor::WorkspaceRoot { workspace_id }],
            "command.workspace_scope"
        );
        let (root, native) = aura_backend_windows_user::file_binding::capture(
            std::path::Path::new(&workspace.canonical_root),
            false,
        )?;
        let identity = workspace
            .root_identity
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("command.workspace_identity_missing"))?;
        ensure!(
            native.info().is_directory
                && identity.volume_serial_number == root.volume_serial
                && identity.file_id
                    == root.file_id.iter().map(|v| format!("{v:02x}")).collect::<String>(),
            "command.workspace_identity_changed"
        );
        let permission = self.persistence.session_permission_state(session)?;
        Ok(ScopeFacts {
            mode: permission.mode,
            revision: permission.revision,
            read_roots: vec![root.clone()],
            write_roots: if permission.mode == PermissionMode::ReadOnly {
                vec![]
            } else {
                vec![root]
            },
        })
    }
    fn prepared_scope(&self, request: &DispatchRequest) -> Result<ScopeFacts> {
        self.retained_scope(request)
    }
    fn claim(
        &self,
        request: &DispatchRequest,
        register: &mut dyn FnMut() -> Result<()>,
    ) -> Result<UtcTimestamp> {
        let session = request
            .operation
            .content
            .identity
            .session_id
            .ok_or_else(|| anyhow::anyhow!("command.session_required"))?;
        self.persistence.with_session_permission_authority(session, |_| {
            self.retained_scope(request)?;
            let expiry = self.journal.lock()?.database.unit_of_work(|tx| {
                use aura_persistence_core::PersistenceCoreError as Error;
                let now = UtcTimestamp::now();
                aura_persistence_v2::agent::checkpoint::verify_bound_operation(
                    tx,
                    request.operation.operation_id,
                )?;
                let authority =
                    aura_persistence_v2::execution::dispatch_authority(tx, request, now)?;
                let attempt = aura_persistence_v2::execution::load(
                    tx,
                    request.operation.operation_id,
                    request.binding.attempt.ordinal.get(),
                )?;
                if attempt.binding != request.binding {
                    return Err(Error::Conflict);
                }
                aura_persistence_v2::execution::mark_dispatched(
                    tx,
                    request.operation.operation_id,
                    request.binding.attempt.ordinal.get(),
                    now,
                )?;
                Ok::<_, Error>(authority.expires_at)
            })?;
            // If registration fails, preserve the claimed Attempt as Unknown;
            // it cannot be dispatched again under another ordinal.
            register()?;
            Ok(expiry)
        })?
    }
    fn started(&self, request: &DispatchRequest, process_id: u32, at: UtcTimestamp) -> Result<()> {
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.started",
                &serde_json::json!({"process_id":process_id,"at":at}),
                at,
            )
        })?;
        Ok(())
    }
    fn begin_launch<'a>(
        &'a self,
        request: &'a DispatchRequest,
    ) -> aura_backend_windows_user::LaunchFuture<'a> {
        Box::pin(async move {
            let session = request
                .operation
                .content
                .identity
                .session_id
                .ok_or_else(|| anyhow::anyhow!("command.session_required"))?;
            // This session's own short start lease, released with the section below.
            let lease = self.persistence.permission_start_lease(session).await;
            self.persistence.with_session_permission_authority(session, |_| -> Result<()> {
                self.retained_scope(request)?;
                let journal = self.journal.lock()?;
                let db = journal.database.connection();
                aura_persistence_v2::agent::checkpoint::verify_start_operation(
                    db,
                    request.operation.operation_id,
                )?;
                aura_persistence_v2::execution::dispatch_authority(
                    db,
                    request,
                    UtcTimestamp::now(),
                )?;
                let attempt = aura_persistence_v2::execution::load(
                    db,
                    request.operation.operation_id,
                    request.binding.attempt.ordinal.get(),
                )?;
                ensure!(
                    attempt.binding == request.binding && attempt.state == AttemptState::Running,
                    "command.attempt_changed_before_start"
                );
                Ok(())
            })??;
            Ok(Box::new(lease) as aura_backend_windows_user::LaunchLease)
        })
    }
    fn progress(
        &self,
        request: &DispatchRequest,
        update: &aura_windows_user_contract::RunnerProgress,
    ) -> Result<()> {
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.progress",
                &serde_json::to_value(update)?,
                update.observed_at,
            )
        })?;
        Ok(())
    }
    fn diagnostic(&self, request: &DispatchRequest, reason: &str) -> Result<()> {
        let at = UtcTimestamp::now();
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.diagnostic",
                &serde_json::json!({"reason":reason}),
                at,
            )
        })?;
        Ok(())
    }
    /// One bounded launch sub-stage, recorded beside the other attempt facts.
    ///
    /// The stage names and the millisecond count are the backend's own measurement of this
    /// process's monotonic clock, so a reader can add up the span between Core's prepare
    /// and the script process starting without inferring it from wall-clock differences
    /// across processes. A stage that never happened has no line at all.
    fn launch_stage(
        &self,
        request: &DispatchRequest,
        stage: aura_backend_windows_user::LaunchStage,
    ) -> Result<()> {
        let at = UtcTimestamp::now();
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.launch_stage",
                &serde_json::to_value(stage)?,
                at,
            )
        })?;
        Ok(())
    }
    /// The bounded workload counts one launch observed, recorded once per launch.
    ///
    /// The stage lines say where the seconds went; these counts say why that stage was
    /// expensive. They are counts only — no path, script text, credential or nonce — so
    /// the record stays safe to keep beside the timings, and a launch that failed before
    /// reaching a step simply never counted it.
    fn launch_work(
        &self,
        request: &DispatchRequest,
        work: aura_backend_windows_user::LaunchWorkV1,
    ) -> Result<()> {
        let at = UtcTimestamp::now();
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.launch_work",
                &serde_json::to_value(work)?,
                at,
            )
        })?;
        Ok(())
    }
    fn background(
        &self,
        request: &DispatchRequest,
        update: &aura_windows_user_contract::RunnerBackgroundUpdate,
    ) -> Result<()> {
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.background",
                &serde_json::to_value(update)?,
                update.observed_at,
            )
        })?;
        // The managed background's own real activity. It is published by the executor
        // that observed the process, under its own identity - no Agent run is manufactured for it -
        // and it is **not** ended when the Agent's foreground call returns.
        //
        // One real instance is one activity: `update.sequence` orders this instance's observations
        // and is used only for that. A repeated or older observation changes nothing, and an
        // observation that arrives after a terminal one cannot resurrect the process.
        let terminal = crate::agent_activity::background_observation_ends(update.state);
        let disposition = self.background_observations.apply(
            update.attempt.operation_id,
            update.sequence,
            terminal,
        );
        let (id, generation) = crate::agent_activity::shell_background_identity(&update.attempt);
        let Some(disposition) = disposition else {
            if terminal {
                self.activity_index.end(id, generation);
            }
            self.activity_index.note_missed_publication();
            return Ok(());
        };
        if disposition != crate::agent_activity::BackgroundObservationDisposition::Publish {
            return Ok(());
        }
        if terminal {
            self.activity_index.end(id, generation);
        } else {
            let record = crate::agent_activity::shell_background_record(request, update);
            let outcome = self.activity_index.publish(record);
            self.activity_observer.observe(outcome);
        }
        Ok(())
    }
    fn runtime_stopped(
        &self,
        request: &DispatchRequest,
        stopped: bool,
        restored: bool,
    ) -> Result<TerminationEvidence> {
        let at = UtcTimestamp::now();
        let id = ArtifactId::new();
        let payload = serde_json::json!({"stopped":stopped,"file_access_restored":restored});
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_core::artifact::put(
                tx,
                id,
                &request.operation.operation_id.to_string(),
                "application/vnd.aura.runtime-stop+json",
                &serde_json::to_vec(&payload)?,
                at,
            )?;
            aura_persistence_v2::execution::shell::append(
                tx,
                request,
                "shell.runtime_stopped",
                &payload,
                at,
            )
        })?;
        Ok(if stopped {
            TerminationEvidence::Confirmed { evidence_ref: id }
        } else {
            TerminationEvidence::Unknown
        })
    }
    fn complete(
        &self,
        request: &DispatchRequest,
        observation: Option<&RunnerObservation>,
        tree_stopped: bool,
        cleanup_complete: bool,
        not_started: Option<NotStarted>,
        failure_code: Option<&str>,
    ) -> Result<TerminationEvidence> {
        // Defense in depth: only the launch path's own bounded identifiers may
        // reach the receipt or the model — never OS text, SDDL or script data.
        let failure_code = failure_code.filter(|code| {
            !code.is_empty()
                && code.len() <= 128
                && code.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        });
        let evidence_id = ArtifactId::new();
        let at = UtcTimestamp::now();
        let termination = if tree_stopped {
            TerminationEvidence::Confirmed { evidence_ref: evidence_id }
        } else if observation.is_some_and(|o| o.exit_code.is_some()) {
            TerminationEvidence::RuntimeManaged { evidence_ref: evidence_id }
        } else {
            TerminationEvidence::Unknown
        };
        let outcome = if !tree_stopped && observation.is_none() {
            ExecutionOutcome::Unknown
        } else {
            match observation {
                Some(value) => match value.stop_reason {
                    RunnerStopReason::Exited if value.exit_code.is_some() => {
                        ExecutionOutcome::Succeeded
                    }
                    RunnerStopReason::Exited => ExecutionOutcome::Unknown,
                    RunnerStopReason::OutputLimit => ExecutionOutcome::Cancelled,
                    RunnerStopReason::Cancelled | RunnerStopReason::CoreDisconnected => {
                        ExecutionOutcome::Cancelled
                    }
                    RunnerStopReason::TimedOut => ExecutionOutcome::TimedOut,
                    RunnerStopReason::Unknown => ExecutionOutcome::Unknown,
                },
                None if matches!(not_started, Some(NotStarted::Cancelled)) => {
                    ExecutionOutcome::Cancelled
                }
                None if not_started.is_some() => ExecutionOutcome::Failed,
                None => ExecutionOutcome::Unknown,
            }
        };
        // No full effect audit is implied by a generic Shell invocation.
        let side_effect = if not_started.is_some() && observation.is_none() && tree_stopped {
            SideEffectStateV1::NoEffect
        } else if observation.is_some() {
            SideEffectStateV1::Unassessed
        } else {
            SideEffectStateV1::Unknown
        };
        let mut warnings = vec![];
        if !tree_stopped && observation.is_none() {
            warnings.push("tool.side_effect.unverified".into());
        }
        if tree_stopped && !cleanup_complete {
            // no_effect only ever means "the script never started"; it must
            // never read as "the granted access was restored".
            warnings.push("tool.cleanup.incomplete".into());
        }
        if request.plan.prepared.backend.class == aura_security_contract::BackendClass::Native {
            warnings.push("execution.isolation_reduced".into());
        }
        let result = ExecutionResult {
            outcome,
            side_effect,
            termination: termination.clone(),
            completed_at: at,
            warnings,
            receipt: ExecutionReceipt {
                attempt: request.binding.attempt.clone(),
                evidence_ref: evidence_id,
                postcondition_fingerprint: None,
                rollback: if side_effect == SideEffectStateV1::NoEffect {
                    RollbackEvidence::NotApplicable
                } else {
                    RollbackEvidence::Unavailable
                },
            },
        };
        result.validate()?;
        let failure_message = if outcome == ExecutionOutcome::Failed {
            observation.and_then(super::output::failure_summary).or_else(|| {
                failure_code.map(|code| {
                    format!(
                        "The execution did not start ({code}). Inspect the recorded result; do not retry an unknown effect."
                    )
                })
            })
        } else {
            None
        };
        let evidence = serde_json::json!({"schema":"windows-user-execution-evidence-v2", "binding":canonical_sha256(&request.binding)?,
            "observation":observation,"outer_job_stopped":tree_stopped,"file_access_restored":cleanup_complete,
            "not_started":not_started,"failure_code":failure_code,"model_safe_failure_message":failure_message});
        self.journal.lock()?.database.unit_of_work(|tx| {
            let stored = aura_persistence_v2::execution::load(
                tx,
                request.operation.operation_id,
                request.binding.attempt.ordinal.get(),
            )?;
            if stored.binding != request.binding {
                return Err(aura_persistence_core::PersistenceCoreError::Conflict);
            }
            if matches!(stored.state, AttemptState::Terminal { .. }) {
                return aura_persistence_v2::execution::shell::append(
                    tx,
                    request,
                    "shell.late_observation",
                    &evidence,
                    at,
                );
            }
            aura_persistence_core::artifact::put(
                tx,
                evidence_id,
                &request.operation.operation_id.to_string(),
                "application/vnd.aura.command-evidence+json",
                &serde_json::to_vec(&evidence)?,
                at,
            )?;
            aura_persistence_v2::execution::complete(
                tx,
                request.operation.operation_id,
                request.binding.attempt.ordinal.get(),
                &result,
            )
        })?;
        Ok(termination)
    }
    fn reconcile(&self, attempt: AttemptRef) -> Result<ReconcileResult> {
        self.journal.reconcile(attempt)
    }
}
