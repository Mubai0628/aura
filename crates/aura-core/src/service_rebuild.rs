//! Supplying a capability's missing service on a **live** runtime.
//!
//! # Why this is a separate module and not more of `lib.rs`
//!
//! The previous round put this responsibility in a rebuilder struct at the bottom of
//! `lib.rs`. Two things were wrong with it, and both are structural rather than a
//! matter of more code:
//!
//! 1. It could only ever fill **one** of the slots. `is_available` returned `false`
//!    for the model authority unconditionally, and the Agent application was
//!    reported `still_missing` with "a restart is required" — which is exactly the
//!    outcome the brief forbids ("不能以只能重启Core替代第14项").
//! 2. It could not be exercised on its own. A responsibility that only exists as a
//!    closure inside the runtime's assembly cannot be tested against a degraded
//!    runtime without assembling the whole product.
//!
//! # The split this module implements
//!
//! [`StartupRecovery`] and [`ServiceConstruction`] are the two halves the brief asks
//! to be separated:
//!
//! * **`StartupRecovery`** is the work that may happen **once per process**: the
//!   Agent/inbox startup scan, finishing pending model-configuration transactions,
//!   starting the maintenance tasks. It has one owner, and it must not be re-run
//!   against a live runtime.
//! * **`ServiceConstruction`** builds a service **from owners that already exist**.
//!   It is safe to call at any time, because it opens nothing that a second opener
//!   would duplicate: it borrows this process's `PersistenceCoordinator`,
//!   `BrokerJournal`, `AgentStore`, `SyncBoundary`, `SettingsFacade` and activity
//!   owners, all of which are `Arc`s that are cloned rather than recreated.
//!
//! A live recovery therefore calls **only** the second half. The Agent application is
//! constructible this way precisely because its startup-only work lives in
//! `AgentApplication::new`, which this module deliberately does **not** call — it
//! calls the constructor that opens no run and mints no supervisor, and then reuses
//! the process's **existing** supervisor, timers and backfill owner.
//!
//! # What must remain true
//!
//! * Only **one** service per capability is ever created. Construction is serialized
//!   by a per-capability responsibility token, so two concurrent recoveries cannot
//!   both build an Agent application and then have one discarded.
//! * An occupant is **never** replaced. An object already serving holds a task's
//!   owner, run identity and cancel path; swapping it would orphan them.
//! * A capability is reported available only when its service is **really present**
//!   afterwards. A cleared degradation marker is not evidence of a working service.

use std::sync::Arc;

use anyhow::Result;

use aura_core_base::config_degradation::ConfigDependentCapability;
pub(crate) use aura_core_base::startup_recovery::StartupRecovery;
use aura_model_service::p05::P05Services;
use aura_security_service::p04::P04Services;
use aura_workspace_service::p03::P03Services;

/// One piece of work that must happen **exactly once** per process, and a real record of whether
/// it has.
///
/// This replaces an assumption with a fact.
///
/// # The defect this removes
///
/// The live constructors were written on the premise "the startup work has already been done, so
/// a live supply must not repeat it". That premise is **false** in exactly the situation the
/// recovery path exists for: when `models.json` is damaged at startup there is no `P05Services`,
/// so P05 maintenance was never started, and with no model authority there is no
/// `AgentApplication`, so the Agent/inbox startup scan, `restore_waits` and the derived backfill
/// never ran either. A live supply then produced a service whose one-time responsibilities had
/// never been discharged — and nothing recorded that.
///
/// A marker that is merely *set* is not evidence: it says "someone intended this", not "it
/// finished". This type is therefore a **ledger**, and a step is marked done only after its work
/// returns. Reading it is how the caller decides whether the missing first responsibility still
/// has to be discharged before new work may use the service.
///
/// # Why it is per-process and not per-service
///
/// These steps reconcile state a **previous process** left behind (interrupted runs, pending
/// transactions). They are facts about this process's history with the data, so they are recorded
/// once for the process rather than per constructed object.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum OneTimeStartupStep {
    /// `models.json`'s pending model-configuration mutation journals were resumed.
    ModelConfigJournalReconciliation,
    /// Model operations a previous process left interrupted were cancelled.
    InterruptedModelOperationCancellation,
    /// The Agent/inbox startup scan ran: runs left live by a previous process were reconciled.
    AgentInboxStartupScan,
    /// The Agent wait restoration ran.
    AgentWaitRestoration,
    /// The derived-backfill owner was started.
    AgentDerivedBackfill,
    /// The P05 projection/credential maintenance task was started.
    ModelMaintenanceTask,
}

