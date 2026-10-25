use std::{
    collections::{HashMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, OnceLock, Weak},
    thread,
    time::Duration,
};

use aura_domain_common::SessionId;

pub(crate) const MAX_ACTIVE: usize = 4;
/// Per-session pending jobs.
pub(crate) const QUEUE_BOUND: usize = 8;
/// Process-wide pending jobs across every session. A per-session bound alone
/// cannot cap total queued memory when many sessions search at once.
pub(crate) const GLOBAL_QUEUE_BOUND: usize = 32;

pub(crate) struct SearchJob {
    pub session: SessionId,
    pub work: Box<dyn FnOnce() + Send>,
}

/// Capacity check and admission happen under one lock, and the job factory runs
/// only after the check passes. A rejected submission gets `rejected` back, so
/// the caller keeps the file and its reservation.
pub(crate) struct Submission<F, R> {
    session: SessionId,
    work: F,
    rejected: R,
}

impl<F, R> Submission<F, R> {
    pub(crate) fn new(session: SessionId, work: F, rejected: R) -> Self {
        Self { session, work, rejected }
    }
}

struct Queued {
    job: SearchJob,
}

struct Inner {
    active: usize,
    active_by: HashMap<SessionId, usize>,
    queues: HashMap<SessionId, VecDeque<Queued>>,
    order: VecDeque<SessionId>,
    queued_total: usize,
    shutdown: bool,
}

/// Process-wide bounded worker pool. At most four OS threads scan files.
/// Pending jobs are capped per session at [`QUEUE_BOUND`] and in total at
/// [`GLOBAL_QUEUE_BOUND`]; a waiter with fewer in-flight workers is preferred.
pub(crate) struct FairPool {
    inner: Mutex<Inner>,
    cv: Condvar,
}

impl FairPool {
    fn construct() -> Arc<Self> {
        let pool = Arc::new(Self {
            inner: Mutex::new(Inner {
                active: 0,
                active_by: HashMap::new(),
                queues: HashMap::new(),
                order: VecDeque::new(),
                queued_total: 0,
                shutdown: false,
            }),
            cv: Condvar::new(),
        });
        for index in 0..MAX_ACTIVE {
            let weak = Arc::downgrade(&pool);
            thread::Builder::new()
                .name(format!("aura-fs-search-{index}"))
                .spawn(move || worker_loop(weak))
                .expect("search worker");
        }
        pool
    }

