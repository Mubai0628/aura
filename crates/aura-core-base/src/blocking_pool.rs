//! Bounded admission for the synchronous workspace/file service calls an async
//! request handler has to make.
//!
//! WHY THIS EXISTS
//! `P03Services::dispatch_main_command` is synchronous by design: the catalog walk, the
//! file read and the workspace lifecycle transition are blocking work with a synchronous
//! API, and turning every one of them into a future would spread `async` through code
//! that has no use for it. What is *not* acceptable is where that work ran: the request
//! handler is an async task, so a synchronous call inside it parks a Tokio worker for as
//! long as the work takes. A rebuild of a large workspace then occupies an executor
//! thread, and enough of them occupy all of them — which is a property of the *pool*, not
//! of the storage layer, and is exactly what this module bounds.
//!
//! WHAT IS BOUNDED
//! * **How many run at once.** [`BlockingPool::admit`] hands out one permit from a
//!   semaphore. An unpermitted call is refused immediately, so a burst produces honest
//!   back-pressure instead of an unbounded queue of blocked reactor threads.
//! * **How long a caller waits.** The permit is acquired under the caller's own
//!   absolute deadline. A deadline that expires *before* a permit is taken means the
//!   work has not started: [`BlockingAdmission::Refused`] is a real "not executed"
//!   answer.
//! * **Who owns the running work.** Once a permit is taken the work runs on a blocking
//!   thread. If the caller's deadline expires afterwards, the answer is
//!   [`BlockingAdmission::Unknown`]: the work may still be running and may still commit,
//!   so it is never reported as "not executed". The permit is released by the blocking
//!   task itself when it finishes, never by the departing caller, so a cancelled caller
//!   cannot free a permit whose work is still holding it.
//! * **Who receives a late result.** [`BlockingPool::run_settled`] hands the real value to a
//!   settlement sink, invoked by the thread that really ran the work. A caller that walked
//!   away therefore cannot make the result disappear; the plain [`BlockingPool::run`] is the
//!   same call with a sink that ignores it.
//!
//! The pool is deliberately small: it exists to keep blocking work off the reactor, not
//! to run more of it at once than the storage layer under it can serve. The one SQLite
//! writer is what actually serialises writes, so a large blocking pool would only move
//! the queue from the executor to the writer's admission lane.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How many synchronous service calls may be in flight at once.
pub const BLOCKING_WORKERS: usize = 4;

/// How long a call waits for a worker when the caller gave no deadline of its own.
///
/// A caller with a deadline passes it in; this only bounds the paths that have none, and
/// it is the pool's own internal budget rather than a public request deadline.
#[allow(dead_code)]
pub const BLOCKING_ADMISSION_BUDGET: Duration = Duration::from_secs(5);

/// What the pool is doing, as counters only.
///
/// Read by the isolation tests and by the acceptance driver that has to prove a refusal
/// really happened. No product path reads it, so it is not on the decision path: it is the
/// record that makes "the pool refused this call" a measurement instead of a claim.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlockingPoolCounters {
    /// Calls that took a permit without waiting.
    pub admitted: u64,
    /// Calls that waited for a permit and then got one.
    pub waited: u64,
    /// Calls refused because no permit came free before their deadline. The work did
    /// **not** run.
    pub refused: u64,
    /// Calls whose deadline expired after their work started. The outcome is unknown.
    pub unknown: u64,
    /// Calls whose work finished inside the pool, however they ended.
    pub completed: u64,
}

struct Counters {
    admitted: std::sync::atomic::AtomicU64,
    waited: std::sync::atomic::AtomicU64,
    refused: std::sync::atomic::AtomicU64,
    unknown: std::sync::atomic::AtomicU64,
    completed: std::sync::atomic::AtomicU64,
}

impl Counters {
    #[allow(dead_code)]
    fn snapshot(&self) -> BlockingPoolCounters {
        use std::sync::atomic::Ordering::Relaxed;
        BlockingPoolCounters {
            admitted: self.admitted.load(Relaxed),
            waited: self.waited.load(Relaxed),
            refused: self.refused.load(Relaxed),
            unknown: self.unknown.load(Relaxed),
            completed: self.completed.load(Relaxed),
        }
    }
}

