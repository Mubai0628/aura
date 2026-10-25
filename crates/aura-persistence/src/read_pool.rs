//! A rotating, bounded read-connection pool.
//!
//! WHY ROTATION IS NOT ENOUGH
//! Round-robin selection alone decides only which connection is *tried*. If the chosen
//! one is busy, a caller that blocks on its mutex has no deadline at all: one slow
//! reader of one directory makes every later read of that workspace wait for it, and a
//! caller whose own budget expired is still parked. This module keeps the rotation — it
//! is what spreads read load — and adds four things that make the wait bounded and
//! honest:
//!
//! * **Try each connection in turn.** A busy connection is skipped rather than waited
//!   on, so a free connection is used immediately even when the rotation points at a
//!   busy one. Only when *every* connection is occupied does the caller wait.
//! * **One total budget.** `budget` is a single monotonic deadline covering connection
//!   acquisition, SQLite's own lock wait and the SQL itself. The deadline is carried into
//!   the connection as its busy budget and as a progress hook, so a statement that runs
//!   past the budget is interrupted rather than allowed to finish at its leisure; the
//!   answer is [`PersistenceError::ReadDeadlineExceeded`] — a real "this call is over",
//!   never an empty result.
//! * **Per-connection health.** A connection that failed is replaced; the others keep
//!   serving, so one broken read connection cannot poison the pool. The replacement
//!   itself is bounded: it never waits indefinitely for a slot, and a replacement it
//!   could not install is recorded instead of parked on.
//! * **An interrupt identity.** Every call publishes an identity the hooks compare before
//!   they act, and the hooks and the connection settings are cleared before the
//!   connection goes back to the pool, so a late deadline cannot interrupt a later call
//!   that took the connection over.
//!
//! WHERE THE BOUNDARY IS
//! A progress hook interrupts SQLite *virtual machine work*. It cannot interrupt an
//! arbitrary synchronous closure: a caller whose operation blocks on something that is not
//! SQLite — a channel, a file, a lock — is not force-stopped, and this module does not
//! claim otherwise. Such a call returns when it returns; the overrun is measured and
//! reported in [`ReadServed::overrun_ms`] and counted in
//! [`ReadPoolCounters::budget_overruns`], and the connection stays out of the pool until
//! the call is really finished, so the budget is never "met" by handing the connection to
//! somebody else while the old work still owns it.
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rusqlite::{Connection, OpenFlags};

use crate::{
    PersistenceError,
    migration::{BUSY_TIMEOUT, configure_reader},
};
// The storage hold points exist only in a debug build. Every use below carries the
// same gate, so a release build compiles none of this module or its callers.
#[cfg(debug_assertions)]
use crate::barrier::{Point, Slot};

/// How long a read may wait for a connection when the caller did not say.
///
/// This is an internal storage budget, not a public request deadline: the public
/// per-request budget lives in the caller, and it is passed in through
/// [`ReadPool::with_deadline`] whenever a caller has one.
pub const DEFAULT_READ_ACQUIRE: Duration = Duration::from_secs(5);

/// How many SQLite virtual-machine instructions pass between two deadline checks.
///
/// Small enough that a long statement is stopped promptly on a real machine, large enough
/// that the hook is invisible beside the work it is watching.
const DEADLINE_PROGRESS_GRANULARITY: i32 = 1_000;

/// How long a failed connection's replacement may wait for its own slot.
///
/// The slot's mutex is released before a replacement is attempted, so this window is only
/// ever reached when another thread took the connection in between. It is bounded because
/// an unbounded wait here is exactly the "a failing connection blocked on its own slot"
/// case: when the budget expires the replacement is recorded as deferred and the slot is
/// left alone rather than parked on.
const REPLACE_LOCK_BUDGET: Duration = Duration::from_millis(250);

/// Counters for the pool, cumulative for the life of the process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadPoolCounters {
    /// Reads served without waiting at all.
    pub served_immediately: u64,
    /// Reads that had to wait for a connection to come free.
    pub waited: u64,
    /// Reads refused because no connection came free before their deadline.
    pub refused_deadline: u64,
    /// Reads whose SQLite work was interrupted at their deadline.
    pub interrupted: u64,
    /// Reads that returned after their budget without being interruptible: the measured
    /// part of the boundary described in the module docs.
    pub budget_overruns: u64,
    /// Connections replaced after a real operation failure.
    pub replaced: u64,
    /// Replacements that could not be installed inside their own budget. The slot keeps its
    /// previous connection and no caller was parked on; the next failure retries.
    pub replacement_deferred: u64,
}

