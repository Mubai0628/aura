use crate::*;

/// The owners a **live** supply reuses, gathered once so it cannot accidentally open a second
/// copy of any of them.
///
/// Every field is the process's existing owner, cloned in. A
/// supply that constructed its own `PersistenceCoordinator` or `BrokerJournal` would be a
/// second route to the same state, and a supply that constructed its own `RunSupervisor`
/// would be a second owner of runs that already have one.
pub(crate) struct P03Dependencies {
    pub(crate) persistence: Arc<PersistenceCoordinator>,
    pub(crate) settings: Arc<SettingsFacade>,
    pub(crate) activity_index: aura_activity_runtime::ActivityIndex,
    pub(crate) activity_observer: aura_activity_runtime::ActivityObserverHandle,
    pub(crate) runtime_instance_id: RuntimeInstanceId,
    /// The process's **one** bounded synchronous boundary. A supplied service must run its
    /// sections on this pool rather than opening a second one, which would be a second
    /// admission capacity over the same disk work.
    pub(crate) sync: crate::sync_boundary::SyncBoundary,
    /// The facts needed to build the process's sidecar set when it never got one.
    ///
    /// `None` only when the runtime was assembled without them, which does not happen on the
    /// real path. It is an `Option` so the struct stays constructible in the minimal test
    /// fixtures that never reach a sidecar supply.
    pub(crate) sidecar_facts: Option<SidecarFacts>,
    /// The committed tool-call concurrency, so a supplied execution layer is built with the
    /// same capacity the startup path would have used rather than an invented one.
    pub(crate) max_concurrent_tool_calls: u32,
    /// The process's own Tokio runtime, captured so a live supply can start the tasks a
    /// service owns (the sidecar supervisor) from the bounded **synchronous** boundary, which
    /// has no reactor of its own.
    pub(crate) runtime_handle: Option<tokio::runtime::Handle>,
}

/// The two release facts `SecuritySidecars::deferred` needs, retained for a live supply.
pub(crate) struct SidecarFacts {
    pub(crate) options: CoreOptions,
    pub(crate) release: Arc<CoreReleaseContext>,
}

