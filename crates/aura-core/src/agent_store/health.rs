//! Per-dependency Store health, separated from run-local diagnostics.
//!
//! WHY THIS EXISTS
//! The Agent path could previously end up in one process-wide `Option<String>`: any failure
//! that reached `record_fault` made *every* session's `status()` refuse, with the text of
//! whichever operation happened to fail first, and nothing ever cleared it. That conflates
//! three different facts:
//!
//! * **A shared dependency is unavailable.** One SQLite database, one writer: if the V2 store
//!   cannot serve an operation, every operation that depends on it is degraded, and pretending
//!   otherwise would be a fabricated success. This is a fact about the dependency.
//! * **One operation failed for its own business reason.** A revision conflict, a stale
//!   revision, an invalid request: the operation is answerable, other sessions are untouched,
//!   and nothing is unavailable. This is a fact about the operation (and, when it is a run's
//!   step, about that run).
//! * **One run's step could not be recorded.** The run keeps an honest local diagnostic; it is
//!   not evidence that a shared dependency is broken.
//!
//! WHAT IS RECORDED
//! [`StoreHealth`] keeps one [`DependencyState`] per dependency with its code, detail,
//! observation time and a monotonic revision, and **clears it on the next real successful
//! operation** - so a fault that is over is over, without a Core restart. A run-local
//! diagnostic is kept in [`RunDiagnostics`], bounded and per run, and never consulted by
//! another run's status.
//!
//! WHAT THIS IS NOT
//! It is not a retry policy and it is not permission to re-run anything: clearing a health fact
//! only means the next *new* legitimate call is served by the real store again. No unknown
//! effect is replayed because a dependency recovered.
use aura_domain_common::UtcTimestamp;
use std::{collections::BTreeMap, sync::Mutex};

/// Which shared dependency an operation needed.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum StoreDependency {
    /// The V2 execution store: the Agent journal, its projections and its transactions.
    Journal,
}

impl StoreDependency {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Journal => "v2_journal",
        }
    }
}

/// One dependency's current fact, as a reader sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoreHealthSnapshot {
    pub(crate) dependency: StoreDependency,
    /// `true` when the last real operation on this dependency succeeded.
    pub(crate) available: bool,
    /// The typed code of the last real failure. Never a message of an unrelated operation.
    pub(crate) code: Option<&'static str>,
    /// Bounded detail of that failure, kept for the answer the caller receives.
    pub(crate) detail: Option<String>,
    /// When that failure was observed on the monotonic-free wall clock.
    pub(crate) observed_at: Option<UtcTimestamp>,
    /// Bumped on every change, so "cleared" is an observable transition and not a guess.
    pub(crate) revision: u64,
    /// How many real failures were observed on this dependency since the process started.
    pub(crate) failures: u64,
    /// How many of those were followed by a real success (the fault really cleared).
    pub(crate) cleared: u64,
}