struct PoolCounters {
    served_immediately: AtomicU64,
    waited: AtomicU64,
    refused_deadline: AtomicU64,
    interrupted: AtomicU64,
    budget_overruns: AtomicU64,
    replaced: AtomicU64,
    replacement_deferred: AtomicU64,
}

impl PoolCounters {
    fn snapshot(&self) -> ReadPoolCounters {
        ReadPoolCounters {
            served_immediately: self.served_immediately.load(Ordering::Relaxed),
            waited: self.waited.load(Ordering::Relaxed),
            refused_deadline: self.refused_deadline.load(Ordering::Relaxed),
            interrupted: self.interrupted.load(Ordering::Relaxed),
            budget_overruns: self.budget_overruns.load(Ordering::Relaxed),
            replaced: self.replaced.load(Ordering::Relaxed),
            replacement_deferred: self.replacement_deferred.load(Ordering::Relaxed),
        }
    }
}

struct PoolSlot {
    connection: Mutex<Connection>,
    /// Set when this connection could not be replaced, so the pool stops handing it
    /// out without disturbing the others.
    poisoned: AtomicBool,
    served: AtomicU64,
    /// The identity of the read call that owns this connection right now, or zero when it
    /// is idle. A deadline hook compares it before it interrupts anything, which is what
    /// keeps a late deadline from landing on the call that took the connection over.
    active_call: Arc<AtomicU64>,
}

