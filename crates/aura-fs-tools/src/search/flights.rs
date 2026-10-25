//! Registry of in-flight searches.
//!
//! The registry is the only place that knows a search exists, so it is also the
//! place that has to get the *end* of one right: a registration may only be
//! released when the search's work really finished, never because a handler
//! returned, a connection went away, or a waiting budget expired. Those events
//! end the caller's interest, not the work.
//!
//! Ownership rules:
//!
//! * [`register`] is called before any work is admitted;
//! * [`take_if_finished`] releases an entry only when
//!   [`SearchCancel::is_finished`] holds — everyone who observes a search uses
//!   this, and nobody removes an entry "because it looks gone";
//! * [`orphan`] records that the connection that started a search is gone. The
//!   work keeps running and stays tracked; a single registry-owned sweeper waits
//!   for it and reports what never finished instead of quietly dropping it.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use aura_domain_common::CorrelationId;

use super::cancel::SearchCancel;

/// How often the sweeper re-checks orphaned searches.
const SWEEP_INTERVAL: Duration = Duration::from_millis(25);
/// After this long, an orphaned search that still has not finished is reported
/// once. The report never releases the entry: the work is still real.
const ORPHAN_REPORT_AFTER: Duration = Duration::from_secs(45);

type OrphanReporter = Box<dyn Fn(CorrelationId, Duration) + Send + Sync>;

static FLIGHTS: OnceLock<Mutex<Registry>> = OnceLock::new();
static ORPHAN_REPORTER: OnceLock<OrphanReporter> = OnceLock::new();

#[derive(Default)]
struct Registry {
    active: BTreeMap<CorrelationId, SearchCancel>,
    /// Searches whose owning connection is gone but whose work is still running.
    orphans: BTreeMap<CorrelationId, Instant>,
    /// Orphans already reported as overdue, so the report happens once.
    reported: BTreeSet<CorrelationId>,
    sweeping: bool,
}

fn registry() -> &'static Mutex<Registry> {
    FLIGHTS.get_or_init(|| Mutex::new(Registry::default()))
}

fn lock() -> std::sync::MutexGuard<'static, Registry> {
    registry().lock().unwrap_or_else(|error| error.into_inner())
}

/// Install the sink for orphan reports. Called once by the host process; without
/// it the reports are dropped rather than faked.
pub fn set_orphan_reporter(reporter: impl Fn(CorrelationId, Duration) + Send + Sync + 'static) {
    let _ = ORPHAN_REPORTER.set(Box::new(reporter));
}

pub fn register(id: CorrelationId, cancel: SearchCancel) {
    lock().active.insert(id, cancel);
}

pub fn cancel(id: CorrelationId) -> bool {
    lock()
        .active
        .get(&id)
        .map(|cancel| {
            cancel.cancel();
            true
        })
        .unwrap_or(false)
}

/// Release the registration for `id` **only if** its work really finished.
///
/// Returns whether the entry was released. A missing entry is never evidence of
/// completion, so callers must not read `false` as "already cleaned up".
pub fn take_if_finished(id: CorrelationId) -> bool {
    let mut registry = lock();
    let finished = registry.active.get(&id).is_some_and(SearchCancel::is_finished);
    if finished {
        registry.active.remove(&id);
        registry.orphans.remove(&id);
        registry.reported.remove(&id);
    }
    finished
}

/// Release a registration for a search that never started.
///
/// Safe by construction: it only removes an entry with no admitted work and no
/// coordinator run, so there is nothing running that could still need the
/// registration. Anything else must go through take_if_finished or
/// [orphan].
/// The caller's assertion that a request never took an execution lane.
///
/// Only the component that owns the queue may construct this, and only from the
/// state that proves it: a request still waiting in a lane's queue never ran its
/// coordinator. The registry cannot tell that state apart from "the coordinator
/// started and has not dispatched yet", so the guarantee is the caller's, stated
/// in the type instead of implied by a comment.
#[derive(Clone, Copy, Debug)]
pub struct NeverStarted;

