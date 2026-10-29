//! The deletions Core is doing in the background — a conversation, a removed workspace's
//! records, deletions an earlier run left — so the status can say what is being deleted. They run
//! in short steps and other requests are served between them. Waits for the store are recorded
//! here too (`agent_store`) so background copying gives way to one that lasts; the status does
//! not name them, since a run of slow steps would show and clear it at every step. One Core
//! process serves one runtime, so the record is the process's own.
use aura_domain_common::UtcTimestamp;
use aura_runtime_contract::{CoreBusyActivityV1, CoreBusyV1};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Work shorter than this is not worth a word.
const SHOWN_AFTER: Duration = Duration::from_secs(2);

/// What one entry is.
#[derive(Clone, Copy)]
enum Work {
    /// Background work the status names.
    Named(CoreBusyActivityV1),
    /// A foreground wait for the store.
    StoreWait,
}

struct Entry {
    id: u64,
    work: Work,
    started: Instant,
    since: UtcTimestamp,
}

pub(crate) struct BusyRecord {
    entries: Mutex<Vec<Entry>>,
    next: AtomicU64,
}

/// The work is busy while its guard lives, however the work ends.
#[must_use = "the work counts as busy only while its guard lives"]
pub(crate) struct BusyGuard {
    record: &'static BusyRecord,
    id: u64,
}

impl BusyRecord {
    pub(crate) const fn new() -> Self {
        Self { entries: Mutex::new(Vec::new()), next: AtomicU64::new(1) }
    }

    pub(crate) fn begin(&'static self, activity: CoreBusyActivityV1) -> BusyGuard {
        self.enter(Work::Named(activity))
    }

    /// A wait for the store, while its caller waits.
    pub(crate) fn wait_for_store(&'static self) -> BusyGuard {
        self.enter(Work::StoreWait)
    }

    fn enter(&'static self, work: Work) -> BusyGuard {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(Entry { id, work, started: Instant::now(), since: UtcTimestamp::now() });
        }
        BusyGuard { record: self, id }
    }

    /// Whether a wait for the store has lasted at least `after`.
    pub(crate) fn store_waiting_after(&self, after: Duration) -> bool {
        self.entries.lock().is_ok_and(|entries| {
            entries.iter().any(|entry| {
                matches!(entry.work, Work::StoreWait) && entry.started.elapsed() >= after
            })
        })
    }

    /// The oldest named work that has run at least `after`.
    pub(crate) fn current_after(&self, after: Duration) -> Option<CoreBusyV1> {
        let entries = self.entries.lock().ok()?;
        entries
            .iter()
            .filter(|entry| entry.started.elapsed() >= after)
            .filter_map(|entry| match entry.work {
                Work::Named(activity) => Some((entry.started, activity, entry.since)),
                Work::StoreWait => None,
            })
            .min_by_key(|(started, ..)| *started)
            .map(|(_, activity, since)| CoreBusyV1 { activity, since })
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.record.entries.lock() {
            entries.retain(|entry| entry.id != self.id);
        }
    }
}

static RECORD: BusyRecord = BusyRecord::new();

pub(crate) fn begin(activity: CoreBusyActivityV1) -> BusyGuard {
    RECORD.begin(activity)
}

/// A foreground wait for the store, while its guard lives.
pub(crate) fn wait_for_store() -> BusyGuard {
    RECORD.wait_for_store()
}

/// What the health projection reports: the oldest work that has run for a while.
pub(crate) fn current() -> Option<CoreBusyV1> {
    RECORD.current_after(SHOWN_AFTER)
}

/// Whether a wait for the store has lasted `after`: background copying gives way then.
pub(crate) fn store_waiting_after(after: Duration) -> bool {
    RECORD.store_waiting_after(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_shows_once_it_has_run_a_while_and_ends_with_its_guard() {
        static RECORD: BusyRecord = BusyRecord::new();
        let deleting = RECORD.begin(CoreBusyActivityV1::DeletingSession);
        assert_eq!(RECORD.current_after(Duration::from_secs(3600)), None);
        std::thread::sleep(Duration::from_millis(5));
        let removing = RECORD.begin(CoreBusyActivityV1::RemovingWorkspace);
        // The oldest work is the one shown.
        assert_eq!(
            RECORD.current_after(Duration::ZERO).map(|busy| busy.activity),
            Some(CoreBusyActivityV1::DeletingSession)
        );
        drop(deleting);
        assert_eq!(
            RECORD.current_after(Duration::ZERO).map(|busy| busy.activity),
            Some(CoreBusyActivityV1::RemovingWorkspace)
        );
        drop(removing);
        assert_eq!(RECORD.current_after(Duration::ZERO), None);
    }

    /// A run of slow store steps would show and clear the status at every step: a wait for the
    /// store is never what the status names, and background copying still gives way to it.
    #[test]
    fn a_wait_for_the_store_is_never_named() {
        static RECORD: BusyRecord = BusyRecord::new();
        let waiting = RECORD.wait_for_store();
        assert!(RECORD.store_waiting_after(Duration::ZERO), "background copying gives way");
        assert_eq!(RECORD.current_after(Duration::ZERO), None, "the status does not name it");
        drop(waiting);
    }

    #[test]
    fn only_a_wait_for_the_store_says_the_store_is_contended() {
        static RECORD: BusyRecord = BusyRecord::new();
        let deleting = RECORD.begin(CoreBusyActivityV1::DeletingSession);
        assert!(!RECORD.store_waiting_after(Duration::ZERO), "a deletion is not a wait");
        let reading = RECORD.wait_for_store();
        assert!(RECORD.store_waiting_after(Duration::ZERO));
        assert!(!RECORD.store_waiting_after(Duration::from_secs(3600)));
        drop(reading);
        assert!(!RECORD.store_waiting_after(Duration::ZERO));
        drop(deleting);
    }
}
