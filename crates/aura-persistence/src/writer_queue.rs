//! Bounded, two-class work queue for the single persistence writer.
//!
//! WHY TWO CLASSES
//! SQLite allows one writer per database, so every durable change in this process is
//! serialised on one connection by construction. What is *not* forced by SQLite is the
//! order in which the waiting work is chosen and how long a caller may wait before it
//! is told the truth. A single FIFO made a long rebuildable index batch sit in front of
//! the status route and the interactive write behind it, which is the incident this
//! module removes: a batch of derived index work is now admitted to its own lane and
//! runs between interactive work rather than ahead of it.
//!
//! WHAT IS GUARANTEED
//! * **Bounded capacity per class.** [`WorkQueue::enqueue`] never blocks and never
//!   grows: a full lane refuses the caller with [`Enqueue::Full`] at once, so no
//!   caller waits behind an unbounded send.
//! * **No preemption.** The writer takes one job and runs it to completion. A
//!   transaction that has begun is never interrupted by the other class; the classes
//!   only decide which of the *waiting* jobs is picked next.
//! * **Interactive first, index always eventually.** [`WorkClass::Sync`] is drained
//!   before [`WorkClass::Index`], each in arrival order, but only up to
//!   [`SYNC_BURST_BEFORE_INDEX`] jobs in a row: a permanently busy interactive lane
//!   therefore cannot starve index work, while an interactive write still progresses
//!   between index batches.
//! * **One owner per job.** A job's response channel has exactly one sender - the job
//!   itself. The queue never holds a response, so an abandoned caller cannot make a
//!   job look finished.
//! * **Honest shutdown.** After [`WorkQueue::finish`] the queue refuses new jobs and
//!   wakes the writer; the writer drains what was already admitted and then exits.
//!   Nothing sends an unbounded number of shutdown messages and nothing joins a thread
//!   that can still be running a job.
use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex},
};

/// How many interactive jobs may be taken in a row before a waiting index job is picked.
///
/// "Interactive first" is what keeps an interactive write from queueing behind a rebuild;
/// without a bound it is also what starves the index lane forever when interactive work
/// never stops arriving. Four is the smallest burst that still lets a short interactive
/// batch finish together, and it guarantees an index job makes progress in any window
/// where at least one interactive job completes.
pub const SYNC_BURST_BEFORE_INDEX: u32 = 4;

/// Which lane a job belongs to.
///
/// The class is a *scheduling* fact, not an authority: a job in either lane still runs
/// its transaction with the same pragmas, the same connection and the same commit
/// boundary on the writer thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkClass {
    /// Work a caller is waiting on: an interactive command, a lifecycle transition, a
    /// status settlement. Drained first.
    Sync,
    /// Rebuildable, interruptible-in-planning index work: catalog deltas, rebuilds,
    /// re-derivations. Runs between interactive jobs.
    Index,
}

impl WorkClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Index => "index",
        }
    }
}

/// One admitted job: its class, its work, and when it has to be done by.
pub struct Job<T> {
    pub class: WorkClass,
    pub work: T,
    /// The caller's identity for this work, when it supplied one.
    ///
    /// It travels with the job so the terminal state of a write whose caller stopped
    /// waiting is recorded under the caller's own identity rather than guessed at.
    pub identity: Option<String>,
    /// The caller's deadline for the whole operation, carried through the queue so the
    /// executing side can tell SQLite how much of the budget is left instead of using a
    /// fixed busy timeout that ignores it.
    pub deadline: Option<std::time::Instant>,
}

impl<T> Job<T> {
    #[must_use]
    pub const fn new(class: WorkClass, deadline: Option<std::time::Instant>, work: T) -> Self {
        Self { class, work, identity: None, deadline }
    }

    #[must_use]
    pub fn with_identity(
        class: WorkClass,
        deadline: Option<std::time::Instant>,
        identity: impl Into<String>,
        work: T,
    ) -> Self {
        Self { class, work, identity: Some(identity.into()), deadline }
    }

    #[must_use]
    pub const fn work_deadline(&self) -> Option<std::time::Instant> {
        self.deadline
    }

    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }
}

/// Why a job was not admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Enqueue<T> {
    /// The job is now owned by the writer thread.
    Admitted,
    /// This class is at capacity. The job was **not** enqueued and its work has not
    /// run; the caller keeps it and decides what to do.
    Full(T),
    /// The queue is closed for shutdown. The job was **not** enqueued.
    Closed(T),
}

/// How full the queue is, as counters only.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QueueDepth {
    pub sync: usize,
    pub index: usize,
    /// Jobs the writer is running right now: zero or one, always.
    pub running: usize,
    /// How many interactive jobs have been taken in a row. It is reset by every index job,
    /// and it is what shows the fairness bound is really applied rather than intended.
    pub interactive_streak: u32,
    /// Callers refused because their class was at capacity.
    pub refused_full: u64,
    /// Callers refused because the queue was closing.
    pub refused_closed: u64,
    /// Jobs the writer really executed. Counted once per job, after it returned.
    pub executed: u64,
}