/// Release a registration for a search that never started, on the caller's
/// explicit guarantee.
///
/// Also requires the observation it can make itself: no admitted work and no
/// coordinator run. A search whose coordinator already ran is refused even if the
/// caller claims otherwise.
pub fn release_unstarted(id: CorrelationId, _never_started: NeverStarted) -> bool {
    let mut registry = lock();
    let unstarted = registry
        .active
        .get(&id)
        .is_some_and(|cancel| cancel.admitted_work() == 0 && !cancel.coordinator_done());
    if unstarted {
        registry.active.remove(&id);
        registry.orphans.remove(&id);
        registry.reported.remove(&id);
    }
    unstarted
}

/// Whether the search registered for `id` has really finished.
///
/// `None` means nothing is registered — which says nothing about whether the
/// work finished, so callers must not read it as success.
#[must_use]
pub fn is_finished(id: CorrelationId) -> Option<bool> {
    lock().active.get(&id).map(SearchCancel::is_finished)
}

#[must_use]
pub fn get(id: CorrelationId) -> Option<SearchCancel> {
    lock().active.get(&id).cloned()
}

/// Number of registrations still held, for diagnostics and tests.
#[must_use]
pub fn active_count() -> usize {
    lock().active.len()
}

/// Hand searches whose connection is gone to the registry's own sweeper.
///
/// They keep their registration, so they can still be cancelled and still be
/// observed finishing; the sweeper is what eventually releases the ones that
/// finish, and what reports the ones that never do.
pub fn orphan(ids: impl IntoIterator<Item = CorrelationId>) {
    let mut registry = lock();
    let now = Instant::now();
    let mut already_gone = Vec::new();
    let mut start = false;
    for id in ids {
        match registry.active.get(&id) {
            // Finished in the window before the hand-off: release it here, in
            // this same operation, instead of leaving an entry nobody owns.
            Some(cancel) if cancel.is_finished() => {
                registry.active.remove(&id);
                registry.orphans.remove(&id);
                registry.reported.remove(&id);
            }
            // Still running: the registry takes over.
            Some(_) => {
                registry.orphans.entry(id).or_insert(now);
            }
            // Nothing registered: that says nothing about the work, so it is
            // reported rather than silently treated as reclaimed.
            None => already_gone.push(id),
        }
    }
    if !registry.orphans.is_empty() && !registry.sweeping {
        registry.sweeping = true;
        start = true;
    }
    drop(registry);
    for id in already_gone {
        report_missing(id);
    }
    if start {
        start_sweeper();
    }
}

/// Hand a search to registry ownership because its handler returned while the
/// scan underneath is still running, even though the connection is alive.
///
/// The same atomic rule as [orphan]: release if it finished in the window,
/// take over if it did not.
pub fn track_after_handler(id: CorrelationId) -> bool {
    let mut registry = lock();
    match registry.active.get(&id) {
        Some(cancel) if cancel.is_finished() => {
            registry.active.remove(&id);
            registry.orphans.remove(&id);
            registry.reported.remove(&id);
            false
        }
        Some(_) => {
            registry.orphans.entry(id).or_insert_with(Instant::now);
            let start = !registry.sweeping;
            if start {
                registry.sweeping = true;
            }
            drop(registry);
            if start {
                start_sweeper();
            }
            true
        }
        None => false,
    }
}

/// Report a hand-off for a search that is no longer registered. Never claims the
/// work finished; it says the caller has no registration to hand over.
fn report_missing(id: CorrelationId) {
    let Some(reporter) = ORPHAN_REPORTER.get() else {
        return;
    };
    reporter(id, Duration::ZERO);
}

