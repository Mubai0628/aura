//! Production Agent adapters. No Main authority, Package installer, test Driver,
//! or model-controlled channel enters this execution service.
mod active_calls;
mod admission;
mod unknown_tool;
use unknown_tool::unknown_tool_feedback;
mod backends;
mod batch;
mod broker;
pub(crate) mod browser;
mod command_tools;
mod control;
mod control_resources;
mod deletion;
mod dispatch;
mod file_targets;
pub(crate) mod files;
mod git;
mod managed_stops;
pub(crate) mod mcp_tools;
pub(crate) mod media;
mod memory;
pub(crate) mod network;
mod operation;
pub(crate) mod package_tools;
mod path_input;
mod permission_changes;
mod planning;
pub(crate) mod protected_paths;
mod recovery;
pub(crate) mod recycle;
mod registry;
mod registry_change;
mod reservation;
mod selection;
mod shell_risks;
pub(crate) mod skill_tools;
mod task;
mod terminal;
#[cfg(test)]
mod tests;
mod web;
mod workspace_init;
use crate::agent_store::{StoreAccess, StoreStep};
pub(crate) use admission::AdmittedBatch;
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_execution_orchestrator::ExecutionOrchestrator;
use aura_security_contract::*;
use aura_tool_contract::*;
use aura_tool_scheduler::ValidatedToolCallV1;
pub(crate) use batch::BatchOutcome;
pub(crate) use dispatch::CallOutcome;
use std::sync::Arc;

type ActivityStartProbe = Arc<dyn Fn(&aura_execution_contract::AttemptRef) + Send + Sync>;

pub(crate) struct AgentExecution {
    pub persistence: Arc<aura_persistence::PersistenceCoordinator>,
    pub journal: Arc<crate::broker::BrokerJournal>,
    /// The one bounded owner of Agent Store access.
    ///
    /// It is the **same** `Arc<AgentStore>` the application layer holds - one pool, one admission
    /// capacity, one receipt owner for the whole process, and the same `BrokerJournal` underneath,
    /// so this is not a second source of state truth. Execution steps that used to take
    /// `journal.lock()` inline on a reactive task run their real synchronous critical section on
    /// this owner instead: [`crate::agent_store::AgentStore::run_critical`] moves the whole section
    /// to a bounded pool thread, and the reactor worker that called it is not parked on the lock.
    pub store: Arc<crate::agent_store::AgentStore>,
    pub fs: Arc<aura_fs_tools::FsToolService>,
    pub workspace: Arc<aura_workspace::WorkspaceRuntime>,
    pub web: Arc<crate::controlled_web::ControlledWeb>,
    pub activities: crate::activity::AgentActivities,
    /// The one run-information index of this process.
    ///
    /// It is a **separate owner** from `activities` above on purpose. `AgentActivities` is the Web
    /// permission/cancel authority; this is the read-only collection of facts the real executors
    /// publish. Merging them would make one component both the observer and the authority over
    /// execution, which the design forbids. Nothing here admits, denies, stops or releases
    /// anything, and A's common-admission service does not consult it.
    pub activity_index: aura_activity_runtime::ActivityIndex,
    /// The observer the publishers report a dropped fact to. It is on the publish path itself, so
    /// "the index lost something" is observable without going through whatever queue overflowed.
    pub activity_observer: aura_activity_runtime::ActivityPublicationObserver,
    /// The one owner of the global ordinary-Tool execution capacity. Its limit
    /// moves with committed settings; it is never wrapped in a second limiter.
    pub orchestrator: ExecutionOrchestrator,
    pub runtime: RuntimeInstanceId,
    /// Resolves which authorized external root a model path belongs to. It is a
    /// stateless view over the same store the security surface uses, and it only
    /// classifies: the permission engine checks the resource and the file layer
    /// re-verifies the capability, the scope and the identity it opened.
    pub external_grants: Arc<crate::external_grants::ExternalGrantService>,
    pub control_resources: Arc<control_resources::ControlResources>,
    /// R3: the verified resource references each run's current task has really named.
    ///
    /// WHY THIS EXISTS, AND WHY IT IS NOT A SECOND SOURCE OF TRUTH
    /// The model step's activity fragment must know which physical targets *this task* can see, so
    /// that a row from another session or workspace touching one of them stays associated. That set
    /// has exactly one legitimate source: the descriptors A already verified on this task's own
    /// operations, before it admitted them. It is recorded here at admission, from the same
    /// `SecuredOperation` the call is dispatched with - no path is re-resolved, no journal is read,
    /// and no *running* row is consulted, which matters because a run's row is removed when its call
    /// ends and so cannot serve as the task's resource history.
    ///
    /// It holds references only: the descriptor, the run and the task. Nothing here admits, denies
    /// or orders anything, and it is bounded by [`MAX_TASK_RESOURCE_RUNS`].
    pub task_resources: std::sync::Mutex<
        std::collections::BTreeMap<AgentRunId, Vec<aura_activity_domain::ActivityResource>>,
    >,
    /// R1 evidence seam: an observer of the exact instant a real attempt is reserved.
    ///
    /// It is `None` in every ordinary run, its return value is discarded, and it cannot influence
    /// the dispatch - it exists so a directed test can inspect the index **at** the publication
    /// instant instead of inferring it afterwards. A probe that could change the outcome would make
    /// the evidence it produces worthless. It is a plain `Option` field rather than a
    /// `#[cfg(test)]` one because the dispatch that reads it compiles in the library crate, so a
    /// test-only field would be invisible from the very call site that has to honour it.
    active_calls: Arc<active_calls::ActiveCalls>,
    pub activity_start_probe: std::sync::Mutex<Option<ActivityStartProbe>>,
    /// The process's MCP servers, set once at startup.
    pub mcp: std::sync::OnceLock<Arc<crate::mcp::McpRuntime>>,
    /// The user's Skills, set once at startup.
    pub skills: std::sync::OnceLock<Arc<crate::skills::SkillRuntime>>,
    /// The installed plugins, whose ready ones bring Hooks too.
    pub plugins: std::sync::OnceLock<Arc<crate::plugins::PluginRuntime>>,
    /// The channel to the browser host and the browser settings it reads.
    pub browser: Arc<browser::BrowserBridge>,
    pub media: Arc<media::MediaBridge>,
}

