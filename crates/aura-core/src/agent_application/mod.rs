//! User -> P06 -> P05 -> R08 -> persisted Result -> P06. The production route
//! owns lifecycle; fixtures may substitute only the external Provider response.
//!
//! OWNERSHIP
//! * [`crate::agent_store::AgentStore`] is the one owner of Agent journal access: every step in
//!   this module runs on the bounded synchronous boundary, so a journal wait never occupies a
//!   reactor worker, and a step's real result has an owner even when its caller stopped waiting.
//! * [`crate::agent_supervision::RunSupervisor`] is the one owner of a run's task. Admission,
//!   start, real end, cancellation and hand-over are separate facts there, and one `Drop` guard
//!   is the only place an active entry is released - on every exit, including a panic.
//! * Failures are attributed by type: a store-level failure is a fact about the shared
//!   dependency (`agent.store_unavailable`), an operation-level answer stays with its operation
//!   (`agent.store_failed`), and a run's own unrecordable step is a run-local diagnostic that no
//!   other session's status can ever see.
mod activity;
#[cfg(test)]
mod activity_tests;
mod approval_expiry;
pub(crate) mod attachment_entries;
mod attachment_files;
mod attachments;
mod branch;
mod browser_screenshot;
mod compaction;
mod configuration;
mod connections;
mod context;
mod context_window;
mod derived_backfill;
mod desktop;
pub(crate) mod fault;
mod hook_events;
mod inbox;
mod media_host;
mod memory_maintenance;
mod mentioned_files;
mod model_store;
mod permission_changes;
mod provider;
mod remediation;
mod submission;
#[cfg(test)]
mod tests;
mod tool_input;
mod tools;
mod transient;
mod waiting;
mod wire_history;
mod worker;
use crate::{
    agent_store::{AgentStore, StoreAccess, StoreError, StoreStep},
    agent_supervision::{RunGeneration, RunSupervisor, ShutdownOutcome},
};
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_model_transport::{
    EndpointGateRegistry, ModelCancellationToken, ProviderBindingViolationDiagnostic,
    TransportRunTerminal,
};
use aura_persistence_v2::agent::{application as store, lifecycle, turns};
use aura_projection_contract::AgentSessionProjection;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::watch;

pub(crate) struct AgentApplication {
    execution: Arc<crate::agent_execution::AgentExecution>,
    commands: Arc<crate::command_execution::CommandExecution>,
    p05: crate::P05Services,
    settings: Arc<aura_config::SettingsFacade>,
    gates: EndpointGateRegistry,
    /// The one owner of journal access on the run path.
    store: Arc<AgentStore>,
    /// The one owner of each run's real task.
    supervisor: RunSupervisor,
    /// The one owner of each run's expiry timer.
    expiry_timers: approval_expiry::ExpiryTimers,
    /// The one owner of the derived-backfill task.
    derived_backfill: derived_backfill::DerivedBackfill,
    memory_maintenance: Arc<memory_maintenance::Maintenance>,
    registry_cache: Mutex<BTreeMap<AgentRunId, tools::RegistryCache>>,
    /// Sessions whose `session.started`/`session.resumed` Hooks ran in this process.
    hook_sessions: Mutex<std::collections::BTreeSet<SessionId>>,
    /// Serializes the tool-settings read-then-save. Asynchronous because the save's journal
    /// transaction now runs on the bounded Store owner, and a `std::sync` guard held across that
    /// await would make the whole handler future non-`Send`.
    tool_configuration_lock: tokio::sync::Mutex<()>,
    /// Serializes reading the applied strategy watermark and applying the capacity, so
    /// the committed revision and the limit the dispatcher admits against can never
    /// disagree.
    strategy_apply_lock: Mutex<()>,
    /// The newest strategy revision whose global capacity has been applied. A
    /// late read of an older snapshot must not push an outdated limit back.
    applied_strategy_revision: std::sync::atomic::AtomicU64,
    /// S04-A probe only: one injected panic inside the real spawned run task.
    #[cfg(test)]
    probe_drive_panic: std::sync::atomic::AtomicBool,
}
impl AgentApplication {
    /// The channel to the browser host, for Main's results and its connection.
    pub(crate) fn browser(&self) -> Arc<crate::agent_execution::browser::BrowserBridge> {
        Arc::clone(&self.execution.browser)
    }

    pub(crate) fn media(&self) -> Arc<crate::agent_execution::media::MediaBridge> {
        Arc::clone(&self.execution.media)
    }