struct Lanes<T> {
    sync: VecDeque<Job<T>>,
    index: VecDeque<Job<T>>,
    running: usize,
    interactive_streak: u32,
    closed: bool,
    refused_full: u64,
    refused_closed: u64,
    executed: u64,
}

/// A two-lane bounded queue with one consumer.
pub struct WorkQueue<T> {
    lanes: Mutex<Lanes<T>>,
    ready: Condvar,
    sync_capacity: usize,
    index_capacity: usize,
}

impl<T> WorkQueue<T> {
    #[must_use]
    pub fn new(sync_capacity: usize, index_capacity: usize) -> Self {
        Self {
            lanes: Mutex::new(Lanes {
                sync: VecDeque::new(),
                index: VecDeque::new(),
                running: 0,
                interactive_streak: 0,
                closed: false,
                refused_full: 0,
                refused_closed: 0,
                executed: 0,
            }),
            ready: Condvar::new(),
            // Zero capacity would mean "never admit anything", which would refuse every
            // production call. One is the smallest queue that can make progress.
            sync_capacity: sync_capacity.max(1),
            index_capacity: index_capacity.max(1),
        }
    }

    /// Admits one job, or refuses it at once. Never blocks.
    pub fn enqueue(&self, job: Job<T>) -> Enqueue<T> {
        let Ok(mut lanes) = self.lanes.lock() else {
            return Enqueue::Closed(job.work);
        };
        if lanes.closed {
            lanes.refused_closed = lanes.refused_closed.saturating_add(1);
            return Enqueue::Closed(job.work);
        }
        let (lane, capacity) = match job.class {
            WorkClass::Sync => (&mut lanes.sync, self.sync_capacity),
            WorkClass::Index => (&mut lanes.index, self.index_capacity),
        };
        if lane.len() >= capacity {
            lanes.refused_full = lanes.refused_full.saturating_add(1);
            return Enqueue::Full(job.work);
        }
        lane.push_back(job);
        drop(lanes);
        self.ready.notify_all();
        Enqueue::Admitted
    }

    /// The next job to run: `Sync` before `Index` — up to [`SYNC_BURST_BEFORE_INDEX`] of
    /// them in a row — and arrival order inside a class.
    ///
    /// The burst bound is the difference between "interactive work goes first" and
    /// "interactive work can starve the index forever": once the interactive lane has been
    /// served that many times without a break, a waiting index job is taken even though
    /// more interactive work is waiting. A permanently busy interactive lane therefore
    /// still lets index work progress, and a short interactive batch still finishes
    /// together.
    ///
    /// `None` means the queue is closed and empty, which is the writer's exit
    /// condition.
    pub fn next_job(&self) -> Option<Job<T>> {
        let Ok(mut lanes) = self.lanes.lock() else {
            return None;
        };
        loop {
            let class = if !lanes.sync.is_empty() {
                if !lanes.index.is_empty() && lanes.interactive_streak >= SYNC_BURST_BEFORE_INDEX {
                    WorkClass::Index
                } else {
                    WorkClass::Sync
                }
            } else if !lanes.index.is_empty() {
                WorkClass::Index
            } else if lanes.closed {
                return None;
            } else {
                match self.ready.wait(lanes) {
                    Ok(next) => {
                        lanes = next;
                        continue;
                    }
                    Err(_) => return None,
                }
            };
            let job = match class {
                WorkClass::Sync => lanes.sync.pop_front(),
                WorkClass::Index => lanes.index.pop_front(),
            }?;
            match class {
                WorkClass::Sync => {
                    lanes.interactive_streak = lanes.interactive_streak.saturating_add(1);
                }
                WorkClass::Index => lanes.interactive_streak = 0,
            }
            lanes.running = 1;
            return Some(job);
        }
    }

    /// Records that the job handed out by [`Self::next_job`] has finished.
    pub fn finished_job(&self) {
        if let Ok(mut lanes) = self.lanes.lock() {
            lanes.running = 0;
            lanes.executed = lanes.executed.saturating_add(1);
        }
        self.ready.notify_all();
    }

    /// Closes the queue for admission and wakes the writer.
    ///
    /// Jobs already admitted still run: refusing them now would abandon callers whose
    /// work is already the writer's. `false` means the queue was already closed, so a
    /// repeated close cannot double-count a settlement.
    pub fn finish(&self) -> bool {
        let Ok(mut lanes) = self.lanes.lock() else {
            return false;
        };
        let first = !lanes.closed;
        lanes.closed = true;
        drop(lanes);
        self.ready.notify_all();
        first
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.lanes.lock().is_ok_and(|lanes| lanes.closed)
    }