    pub(crate) fn global() -> &'static Arc<Self> {
        static POOL: OnceLock<Arc<FairPool>> = OnceLock::new();
        POOL.get_or_init(Self::construct)
    }

    #[cfg(test)]
    pub(crate) fn isolated() -> Arc<Self> {
        Self::construct()
    }

    /// Admit one job if capacity allows. The factory runs only after the
    /// capacity check passes under the same lock; a rejection returns the
    /// caller's value untouched, so no file or reservation is lost.
    pub(crate) fn try_submit<F, R>(&self, submission: Submission<F, R>) -> Result<(), R>
    where
        F: FnOnce() -> Result<Box<dyn FnOnce() + Send>, R>,
    {
        let mut inner = self.lock();
        if inner.shutdown {
            return Err(submission.rejected);
        }
        let session = submission.session;
        let queued = inner.queues.get(&session).map(VecDeque::len).unwrap_or(0);
        if queued >= QUEUE_BOUND || inner.queued_total >= GLOBAL_QUEUE_BOUND {
            return Err(submission.rejected);
        }
        let work = (submission.work)()?;
        let was_empty = inner.queues.get(&session).is_none_or(VecDeque::is_empty);
        inner
            .queues
            .entry(session)
            .or_default()
            .push_back(Queued { job: SearchJob { session, work } });
        inner.queued_total += 1;
        if was_empty {
            inner.order.push_back(session);
        }
        self.cv.notify_all();
        Ok(())
    }

    /// Whether the pool has been shut down. Submissions are refused from then on.
    #[must_use]
    pub(crate) fn is_shutdown(&self) -> bool {
        self.lock().shutdown
    }

    fn take_job(&self) -> Option<SearchJob> {
        let mut inner = self.lock();
        loop {
            if inner.shutdown {
                return None;
            }
            if inner.active < MAX_ACTIVE
                && let Some(session) = preferred_session(&inner).copied()
                && let Some(queued) = inner.queues.get_mut(&session).and_then(VecDeque::pop_front)
            {
                inner.order.retain(|queued_session| queued_session != &session);
                if inner.queues.get(&session).is_some_and(|queue| !queue.is_empty()) {
                    inner.order.push_back(session);
                } else {
                    inner.queues.remove(&session);
                }
                inner.queued_total = inner.queued_total.saturating_sub(1);
                inner.active += 1;
                *inner.active_by.entry(session).or_insert(0) += 1;
                self.cv.notify_all();
                return Some(queued.job);
            }
            let waited = self.cv.wait_timeout(inner, Duration::from_millis(50));
            inner = waited.unwrap_or_else(|error| error.into_inner()).0;
        }
    }

    fn release(&self, session: SessionId) {
        let mut inner = self.lock();
        inner.active = inner.active.saturating_sub(1);
        if let Some(count) = inner.active_by.get_mut(&session) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                inner.active_by.remove(&session);
            }
        }
        self.cv.notify_all();
    }

    #[must_use]
    pub(crate) fn active(&self) -> usize {
        self.lock().active
    }

    #[must_use]
    pub(crate) fn queued(&self) -> usize {
        self.lock().queued_total
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchPoolLoad {
    pub active: usize,
    pub queued: usize,
}

#[must_use]
pub fn search_pool_load() -> SearchPoolLoad {
    let pool = FairPool::global();
    SearchPoolLoad { active: pool.active(), queued: pool.queued() }
}

impl Drop for FairPool {
    fn drop(&mut self) {
        self.lock().shutdown = true;
        self.cv.notify_all();
    }
}

fn preferred_session(inner: &Inner) -> Option<&SessionId> {
    inner.order.iter().min_by_key(|session| inner.active_by.get(*session).copied().unwrap_or(0))
}

fn worker_loop(weak: Weak<FairPool>) {
    loop {
        let Some(pool) = weak.upgrade() else {
            return;
        };
        let Some(job) = pool.take_job() else {
            return;
        };
        let session = job.session;
        let _ = catch_unwind(AssertUnwindSafe(job.work));
        pool.release(session);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use aura_domain_common::SessionId;

    use super::*;

    /// Submit a job that blocks on `hold` until the guard is dropped. Returns
    /// whether the pool admitted it.
    fn submit_blocking(
        pool: &FairPool,
        session: SessionId,
        hold: &Arc<Mutex<()>>,
        marker: Option<&Arc<Mutex<Vec<u8>>>>,
    ) -> bool {
        let hold = Arc::clone(hold);
        let marker = marker.cloned();
        pool.try_submit(Submission::new(
            session,
            move || {
                Ok(Box::new(move || {
                    let _guard = hold.lock().expect("wait");
                    if let Some(marker) = marker {
                        marker.lock().unwrap_or_else(|error| error.into_inner()).push(1);
                    }
                }) as Box<dyn FnOnce() + Send>)
            },
            (),
        ))
        .is_ok()
    }

    #[test]
    fn queue_bound_rejects_additional_jobs() {
        let pool = FairPool::isolated();
        let session = SessionId::new();
        let hold = Arc::new(Mutex::new(()));
        let guard = hold.lock().expect("hold");
        // Far more submissions than the pool can ever accept: the bound is only
        // meaningful if the surplus is rejected.
        let mut admitted = 0_usize;
        let mut rejected = 0_usize;
        for _ in 0..64 {
            if submit_blocking(&pool, session, &hold, None) {
                admitted += 1;
            } else {
                rejected += 1;
            }
        }
        assert!(rejected > 0, "a bounded pool must reject surplus submissions");
        assert!(
            admitted <= MAX_ACTIVE + QUEUE_BOUND,
            "admitted {admitted} exceeds active+queued capacity"
        );
        // The rejection hands the caller's value straight back.
        let returned = pool.try_submit(Submission::new(
            session,
            || Ok(Box::new(|| {}) as Box<dyn FnOnce() + Send>),
            7_u8,
        ));
        assert_eq!(returned, Err(7));
        drop(guard);
        std::thread::sleep(Duration::from_millis(200));
        assert!(pool.active() <= MAX_ACTIVE);
        assert_eq!(pool.queued(), 0);
    }

    #[test]
    fn global_queue_bound_caps_many_sessions() {
        let pool = FairPool::isolated();
        let hold = Arc::new(Mutex::new(()));
        let guard = hold.lock().expect("hold");
        let mut admitted = 0_usize;
        // Each session submits once, so only the global bound can stop this.
        for _ in 0..(GLOBAL_QUEUE_BOUND * 2) {
            let session = SessionId::new();
            if submit_blocking(&pool, session, &hold, None) {
                admitted += 1;
            } else {
                break;
            }
        }
        assert!(admitted <= MAX_ACTIVE + GLOBAL_QUEUE_BOUND, "admitted {admitted}");
        assert!(admitted >= MAX_ACTIVE, "only {admitted} admitted before the cap");
        drop(guard);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(pool.queued(), 0);
    }

    #[test]
    fn fairness_prefers_the_session_with_fewer_active_workers() {
        // Deterministic unit test of the selection rule: the queue never gets a
        // chance to hide it behind worker timing.
        let busy = SessionId::new();
        let waiting = SessionId::new();
        let mut inner = Inner {
            active: MAX_ACTIVE,
            active_by: HashMap::new(),
            queues: HashMap::new(),
            order: VecDeque::new(),
            queued_total: 0,
            shutdown: false,
        };
        inner.active_by.insert(busy, 3);
        inner.active_by.insert(waiting, 1);
        inner.queues.entry(busy).or_default();
        inner.queues.entry(waiting).or_default();
        inner.order.push_back(busy);
        inner.order.push_back(waiting);
        // The session with fewer active workers wins, not the queue order.
        assert_eq!(preferred_session(&inner).copied(), Some(waiting));

        // A session holding no slot at all beats one that already holds slots,
        // even when the busier session queued first.
        let idle = SessionId::new();
        inner.queues.entry(idle).or_default();
        inner.order.clear();
        inner.order.push_back(busy);
        inner.order.push_back(idle);
        assert_eq!(preferred_session(&inner).copied(), Some(idle));
    }
}