    /// The process-startup constructor.
    ///
    /// It runs the **one-time** startup responsibilities — the Agent recovery scan and the
    /// inbox startup — and then mints the process's one run supervisor. It must be called at
    /// most once per process, which is why its first argument is
    /// [`crate::service_rebuild::StartupRecovery`]: the type states who may call it.
    ///
    /// The work used to be inseparable from the construction, so
    /// a live runtime that had lost its Agent application could not get one back without
    /// repeating the startup scan and minting a second supervisor. Splitting them is what lets
    /// [`Self::open_from_existing_owners`] supply the missing first service instead of telling
    /// the user to restart Core.
    pub fn new_for_process_startup(
        _startup: crate::service_rebuild::StartupRecovery,
        execution: Arc<crate::agent_execution::AgentExecution>,
        commands: Arc<crate::command_execution::CommandExecution>,
        p05: crate::P05Services,
        settings: Arc<aura_config::SettingsFacade>,
        store: Arc<AgentStore>,
    ) -> anyhow::Result<Self> {
        // The store is **given**, not built here: the execution layer holds the same
        // `Arc<AgentStore>`, one pool and one admission capacity for the whole process. Building a
        // second one from the same journal would be a second route to the same state.
        debug_assert!(Arc::ptr_eq(store.journal(), &execution.journal), "agent.store_single_owner");
        // S04-E: bind the Agent Store owner to an externally named control document, when this image
        // was compiled for acceptance and the environment names one. Absent the variable this is a
        // single `env::var_os` and nothing about the run path changes.
        #[cfg(feature = "acceptance-barrier")]
        crate::agent_store::acceptance_hold::install_from_environment(&store);
        store.journal().lock()?.database.unit_of_work(|tx| {
            aura_persistence_v2::agent::recovery::startup(tx, UtcTimestamp::now())?;
            aura_persistence_v2::agent::inbox::startup(tx, UtcTimestamp::now())?;
            aura_persistence_v2::model::memory_maintenance::interrupt_previous_process(
                tx,
                execution.runtime,
                UtcTimestamp::now(),
            )
        })?;
        Ok(Self::assemble(execution, commands, p05, settings, store, RunSupervisor::new()))
    }

    /// The **live** constructor: no startup scan, no second supervisor, no second owner.
    ///
    /// This is the half that may run against a running process,
    /// because it opens nothing that the process does not already own:
    ///
    /// * the Agent Store is the **given** one, so the process keeps one journal connection and
    ///   one admission capacity;
    /// * the run supervisor is the **given** one, so an in-flight run keeps the exact task,
    ///   owner, run identity and cancel path it has now — a second supervisor over the same
    ///   runs is the specific thing that must never exist;
    /// * the expiry timers, the derived-backfill owner and the registry cache are new because
    ///   they are per-application bookkeeping with no process-wide identity: no run is
    ///   registered in them yet at this point, since every run's task belongs to the
    ///   supervisor that was passed in.
    ///
    /// The startup scan is deliberately **not** run. Its job is to reconcile runs left by a
    /// previous process; this process has already done that once, and repeating it against a
    /// live runtime would be a second reconciliation of the same rows.
    pub(crate) fn open_from_existing_owners(
        execution: Arc<crate::agent_execution::AgentExecution>,
        commands: Arc<crate::command_execution::CommandExecution>,
        p05: crate::P05Services,
        settings: Arc<aura_config::SettingsFacade>,
        store: Arc<AgentStore>,
        supervisor: RunSupervisor,
    ) -> anyhow::Result<Self> {
        debug_assert!(Arc::ptr_eq(store.journal(), &execution.journal), "agent.store_single_owner");
        Ok(Self::assemble(execution, commands, p05, settings, store, supervisor))
    }