    #[must_use]
    pub fn depth(&self) -> QueueDepth {
        self.lanes.lock().map_or_else(
            |_| QueueDepth::default(),
            |lanes| QueueDepth {
                sync: lanes.sync.len(),
                index: lanes.index.len(),
                running: lanes.running,
                interactive_streak: lanes.interactive_streak,
                refused_full: lanes.refused_full,
                refused_closed: lanes.refused_closed,
                executed: lanes.executed,
            },
        )
    }

    #[must_use]
    pub const fn capacities(&self) -> (usize, usize) {
        (self.sync_capacity, self.index_capacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(class: WorkClass, work: i32) -> Job<i32> {
        Job::new(class, None, work)
    }

    #[test]
    fn a_full_class_refuses_at_once_and_the_other_class_is_unaffected() {
        let queue = WorkQueue::new(2, 1);
        assert_eq!(queue.enqueue(job(WorkClass::Sync, 1)), Enqueue::Admitted);
        assert_eq!(queue.enqueue(job(WorkClass::Sync, 2)), Enqueue::Admitted);
        assert_eq!(queue.enqueue(job(WorkClass::Sync, 3)), Enqueue::Full(3));
        assert_eq!(queue.enqueue(job(WorkClass::Index, 4)), Enqueue::Admitted);
        assert_eq!(queue.enqueue(job(WorkClass::Index, 5)), Enqueue::Full(5));
        let depth = queue.depth();
        assert_eq!((depth.sync, depth.index, depth.refused_full), (2, 1, 2));
        assert_eq!(depth.executed, 0, "a refused job is not an executed job");
    }

    #[test]
    fn interactive_work_is_drained_before_index_work() {
        let queue = WorkQueue::new(4, 4);
        for work in [10, 20] {
            assert_eq!(queue.enqueue(job(WorkClass::Index, work)), Enqueue::Admitted);
        }
        assert_eq!(queue.enqueue(job(WorkClass::Sync, 30)), Enqueue::Admitted);
        let first = queue.next_job().expect("a job");
        assert_eq!((first.class, first.work), (WorkClass::Sync, 30));
        queue.finished_job();
        let second = queue.next_job().expect("a job");
        assert_eq!((second.class, second.work), (WorkClass::Index, 10));
        queue.finished_job();
        assert_eq!(queue.depth().executed, 2);
    }

    #[test]
    fn a_job_carries_the_caller_deadline_through_the_queue() {
        let queue = WorkQueue::new(1, 1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        assert_eq!(queue.enqueue(Job::new(WorkClass::Sync, Some(deadline), 7)), Enqueue::Admitted);
        let job = queue.next_job().expect("a job");
        assert_eq!(job.work_deadline(), Some(deadline));
    }

    /// The fairness bound: a permanently busy interactive lane must not starve the index
    /// lane, and a short interactive burst must still be served together.
    #[test]
    fn a_permanently_busy_interactive_lane_cannot_starve_the_index_lane() {
        let queue = WorkQueue::new(64, 4);
        assert_eq!(queue.enqueue(job(WorkClass::Index, 99)), Enqueue::Admitted);
        let mut taken = Vec::new();
        for work in 0..(SYNC_BURST_BEFORE_INDEX + 1) {
            let work = i32::try_from(work).expect("a small burst index");
            assert_eq!(queue.enqueue(job(WorkClass::Sync, work)), Enqueue::Admitted);
            let job = queue.next_job().expect("a job");
            taken.push((job.class, job.work));
            queue.finished_job();
        }
        println!("fairness taken={taken:?} depth={:?}", queue.depth());
        assert_eq!(
            taken,
            vec![
                (WorkClass::Sync, 0),
                (WorkClass::Sync, 1),
                (WorkClass::Sync, 2),
                (WorkClass::Sync, 3),
                (WorkClass::Index, 99),
            ],
            "the index job has to be taken once the interactive burst bound is reached"
        );
        assert_eq!(queue.depth().interactive_streak, 0, "an index job resets the streak");
        assert_eq!(queue.depth().executed, 5, "every job was really taken exactly once");
    }

    #[test]
    fn a_job_carries_the_caller_identity_through_the_queue() {
        let queue = WorkQueue::new(1, 1);
        assert_eq!(
            queue.enqueue(Job::with_identity(WorkClass::Index, None, "delta-7", 7)),
            Enqueue::Admitted
        );
        let job = queue.next_job().expect("a job");
        assert_eq!(job.identity(), Some("delta-7"));
        assert_eq!(
            Job::<i32>::new(WorkClass::Sync, None, 1).identity(),
            None,
            "a job submitted without an identity has none"
        );
    }

    #[test]
    fn a_closed_queue_refuses_and_a_second_close_is_not_a_second_settlement() {
        let queue = WorkQueue::new(1, 1);
        assert!(queue.finish());
        assert!(!queue.finish(), "a repeated close must not settle twice");
        assert_eq!(queue.enqueue(job(WorkClass::Sync, 1)), Enqueue::Closed(1));
        assert_eq!(queue.next_job().map(|job| job.work), None);
    }
}