#[cfg(test)]
impl AgentExecution {
    /// Installs the R1 start observer for one test.
    ///
    /// It is behind `cfg(test)` deliberately: no product code can install one, so a probe can
    /// never exist in a shipped runtime. The **field** it writes is compiled unconditionally
    /// because the real dispatch reads it, and that read is what makes the evidence a measurement.
    pub(crate) fn set_activity_start_probe(&self, probe: ActivityStartProbe) {
        if let Ok(mut slot) = self.activity_start_probe.lock() {
            *slot = Some(probe);
        }
    }
}

/// How many runs' resource references are retained at once (R3).
///
/// The map exists so one model step can ask what its own task named. It is bounded because a
/// long-lived Core serves many runs: the oldest entry is dropped rather than growing without
/// limit, and the bound is generous enough that any run still producing model steps is present.
pub(crate) const MAX_TASK_RESOURCE_RUNS: usize = 256;
/// Constructed from the exact admitted Registry and immutable call, then kept
/// for every pre-dispatch check. Never deserialize this trusted evidence.
#[derive(Clone)]
pub(crate) struct CallContext {
    pub batch: ToolBatchRequestV1,
    pub registry: ToolRegistrySnapshotV1,
    pub validated: ValidatedToolCallV1,
    pub snapshot: PermissionSnapshot,
    pub operation: SecuredOperation,
    pub verified_external_capabilities: Vec<ResolvedCapability>,
    pub verified_external_grants: Vec<aura_security_contract::VerifiedExternalCapability>,
    pub observed_resources: Arc<std::sync::Mutex<Vec<aura_activity_domain::ActivityResource>>>,
}
impl AgentExecution {
    /// Constructs the execution service with the committed global ordinary-Tool
    /// capacity already applied.
    ///
    /// The capacity is not a settings-page side effect: startup passes the
    /// committed value here, so a restarted runtime enforces the stored limit
    /// before any request or UI read happens.
    ///
    /// `store` is a separate argument rather than something built here on purpose: the caller
    /// passes the **same** `Arc<AgentStore>` it gives to `AgentApplication::new`, so the process
    /// has one bounded owner and one admission capacity instead of two.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_global_concurrency(
        services: &crate::P03Services,
        persistence: Arc<aura_persistence::PersistenceCoordinator>,
        journal: Arc<crate::broker::BrokerJournal>,
        store: Arc<crate::agent_store::AgentStore>,
        web: Arc<crate::controlled_web::ControlledWeb>,
        activities: crate::activity::AgentActivities,
        activity_index: aura_activity_runtime::ActivityIndex,
        runtime: RuntimeInstanceId,
        global_tool_concurrency: u32,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (MIN_MAX_CONCURRENT_TOOL_CALLS..=MAX_MAX_CONCURRENT_TOOL_CALLS)
                .contains(&global_tool_concurrency),
            "tools.invalid_setting:max_concurrent_tool_calls"
        );
        let external_grants =
            Arc::new(crate::external_grants::ExternalGrantService::new(persistence.clone()));
        external_grants.bind_turn_journal(Arc::clone(&journal));
        Ok(Self {
            control_resources: Arc::new(control_resources::ControlResources::for_databases(
                &persistence,
                &journal,
            )?),
            external_grants,
            persistence,
            journal,
            store,
            fs: services.fs.clone(),
            workspace: services.workspace.clone(),
            web,
            activities,
            activity_index,
            activity_observer: aura_activity_runtime::ActivityPublicationObserver::default(),
            runtime,
            orchestrator: ExecutionOrchestrator::new(global_tool_concurrency)?,
            task_resources: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            active_calls: Arc::new(active_calls::ActiveCalls::default()),
            activity_start_probe: std::sync::Mutex::new(None),
            mcp: std::sync::OnceLock::new(),
            skills: std::sync::OnceLock::new(),
            plugins: std::sync::OnceLock::new(),
            browser: Arc::default(),
            media: Arc::default(),
        })
    }

    /// Records the verified resource references this call's task really named (R3).
    ///
    /// It copies the descriptors straight off the secured operation A already resolved, so it
    /// performs no I/O and cannot fail. It is idempotent per reference: the same descriptor
    /// recorded twice is stored once, and two different runs never share an entry.
    pub(crate) fn record_task_resources(&self, call: &CallContext) {
        let run_id = call.batch.invocation_identity.agent_run_id;
        let Ok(mut recorded) = self.task_resources.try_lock() else {
            return;
        };
        let Ok(observed) = call.observed_resources.try_lock() else {
            return;
        };
        let entry = recorded.entry(run_id).or_default();
        for descriptor in observed.iter().take(aura_activity_domain::MAX_ACTIVITY_RESOURCES) {
            if entry.len() < 128 && !entry.contains(descriptor) {
                entry.push(descriptor.clone());
            }
        }
        // Bounded: the oldest run is dropped rather than letting a long-lived process grow. A run
        // whose entry was dropped simply reports no relevant targets, which is the honest answer
        // for a task whose references are no longer retained.
        while recorded.len() > MAX_TASK_RESOURCE_RUNS
            && let Some(oldest) = recorded.keys().next().copied()
        {
            recorded.remove(&oldest);
        }
    }

    /// The verified resource references one run's current task can see (R3).
    ///
    /// This is the whole input to "which physical targets is this step associated with": the
    /// descriptors of the operations this run really admitted. Nothing is re-resolved here.
    #[must_use]
    pub(crate) fn task_resource_references(
        &self,
        run_id: AgentRunId,
    ) -> Vec<aura_activity_domain::ActivityResource> {
        self.task_resources
            .try_lock()
            .ok()
            .and_then(|references| references.get(&run_id).cloned())
            .unwrap_or_default()
    }

    /// The runtime generation this executor publishes under.
    ///
    /// It is derived from the runtime instance the whole process shares, so an
    /// activity published by one Core generation is distinguishable from the same
    /// nominal call in the next. It is not a second source of truth: it is a
    /// deterministic fold of the identity the process already has.
    #[must_use]
    pub(crate) fn runtime_generation(&self) -> u32 {
        let digest = aura_domain_common::jcs::canonical_sha256(&(
            "aura.activity.runtime_generation.v1",
            self.runtime,
        ));
        digest
            .map(|hash| {
                u32::from_str_radix(hash.hex().get(..8).unwrap_or("0"), 16).unwrap_or(1).max(1)
            })
            .unwrap_or(1)
    }

    /// One activity publication for a real Agent tool call.
    ///
    /// It is a **short, synchronous, in-memory** publication: it takes the index's own mutex,
    /// copies facts this executor already holds, and returns. It never re-opens a path, resolves a
    /// file identity, waits on the journal, or takes a business lock. Every outcome - including a
    /// refusal - is a value, so a broken or full index can never fail the tool it is describing.
    ///
    /// WHERE IT IS CALLED FROM, AND WHY THAT MATTERS
    /// Not from the top of `execute_call`: the scheduler slot, A's resource admission and the real
    /// `Attempt` reservation all happen inside `ExecutionOrchestrator::execute_observed`, so a
    /// publication before it would report approval, capacity and resource waiting as `running`.
    /// The orchestrator invokes the publisher at the one instant a slot is held and the attempt is
    /// reserved, and hands over **that** `AttemptRef`.
    ///
    /// The published row therefore carries the real attempt ordinal and the real operation id
    /// rather than a constant. The returned [`PublishedActivity`] is recorded in the dispatch's
    /// [`crate::agent_activity::AgentActivityLease`], whose `Drop` ends it on every exit path -
    /// including a panic unwind - so "no terminated call is left running" does not depend on a
    /// future edit remembering to add an `end()`.
    pub(crate) fn publish_agent_tool_activity(
        &self,
        call: &CallContext,
        generation: u32,
        operation_id: aura_domain_common::SecurityOperationId,
        attempt_ordinal: u32,
    ) -> crate::agent_activity::PublishedActivity {
        use aura_activity_domain::*;
        let tool_group = crate::agent_activity::tool_group_for(&call.operation.content.action_kind);
        // A Shell call is its own publisher kind. It shares the Agent's real identity -
        // it really is the Agent's call - but it is reported as a Shell, because its
        // write set is not provable and its descendants outlive the call.
        let kind = if tool_group == ActivityToolGroup::Shell {
            ActivityKind::ShellForeground
        } else {
            ActivityKind::AgentTool
        };
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: call.validated.call.tool_call_id,
            operation_id,
            attempt_ordinal,
        };
        let id = ActivityId::derive(kind, &execution, generation);
        let record = ActivityRecord {
            tool_name: call.operation.content.action_kind.clone(),
            private_source: if call.operation.content.resources.iter().all(|resource| {
                matches!(
                    resource,
                    ResourceDescriptor::WorkspacePath { .. }
                        | ResourceDescriptor::WorkspaceRoot { .. }
                )
            }) {
                aura_activity_domain::ActivityPrivateSource::WorkspaceMetadata
            } else {
                aura_activity_domain::ActivityPrivateSource::PersonalData
            },
            id,
            kind,
            execution,
            generation,
            tool_id: call.validated.definition.tool_id,
            tool_group,
            owner: ActivityOwnerScope {
                session_id: call.batch.invocation_identity.session_id,
                workspace_id: call.snapshot.workspace_id,
                agent_run_id: Some(call.batch.invocation_identity.agent_run_id),
                turn_task_id: Some(call.batch.invocation_identity.turn_task_id),
                model_step_id: Some(call.batch.invocation_identity.model_step_id),
                runtime_instance_id: self.runtime,
            },
            resources: crate::agent_activity::project_resources(call, tool_group),
            parameters: crate::agent_activity::project_parameters(call, tool_group),
            started_at: UtcTimestamp::now(),
            runtime_instance_id: self.runtime,
            published_at_revision: Revision::INITIAL,
        };
        let outcome = self.activity_index.publish(record);
        self.activity_observer.observe(outcome);
        crate::agent_activity::PublishedActivity { id, generation }
    }

    /// One real synchronous critical section of this execution service, run **whole** on the one
    /// bounded Store owner the application layer also uses.
    ///
    /// WHY THIS EXISTS
    /// Several execution steps take `journal.lock()` directly, and that is only correct while the
    /// dynamic path that reaches them is a real blocking thread. Before this change the path that
    /// reached them was the reactive task polling the dispatch future -
    /// `aura-execution-orchestrator` drives backend futures on the reactor and contains no
    /// `spawn_blocking`, `block_in_place` or `std::thread` - so a slow SQLite section parked a
    /// Tokio worker that also has to deliver the Core heartbeat.
    ///
    /// WHAT IT DOES NOT CHANGE
    /// It is the same `Arc<AgentStore>`, the same pool, the same admission and the same budget as
    /// every other Agent step: not a second pool and not an unbounded `spawn_blocking`. The section
    /// is handed over **whole** - its lock order and its indivisibility are unchanged - and it is
    /// still the same `BrokerJournal` over the same database, so this moves *where the wait
    /// happens*, not what is read or written. No network wait and no whole Agent run is moved here:
    /// every call below is a synchronous section with no `await` inside it.
    pub(super) async fn section<T, F>(
        self: &Arc<Self>,
        step: StoreStep,
        access: StoreAccess,
        work: F,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    {
        self.store.run_critical(step, access, work).await
    }

    /// [`Self::verify_source`] on the bounded path.
    ///
    /// `verify_source` takes the journal lock directly, so it is reached from an async body
    /// only through this wrapper, which runs it whole on the one bounded Agent Store owner.
    pub(super) async fn verify_source_section(
        self: &Arc<Self>,
        call: &CallContext,
    ) -> anyhow::Result<()> {
        let service = Arc::clone(self);
        let verified = call.clone();
        self.section(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "agent.verify_source"),
            StoreAccess::Read,
            move || service.verify_source(&verified),
        )
        .await
    }

    /// Applies a committed execution-strategy value to the global capacity owner.
    /// Existing members are never revoked; only new admissions see the change.
    pub fn set_global_tool_concurrency(&self, limit: u32) -> anyhow::Result<()> {
        anyhow::ensure!(
            (MIN_MAX_CONCURRENT_TOOL_CALLS..=MAX_MAX_CONCURRENT_TOOL_CALLS).contains(&limit),
            "tools.invalid_setting:max_concurrent_tool_calls"
        );
        self.orchestrator.set_global_concurrency(limit)?;
        Ok(())
    }

    /// The committed global limit and how many calls currently hold a slot.
    ///
    /// Reading the applied fact back is what separates "the save returned a new
    /// revision" from "dispatch really admits against the new limit".
    #[must_use]
    #[allow(dead_code)]
    pub fn global_tool_capacity(&self) -> (u32, u32) {
        self.orchestrator.global_capacity()
    }

    /// The synchronous body of the permission freeze.
    ///
    /// Contract: it takes the session permission authority and then the journal, in that
    /// order, and holds them across the snapshot insert, so it is a synchronous critical section
    /// and only correct on a real blocking thread. Production reaches it through
    /// `AgentApplication::freeze_permission` (one `run_critical`) and through
    /// [`Self::admit_output_sync`] (itself inside the admission section); the `agent_application`
    /// and `agent_execution` test modules call it directly on a test thread. The name ends in
    /// `_sync` so that no reader has to guess which of the two entry points is the bounded one.
    pub fn freeze_permission_sync(&self, session: SessionId) -> anyhow::Result<PermissionSnapshot> {
        self.persistence.with_session_permission_authority(
            session,
            |current| -> anyhow::Result<_> {
                let mut store = self.journal.lock()?;
                let metadata =
                    aura_persistence_v2::workspace::session(store.database.connection(), session)?
                        .ok_or_else(|| anyhow::anyhow!("agent.session_missing"))?;
                let snapshot = PermissionSnapshot {
                    id: PermissionSnapshotId::new(),
                    session_id: session,
                    workspace_id: metadata.workspace_id,
                    mode: current.mode,
                    session_permission_revision: current.revision,
                    active_grant_ids: vec![],
                    created_at: UtcTimestamp::now(),
                };
                store.database.unit_of_work(|tx| {
                    aura_persistence_v2::security::insert_snapshot(tx, &snapshot)
                })?;
                Ok(snapshot)
            },
        )?
    }
}
fn rejected(error: impl std::fmt::Display) -> BackendError {
    BackendError::DispatchRejected { reason: error.to_string() }
}
fn preparation(error: impl std::fmt::Display) -> BackendError {
    BackendError::PreparationRejected { reason: error.to_string() }
}
pub(crate) fn registry_mode_hash(
    snapshot: &PermissionSnapshot,
) -> Result<aura_domain_common::jcs::CanonicalHashV1, aura_domain_common::jcs::CanonicalJsonError> {
    aura_domain_common::jcs::canonical_sha256(&(
        snapshot.mode,
        snapshot.session_permission_revision,
        snapshot.session_id,
        snapshot.workspace_id,
    ))
}