impl PoolSlot {
    fn new(connection: Connection) -> Self {
        Self {
            connection: Mutex::new(connection),
            poisoned: AtomicBool::new(false),
            served: AtomicU64::new(0),
            active_call: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// One read call's deadline and its identity.
struct CallDeadline {
    id: u64,
    deadline: Instant,
    /// Set by the connection's own progress hook when the deadline stopped this call's
    /// SQLite work. An interrupt that did not come from this hook is not attributed here.
    fired: Arc<AtomicBool>,
}

pub(crate) struct ReadPool {
    path: PathBuf,
    slots: Vec<Arc<PoolSlot>>,
    next: AtomicU64,
    next_call: AtomicU64,
    counters: Arc<PoolCounters>,
    /// The hold point a read arrival passes. Debug-only: a release build has no
    /// hold point to arrive at, and no reader of it.
    #[cfg(debug_assertions)]
    barrier: Arc<Slot>,
}

/// What one read did, for the caller that has to explain it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadServed {
    /// Which pool slot served the read. An index, never a path or a connection string.
    pub slot: usize,
    /// Whether the caller had to wait for a connection at all.
    pub waited: bool,
    /// Monotonic milliseconds spent acquiring the connection.
    pub acquire_ms: u64,
    /// Monotonic milliseconds the whole call took: acquisition, execution and cleanup.
    pub elapsed_ms: u64,
    /// How much of the call was past the caller's budget. Zero for a call that finished
    /// inside it. Non-zero is only reachable for work the progress hook cannot interrupt —
    /// see the module docs — and it is reported rather than hidden.
    pub overrun_ms: u64,
    /// Whether the deadline interrupted this call's SQLite work.
    pub interrupted: bool,
}

impl ReadPool {
    pub(crate) fn open(path: &Path, size: usize) -> Result<Self, PersistenceError> {
        let size = size.max(1);
        let mut slots = Vec::with_capacity(size);
        for _ in 0..size {
            let connection = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            configure_reader(&connection)?;
            slots.push(Arc::new(PoolSlot::new(connection)));
        }
        Ok(Self {
            path: path.to_path_buf(),
            slots,
            next: AtomicU64::new(0),
            next_call: AtomicU64::new(1),
            counters: Arc::new(PoolCounters {
                served_immediately: AtomicU64::new(0),
                waited: AtomicU64::new(0),
                refused_deadline: AtomicU64::new(0),
                interrupted: AtomicU64::new(0),
                budget_overruns: AtomicU64::new(0),
                replaced: AtomicU64::new(0),
                replacement_deferred: AtomicU64::new(0),
            }),
            #[cfg(debug_assertions)]
            barrier: Arc::new(Slot::default()),
        })
    }

    /// Installs a hold point on connection acquisition. Returns the barrier so a test
    /// can assert the hold really reached this layer.
    #[allow(dead_code)]
    #[cfg(debug_assertions)]
    pub(crate) fn install_barrier(
        &self,
        barrier: crate::barrier::Barrier,
    ) -> crate::barrier::Barrier {
        self.barrier.install(barrier)
    }

    /// The same install, for a hold that applies to the first arrival only.
    #[cfg(debug_assertions)]
    pub(crate) fn install_once_barrier(
        &self,
        barrier: crate::barrier::Barrier,
    ) -> crate::barrier::Barrier {
        self.barrier.install(barrier)
    }

    #[must_use]
    pub(crate) fn counters(&self) -> ReadPoolCounters {
        self.counters.snapshot()
    }

    #[must_use]
    pub(crate) fn size(&self) -> usize {
        self.slots.len()
    }

    /// How many connections are free right now. Used by the honest "no connection was
    /// available" answer and by tests; never to decide a result.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn available(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| {
                !slot.poisoned.load(Ordering::Relaxed) && slot.connection.try_lock().is_ok()
            })
            .count()
    }

    /// Runs `operation` on a read connection, waiting at most `budget` for one.
    ///
    /// The rotation picks the starting index; a busy connection at that index is
    /// skipped and the next one is tried in the same pass. If every connection is
    /// occupied the caller waits — on a monotonic deadline — and then gets
    /// [`PersistenceError::ReadUnavailable`].
    pub(crate) fn with_deadline<T, F>(
        &self,
        budget: Duration,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        self.served_with_deadline(budget, operation).map(|(value, _)| value)
    }

    /// Runs one read with the pool's own connection settings and **no** per-call deadline.
    ///
    /// Acquisition is still bounded and the connection is still exclusive; what is absent
    /// is the per-call busy budget and the deadline hook. This is the entry for a call that
    /// *reports the connection's configuration* — the busy budget a real read installs for
    /// its own duration would otherwise be what the diagnostic measures, which would make
    /// the measurement depend on itself. No call that reads data uses this.
    pub(crate) fn with_configured<T, F>(&self, operation: F) -> Result<T, PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        self.served_inner(DEFAULT_READ_ACQUIRE, false, operation).map(|(value, _)| value)
    }

    /// The same call, also reporting which slot served it and how long the connection
    /// took to acquire. `acquire_ms` is the stage the total budget has to cover.
    ///
    /// The budget is one monotonic deadline. It is carried into the connection as that
    /// connection's busy budget — so SQLite's own lock wait is spent from the same budget
    /// rather than from a fixed timeout of its own — and as a progress hook that stops
    /// statement work which runs past it. What it cannot stop is a closure blocked on
    /// something other than SQLite; that case returns after its budget with the overrun
    /// measured in [`ReadServed::overrun_ms`], never as an empty answer.
    pub(crate) fn served_with_deadline<T, F>(
        &self,
        budget: Duration,
        operation: F,
    ) -> Result<(T, ReadServed), PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        self.served_inner(budget, true, operation)
    }

    fn served_inner<T, F>(
        &self,
        budget: Duration,
        arm_deadline: bool,
        operation: F,
    ) -> Result<(T, ReadServed), PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        let started = Instant::now();
        let deadline = started + budget;
        let len = self.slots.len() as u64;
        let first = self.next.fetch_add(1, Ordering::Relaxed) % len;
        let mut waited = false;
        loop {
            for offset in 0..len {
                let index = ((first + offset) % len) as usize;
                let slot = Arc::clone(&self.slots[index]);
                if slot.poisoned.load(Ordering::Relaxed) {
                    continue;
                }
                let Ok(guard) = slot.connection.try_lock() else {
                    continue;
                };
                let acquire_ms = elapsed_ms(started);
                if waited {
                    self.counters.waited.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.counters.served_immediately.fetch_add(1, Ordering::Relaxed);
                }
                let call = CallDeadline {
                    id: self.next_call.fetch_add(1, Ordering::Relaxed),
                    deadline,
                    fired: Arc::new(AtomicBool::new(false)),
                };
                // The hold point sits after a real connection was taken out of the
                // pool and before the operation used it: a caller held here is holding
                // a real read connection, not a stand-in.
                let result = self.run_held(&slot, guard, operation, &call, arm_deadline);
                let interrupted = call.fired.load(Ordering::Acquire);
                let elapsed = started.elapsed();
                let overrun = elapsed.saturating_sub(budget);
                let failed = result.as_ref().err().is_some_and(is_connection_failure);
                if failed && !interrupted {
                    // The connection is back in the pool before a replacement is opened,
                    // so a replacement never waits for its own slot.
                    self.replace(index);
                }
                let served = ReadServed {
                    slot: index,
                    waited,
                    acquire_ms,
                    elapsed_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                    overrun_ms: u64::try_from(overrun.as_millis()).unwrap_or(u64::MAX),
                    interrupted,
                };
                if interrupted {
                    // The call's SQLite work was stopped at the deadline: the call is over
                    // and no result can still arrive. A read-side deadline is therefore a
                    // definite answer, and it is kept apart from a write's "unresponsive",
                    // where the work may still commit.
                    self.counters.interrupted.fetch_add(1, Ordering::Relaxed);
                    return Err(PersistenceError::ReadDeadlineExceeded {
                        budget_ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
                        elapsed_ms: served.elapsed_ms,
                    });
                }
                if result.is_ok() && !overrun.is_zero() {
                    // The call returned, but not inside its budget, and nothing could stop
                    // it. That boundary is measured instead of claimed.
                    self.counters.budget_overruns.fetch_add(1, Ordering::Relaxed);
                }
                return result.map(|value| (value, served));
            }
            waited = true;
            if Instant::now() >= deadline {
                self.counters.refused_deadline.fetch_add(1, Ordering::Relaxed);
                return Err(PersistenceError::ReadUnavailable { waited_ms: elapsed_ms(started) });
            }
            std::thread::sleep(
                Duration::from_millis(2).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    /// Runs the operation while the connection is checked out, then puts the connection
    /// back the way the pool configured it.
    ///
    /// The order matters: the deadline is armed before the operation, the hook and the
    /// connection settings are cleared after it, and only then is the guard released —
    /// so a connection is never handed out again carrying another call's deadline, and a
    /// late deadline can never fire for work that would belong to somebody else.
    fn run_held<T, F>(
        &self,
        slot: &Arc<PoolSlot>,
        guard: MutexGuard<'_, Connection>,
        operation: F,
        call: &CallDeadline,
        arm_deadline: bool,
    ) -> Result<T, PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        let armed = if arm_deadline { self.arm_deadline(slot, &guard, call) } else { Ok(()) };
        #[cfg(debug_assertions)]
        let permit = self.barrier.arrive(Point::ReadConnection, "read_connection");
        let result = match armed {
            Ok(()) => operation(&guard),
            Err(error) => Err(error),
        };
        #[cfg(debug_assertions)]
        drop(permit);
        if arm_deadline {
            self.disarm(slot, &guard);
        }
        drop(guard);
        slot.served.fetch_add(1, Ordering::Relaxed);
        result
    }

    /// Publishes this call's identity and spends what is left of the budget on the
    /// connection's own lock wait and on a hook that can stop statement work.
    fn arm_deadline(
        &self,
        slot: &Arc<PoolSlot>,
        connection: &Connection,
        call: &CallDeadline,
    ) -> Result<(), PersistenceError> {
        // The identity is published before any hook is armed: a hook can then never act
        // for a call that has already returned.
        slot.active_call.store(call.id, Ordering::Release);
        let remaining = call.deadline.saturating_duration_since(Instant::now());
        connection.busy_timeout(remaining.max(Duration::from_millis(1)))?;
        let fired = Arc::clone(&call.fired);
        let active = Arc::clone(&slot.active_call);
        let id = call.id;
        let deadline = call.deadline;
        connection.progress_handler(
            DEADLINE_PROGRESS_GRANULARITY,
            Some(move || {
                if active.load(Ordering::Acquire) != id {
                    // This connection belongs to a later call now. A deadline of ours has no
                    // business interrupting it.
                    return false;
                }
                if Instant::now() >= deadline {
                    fired.store(true, Ordering::Release);
                    return true;
                }
                false
            }),
        )?;
        Ok(())
    }

    /// Clears the hook, restores the pool's own busy budget and retires the identity.
    fn disarm(&self, slot: &Arc<PoolSlot>, connection: &Connection) {
        if let Err(error) = connection.progress_handler(0, None::<fn() -> bool>) {
            crate::diagnostics::append_persistence_log(
                "persistence.read_hook_clear_failed",
                serde_json::json!({ "reason": error.to_string() }),
            );
        }
        if let Err(error) = connection.busy_timeout(BUSY_TIMEOUT) {
            crate::diagnostics::append_persistence_log(
                "persistence.read_busy_restore_failed",
                serde_json::json!({ "reason": error.to_string() }),
            );
        }
        slot.active_call.store(0, Ordering::Release);
    }

    /// The compatibility entry point: an internal default budget, never an unbounded
    /// wait.
    pub(crate) fn with<T, F>(&self, operation: F) -> Result<T, PersistenceError>
    where
        F: FnOnce(&Connection) -> Result<T, PersistenceError>,
    {
        self.with_deadline(DEFAULT_READ_ACQUIRE, operation)
    }

    /// Replaces one failed connection without touching any other slot.
    ///
    /// The replacement is opened first, then installed under a bounded wait: a slot whose
    /// mutex is taken for longer than [`REPLACE_LOCK_BUDGET`] is left exactly as it is,
    /// with the deferral recorded, rather than parking this thread on it. No caller is
    /// ever handed the connection while this runs, because the previous owner released it
    /// before asking for a replacement.
    fn replace(&self, index: usize) {
        let Some(slot) = self.slots.get(index) else {
            return;
        };
        let reopened = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(PersistenceError::from)
        .and_then(|connection| {
            configure_reader(&connection)?;
            Ok(connection)
        });
        let Ok(connection) = reopened else {
            // The connection could not be replaced. The slot says so instead of
            // handing out a broken connection, and the others are untouched.
            slot.poisoned.store(true, Ordering::Relaxed);
            return;
        };
        let deadline = Instant::now() + REPLACE_LOCK_BUDGET;
        loop {
            match slot.connection.try_lock() {
                Ok(mut current) => {
                    *current = connection;
                    slot.poisoned.store(false, Ordering::Relaxed);
                    self.counters.replaced.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                Err(TryLockError::Poisoned(_)) => {
                    slot.poisoned.store(true, Ordering::Relaxed);
                    return;
                }
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        // Bounded, and recorded: this slot keeps the connection it had and
                        // the next failure will ask for a replacement again. Nothing waits
                        // on this slot's mutex, and the other slots are untouched.
                        self.counters.replacement_deferred.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Whether a failed read is the connection's fault rather than the query's.
///
/// Only a real SQLite-level failure replaces a connection. A domain error - a missing
/// workspace, an invalid row - leaves every connection exactly as it was, because the
/// connection did its job and the answer is about the data. An interrupt is not the
/// connection's fault either: it is this pool's own deadline, and it is reported as one.
fn is_connection_failure(error: &PersistenceError) -> bool {
    match error {
        PersistenceError::Sqlite(rusqlite::Error::SqliteFailure(code, _)) => {
            code.code != rusqlite::ErrorCode::OperationInterrupted
        }
        PersistenceError::Sqlite(rusqlite::Error::InvalidQuery) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(directory: &Path, size: usize) -> ReadPool {
        let path = directory.join("pool.sqlite3");
        let writer = Connection::open(&path).expect("writer");
        writer.pragma_update(None, "journal_mode", "WAL").expect("wal");
        writer
            .execute_batch("CREATE TABLE probe(value INTEGER); INSERT INTO probe VALUES (7);")
            .expect("schema");
        drop(writer);
        ReadPool::open(&path, size).expect("pool")
    }

    fn read(pool: &ReadPool, budget: Duration) -> Result<i64, PersistenceError> {
        pool.with_deadline(budget, |connection| {
            connection
                .query_row("SELECT value FROM probe", [], |row| row.get::<_, i64>(0))
                .map_err(Into::into)
        })
    }

    #[test]
    fn a_busy_connection_is_skipped_and_the_read_still_answers() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 3);
        // Occupy the connection the rotation will pick first.
        let held_slot = Arc::clone(&pool.slots[0]);
        let held = held_slot.connection.lock().expect("held");
        assert_eq!(read(&pool, Duration::from_secs(2)).expect("a free connection serves"), 7);
        drop(held);
    }

    #[test]
    fn every_connection_held_returns_bounded_instead_of_waiting_forever() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 2);
        let first_slot = Arc::clone(&pool.slots[0]);
        let second_slot = Arc::clone(&pool.slots[1]);
        let _first = first_slot.connection.lock().expect("first");
        let _second = second_slot.connection.lock().expect("second");
        let started = Instant::now();
        let error = read(&pool, Duration::from_millis(60)).expect_err("no connection was free");
        assert!(matches!(error, PersistenceError::ReadUnavailable { .. }), "{error:?}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(pool.counters().refused_deadline, 1);
    }

    #[test]
    fn one_poisoned_connection_does_not_stop_the_others() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 2);
        pool.slots[0].poisoned.store(true, Ordering::Relaxed);
        assert_eq!(read(&pool, Duration::from_secs(2)).expect("the healthy connection answers"), 7);
    }

    #[test]
    fn a_connection_that_cannot_be_replaced_is_quarantined_not_handed_out() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 1);
        let error = pool
            .with_deadline(Duration::from_secs(1), |connection| {
                connection
                    .query_row("SELECT value FROM absent_table", [], |row| row.get::<_, i64>(0))
                    .map_err(Into::into)
            })
            .expect_err("the query is invalid");
        assert!(is_connection_failure(&error), "{error:?}");
    }

    /// A real long SELECT, not a stand-in: a recursive CTE that would run for seconds if
    /// nothing stopped it.
    const LONG_SELECT: &str = "WITH RECURSIVE counter(value) AS (
            SELECT 1 UNION ALL SELECT value+1 FROM counter WHERE value < 20000000
         ) SELECT COUNT(*) FROM counter";

    fn long_select(pool: &ReadPool, budget: Duration) -> Result<i64, PersistenceError> {
        pool.with_deadline(budget, |connection| {
            connection.query_row(LONG_SELECT, [], |row| row.get::<_, i64>(0)).map_err(Into::into)
        })
    }

    /// R2 gate. A real long SELECT is interrupted at its budget instead of being allowed to
    /// finish, and the connection is usable again straight afterwards.
    #[test]
    fn a_long_select_is_interrupted_at_its_budget_and_the_connection_is_reusable() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 1);
        let started = Instant::now();
        let error = long_select(&pool, Duration::from_millis(40)).expect_err("the budget ends it");
        let interrupted_ms = started.elapsed();
        println!(
            "R2 long select interrupted={error:?} elapsed_ms={} counters={:?}",
            interrupted_ms.as_millis(),
            pool.counters()
        );
        match &error {
            PersistenceError::ReadDeadlineExceeded { budget_ms, .. } => {
                assert_eq!(*budget_ms, 40, "the answer names the budget it really used");
            }
            other => panic!("a read deadline must be a definite answer, not {other:?}"),
        }
        assert!(
            interrupted_ms < Duration::from_secs(3),
            "the interrupted call returned after {} ms",
            interrupted_ms.as_millis()
        );
        assert_eq!(pool.counters().interrupted, 1);
        assert_eq!(pool.counters().replaced, 0, "a deadline is not a broken connection");
        // The same connection serves the next read: the deadline left nothing armed.
        let reused = read(&pool, Duration::from_secs(2)).expect("the connection is reusable");
        assert_eq!(reused, 7);
        assert_eq!(pool.counters().interrupted, 1, "the reuse was not interrupted");
    }

    /// R2 gate. A deadline hook armed for a call that no longer owns the connection cannot
    /// interrupt the call that took it over.
    #[test]
    fn a_deadline_armed_for_a_finished_call_cannot_hit_the_next_one() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 1);
        let slot = Arc::clone(&pool.slots[0]);
        let guard = slot.connection.lock().expect("connection");
        // A hook whose deadline is already past, armed for this call, and then the
        // connection handed over to another call exactly as a takeover would: the identity
        // the hook captured is no longer the connection's.
        let finished = CallDeadline {
            id: 1,
            deadline: Instant::now() - Duration::from_secs(1),
            fired: Arc::new(AtomicBool::new(false)),
        };
        pool.arm_deadline(&slot, &guard, &finished).expect("the hook is armed");
        slot.active_call.store(2, Ordering::Release);
        let result: Result<i64, PersistenceError> = guard
            .query_row(
                "WITH RECURSIVE counter(value) AS (
                    SELECT 1 UNION ALL SELECT value+1 FROM counter WHERE value < 400000
                 ) SELECT COUNT(*) FROM counter",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(Into::into);
        let fired = finished.fired.load(Ordering::Acquire);
        pool.disarm(&slot, &guard);
        drop(guard);
        println!("R2 late deadline fired={fired} result_ok={}", result.is_ok());
        assert!(!fired, "a hook for another call must not fire");
        assert_eq!(result.expect("the new call is not interrupted"), 400_000);
    }

    /// R2 gate. What cannot be interrupted is measured and reported rather than claimed:
    /// a closure blocked on something that is not SQLite returns after its budget with the
    /// overrun recorded, and the other connections are unaffected.
    #[test]
    fn a_call_that_cannot_be_interrupted_reports_its_overrun_and_leaves_the_others_working() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 2);
        let (value, served) = pool
            .served_with_deadline(Duration::from_millis(10), |connection| {
                // Not SQLite: no progress hook can stop this, and this module does not
                // pretend otherwise.
                std::thread::sleep(Duration::from_millis(80));
                connection
                    .query_row("SELECT value FROM probe", [], |row| row.get::<_, i64>(0))
                    .map_err(Into::into)
            })
            .expect("the call returns its honest value");
        println!("R2 uninterruptible served={served:?} counters={:?}", pool.counters());
        assert_eq!(value, 7);
        assert!(!served.interrupted, "nothing interrupted it: it simply took longer");
        assert!(served.overrun_ms >= 40, "the overrun has to be measured, not hidden: {served:?}");
        assert!(served.elapsed_ms >= served.overrun_ms);
        assert_eq!(pool.counters().budget_overruns, 1);
        // Another connection is untouched by the overrun.
        assert_eq!(read(&pool, Duration::from_secs(2)).expect("another slot serves"), 7);
    }

    /// R2 gate. A replacement that cannot take its own slot is bounded instead of parking
    /// on the slot's mutex.
    #[test]
    fn a_replacement_that_cannot_take_its_slot_is_bounded() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = pool(directory.path(), 1);
        let slot = Arc::clone(&pool.slots[0]);
        // Somebody else owns the slot: the replacement has nowhere to install itself.
        let held = slot.connection.lock().expect("held");
        let started = Instant::now();
        pool.replace(0);
        let elapsed = started.elapsed();
        let counters = pool.counters();
        drop(held);
        println!("R2 replace blocked_for_ms={} counters={counters:?}", elapsed.as_millis());
        assert_eq!(counters.replacement_deferred, 1, "the deferral has to be recorded");
        assert_eq!(counters.replaced, 0);
        assert!(
            elapsed < REPLACE_LOCK_BUDGET + Duration::from_millis(250),
            "the replacement waited {} ms",
            elapsed.as_millis()
        );
        // The slot still serves: no caller was left with a poisoned slot.
        assert_eq!(read(&pool, Duration::from_secs(2)).expect("the slot still serves"), 7);
    }

    /// R2 gate, the held-connection half: while one connection is running real SQL, another
    /// connection answers, and when every one of them is busy the refusal is bounded.
    #[test]
    fn one_busy_connection_leaves_the_others_reading_and_a_full_pool_refuses_bounded() {
        let directory = tempfile::tempdir().expect("data root");
        let pool = Arc::new(pool(directory.path(), 2));
        // Hold one connection with a statement that really runs.
        let holder_pool = Arc::clone(&pool);
        let holder = std::thread::spawn(move || {
            long_select(&holder_pool, Duration::from_millis(150))
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        // The rotation will pick the busy one first often enough to make this meaningful;
        // either way a free connection has to answer.
        let answered = read(&pool, Duration::from_secs(2)).expect("a free connection answers");
        assert_eq!(answered, 7);
        let holder_result = holder.join().expect("holder thread");
        println!("R2 busy-slot holder={holder_result:?} counters={:?}", pool.counters());

        // Every connection busy: the refusal is bounded and named.
        let first = pool.slots[0].connection.lock().expect("first");
        let second = pool.slots[1].connection.lock().expect("second");
        let started = Instant::now();
        let refused = read(&pool, Duration::from_millis(50)).expect_err("nothing is free");
        let elapsed = started.elapsed();
        drop(first);
        drop(second);
        println!("R2 full pool refused={refused:?} elapsed_ms={}", elapsed.as_millis());
        assert!(matches!(refused, PersistenceError::ReadUnavailable { .. }), "{refused:?}");
        assert!(elapsed < Duration::from_secs(2));
    }
}
