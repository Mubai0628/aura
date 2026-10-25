//! Internal search policy. Not model-tunable.
use std::time::Duration;

pub(crate) const MAX_PATTERN_BYTES: usize = 4096;
pub(crate) const MAX_PATH_GLOB_BYTES: usize = 1024;
pub(crate) const MAX_RESULTS: u32 = 100;
pub(crate) const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const DEFAULT_DEADLINE: Duration = Duration::from_secs(10);
pub(crate) const HARD_DEADLINE: Duration = Duration::from_secs(30);
pub(crate) const MAX_EXCERPT_CHARS: usize = 1024;
pub(crate) const LINE_HEAP_LIMIT: usize = 64 * 1024 * 1024;
pub(crate) const PROBE_BYTES: usize = 8192;
/// Upper bound on one candidate's reservation: the largest file this engine
/// reads, plus the probe pass that precedes the scan. Reserving both up front is
/// what lets an in-order scan read start to finish without asking for more
/// allowance mid-read; only the re-hash and a retry grow later, in ordinal order.
pub(crate) const MAX_RESERVE_BYTES: u64 = MAX_FILE_BYTES + PROBE_BYTES as u64;
/// How often a waiting worker re-checks the ordinal turn and the cancel flag.
///
/// There is no attempt cap: "not my turn yet" is not budget exhaustion, so a
/// worker waits until the turn arrives or the search is cancelled. That cannot
/// deadlock the pool — jobs run in submission order per session, so the
/// predecessor a worker waits for is already running or already finished, and
/// every path that abandons a predecessor cancels the search.
pub(crate) const BUDGET_WAIT_POLL: Duration = Duration::from_millis(2);

/// Total byte budget for one search.
#[must_use]
pub(crate) fn total_bytes() -> u64 {
    #[cfg(test)]
    if let Some(value) = test_overrides::total_bytes() {
        return value;
    }
    MAX_TOTAL_BYTES
}

/// Soft deadline for one search, after which the engine returns what it has with
/// the existing `TimeBudget` reason.
#[must_use]
pub(crate) fn default_deadline() -> Duration {
    #[cfg(test)]
    if let Some(value) = test_overrides::default_deadline() {
        return value;
    }
    DEFAULT_DEADLINE
}

/// Process-wide, test-only policy overrides.
///
/// They exist so budget-boundary and deadline behaviour can be tested
/// deterministically instead of by waiting ten seconds or by filling 512 MiB.
/// Each override is installed through a guard that clears it on drop, so a
/// failing test cannot leak an override into another one.
#[cfg(test)]
pub(crate) mod test_overrides {
    use std::sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    };
    use std::time::Duration;

    static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);
    static DEADLINE_MICROS: AtomicU64 = AtomicU64::new(0);
    static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();

    fn serial() -> MutexGuard<'static, ()> {
        SERIAL.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|error| error.into_inner())
    }

    #[must_use]
    pub(crate) fn total_bytes() -> Option<u64> {
        match TOTAL_BYTES.load(Ordering::SeqCst) {
            0 => None,
            value => Some(value),
        }
    }

    #[must_use]
    pub(crate) fn default_deadline() -> Option<Duration> {
        match DEADLINE_MICROS.load(Ordering::SeqCst) {
            0 => None,
            micros => Some(Duration::from_micros(micros)),
        }
    }

    /// Serialises tests that change search policy, so one test's override cannot
    /// be observed by another running at the same time.
    pub(crate) struct PolicyGuard {
        _serial: MutexGuard<'static, ()>,
    }

    impl PolicyGuard {
        #[must_use]
        pub(crate) fn install(total_bytes: Option<u64>, deadline: Option<Duration>) -> Self {
            let serial = serial();
            TOTAL_BYTES.store(total_bytes.unwrap_or(0), Ordering::SeqCst);
            DEADLINE_MICROS.store(
                deadline.map_or(0, |value| u64::try_from(value.as_micros()).unwrap_or(u64::MAX)),
                Ordering::SeqCst,
            );
            Self { _serial: serial }
        }
    }

    impl Drop for PolicyGuard {
        fn drop(&mut self) {
            TOTAL_BYTES.store(0, Ordering::SeqCst);
            DEADLINE_MICROS.store(0, Ordering::SeqCst);
        }
    }
}