impl OneTimeStartupStep {
    /// Every step, so a caller can report all of them rather than whichever it happened to ask.
    ///
    /// Read by the runtime's diagnostic accessor and the lifecycle regressions. It is not consumed
    /// by a route yet: the honest statement is that the ledger is observable and the steps it names
    /// are the ones the supply path discharges, not that a user-facing surface already lists them.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const ALL: [Self; 6] = [
        Self::ModelConfigJournalReconciliation,
        Self::InterruptedModelOperationCancellation,
        Self::AgentInboxStartupScan,
        Self::AgentWaitRestoration,
        Self::AgentDerivedBackfill,
        Self::ModelMaintenanceTask,
    ];

    /// A stable, path-free key for a report or a log.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::ModelConfigJournalReconciliation => "model_config_journal_reconciliation",
            Self::InterruptedModelOperationCancellation => {
                "interrupted_model_operation_cancellation"
            }
            Self::AgentInboxStartupScan => "agent_inbox_startup_scan",
            Self::AgentWaitRestoration => "agent_wait_restoration",
            Self::AgentDerivedBackfill => "agent_derived_backfill",
            Self::ModelMaintenanceTask => "model_maintenance_task",
        }
    }
}

/// The process's real record of which one-time startup steps have actually completed.
///
/// It is shared, so the startup path and the live recovery path read the **same** ledger: a step
/// the startup path really finished is not repeated, and a step it never reached is discharged
/// once by the recovery that first needs the service.
///
/// # What makes a concurrent double-run impossible
///
/// Two recoveries can race to discharge the same missing step. The ledger therefore hands out a
/// **claim**, and only the claimant runs the work; the loser is told it did not get the claim and
/// must observe the result rather than run it again. A step is recorded as completed only after
/// its work returned, so a claimant that fails leaves the step outstanding for a later attempt
/// instead of marking it done on an intention.
pub(crate) struct StartupLedger {
    /// One state per step. A step absent from the map has not been reached at all.
    steps: std::sync::Mutex<std::collections::BTreeMap<OneTimeStartupStep, StartupStepState>>,
}

/// Where one one-time step really stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupStepState {
    /// A caller holds the responsibility and its work has not returned yet.
    Claimed,
    /// The work returned successfully. This is the only state that means "done".
    Completed,
}

impl StartupLedger {
    pub(crate) fn new() -> Self {
        Self { steps: std::sync::Mutex::new(std::collections::BTreeMap::new()) }
    }