/// The result of asking the pool to run something.
pub enum BlockingAdmission<T> {
    /// The work ran and this is its result.
    Completed(T),
    /// No worker was free inside the budget. The work was **not** started.
    Refused { waited_ms: u64 },
    /// The work started and did not answer inside the budget. It may still be running
    /// and may still have an effect, so this is never presented as "not executed".
    Unknown { waited_ms: u64 },
}

/// A bounded pool of blocking workers for synchronous service calls.
#[derive(Clone)]
pub struct BlockingPool {
    permits: Arc<Semaphore>,
    workers: usize,
    /// How many calls may run at once now, at most `workers` ([`BlockingPool::set_limit`]).
    limit: Arc<Mutex<usize>>,
    /// Permits kept back when their calls end: the limit was lowered while they were in use.
    owed: Arc<AtomicUsize>,
    counters: Arc<Counters>,
}

impl Default for BlockingPool {
    fn default() -> Self {
        Self::new(BLOCKING_WORKERS)
    }
}

impl BlockingPool {
    #[must_use]
    pub fn new(workers: usize) -> Self {
        let workers = workers.max(1);
        Self {
            permits: Arc::new(Semaphore::new(workers)),
            workers,
            limit: Arc::new(Mutex::new(workers)),
            owed: Arc::new(AtomicUsize::new(0)),
            counters: Arc::new(Counters {
                admitted: std::sync::atomic::AtomicU64::new(0),
                waited: std::sync::atomic::AtomicU64::new(0),
                refused: std::sync::atomic::AtomicU64::new(0),
                unknown: std::sync::atomic::AtomicU64::new(0),
                completed: std::sync::atomic::AtomicU64::new(0),
            }),
        }
    }

    /// The pool's fixed worker count.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.workers
    }

    /// Lets `limit` calls (one to the pool's size) run at once from now on. Calls already
    /// running finish: a lower limit is met as they end, and waiting calls are admitted in turn.
    pub fn set_limit(&self, limit: usize) {
        let limit = limit.clamp(1, self.workers);
        let Ok(mut current) = self.limit.lock() else {
            return;
        };
        if limit > *current {
            let mut raise = limit - *current;
            // A permit still owed was never given back: no longer owing it raises the limit too.
            while raise > 0 && take_one(&self.owed) {
                raise -= 1;
            }
            self.permits.add_permits(raise);
        } else if limit < *current {
            let lower = *current - limit;
            let forgotten = self.permits.forget_permits(lower);
            self.owed.fetch_add(lower - forgotten, Ordering::AcqRel);
        }
        *current = limit;
    }

    /// How many permits are free right now. Used by the isolation tests to wait for the
    /// pool to be genuinely occupied rather than to sleep and hope.
    #[must_use]
    #[allow(dead_code)]
    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }

    /// The pool's own record of what it admitted, waited on, refused and could not answer.
    ///
    /// No product path reads this. It exists so a test or the acceptance driver can assert
    /// that a refusal really was a refusal instead of inferring it from timing.
    #[must_use]
    #[allow(dead_code)]
    pub fn counters(&self) -> BlockingPoolCounters {
        self.counters.snapshot()
    }

    /// Runs `work` on a blocking thread, admitted under `deadline`.
    ///
    /// `work` is the real synchronous production call. It is moved to a blocking thread
    /// whole: nothing about the call is re-implemented here.
    #[cfg(any(test, feature = "test-hooks"))]
    pub async fn run<T, F>(&self, deadline: Instant, work: F) -> BlockingAdmission<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        self.run_settled(deadline, work, |_value: &T| {}).await
    }

    /// The same call, with the one addition that decides whether a late result is owned.
    ///
    /// `settle` is invoked by the blocking thread with the real value the work produced, at
    /// the moment the work really finished — which may be long after the caller walked away.
    /// This is the seam that replaces "the deadline expired, so the JoinHandle is dropped and
    /// its result is discarded": the value reaches the owner that must settle it instead of
    /// disappearing. The caller still receives its own copy of the result on the normal path,
    /// and `settle` runs exactly once, from the thread that really ran the work.
    ///
    /// A blocking task that panics produces no value and therefore delivers nothing; the
    /// caller is told `Unknown` (it may have had a partial effect) and whatever owns this
    /// call's settlement is responsible for its own finite budget. No result is invented here.
    pub async fn run_settled<T, F, S>(
        &self,
        deadline: Instant,
        work: F,
        settle: S,
    ) -> BlockingAdmission<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
        S: FnOnce(&T) + Send + 'static,
    {
        use std::sync::atomic::Ordering::Relaxed;
        let started = Instant::now();
        let remaining = deadline.saturating_duration_since(started);
        if remaining.is_zero() {
            self.counters.refused.fetch_add(1, Relaxed);
            return BlockingAdmission::Refused { waited_ms: 0 };
        }
        let waited = self.permits.available_permits() == 0;
        // The permit is taken under the caller's own deadline. A refusal here is a
        // proven "the work has not started".
        let permit: OwnedSemaphorePermit = match tokio::time::timeout(
            remaining,
            Arc::clone(&self.permits).acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            // The semaphore is never closed in this process; treat a closed pool as a
            // refusal, which is what it is: no worker will ever be free.
            Ok(Err(_)) | Err(_) => {
                self.counters.refused.fetch_add(1, Relaxed);
                return BlockingAdmission::Refused { waited_ms: elapsed_ms(started) };
            }
        };
        if waited {
            self.counters.waited.fetch_add(1, Relaxed);
        } else {
            self.counters.admitted.fetch_add(1, Relaxed);
        }
        let counters = Arc::clone(&self.counters);
        let owed = Arc::clone(&self.owed);
        let handle = tokio::task::spawn_blocking(move || {
            // The permit is released by the work itself: a caller that walked away must
            // not free a worker whose call is still running.
            let permit = permit;
            let value = work();
            // The real value is handed to its settlement owner before the caller is told
            // anything, so a caller that has already given up cannot lose it.
            settle(&value);
            counters.completed.fetch_add(1, Relaxed);
            if take_one(&owed) {
                permit.forget();
            }
            value
        });
        match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), handle).await {
            Ok(Ok(value)) => BlockingAdmission::Completed(value),
            // The blocking task panicked. The permit was released by the unwinding task
            // and the caller gets a real failure rather than a hang.
            Ok(Err(_)) => {
                self.counters.unknown.fetch_add(1, Relaxed);
                BlockingAdmission::Unknown { waited_ms: elapsed_ms(started) }
            }
            // The deadline expired after the work started. The task is not aborted:
            // aborting a running blocking task is not possible, and dropping the handle
            // would not stop it. It keeps running to completion and releases its own
            // permit; the caller is told the outcome is unknown.
            Err(_) => {
                self.counters.unknown.fetch_add(1, Relaxed);
                BlockingAdmission::Unknown { waited_ms: elapsed_ms(started) }
            }
        }
    }
}