/// The live runtime's answer to "supply the services this repair now permits".
///
/// This is what makes a configuration recovery able to report
/// **capability** restoration rather than only a cleared record, and it is what removes the
/// "a restart is required" answer the previous round gave for the Agent application, the
/// security authority and the model authority.
///
/// # How it avoids repeating startup work
///
/// The startup-only responsibilities are named in [`service_rebuild::StartupRecovery`] and are
/// **not** reachable from here:
///
/// * the Agent/inbox startup scan belongs to `AgentApplication::new_for_process_startup`, and
///   this path calls `open_from_existing_owners`, which scans nothing;
/// * finishing pending model-configuration transactions belongs to the P05 startup half, and
///   this path calls the constructor that skips it;
/// * the P05/P07 maintenance tasks are started by the startup path and are retained by the
///   runtime, not restarted here.
///
/// # How it avoids a second owner
///
/// * the `AgentStore`, `PersistenceCoordinator`, `BrokerJournal`, `SyncBoundary`,
///   `SettingsFacade`, activity owners and `SecuritySidecars` readiness store are the
///   process's existing ones, cloned in;
/// * the **run supervisor, expiry timers and derived-backfill owner are the process's
///   existing ones** — taken by cloning `AgentApplication`'s own handles — so a run that is
///   already in flight keeps the exact task, owner, run identity and cancel path it has;
/// * a per-capability [`service_rebuild::ConstructionClaim`] is held across the construction,
///   so two concurrent recoveries cannot both build a service and then discard one.
pub(crate) struct RuntimeDependentRebuilder {
    /// The runtime's **own** slots, shared rather than recreated. A supply fills these, so the
    /// service it supplies is the very one the runtime then serves.
    pub(crate) slots: service_rebuild::LiveServices,
    /// The owners a live construction reuses.
    pub(crate) dependencies: Arc<P03Dependencies>,
    /// The process's existing Agent Store, execution layer and the Agent application's own
    /// handles, retained so a supplied Agent application reuses them instead of minting a
    /// second of any.
    ///
    /// The execution layer is now a **slot** too. It used to be a
    /// plain `Option` decided at startup, and a damaged `tools.json` means it was never built —
    /// so there was nothing for a supplied command backend or Agent application to attach to,
    /// and the capability could not come back without a restart. Supplying it creates the one
    /// execution layer the process never got, over the process's existing owners.
    pub(crate) agent_owners: Arc<std::sync::Mutex<Option<Arc<AgentLiveOwners>>>>,
    /// The Agent Store the execution layer is bound to. The process's one, never a second.
    pub(crate) agent_store: Arc<agent_store::AgentStore>,
    /// The controlled-web owner the execution layer needs, retained so a supply reuses it.
    pub(crate) activities: Arc<activity::AgentActivities>,
    /// The process's one HTTP gateway, reused by a supplied execution layer's controlled web so
    /// a supply does not open a second egress owner.
    pub(crate) broker_http: Arc<aura_network_control::CoreHttpClientGateway>,
    pub(crate) broker: Arc<broker::BrokerJournal>,
    pub(crate) release: Arc<CoreReleaseContext>,
    /// The process's real record of which one-time startup steps have finished.
    ///
    /// The live constructors were written on the premise "the
    /// startup work has already been done". That premise is false whenever a capability was refused
    /// at startup — a damaged `models.json` means no P05, so no maintenance task; no model
    /// authority means no Agent application, so no inbox scan, no wait restoration and no derived
    /// backfill. The ledger is how the supply path learns which of those still has to happen, and
    /// how it ensures exactly one caller discharges each.
    pub(crate) startup_ledger: Arc<service_rebuild::StartupLedger>,
    /// The bounded synchronous boundary, for the one-time steps that must read state through it.
    pub(crate) sync: crate::sync_boundary::SyncBoundary,
    /// The settings facade, for the maintenance task a supplied model authority owns.
    pub(crate) settings: Arc<SettingsFacade>,
    /// Retains the maintenance task a supplied model authority starts, so the process owns it and
    /// it is aborted with the runtime rather than leaked.
    pub(crate) extra_tasks: Arc<std::sync::Mutex<Vec<RuntimeBackgroundTask>>>,
}

/// The process's existing Agent-side owners, retained for a live Agent supply.
pub(crate) struct AgentLiveOwners {
    pub(crate) execution: Arc<agent_execution::AgentExecution>,
    pub(crate) store: Arc<agent_store::AgentStore>,
    /// The **existing** supervisor. Cloned, never newly minted: a second supervisor over the
    /// same runs is the one thing item 14 forbids outright.
    pub(crate) supervisor: agent_supervision::RunSupervisor,
}

/// Turns one construction-claim outcome into the supply path's result.
///
/// `Contended` is its own outcome. A caller that could not take
/// the responsibility within the bounded wait **built nothing**, and must say so with the real
/// wait rather than reporting a generic failure — and it must never fall through to constructing a
/// second service, which is the opposite of what a contended claim means.
///
/// `AlreadyPresent` is a **success**: a concurrent recovery published the service, so the
/// capability really is supplied. Reporting a refusal there would tell the user a capability is
/// missing when it is present.
pub(crate) fn claim_refusal<T, E>(
    outcome: service_rebuild::ClaimOutcome<std::result::Result<T, E>>,
) -> std::result::Result<(), service_rebuild::ConstructionRefusal>
where
    E: std::fmt::Display,
{
    use service_rebuild::{ClaimOutcome, ConstructionRefusal as Refusal};
    match outcome {
        ClaimOutcome::Built(Ok(_)) => Ok(()),
        ClaimOutcome::Built(Err(error)) => Err(Refusal::Failed(error.to_string())),
        ClaimOutcome::AlreadyPresent => Ok(()),
        ClaimOutcome::Contended { waited_ms } => Err(Refusal::Contended { waited_ms }),
        ClaimOutcome::Poisoned => {
            Err(Refusal::Failed("the construction responsibility was unavailable".into()))
        }
    }
}