/// One sweep: release every orphan whose work finished, and report the ones that
/// have been running past the report budget. Never releases unfinished work.
///
/// `finish` asks for the atomic exit: the emptiness check and clearing the
/// running flag happen under the same lock, so a task added while the sweeper is
/// leaving either sees the flag cleared (and starts a new sweeper) or is already
/// in the set the exiting sweeper refuses to leave behind.
///
/// Returns `(released, still_running, overdue_reports, should_exit)`.
fn sweep_once(finish: bool) -> (Vec<CorrelationId>, Vec<CorrelationId>, Vec<CorrelationId>, bool) {
    let mut registry = lock();
    let finished: Vec<CorrelationId> = registry
        .orphans
        .keys()
        .copied()
        .filter(|id| registry.active.get(id).is_none_or(SearchCancel::is_finished))
        .collect();
    for id in &finished {
        registry.active.remove(id);
        registry.orphans.remove(id);
        registry.reported.remove(id);
    }
    let now = Instant::now();
    let still_running: Vec<CorrelationId> = registry.orphans.keys().copied().collect();
    let overdue: Vec<CorrelationId> = registry
        .orphans
        .iter()
        .filter(|(id, since)| {
            now.duration_since(**since) >= ORPHAN_REPORT_AFTER && !registry.reported.contains(id)
        })
        .map(|(id, since)| {
            let waited = now.duration_since(*since);
            (*id, waited)
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|(id, waited)| {
            registry.reported.insert(id);
            let _ = waited;
            id
        })
        .collect();
    let should_exit = finish && still_running.is_empty();
    if should_exit {
        // The exit decision and the emptiness it is based on are the same
        // critical section: nothing can slip in between.
        registry.sweeping = false;
    }
    (finished, still_running, overdue, should_exit)
}

fn report(ids: &[CorrelationId]) {
    let Some(reporter) = ORPHAN_REPORTER.get() else {
        return;
    };
    for id in ids {
        reporter(*id, ORPHAN_REPORT_AFTER);
    }
}

/// Wait (bounded) for orphaned searches to finish, releasing the ones that do.
///
/// A *waiter* is not the sweeper: it never clears the running flag, so it cannot
/// declare the sweeper's job done while the sweeper is still watching. Returns
/// the ids still running when the budget expired; they stay registered, because
/// the work is still real.
///
/// Blocking on purpose: the caller is already inside synchronous teardown or on a
/// dedicated thread, and this crate owns no async runtime.
pub fn wait_for_orphans(budget: Duration) -> Vec<CorrelationId> {
    let deadline = Instant::now() + budget;
    loop {
        let (_, still_running, overdue, _) = sweep_once(false);
        report(&overdue);
        if still_running.is_empty() {
            return Vec::new();
        }
        if Instant::now() >= deadline {
            return still_running;
        }
        std::thread::sleep(SWEEP_INTERVAL);
    }
}

/// Wait (bounded) for *these* searches only, releasing the ones that finish.
///
/// Connection-scoped: another connection's work can neither satisfy nor delay
/// this wait.
pub fn wait_for_orphans_of(ids: &[CorrelationId], budget: Duration) -> Vec<CorrelationId> {
    let deadline = Instant::now() + budget;
    let mut remaining: Vec<CorrelationId> = ids.to_vec();
    let mut gone: Vec<CorrelationId> = Vec::new();
    let outcome = loop {
        {
            let mut registry = lock();
            remaining.retain(|id| match registry.active.get(id) {
                Some(cancel) if cancel.is_finished() => {
                    registry.active.remove(id);
                    registry.orphans.remove(id);
                    registry.reported.remove(id);
                    false
                }
                Some(_) => true,
                // Not registered: nothing more can arrive for it. It is not
                // reported as finished — it is reported as unknown below.
                None => {
                    gone.push(*id);
                    false
                }
            });
        }
        if remaining.is_empty() || Instant::now() >= deadline {
            break remaining;
        }
        std::thread::sleep(SWEEP_INTERVAL);
    };
    for id in gone {
        report_missing(id);
    }
    outcome
}

/// Start at most one registry sweeper thread for the current orphan set.
///
/// One thread, started on demand, exiting once nothing is orphaned: the registry
/// never accumulates a task per search, and a search whose work never ends keeps
/// its entry (reported) rather than being silently released.
fn start_sweeper() {
    let spawned = std::thread::Builder::new()
        .name("aura-search-flight-sweeper".to_owned())
        .spawn(|| {
            loop {
                let (_, _still_running, overdue, should_exit) = sweep_once(true);
                report(&overdue);
                if should_exit {
                    return;
                }
                std::thread::sleep(SWEEP_INTERVAL);
            }
        })
        .is_ok();
    if !spawned {
        // No sweeper exists, so no one may claim the orphans are handled: keep
        // the entries and the flag cleared, and report that ownership is
        // waiting for the next hand-off. A later orphan call starts a new
        // attempt instead of silently leaving the work unowned.
        lock().sweeping = false;
        let pending: Vec<CorrelationId> = lock().orphans.keys().copied().collect();
        for id in pending {
            report_missing(id);
        }
    }
}

/// Test seam: how many searches are orphaned right now.
#[must_use]
pub fn orphan_count() -> usize {
    lock().orphans.len()
}
