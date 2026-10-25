//! The long work Core is doing that holds its stores — deleting a
//! conversation, removing a workspace's records, finishing deletions an earlier run left — so the
//! status can say Core is busy and why while other requests wait behind it. One Core process
//! serves one runtime, so the record is the process's own.
use aura_domain_common::UtcTimestamp;
use aura_runtime_contract::{CoreBusyActivityV1, CoreBusyV1};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Work shorter than this is not worth a word.
const SHOWN_AFTER: Duration = Duration::from_secs(2);

struct Entry {
    id: u64,
    activity: CoreBusyActivityV1,
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
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(Entry {
                id,
                activity,
                started: Instant::now(),
                since: UtcTimestamp::now(),
            });
        }
        BusyGuard { record: self, id }
    }

    /// The oldest work that has run at least `after`.
    pub(crate) fn current_after(&self, after: Duration) -> Option<CoreBusyV1> {
        let entries = self.entries.lock().ok()?;
        entries
            .iter()
            .filter(|entry| entry.started.elapsed() >= after)
            .min_by_key(|entry| entry.started)
            .map(|entry| CoreBusyV1 { activity: entry.activity, since: entry.since })
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

/// What the health projection reports: the oldest work that has run for a while.
pub(crate) fn current() -> Option<CoreBusyV1> {
    RECORD.current_after(SHOWN_AFTER)
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
}