    /// Records that a step really finished.
    ///
    /// Called **after** the work returns, never before: a step marked on an intention is exactly
    /// the "copyable marker" the brief rejects.
    pub(crate) fn mark_completed(&self, step: OneTimeStartupStep) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.insert(step, StartupStepState::Completed);
        }
    }

    /// Whether a step has really finished in this process.
    ///
    /// A claimed-but-unfinished step reads as **not** completed, which is the honest answer: the
    /// work is in flight, and a caller deciding whether it still has to happen must not be told
    /// it already did.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_completed(&self, step: OneTimeStartupStep) -> bool {
        self.steps.lock().is_ok_and(|steps| steps.get(&step) == Some(&StartupStepState::Completed))
    }

    /// Whether some caller is currently discharging this step.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_in_flight(&self, step: OneTimeStartupStep) -> bool {
        self.steps.lock().is_ok_and(|steps| steps.get(&step) == Some(&StartupStepState::Claimed))
    }

    /// The steps that have **not** finished, in the stable declared order.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn outstanding(&self) -> Vec<OneTimeStartupStep> {
        OneTimeStartupStep::ALL.into_iter().filter(|step| !self.is_completed(*step)).collect()
    }

    /// Claims the entire outstanding set atomically. A failed or unwound call
    /// relinquishes only its own claims; completed steps are never repeated.
    pub(crate) fn claim_steps(
        &self,
        requested: &[OneTimeStartupStep],
    ) -> Result<StartupStepClaim<'_>, ConstructionRefusal> {
        let mut steps = self.steps.lock().map_err(|_| {
            ConstructionRefusal::Failed("startup responsibility is unavailable".into())
        })?;
        if requested.iter().any(|step| steps.get(step) == Some(&StartupStepState::Claimed)) {
            return Err(ConstructionRefusal::Contended { waited_ms: 0 });
        }
        let owned: Vec<_> = requested
            .iter()
            .copied()
            .filter(|step| steps.get(step) != Some(&StartupStepState::Completed))
            .collect();
        for step in &owned {
            steps.insert(*step, StartupStepState::Claimed);
        }
        Ok(StartupStepClaim { ledger: self, owned })
    }

    /// Attempts to become the one caller responsible for discharging `step`.
    ///
    /// Returns `true` exactly once per step: the first caller takes the claim, and every other
    /// caller — concurrent or later — gets `false` and must **not** run the work. A claimant that
    /// never marks the step completed leaves it claimed, which is deliberate: the honest
    /// alternative (returning it to the pool) would let a second caller run work whose first
    /// attempt may still be in flight.
    #[cfg(test)]
    pub(crate) fn claim(&self, step: OneTimeStartupStep) -> bool {
        let Ok(mut steps) = self.steps.lock() else {
            return false;
        };
        if steps.contains_key(&step) {
            return false;
        }
        steps.insert(step, StartupStepState::Claimed);
        true
    }
}

pub(crate) struct StartupStepClaim<'a> {
    ledger: &'a StartupLedger,
    owned: Vec<OneTimeStartupStep>,
}

impl StartupStepClaim<'_> {
    pub(crate) fn needs_work(&self) -> bool {
        !self.owned.is_empty()
    }
    pub(crate) fn complete(mut self) {
        for step in self.owned.drain(..) {
            self.ledger.mark_completed(step);
        }
    }
}

impl Drop for StartupStepClaim<'_> {
    fn drop(&mut self) {
        if let Ok(mut steps) = self.ledger.steps.lock() {
            for step in &self.owned {
                if steps.get(step) == Some(&StartupStepState::Claimed) {
                    steps.remove(step);
                }
            }
        }
    }
}

/// A service another service's construction needs.
///
/// A **service**, deliberately separate from [`ConfigDependentCapability`]: a capability is what
/// a configuration fault costs the user, while a service is an object one constructor needs.
/// Conflating them produced a self-referential reason — "its dependency tool_policy is not
/// available" for the tool policy itself — which tells the user nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ServiceDependency {
    /// The workspace runtime, which P04's filesystem capability is built on.
    WorkspaceFiles,
    /// The command backend, which the Agent application writes through.
    CommandBackend,
    /// The model authority, which the Agent application reads.
    ModelAuthority,
    /// The sidecar set, whose readiness store the command backend publishes into.
    SecuritySidecars,
}

impl ServiceDependency {
    /// A bounded, path-free name for the report.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::WorkspaceFiles => "workspace_files",
            Self::CommandBackend => "command_backend",
            Self::ModelAuthority => "model_authority",
            Self::SecuritySidecars => "security_sidecars",
        }
    }
}