/// One fewer owed, when any is.
fn take_one(owed: &AtomicUsize) -> bool {
    owed.fetch_update(Ordering::AcqRel, Ordering::Acquire, |owed| owed.checked_sub(1)).is_ok()
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_full_pool_refuses_without_running_the_work() {
        let pool = BlockingPool::new(1);
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let held = pool.clone();
        let holder = tokio::spawn(async move {
            held.run(Instant::now() + Duration::from_secs(10), move || {
                let _ = wait.recv_timeout(Duration::from_secs(10));
                1
            })
            .await
        });
        // Wait until the one worker really is occupied.
        while pool.available() != 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let admission = pool
            .run(Instant::now() + Duration::from_millis(80), move || {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                2
            })
            .await;
        assert!(matches!(admission, BlockingAdmission::Refused { .. }), "not refused");
        assert!(
            !ran.load(std::sync::atomic::Ordering::SeqCst),
            "a refused call must not have run its work"
        );
        let _ = release.send(());
        let _ = holder.await;
        // Once the worker is released the same call is admitted.
        let admission = pool.run(Instant::now() + Duration::from_secs(5), || 3).await;
        assert!(matches!(admission, BlockingAdmission::Completed(3)));
        assert_eq!(pool.counters().refused, 1);
    }

    #[tokio::test]
    async fn a_deadline_after_the_work_started_is_reported_as_unknown_not_as_not_run() {
        let pool = BlockingPool::new(1);
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&released);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let admission = pool
            .run(Instant::now() + Duration::from_millis(60), move || {
                // A real effect that really happens after the caller stopped waiting.
                let _ = rx.recv_timeout(Duration::from_secs(5));
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            })
            .await;
        assert!(matches!(admission, BlockingAdmission::Unknown { .. }), "not unknown");
        assert!(
            !released.load(std::sync::atomic::Ordering::SeqCst),
            "the work is still running, which is exactly why the answer is unknown"
        );
        let _ = tx.send(());
        // The permit comes back only when the work itself finishes.
        for _ in 0..500 {
            if pool.available() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(4)).await;
        }
        assert!(released.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(pool.available(), 1, "the permit is released by the work, not the caller");
    }

    /// The late value reaches its settlement sink even though the caller already gave up.
    ///
    /// This is the difference between "the deadline expired and the JoinHandle's result was
    /// discarded" and "the work's result has an owner". With `run` - the sink that ignores its
    /// value - the same run delivers nothing, which is what makes this case a regression
    /// rather than a restatement.
    #[tokio::test]
    async fn a_result_that_arrives_after_the_caller_gave_up_reaches_its_settlement_sink() {
        let pool = BlockingPool::new(1);
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let delivered = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = Arc::clone(&delivered);
        let admission = pool
            .run_settled(
                Instant::now() + Duration::from_millis(50),
                move || {
                    let _ = hold.recv_timeout(Duration::from_secs(5));
                    "the real late value".to_owned()
                },
                move |value: &String| {
                    if let Ok(mut delivered) = sink.lock() {
                        delivered.push(value.clone());
                    }
                },
            )
            .await;
        assert!(matches!(admission, BlockingAdmission::Unknown { .. }));
        assert!(
            delivered.lock().map(|delivered| delivered.is_empty()).unwrap_or(false),
            "the work has not finished, so nothing may be settled yet"
        );
        let _ = release.send(());
        let settled = {
            let mut settled = false;
            for _ in 0..500 {
                if delivered.lock().map(|delivered| delivered.len() == 1).unwrap_or(false) {
                    settled = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(4)).await;
            }
            settled
        };
        assert!(settled, "the late result must reach the owner that settles it");
        assert_eq!(
            delivered.lock().map(|delivered| delivered.clone()).unwrap_or_default(),
            vec!["the real late value".to_owned()]
        );
        assert_eq!(pool.available(), 1, "the permit came back with the work, not with the caller");
    }

    /// A call that holds its permit until `release` sends.
    fn holding(
        pool: &BlockingPool,
    ) -> (std::sync::mpsc::Sender<()>, tokio::task::JoinHandle<BlockingAdmission<i32>>) {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let held = pool.clone();
        let call = tokio::spawn(async move {
            held.run(Instant::now() + Duration::from_secs(10), move || {
                let _ = wait.recv_timeout(Duration::from_secs(10));
                1
            })
            .await
        });
        (release, call)
    }

    async fn until(condition: impl Fn() -> bool) {
        for _ in 0..2_500 {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        panic!("the pool never reached the expected state");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_lowered_limit_admits_fewer_and_a_raised_one_admits_more() {
        let pool = BlockingPool::new(3);
        pool.set_limit(1);
        assert_eq!(pool.available(), 1);
        let (release, holder) = holding(&pool);
        until(|| pool.available() == 0).await;
        let refused = pool.run(Instant::now() + Duration::from_millis(80), || 2).await;
        assert!(matches!(refused, BlockingAdmission::Refused { .. }), "one call runs at once");
        pool.set_limit(2);
        let admitted = pool.run(Instant::now() + Duration::from_secs(5), || 2).await;
        assert!(matches!(admitted, BlockingAdmission::Completed(2)), "now a second runs too");
        let _ = release.send(());
        let _ = holder.await;
        assert_eq!(pool.available(), 2);
        assert_eq!(pool.capacity(), 3, "the pool's size is unchanged");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_limit_lowered_while_calls_run_is_met_as_they_end() {
        let pool = BlockingPool::new(2);
        let (release_a, a) = holding(&pool);
        let (release_b, b) = holding(&pool);
        until(|| pool.available() == 0).await;
        pool.set_limit(1);
        let _ = release_a.send(());
        let _ = a.await;
        let _ = release_b.send(());
        let _ = b.await;
        assert_eq!(pool.available(), 1, "one permit was kept back as its call ended");
        pool.set_limit(2);
        assert_eq!(pool.available(), 2, "and is there again once the limit rises");
    }
}