    /// Performs the **startup scan and wait restoration** for a live supply.
    ///
    /// [`Self::open_from_existing_owners`] deliberately runs
    /// none of this, because repeating it against a live runtime would be a second reconciliation of
    /// rows this process already settled. That reasoning is only sound when startup really settled
    /// them — and with no model authority at startup there was no Agent application, so the scan,
    /// `restore_waits` and the backfill never ran at all.
    ///
    /// This is the **same production work** the startup constructor performs, reached through the
    /// already-constructed application. The process's ledger decides whether it is owed; this
    /// method never decides that for itself, so it cannot run twice.
    ///
    /// # What it deliberately preserves
    ///
    /// It does not re-dispatch anything and does not replay an `Unknown`. Runs left live by a
    /// previous process are settled by the existing recovery entry, which keeps each run's original
    /// `Attempt` and typed outcome; a task this process already has in flight is not re-scanned into
    /// a second owner, because the supervisor it supervises from is the process's existing one.
    pub(crate) fn reconcile_at_live_supply(
        self: &Arc<Self>,
        handle: &tokio::runtime::Handle,
    ) -> anyhow::Result<()> {
        // `recovery::startup` / `inbox::startup` and `restore_waits` are async, and this is reached
        // from a **synchronous** caller: the bounded configuration-recovery section, or a test
        // thread. Neither has a reactor of its own, so the work is driven on the process's own
        // runtime handle.
        //
        // # Why the call is dispatched rather than blocked on in place
        //
        // `Handle::block_on` panics when the calling thread is itself driving the runtime — which
        // happens when a recovery is invoked directly from an async test — and `block_in_place`
        // panics on a current-thread runtime. The one form that is correct for every caller is to
        // run the wait on a thread that is not a runtime worker, with the process handle entered so
        // the tasks it spawns belong to the process runtime. A failure inside is carried back rather
        // than lost, so the caller still reports why the first recovery did not complete.
        let work = Arc::clone(self);
        let handle = handle.clone();
        std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    let _entered = handle.enter();
                    handle.block_on(async {
                        let journal = work.store.journal();
                        journal.lock()?.database.unit_of_work(|tx| {
                            aura_persistence_v2::agent::recovery::startup(tx, UtcTimestamp::now())?;
                            aura_persistence_v2::agent::inbox::startup(tx, UtcTimestamp::now())?;
                            aura_persistence_v2::model::memory_maintenance::interrupt_previous_process(tx,work.execution.runtime,UtcTimestamp::now())
                        })?;
                        work.restore_waits().await
                    })
                })
                .join()
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!("the Agent first recovery did not complete"))
                })
        })
    }

    fn assemble(
        execution: Arc<crate::agent_execution::AgentExecution>,
        commands: Arc<crate::command_execution::CommandExecution>,
        p05: crate::P05Services,
        settings: Arc<aura_config::SettingsFacade>,
        store: Arc<AgentStore>,
        supervisor: RunSupervisor,
    ) -> Self {
        let memory_maintenance = memory_maintenance::Maintenance::for_runtime(execution.runtime);
        // Web.search and web.fetch read the settings' DeepSeek choice over this
        // application's model configuration.
        execution.web.set_web_service_source(Arc::new(
            crate::controlled_web::web_service::ConfiguredWebService::new(
                Arc::clone(&settings),
                p05.clone(),
            ),
        ));
        // The browser tools read the browser settings the user saved.
        execution.browser.bind_settings(Arc::clone(&settings));
        Self {
            execution,
            commands,
            p05,
            settings,
            gates: Default::default(),
            store,
            supervisor,
            expiry_timers: approval_expiry::ExpiryTimers::default(),
            derived_backfill: derived_backfill::DerivedBackfill::default(),
            memory_maintenance,
            registry_cache: Mutex::new(BTreeMap::new()),
            hook_sessions: Mutex::new(Default::default()),
            tool_configuration_lock: tokio::sync::Mutex::new(()),
            strategy_apply_lock: Mutex::new(()),
            applied_strategy_revision: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            probe_drive_panic: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The one Store owner, for the regressions and the acceptance driver.
    #[allow(dead_code)] // read by the regression tests and the acceptance driver
    #[must_use]
    pub(crate) fn store(&self) -> &Arc<AgentStore> {
        &self.store
    }

    /// The one run supervisor, for the regressions and the acceptance driver.
    #[allow(dead_code)] // read by the regression tests and the acceptance driver
    #[must_use]
    pub(crate) fn supervisor(&self) -> &RunSupervisor {
        &self.supervisor
    }

    pub(crate) async fn turn(&self, run: AgentRunId) -> Result<TurnTask, StoreError> {
        self.store
            .read(StoreStep::run(run, "agent.turn.load"), move |db| {
                turns::load(db, turns::load_run(db, run)?.turn_task_id)
            })
            .await
    }

    /// The same read on the bounded path, for the synchronous closures of the compaction runtime
    /// that cannot await their own section.
    ///
    /// Boundary: context assembly (compaction attempt preparation) runs inside a
    /// synchronous closure of the compaction runtime. The section still reads the journal directly
    /// (same connection, same `turns::load`, same errors), but it no longer runs on the reactive
    /// task that polls the compaction future: the whole read is handed to the one bounded Store
    /// owner, so the reactor worker that called it is not parked on the journal lock.
    pub(crate) async fn turn_blocking(
        self: &Arc<Self>,
        run: AgentRunId,
    ) -> anyhow::Result<TurnTask> {
        let journal = Arc::clone(&self.execution.journal);
        self.store
            .run_critical(
                StoreStep::run(run, "agent.turn.load_blocking"),
                StoreAccess::Read,
                move || {
                    let journal = journal.lock()?;
                    Ok(turns::load(
                        journal.database.connection(),
                        turns::load_run(journal.database.connection(), run)?.turn_task_id,
                    )?)
                },
            )
            .await
    }

    /// The session permission snapshot, frozen inside one bounded section.
    ///
    /// The freeze takes the session permission authority and then the journal, in that
    /// order, and everything inside it is synchronous. It is moved **whole** onto the one bounded
    /// Store owner: the ordering that makes the recorded revision the one really in force is
    /// unchanged, and the reactor that called it is not parked on either lock. A section that
    /// cannot get a permit is `agent.store_refused` - it provably did not run; a section that
    /// started and did not answer is `agent.store_unknown` and keeps its own permit.
    pub(crate) async fn freeze_permission(
        &self,
        session: SessionId,
    ) -> anyhow::Result<aura_security_contract::PermissionSnapshot> {
        let execution = Arc::clone(&self.execution);
        self.store
            .run_critical(
                StoreStep::operation("agent.permission.freeze"),
                StoreAccess::Write,
                move || execution.freeze_permission_sync(session),
            )
            .await
    }

    /// The run's projection, read through the bounded store.
    ///
    /// A read never depends on another run's historical failure text: the only thing that can
    /// refuse it is the shared store itself being unable to serve it, and that answer carries
    /// the dependency fact and the last projection this run was really seen at.
    pub(crate) async fn status(
        &self,
        input: &AgentRunRequest,
    ) -> Result<AgentRunProjection, StoreError> {
        let request = input.clone();
        let projection = self
            .store
            .read(StoreStep::run(request.agent_run_id, "agent.status.projection"), move |db| {
                store::projection(db, &request)
            })
            .await?;
        self.store
            .remember_projection(projection.run.agent_run_id, projection.run.revision.value());
        Ok(projection)
    }

    /// Records the failure of one run's own step.
    ///
    /// The record is one admitted write whose real result is handed to the receipt owner: a
    /// caller that stopped waiting, a cancel or a shutdown cannot lose it. When the record
    /// itself cannot be written, the run keeps an honest local diagnostic - it does not become
    /// a process-wide fact about every other session.
    pub(crate) async fn persist_drive_failure(
        &self,
        run: AgentRunId,
        generation: RunGeneration,
        error: &anyhow::Error,
        cancelled: bool,
    ) {
        // A task from an older generation must not overwrite the projection a newer one owns.
        if self.supervisor.generation_of(run).is_some_and(|current| current != generation) {
            self.note_run_diagnostic(
                run,
                "recording",
                "agent.stale_generation",
                "a late failure from an older run generation was not recorded",
            );
            return;
        }
        // One transaction: a wait that committed between the old read and
        // interrupt must not be cancelled, and a CAS miss must not poison
        // process-wide status (Native approval stays visible to agent.status).
        let at = UtcTimestamp::now();
        // The class is decided by the error's own type (see `fault`). No field below is derived
        // from the error's text: a binding refusal is recognized because the transport built a
        // `ProviderBindingViolationDiagnostic`, not because a message quoted a phrase.
        let classified = fault::StepFault::of(error);
        let stored = classified.stored_text();
        let diagnostic = classified.structured();
        let stored: String = stored.chars().take(500).collect();
        let payload = match &diagnostic {
            Some(diagnostic) => serde_json::json!({"error": stored, "diagnostic": diagnostic}),
            None => serde_json::json!({"error": stored}),
        };
        let step = StoreStep::run(run, "agent.drive_failure_record");
        let outcome = self
            .store
            .write_retained(step, move |journal| {
                journal.database.unit_of_work(|tx| {
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
                    if matches!(
                        turns::load_run(tx, run)?.state,
                        AgentRunState::AwaitingApproval { .. }
                            | AgentRunState::AwaitingQuery { .. }
                            | AgentRunState::AwaitingContextRemediation { .. }
                    ) {
                        return Ok(());
                    }
                    store::interrupt_with_diagnostic(
                        tx,
                        run,
                        "agent.execution_interrupted",
                        cancelled,
                        at,
                        diagnostic.as_ref(),
                    )
                })
            })
            .await;
        match outcome {
            Ok(_) => {
                // The run's own failure is durable again: its local diagnostic is spent.
                self.store.diagnostics().clear(run);
            }
            Err(error) if error.not_started() => {
                self.note_run_diagnostic(
                    run,
                    "recording",
                    "agent.failure_record_not_started",
                    error.to_string(),
                );
            }
            Err(error) => {
                if self.run_is_waiting(run).await {
                    // A durable wait is still visible; the interruption belongs to the wait.
                    return;
                }
                self.note_run_diagnostic(
                    run,
                    "recording",
                    error.code(),
                    format!(
                        "the run's failure record could not be written: {error}{}",
                        error
                            .receipt()
                            .map(|receipt| format!(" (receipt {receipt})"))
                            .unwrap_or_default()
                    ),
                );
            }
        }
    }

    /// One run-local diagnostic, recorded and logged. Never a process-wide fault.
    pub(crate) fn note_run_diagnostic(
        &self,
        run: AgentRunId,
        stage: &'static str,
        code: &'static str,
        detail: impl Into<String>,
    ) {
        let detail = detail.into();
        crate::append_core_log(
            "agent.run_diagnostic",
            serde_json::json!({
                "agent_run_id": run.to_string(),
                "stage": stage,
                "code": code,
                "detail": detail,
            }),
        );
        self.store.diagnostics().record(run, stage, code, detail);
    }

    async fn run_is_waiting(&self, run: AgentRunId) -> bool {
        self.store
            .read(StoreStep::run(run, "agent.run_is_waiting"), move |db| {
                turns::load_run(db, run).map(|loaded| {
                    matches!(
                        loaded.state,
                        AgentRunState::AwaitingApproval { .. }
                            | AgentRunState::AwaitingQuery { .. }
                            | AgentRunState::AwaitingContextRemediation { .. }
                    )
                })
            })
            .await
            .unwrap_or(false)
    }

    /// The store fact the UI and the logs read, without inventing any current state.
    #[allow(dead_code)] // read by the regression tests and any display that reports staleness
    #[must_use]
    pub(crate) fn store_health(
        &self,
        dependency: crate::agent_store::StoreDependency,
    ) -> crate::agent_store::StoreHealthSnapshot {
        self.store.health(dependency)
    }

    /// Stops every live run under one deadline and reports real residue separately.
    ///
    /// The product `Drop` path uses [`RunSupervisor::shutdown_now`], which cannot wait; this is
    /// the bounded entry a caller that can await uses, and the one the regressions drive.
    #[allow(dead_code)]
    pub(crate) async fn stop_all_within(&self, deadline: Instant) -> ShutdownOutcome {
        let outcome = self.supervisor.stop_all_within(deadline).await;
        crate::append_core_log(
            "agent.shutdown",
            serde_json::json!({
                "ended": outcome.ended.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "residue": outcome.residue.iter().map(|residue| serde_json::json!({
                    "agent_run_id": residue.run.to_string(),
                    "generation": residue.generation,
                    "stage": residue.stage,
                    "observed": residue.observed,
                })).collect::<Vec<_>>(),
                "waited_ms": outcome.waited_ms,
            }),
        );
        outcome
    }

    #[cfg(test)]
    pub(crate) async fn persist_drive_failure_for_test(&self, run: AgentRunId, error: &str) {
        let generation = self.supervisor.generation_of(run).unwrap_or(0);
        let error = anyhow::anyhow!("{error}");
        self.persist_drive_failure(run, generation, &error, false).await;
    }
    /// S04-A probe: arms one injected panic in the real spawned run task.
    #[cfg(test)]
    pub(crate) fn probe_arm_drive_panic(&self) {
        self.probe_drive_panic.store(true, std::sync::atomic::Ordering::Release);
    }
    /// S04-R1 probe: makes the one Store owner run its sections inline, i.e. the pre-boundary
    /// behaviour, so the isolation regression can distinguish the property from the call shapes.
    #[cfg(test)]
    pub(crate) fn probe_force_inline_store_boundary(&self, inline: bool) {
        self.store.force_inline_for_test(inline);
    }
    /// S04-A probe: the runs the application still believes are active.
    #[cfg(test)]
    pub(crate) fn probe_active_runs(&self) -> Vec<AgentRunId> {
        self.supervisor.active_runs()
    }
    /// S04-A probe: whether the run's real task ended inside `within`.
    #[cfg(test)]
    pub(crate) async fn probe_await_stopped(
        &self,
        run: AgentRunId,
        within: std::time::Duration,
    ) -> bool {
        self.supervisor.wait_ended(run, Instant::now() + within).await.real_end
    }
}
#[cfg(test)]
mod test_shutdown;