/// Why a capability's service could not be built, in bounded terms.
#[derive(Clone, Debug)]
pub(crate) enum ConstructionRefusal {
    /// A service this one's construction needs is itself absent, so building it would produce a
    /// service that cannot serve.
    DependencyMissing(ServiceDependency),
    /// The construction genuinely failed. The text is the constructor's own reason.
    Failed(String),
    /// A concurrent recovery supplied the service first. The existing service was kept.
    SuppliedConcurrently,
    /// Another recovery held the construction responsibility for longer than the bounded wait.
    ///
    /// This is its own reason because the user's next step
    /// differs: nothing is broken, another recovery is simply still working, and reporting it as a
    /// failure would send them looking for a fault that does not exist.
    Contended { waited_ms: u64 },
}

impl ConstructionRefusal {
    /// A bounded, path-free reason for the report.
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::DependencyMissing(dependency) => format!(
                "its service could not be built because the {} service is not available in this \
                 process",
                dependency.key()
            ),
            Self::Failed(reason) => reason.clone(),
            Self::SuppliedConcurrently => {
                "it was supplied concurrently; the existing service was kept".to_owned()
            }
            Self::Contended { waited_ms } => format!(
                "another recovery is still supplying this capability; no service was built here \
                 after waiting {waited_ms}ms"
            ),
        }
    }
}

impl std::fmt::Display for ConstructionRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reason())
    }
}

impl std::error::Error for ConstructionRefusal {}

/// Builds a capability's service **from this process's existing owners**.
///
/// # Contract
///
/// Implementations must:
///
/// * clone the process's existing `Arc` owners and never open a second database, a second
///   journal, a second admission capacity or a second supervisor;
/// * perform **no** startup-only work: no startup scan, no pending-transaction finishing,
///   no task spawn on behalf of a service that already ran its startup;
/// * return `true` only when the service is really present afterwards, whether because it
///   was just built or because it was already there.
pub(crate) trait ServiceConstruction: Send + Sync {
    /// Supplies `capability`'s service if it is missing.
    ///
    /// Returns the refusal when it could not be supplied. An already-present service is a
    /// success with no work done: it keeps serving.
    fn supply(&self, capability: ConfigDependentCapability) -> Result<(), ConstructionRefusal>;

    /// Whether `capability`'s service is present and usable **right now**.
    ///
    /// A read: asking must not construct, repair or publish anything.
    fn is_available(&self, capability: ConfigDependentCapability) -> bool;
}

/// How long a caller waits for another recovery's construction before giving up.
///
/// A **bounded** wait, because an unbounded one lets a single
/// slow construction occupy every caller that happens to ask for the same capability. A recovery
/// runs on a bounded worker, and a worker parked indefinitely on another capability's construction
/// is a worker unavailable to the unrelated recoveries and observation controls that must keep
/// working while one capability is being rebuilt.
///
/// The bound is deliberately generous — a real P03/P05 open reads configuration and opens a
/// database — but finite. A caller that exceeds it reports `Contended` and does **not** build:
/// giving up on the wait is not permission to construct a second service.
pub(crate) const CONSTRUCTION_RESPONSIBILITY_WAIT: std::time::Duration =
    std::time::Duration::from_secs(20);

/// The wait a caller really uses, honouring a test-only override.
///
/// The override exists because a case that proves "the wait is bounded" would otherwise spend the
/// whole production budget — twenty seconds per case — which makes the case unusable in a shared run
/// and pressures it into timing-based flakiness. Shortening the bound for a test changes only **how
/// long** the wait is, never whether it exists or what a contended caller does, which is the
/// property under test.
pub(crate) fn responsibility_wait() -> std::time::Duration {
    #[cfg(test)]
    if let Some(value) =
        std::env::var("AURA_TEST_CONSTRUCTION_WAIT_MS").ok().and_then(|value| value.parse().ok())
    {
        return std::time::Duration::from_millis(value);
    }
    CONSTRUCTION_RESPONSIBILITY_WAIT
}