impl StoreHealthSnapshot {
    /// The answer a dependent operation gives while this dependency is unusable.
    #[must_use]
    pub(crate) fn detail_text(&self) -> String {
        match (&self.code, &self.detail) {
            (Some(code), Some(detail)) => format!("{code}: {detail}"),
            (Some(code), None) => (*code).to_owned(),
            _ => "unavailable".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct DependencyState {
    available: bool,
    /// Which access recorded the unavailability. A read success may only clear a read fault.
    failed_access: Option<super::StoreAccess>,
    code: Option<&'static str>,
    detail: Option<String>,
    observed_at: Option<UtcTimestamp>,
    revision: u64,
    failures: u64,
    cleared: u64,
}

/// The health of every shared dependency the Agent Store uses.
pub(crate) struct StoreHealth {
    inner: Mutex<BTreeMap<StoreDependency, DependencyState>>,
}

impl Default for StoreHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl StoreHealth {
    #[must_use]
    pub(crate) fn new() -> Self {
        let mut states = BTreeMap::new();
        states.insert(
            StoreDependency::Journal,
            DependencyState { available: true, ..DependencyState::default() },
        );
        Self { inner: Mutex::new(states) }
    }

    /// A real operation on this dependency completed. Clears the unavailability it disproves.
    ///
    /// This is the whole recovery entry: no restart, no probe, no operator action. A fact is
    /// only ever set by a real failure and only ever cleared by a real success that disproves
    /// it - a write that committed disproves either fact, a read that succeeded disproves only
    /// a read-side one, because in WAL a reader is served while the writer is still blocked.
    pub(crate) fn observe_success(&self, dependency: StoreDependency, access: super::StoreAccess) {
        let Ok(mut states) = self.inner.lock() else {
            return;
        };
        let state = states.entry(dependency).or_default();
        if state.available {
            return;
        }
        let disproves = match state.failed_access {
            Some(super::StoreAccess::Write) => access == super::StoreAccess::Write,
            _ => true,
        };
        if !disproves {
            return;
        }
        state.available = true;
        state.failed_access = None;
        state.cleared = state.cleared.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        // The failure text is dropped with the fact that produced it: keeping it would let a
        // recovered dependency still be described by an error it no longer has.
        state.code = None;
        state.detail = None;
        state.observed_at = None;
    }

    /// A real operation on this dependency failed for a store-level reason.
    pub(crate) fn observe_unavailable(
        &self,
        dependency: StoreDependency,
        access: super::StoreAccess,
        code: &'static str,
        detail: String,
    ) {
        let Ok(mut states) = self.inner.lock() else {
            return;
        };
        let state = states.entry(dependency).or_default();
        state.available = false;
        state.failed_access = Some(access);
        state.code = Some(code);
        state.detail = Some(detail);
        state.observed_at = Some(UtcTimestamp::now());
        state.failures = state.failures.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
    }

    #[must_use]
    pub(crate) fn snapshot(&self, dependency: StoreDependency) -> StoreHealthSnapshot {
        let state = self
            .inner
            .lock()
            .ok()
            .and_then(|states| states.get(&dependency).cloned())
            .unwrap_or_default();
        StoreHealthSnapshot {
            dependency,
            available: state.available,
            code: state.code,
            detail: state.detail,
            observed_at: state.observed_at,
            revision: state.revision,
            failures: state.failures,
            cleared: state.cleared,
        }
    }
}

/// One run's own diagnostic fact: what its step could not do, and why.
///
/// It exists for the facts that could **not** be persisted - a run whose failure record itself
/// failed keeps an honest local entry instead of a fabricated store-wide fault. Bounded per
/// run, and read only for that run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunDiagnostic {
    pub(crate) run: aura_domain_common::AgentRunId,
    pub(crate) stage: &'static str,
    pub(crate) code: &'static str,
    pub(crate) detail: String,
    pub(crate) at: UtcTimestamp,
}

/// How many run diagnostics are kept. A ring by run, so a long session cannot grow it.
const MAX_RUN_DIAGNOSTICS: usize = 128;

/// The run-local diagnostics of the Agent application.
#[derive(Default)]
pub(crate) struct RunDiagnostics {
    inner: Mutex<BTreeMap<aura_domain_common::AgentRunId, Vec<RunDiagnostic>>>,
}

impl RunDiagnostics {
    pub(crate) fn record(
        &self,
        run: aura_domain_common::AgentRunId,
        stage: &'static str,
        code: &'static str,
        detail: impl Into<String>,
    ) {
        let Ok(mut all) = self.inner.lock() else {
            return;
        };
        let entries = all.entry(run).or_default();
        if entries.len() >= MAX_RUN_DIAGNOSTICS {
            entries.remove(0);
        }
        entries.push(RunDiagnostic {
            run,
            stage,
            code,
            detail: detail.into(),
            at: UtcTimestamp::now(),
        });
    }

    #[allow(dead_code)] // the run-local diagnostic a regression reads
    #[must_use]
    pub(crate) fn of(&self, run: aura_domain_common::AgentRunId) -> Vec<RunDiagnostic> {
        self.inner.lock().ok().and_then(|all| all.get(&run).cloned()).unwrap_or_default()
    }

    /// Forgets a run's diagnostics once the run's own record is durable again.
    pub(crate) fn clear(&self, run: aura_domain_common::AgentRunId) {
        if let Ok(mut all) = self.inner.lock() {
            all.remove(&run);
        }
    }

    #[allow(dead_code)] // inspected by the regression tests
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.inner.lock().map(|all| all.values().map(Vec::len).sum()).unwrap_or(0)
    }
}
