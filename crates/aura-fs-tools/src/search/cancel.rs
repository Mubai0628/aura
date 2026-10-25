use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Cooperative cancellation for one search. Workers observe this at file and
/// chunk boundaries and must stop; the coordinator then joins and releases
/// slots/handles.
///
/// It also carries the search's *work lifecycle*, because three different facts
/// are easy to confuse and must not stand in for each other:
///
/// * cancellation was **requested** ([`Self::cancel`]);
/// * the **coordinator** returned ([`Self::mark_coordinator_done`]) — it stopped
///   admitting work and produced a result or an error;
/// * every piece of work it **admitted** has really ended
///   ([`Self::is_finished`]).
///
/// Only the third one means the scan is over. The coordinator can return while a
/// worker is still inside synchronous I/O, and a cancelled search can still have
/// a file being read.
#[derive(Clone, Debug)]
pub struct SearchCancel {
    flags: Arc<Vec<Arc<AtomicBool>>>,
    started: Arc<AtomicBool>,
    lifecycle: Arc<Lifecycle>,
}

#[derive(Debug, Default)]
struct Lifecycle {
    /// Work admitted to the pool and not yet reported as ended.
    admitted: AtomicUsize,
    /// The coordinator has stopped admitting and returned.
    coordinator_done: AtomicBool,
}

impl Default for SearchCancel {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchCancel {
    #[must_use]
    pub fn new() -> Self {
        Self {
            flags: Arc::new(vec![Arc::new(AtomicBool::new(false))]),
            started: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(Lifecycle::default()),
        }
    }

    #[must_use]
    pub fn merge(left: &Self, right: &Self) -> Self {
        let mut flags = Vec::with_capacity(left.flags.len() + right.flags.len());
        flags.extend(left.flags.iter().cloned());
        flags.extend(right.flags.iter().cloned());
        Self {
            flags: Arc::new(flags),
            started: Arc::clone(&left.started),
            // The merged handle reports the same work lifecycle as the handle
            // the caller registered, so admitted work is visible to both.
            lifecycle: Arc::clone(&left.lifecycle),
        }
    }

    pub fn cancel(&self) {
        for flag in self.flags.iter() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flags.iter().any(|flag| flag.load(Ordering::SeqCst))
    }

    pub fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    /// Count one piece of work as admitted, before it can start. The returned
    /// guard reports it as ended when it drops, so a worker that panics or is
    /// abandoned still settles its share.
    #[must_use]
    pub fn admit_work(&self) -> SearchWorkGuard {
        self.lifecycle.admitted.fetch_add(1, Ordering::SeqCst);
        SearchWorkGuard { lifecycle: Arc::clone(&self.lifecycle) }
    }

    /// Record that the coordinator returned (successfully or not).
    pub fn mark_coordinator_done(&self) {
        self.lifecycle.coordinator_done.store(true, Ordering::SeqCst);
    }

    /// Whether the coordinator has returned.
    #[must_use]
    pub fn coordinator_done(&self) -> bool {
        self.lifecycle.coordinator_done.load(Ordering::SeqCst)
    }

    /// Work admitted but not yet reported as ended.
    #[must_use]
    pub fn admitted_work(&self) -> usize {
        self.lifecycle.admitted.load(Ordering::SeqCst)
    }

    /// Whether the whole search is over: the coordinator returned *and* every
    /// piece of admitted work ended. This is the only completion signal callers
    /// may treat as reclamation.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.coordinator_done() && self.admitted_work() == 0
    }
}

/// Reports one admitted piece of search work as ended, however it ends.
pub struct SearchWorkGuard {
    lifecycle: Arc<Lifecycle>,
}

impl Drop for SearchWorkGuard {
    fn drop(&mut self) {
        self.lifecycle.admitted.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Marks a coordinator run as returned when it goes out of scope, so an early
/// return or a propagated error still reports "no more work will be admitted".
///
/// This deliberately does **not** claim the admitted work is finished.
pub struct SearchCompletion {
    cancel: SearchCancel,
}

impl SearchCompletion {
    #[must_use]
    pub fn arm(cancel: &SearchCancel) -> Self {
        Self { cancel: cancel.clone() }
    }
}

impl Drop for SearchCompletion {
    fn drop(&mut self) {
        self.cancel.mark_coordinator_done();
    }
}