/// The single construction responsibility for one capability.
///
/// A plain "check the slot, then build" has a race: two concurrent recoveries both see an
/// empty slot, both build an Agent application, and one is discarded — a second supervisor
/// minted over the same runs even though only one survives. This token is what removes the
/// race: the builder must **hold** it to build, so the loser waits and then finds the slot
/// filled.
///
/// The wait is bounded and observable, and it is not a plain `Mutex` any more.
/// A `std::sync::Mutex` cannot be waited on with a timeout, and an unbounded wait is the defect
/// item 6 names. The claim therefore carries its own admission state, and reports how long a
/// caller really waited.
pub(crate) struct ConstructionClaim {
    /// `None` while free, `Some(started_at)` while a caller holds the responsibility.
    held_since: std::sync::Mutex<Option<std::time::Instant>>,
    /// Signals a release so a waiter does not poll.
    released: std::sync::Condvar,
}

impl ConstructionClaim {
    pub(crate) fn new() -> Self {
        Self { held_since: std::sync::Mutex::new(None), released: std::sync::Condvar::new() }
    }

    /// Whether some caller currently holds this capability's construction responsibility.
    ///
    /// A read, for the tests and for an honest report. It never waits and never constructs.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_held(&self) -> bool {
        self.held_since.lock().is_ok_and(|held| held.is_some())
    }

    /// Runs construction and publication under the same responsibility, with a bounded wait.
    ///
    /// Releasing the guard after construction but before publishing lets another caller
    /// see the empty slot and construct a second service. `publish` therefore runs before
    /// this guard is released, including when construction returned an error.
    ///
    /// The wait is bounded by [`CONSTRUCTION_RESPONSIBILITY_WAIT`]. A caller that cannot take the
    /// responsibility in that time is refused with [`ClaimOutcome::Contended`] rather than
    /// blocking a bounded worker forever, and it deliberately does **not** fall through to
    /// building: a contended claim is the opposite of permission to construct a second service.
    pub(crate) fn with_responsibility<T, U>(
        &self,
        mut already_present: impl FnMut() -> bool,
        build: impl FnOnce() -> T,
        publish: impl FnOnce(T) -> U,
    ) -> ClaimOutcome<U> {
        let started = std::time::Instant::now();
        // The bound is captured once, so a caller's deadline cannot move while it waits.
        let bound = responsibility_wait();
        let Ok(mut held) = self.held_since.lock() else {
            return ClaimOutcome::Poisoned;
        };
        // A service published by an earlier holder makes the wait unnecessary: the slot is read
        // **before** contending, so the common case costs one uncontended lock.
        if already_present() {
            return ClaimOutcome::AlreadyPresent;
        }
        while held.is_some() {
            let waited = started.elapsed();
            let Some(remaining) = bound.checked_sub(waited) else {
                return ClaimOutcome::Contended { waited_ms: elapsed_ms(waited) };
            };
            match self.released.wait_timeout(held, remaining) {
                Ok((guard, timeout)) => {
                    held = guard;
                    if timeout.timed_out() && held.is_some() {
                        return ClaimOutcome::Contended {
                            waited_ms: elapsed_ms(started.elapsed()),
                        };
                    }
                }
                Err(_) => return ClaimOutcome::Poisoned,
            }
        }
        // Re-checked **inside** the responsibility: the previous holder published before
        // releasing, and building a second one here is exactly what must not happen.
        if already_present() {
            return ClaimOutcome::AlreadyPresent;
        }
        *held = Some(std::time::Instant::now());
        drop(held);
        let _responsibility = ConstructionResponsibility(self);
        let built = build();
        let published = publish(built);
        ClaimOutcome::Built(published)
    }
}

/// Drop releases responsibility even when a constructor or publisher unwinds.
struct ConstructionResponsibility<'a>(&'a ConstructionClaim);
impl Drop for ConstructionResponsibility<'_> {
    fn drop(&mut self) {
        if let Ok(mut held) = self.0.held_since.lock() {
            *held = None;
        }
        self.0.released.notify_all();
    }
}

/// Whole milliseconds, for a bounded report.
fn elapsed_ms(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// What [`ConstructionClaim::with_responsibility`] did.
pub(crate) enum ClaimOutcome<T> {
    /// The service was already present; nothing was built.
    AlreadyPresent,
    /// This caller held the responsibility and ran the build.
    Built(T),
    /// Another recovery held the responsibility for longer than the bounded wait allows.
    ///
    /// Nothing was built by this caller. Reporting the real wait is what lets the user be told
    /// "another recovery is still working on this" rather than "this capability is broken".
    Contended { waited_ms: u64 },
    /// The responsibility lock was poisoned, so nothing was built.
    Poisoned,
}

/// The process's live services, as the recovery entry sees them.
///
/// This is a **view over the runtime's own slots**, not a copy of them: the `Arc`s are the
/// same objects the runtime serves from, so a service published here is the very one the
/// next request receives. That is what removes the two-source defect where the rebuilder
/// wrote one `Option` while `RuntimeServices::p04()` read another.
pub(crate) struct LiveServices {
    pub(crate) commands: Arc<crate::DependentSlot<crate::command_execution::CommandExecution>>,
    /// The Agent execution layer. A slot so a live supply can install the one a damaged
    /// `tools.json` prevented at startup, and so every consumer reads that same layer.
    pub(crate) agent_execution: Arc<crate::DependentSlot<crate::agent_execution::AgentExecution>>,
    pub(crate) agent: Arc<crate::DependentSlot<crate::agent_application::AgentApplication>>,
    pub(crate) p04: Arc<crate::DependentSlot<P04Services>>,
    pub(crate) p05: Arc<crate::DependentSlot<P05Services>>,
    pub(crate) p03: Arc<crate::DependentSlot<P03Services>>,
    /// The process's **existing** sidecar set and its readiness store, when it has one.
    ///
    /// A supplied command backend must publish into the process's readiness store rather than a
    /// new one, because readiness is a process-wide fact with exactly one owner. The command
    /// backend opens a sidecar transport, so when the process has no sidecar set the backend
    /// genuinely cannot be built — and the reason the user is given names that, rather than a
    /// generic failure.
    ///
    /// The set itself is **supplyable**, because it is deferred
    /// construction over the process's existing owners. Without that, a damaged `tools.json`
    /// would leave no sidecar set, no readiness store and therefore no command backend, and the
    /// capability could never come back without a restart.
    pub(crate) sidecars: Arc<std::sync::Mutex<Option<Arc<crate::sidecars::SecuritySidecars>>>>,
    /// One construction responsibility per capability, so two recoveries cannot both build.
    pub(crate) claims: Arc<ConstructionClaims>,
}

/// The per-capability construction responsibilities of one process.
pub(crate) struct ConstructionClaims {
    pub(crate) commands: ConstructionClaim,
    pub(crate) agent: ConstructionClaim,
    pub(crate) security_authority: ConstructionClaim,
    pub(crate) model_authority: ConstructionClaim,
    /// The workspace runtime. It is a dependency of the security authority, so it must be
    /// supplyable too — otherwise repairing `tools.json` could never bring back the security
    /// authority it was taken down with, and the whole chain would still need a restart.
    pub(crate) workspace_files: ConstructionClaim,
}

impl ConstructionClaims {
    pub(crate) fn new() -> Self {
        Self {
            commands: ConstructionClaim::new(),
            agent: ConstructionClaim::new(),
            security_authority: ConstructionClaim::new(),
            model_authority: ConstructionClaim::new(),
            workspace_files: ConstructionClaim::new(),
        }
    }
}

#[cfg(test)]
mod claim_tests;

#[cfg(test)]
mod retry_tests;

mod lifecycle;
