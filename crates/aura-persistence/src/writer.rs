//! The single persistence writer: its queue, budgets, health and teardown.
//!
//! WHAT THIS FILE OWNS
//! One SQLite database has one writer, so this process has one writer thread and one
//! writer connection. Everything below is about the *waiting* around that fact, which
//! SQLite does not decide for us:
//!
//! * **Admission** - [`WorkQueue`] holds two bounded lanes. [`WorkClass::Sync`] work is
//!   drained before [`WorkClass::Index`] work, so an interactive write progresses
//!   between index batches instead of queuing behind them. A full lane refuses at once.
//! * **Execution** - the writer thread runs one job at a time on the one connection.
//!   Before each job it sets the SQLite busy timeout from what is left of that job's
//!   deadline, so the total a caller can experience is enqueue wait + SQL busy wait +
//!   the job itself, and never an unbounded fourth thing. The default busy timeout
//!   applies when a job carries no deadline.
//! * **Classification** - the caller, not this module, decides what a result means: a
//!   refusal before enqueue is [`PersistenceError::WriterBusy`] ("not executed"), a
//!   deadline that expired *after* enqueue is [`PersistenceError::WriterUnresponsive`]
//!   ("no answer; the write may have run"), and the job's own `Result` is passed
//!   through untouched.
//! * **Shutdown** - [`Writer::shutdown`] closes the queue once, wakes the writer, and
//!   waits a bounded time for it to drain. A writer still inside a job when the budget
//!   expires keeps a real owner, a readable terminal state ([`WriterHealth`]) and a
//!   named outcome (`abandoned`); it is never abandoned silently and a repeated close
//!   does not settle twice.
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use rusqlite::Connection;

use crate::{
    PersistenceError,
    writer_queue::{Enqueue, Job, WorkClass, WorkQueue},
};
// The storage hold points exist only in a debug build. Every use below carries the
// same gate, so a release build compiles none of this module or its callers.
#[cfg(debug_assertions)]
use crate::barrier::{Barrier, Slot};

/// How much synchronous work may wait in its lane before new callers are refused.
pub const SYNC_QUEUE_CAPACITY: usize = 32;
/// How much rebuildable index work may wait in its lane.
pub const INDEX_QUEUE_CAPACITY: usize = 8;
/// How long a graceful shutdown waits for the writer to drain before it stops waiting.
pub const SHUTDOWN_JOIN_BUDGET: Duration = Duration::from_secs(2);
/// How long a caller keeps listening for the result of an identified job **after** the
/// caller's own wait for that job has already ended.
///
/// WHY THIS IS A BUDGET AND NOT AN OPEN WAIT. The job was already admitted, so the value
/// it produces exists whether or not anybody is still waiting; this is the window in which
/// the caller that gave up can still *receive* it instead of it dying with its own wait.
/// It is the same order as [`WRITER_BUSY_TIMEOUT`], the connection's own per-statement lock
/// budget, so it is bounded by construction: it is not a second chance for the write to be
/// fast, it is the receipt of a result that may already exist. When nothing arrives inside
/// it the answer stays the honest "admitted, not answered" - and a caller may shorten it,
/// never lengthen it, for its own instance.
pub const DEFAULT_SETTLED_RECEIPT_BUDGET: Duration = Duration::from_secs(5);
/// SQLite's own busy-handler budget when a job carries no deadline of its own.
const WRITER_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// A job's own deadline: the point in time by which the whole operation, including
/// SQLite's own lock waits, should have finished. `None` uses the connection default.
pub type Deadline = Option<Instant>;

/// What one job did, as the writer thread saw it: whether its own operation succeeded, and
/// whether the answer reached the caller.
type JobDone = (bool, bool);

type WriterTask = Box<dyn FnOnce(&mut Connection, Deadline) -> JobDone + Send + 'static>;

/// One write-class job's terminal record, kept after its caller may have stopped waiting.
///
/// The identity is the caller's own, so "did the write I stopped waiting for commit?" is a
/// lookup rather than a guess or a resend. A job that was refused before admission has no
/// record at all: nothing ran, and "no record" is the honest answer for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSettlement {
    pub class: WorkClass,
    /// The caller's identity for this work, exactly as it was submitted.
    pub identity: String,
    /// Whether the job's own operation returned success — the transaction committed or it
    /// did not, as the closure reported.
    pub succeeded: bool,
    /// Whether the answer reached the caller. `false` means the caller had already stopped
    /// waiting, so this record is what resolves an outcome the caller was told was unknown.
    pub answered: bool,
}

/// Where the thread that ran an identified job publishes what that job produced.
///
/// WHY THIS EXISTS. A terminal record says *whether* the write committed; it cannot say
/// what it wrote. The value travelled through the response channel, and that channel dies
/// with the caller's wait - so a caller told [`PersistenceError::WriterUnresponsive`] lost
/// a value the store had already produced, even when its transaction committed a moment
/// later. A business write is not allowed to lose its value that way.
///
/// So an identified job that opted in publishes its own `Result` here **before** it wakes
/// its caller: whatever the job produced is readable from this slot by whoever still has a
/// reason to ask, and the value can no longer race the caller's deadline out of existence.
/// Nothing is recomputed and nothing is re-run; this is the result the job's own thread
/// produced. The slot is owned by the one call that created it, so it needs no registry,
/// no eviction and no timer: when that call and the job are both done it is freed.
struct RetainedSlot<T> {
    outcome: Mutex<Option<Result<T, PersistenceError>>>,
    published: Condvar,
}

impl<T> RetainedSlot<T> {
    fn new() -> Self {
        Self { outcome: Mutex::new(None), published: Condvar::new() }
    }

    fn publish(&self, outcome: Result<T, PersistenceError>) {
        if let Ok(mut slot) = self.outcome.lock() {
            *slot = Some(outcome);
        }
        self.published.notify_all();
    }

    fn take(&self) -> Option<Result<T, PersistenceError>> {
        self.outcome.lock().ok()?.take()
    }

    /// Takes the published result, waiting for the job's own thread to publish it.
    ///
    /// The wait is on the job's real completion - the publish that wakes it is the one the
    /// thread that ran the work performs - so this is a receipt and not a poller. `None`
    /// means nothing was published inside `budget`, which is a fact about the wait and
    /// never a claim that the write did not happen.
    fn take_within(&self, budget: Duration) -> Option<Result<T, PersistenceError>> {
        let deadline = Instant::now() + budget;
        let mut slot = self.outcome.lock().ok()?;
        loop {
            if let Some(outcome) = slot.take() {
                return Some(outcome);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            slot = self.published.wait_timeout(slot, remaining).ok()?.0;
        }
    }
}

/// The terminal records of the write-class jobs this runtime has run, newest last.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WriterSettlements {
    /// Records kept: bounded by [`SETTLEMENT_CAPACITY`].
    pub recent: Vec<JobSettlement>,
    /// Jobs whose terminal state was recorded under an identity.
    pub settled: u64,
    /// Of those, the ones whose caller had already stopped waiting.
    pub settled_after_caller_left: u64,
    /// Records dropped because the log is bounded.
    pub dropped: u64,
}

/// How many terminal records are kept. Bounded so a long-lived runtime cannot grow this;
/// a caller that needs an older answer re-derives it from the durable facts.
pub const SETTLEMENT_CAPACITY: usize = 64;

/// The bounded log of job terminal states.
#[derive(Default)]
struct SettlementLog {
    entries: Mutex<VecDeque<JobSettlement>>,
    settled: AtomicI64,
    after_caller_left: AtomicI64,
    dropped: AtomicI64,
}

impl SettlementLog {
    fn record(&self, settlement: JobSettlement) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if !settlement.answered {
            self.after_caller_left.fetch_add(1, Ordering::Relaxed);
        }
        self.settled.fetch_add(1, Ordering::Relaxed);
        while entries.len() >= SETTLEMENT_CAPACITY {
            entries.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        entries.push_back(settlement);
    }

    fn of(&self, identity: &str) -> Option<JobSettlement> {
        self.entries
            .lock()
            .ok()?
            .iter()
            .rev()
            .find(|settlement| settlement.identity == identity)
            .cloned()
    }

    fn snapshot(&self) -> WriterSettlements {
        WriterSettlements {
            recent: self
                .entries
                .lock()
                .map(|entries| entries.iter().cloned().collect())
                .unwrap_or_default(),
            settled: u64::try_from(self.settled.load(Ordering::Relaxed)).unwrap_or(u64::MAX),
            settled_after_caller_left: u64::try_from(
                self.after_caller_left.load(Ordering::Relaxed),
            )
            .unwrap_or(u64::MAX),
            dropped: u64::try_from(self.dropped.load(Ordering::Relaxed)).unwrap_or(u64::MAX),
        }
    }
}

/// What the writer thread is doing, as an explicit terminal state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriterState {
    /// Accepting jobs, nothing running.
    Running,
    /// A job is running right now.
    Executing,
    /// Closed for admission; running what was already admitted.
    Draining,
    /// The loop returned. Nothing else will be executed on this connection.
    Stopped,
    /// The writer thread ended by unwinding: its jobs did not get a normal answer.
    Panicked,
}

impl WriterState {
    const fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Running,
            1 => Self::Executing,
            2 => Self::Draining,
            3 => Self::Stopped,
            _ => Self::Panicked,
        }
    }

    const fn code(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::Executing => 1,
            Self::Draining => 2,
            Self::Stopped => 3,
            Self::Panicked => 4,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Executing => "executing",
            Self::Draining => "draining",
            Self::Stopped => "stopped",
            Self::Panicked => "panicked",
        }
    }
}

/// Absolute, monotonic SQLite work counters for this runtime's writer connection.
///
/// `vm_steps` is the number of virtual-machine instructions the writer connection has
/// executed. Since SQLite exposes that per prepared statement rather than per
/// connection, the writer installs a progress handler that fires every
/// [`VM_STEP_GRANULARITY`] instructions while a caller has asked for a measurement:
/// that hook sees *every* statement the writer runs, including the ones inside a
/// statement cache. The counters only ever grow, so a caller reports their delta around
/// one operation. They are never read on a production decision path.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PersistenceArtifactCounters {
    pub vm_steps: i64,
    pub changed_rows: i64,
    /// Jobs the writer really executed, cumulative.
    pub jobs_executed: i64,
}

impl PersistenceArtifactCounters {
    /// The work between two readings of the same runtime.
    #[must_use]
    pub const fn since(self, earlier: Self) -> Self {
        Self {
            vm_steps: self.vm_steps.saturating_sub(earlier.vm_steps),
            changed_rows: self.changed_rows.saturating_sub(earlier.changed_rows),
            jobs_executed: self.jobs_executed.saturating_sub(earlier.jobs_executed),
        }
    }
}

/// How many virtual-machine instructions one progress-handler callback stands for.
///
/// Small enough that a single-row operation is still measurable — which is exactly
/// what "one path costs the same at every index size" has to be able to show — and
/// large enough that counting is invisible beside the work it counts.
const VM_STEP_GRANULARITY: i32 = 100;

#[derive(Default)]
pub(crate) struct PersistenceCounters {
    vm_steps: AtomicI64,
    changed_rows: AtomicI64,
    jobs_executed: AtomicI64,
    measuring: AtomicBool,
}

impl PersistenceCounters {
    /// Adds whatever the writer connection has accumulated since the last read.
    ///
    /// The instruction count comes from the progress handler, which only runs while a
    /// caller has asked to measure — so an ordinary production runtime pays nothing
    /// for this beyond one relaxed load.
    fn record(&self, connection: &Connection) {
        self.changed_rows.store(
            i64::try_from(connection.total_changes()).unwrap_or(i64::MAX),
            Ordering::Relaxed,
        );
    }

    /// Copies the queue's authoritative executed-job count into the counters a caller
    /// reads, so "how many jobs ran" has one answer and not two.
    fn record_jobs(&self, executed: u64) {
        self.jobs_executed.store(i64::try_from(executed).unwrap_or(i64::MAX), Ordering::Relaxed);
    }

    fn progress(&self) -> bool {
        if self.measuring.load(Ordering::Relaxed) {
            self.vm_steps.fetch_add(i64::from(VM_STEP_GRANULARITY), Ordering::Relaxed);
        }
        // Never non-zero: this hook measures, it does not interrupt.
        false
    }

    pub(crate) fn snapshot(&self) -> PersistenceArtifactCounters {
        PersistenceArtifactCounters {
            vm_steps: self.vm_steps.load(Ordering::Relaxed),
            changed_rows: self.changed_rows.load(Ordering::Relaxed),
            jobs_executed: self.jobs_executed.load(Ordering::Relaxed),
        }
    }
}

/// The writer thread's own record of how it ended.
struct Lifecycle {
    state: AtomicU8,
    /// How many times shutdown was requested. A repeated close must not settle twice,
    /// and this is what shows whether it did.
    close_requests: AtomicUsize,
    /// Whether the thread was really joined.
    joined: AtomicBool,
    /// A writer thread still inside a job when the join budget expired. Recorded so the
    /// situation has a name; it is never presented as "stopped".
    abandoned: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Lifecycle {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(WriterState::Running.code()),
            close_requests: AtomicUsize::new(0),
            joined: AtomicBool::new(false),
            abandoned: AtomicBool::new(false),
            thread: Mutex::new(None),
        }
    }

    fn set(&self, state: WriterState) {
        // A terminal state is never overwritten by a later non-terminal one, so a
        // "stopped"/"panicked" answer cannot be replaced by "running".
        let current = WriterState::from_code(self.state.load(Ordering::Acquire));
        let next = match (current, state) {
            (WriterState::Stopped | WriterState::Panicked, _) => current,
            _ => state,
        };
        self.state.store(next.code(), Ordering::Release);
    }

    fn state(&self) -> WriterState {
        WriterState::from_code(self.state.load(Ordering::Acquire))
    }
}

/// What the writer is doing, and whether it is still there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriterHealth {
    pub state: WriterState,
    pub sync_queued: usize,
    pub index_queued: usize,
    pub sync_capacity: usize,
    pub index_capacity: usize,
    pub running: usize,
    pub refused_full: u64,
    pub refused_closed: u64,
    pub executed: u64,
    pub close_requests: u64,
    /// True once the writer thread was really joined.
    pub joined: bool,
    /// True when the join budget expired with the writer still inside a job.
    pub abandoned: bool,
}

/// The whole writer: its connection thread, its queue, its counters and its teardown.
pub(crate) struct Writer {
    counters: Arc<PersistenceCounters>,
    lifecycle: Arc<Lifecycle>,
    queue: Arc<WorkQueue<WriterTask>>,
    /// The bounded terminal records of the jobs that really ran.
    settlements: Arc<SettlementLog>,
    gate: Arc<Mutex<Option<Arc<crate::coordinator::PersistenceDispatchGate>>>>,
    /// The hold point the writer loop reads. It is shared with the loop thread, so an
    /// install after the thread started still reaches the loop. Debug-only: a release
    /// build has no hold point to read.
    #[cfg(debug_assertions)]
    barrier: Arc<Slot>,
    /// The hold point admission reads. Kept separate from the loop's: a test that parks
    /// a caller *before* it is enqueued must not also park the writer thread, or the two
    /// holds would hide each other. Debug-only, like the loop's slot.
    #[cfg(debug_assertions)]
    admission_barrier: Arc<Slot>,
    /// Set once the connection thread has stopped, so joining it is a decision the
    /// caller makes rather than something `Drop` has to guess.
    thread_id: Option<thread::ThreadId>,
}

impl Writer {
    pub(crate) fn start(
        mut connection: Connection,
        counters: Arc<PersistenceCounters>,
        gate: Arc<Mutex<Option<Arc<crate::coordinator::PersistenceDispatchGate>>>>,
        database_path: PathBuf,
        #[cfg(debug_assertions)] barrier: Barrier,
    ) -> Result<Self, PersistenceError> {
        let queue: Arc<WorkQueue<WriterTask>> =
            Arc::new(WorkQueue::new(SYNC_QUEUE_CAPACITY, INDEX_QUEUE_CAPACITY));
        let lifecycle = Arc::new(Lifecycle::new());
        let settlements = Arc::new(SettlementLog::default());
        let thread_queue = Arc::clone(&queue);
        let thread_lifecycle = Arc::clone(&lifecycle);
        let thread_counters = Arc::clone(&counters);
        let thread_settlements = Arc::clone(&settlements);
        let thread_gate = Arc::clone(&gate);
        // One shared slot for the hold point: the loop reads it, and an install after the
        // thread started still reaches the loop. An environment-armed barrier reaches BOTH
        // slots - the loop's and admission's - so a driver can name any of the layer's points
        // with the same control document. Each slot only ever holds the point it is asked
        // about, so installing one barrier in both is what makes `queue_admission` reachable
        // from the environment at all: without it, a document naming that point would be read
        // (and journalled) on every arrival and could never hold one.
        #[cfg(debug_assertions)]
        let admission_barrier = Arc::new(Slot::default());
        #[cfg(debug_assertions)]
        let thread_barrier = Arc::new(Slot::default());
        #[cfg(debug_assertions)]
        if barrier.is_armed() {
            admission_barrier.install(barrier.clone());
            thread_barrier.install(barrier);
        }
        #[cfg(debug_assertions)]
        let loop_barrier = Arc::clone(&thread_barrier);
        let join = thread::Builder::new()
            .name("aura-persistence-writer".to_owned())
            .spawn(move || {
                let hook = Arc::clone(&thread_counters);
                // One handler for the life of the connection. It only counts while a
                // caller has asked for a measurement, so no production path pays for
                // it and no SQL has to be re-shaped to be measurable.
                if let Err(error) =
                    connection.progress_handler(VM_STEP_GRANULARITY, Some(move || hook.progress()))
                {
                    crate::diagnostics::append_persistence_log(
                        "persistence.measurement_unavailable",
                        serde_json::json!({ "reason": error.to_string() }),
                    );
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    writer_loop(
                        &mut connection,
                        &thread_queue,
                        &thread_counters,
                        &thread_settlements,
                        &thread_lifecycle,
                        &thread_gate,
                        #[cfg(debug_assertions)]
                        &loop_barrier,
                    );
                }));
                thread_lifecycle.set(if outcome.is_ok() {
                    WriterState::Stopped
                } else {
                    WriterState::Panicked
                });
            })
            .map_err(|source| PersistenceError::Io {
                action: "spawn persistence writer",
                path: database_path,
                source,
            })?;
        let thread_id = join.thread().id();
        if let Ok(mut slot) = lifecycle.thread.lock() {
            *slot = Some(join);
        }
        Ok(Self {
            counters,
            lifecycle,
            queue,
            settlements,
            gate,
            #[cfg(debug_assertions)]
            barrier: thread_barrier,
            #[cfg(debug_assertions)]
            admission_barrier,
            thread_id: Some(thread_id),
        })
    }

    /// Admits one job and waits for its answer, bounded by the job's deadline.
    ///
    /// The three outcomes are kept apart on purpose:
    /// * the job's own result, when the writer answered;
    /// * [`PersistenceError::WriterBusy`] when the lane was full - the job was **not**
    ///   enqueued, so its work has not run;
    /// * [`PersistenceError::WriterUnresponsive`] when the deadline expired after the
    ///   job was enqueued - the answer is unknown and the caller must not assume the
    ///   write did not happen.
    pub(crate) fn submit<T, F>(
        &self,
        class: WorkClass,
        deadline: Deadline,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        self.submit_identified(class, deadline, None, operation)
    }

    /// The same call, with the caller's identity for the work.
    ///
    /// An identity is what makes a write whose caller stopped waiting resolvable: the job
    /// records its terminal state under that name once it really finishes, whether or not
    /// anybody was still listening. The identity travels in the job, so the record is made
    /// by the thread that ran the work and not by the caller that gave up on it.
    pub(crate) fn submit_identified<T, F>(
        &self,
        class: WorkClass,
        deadline: Deadline,
        identity: Option<String>,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let work: WriterTask = Box::new(move |connection, _job_deadline| {
            // The deadline is already applied to the connection's SQLite busy handler
            // by the loop, so the job itself sees the ordinary `&mut Connection` every
            // caller has always had.
            let result = operation(connection);
            let succeeded = result.is_ok();
            // A send that fails means the caller's receiver is gone — it stopped waiting —
            // which is exactly the difference between "answered" and "settled after the
            // caller left", and it is observed here rather than inferred later.
            let answered = sender.send(result).is_ok();
            (succeeded, answered)
        });
        // A hold point before enqueue: a caller parked here has not been admitted, so
        // its work is provably not running. A test uses it to occupy the lane without
        // occupying the writer. The caller's own identity travels with the arrival, so a
        // control document that selects one case can tell this one from a lookalike.
        #[cfg(debug_assertions)]
        let _holding = self.admission_barrier.arrive_identified(
            crate::barrier::Point::QueueAdmission,
            class.as_str(),
            identity.as_deref(),
        );
        let job = match identity {
            Some(identity) => Job::with_identity(class, deadline, identity, work),
            None => Job::new(class, deadline, work),
        };
        match self.queue.enqueue(job) {
            Enqueue::Admitted => {}
            Enqueue::Full(_) => {
                return Err(PersistenceError::WriterBusy { class: class.as_str() });
            }
            Enqueue::Closed(_) => return Err(PersistenceError::WriterUnavailable),
        }
        match receiver.recv_timeout(deadline.map_or(WRITER_BUSY_TIMEOUT, |deadline| {
            deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1))
        })) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                Err(PersistenceError::WriterUnresponsive { class: class.as_str() })
            }
            // The job's response sender is dropped only when the job closure is
            // dropped, which happens without running it only if the queue itself was
            // dropped. The writer was closed before it could answer.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(PersistenceError::WriterUnavailable)
            }
        }
    }

    /// The terminal record of the job submitted under `identity`, once it has settled.
    ///
    /// `None` means no job with that identity has finished — because it was never
    /// admitted (nothing ran), or because it is still executing. It never means "it did not
    /// happen".
    #[must_use]
    pub(crate) fn settlement_of(&self, identity: &str) -> Option<JobSettlement> {
        self.settlements.of(identity)
    }

    /// Enqueue once and give the result channel to an asynchronous completion owner. No receipt
    /// timer discards the value of an admitted job; the real writer publishes it exactly once.
    pub(crate) fn submit_receipt<T, F>(
        &self,
        class: WorkClass,
        deadline: Deadline,
        identity: String,
        operation: F,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<T, PersistenceError>>, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let work: WriterTask = Box::new(move |connection, _| {
            let result = operation(connection);
            let succeeded = result.is_ok();
            let answered = sender.send(result).is_ok();
            (succeeded, answered)
        });
        #[cfg(debug_assertions)]
        let _holding = self.admission_barrier.arrive_identified(
            crate::barrier::Point::QueueAdmission,
            class.as_str(),
            Some(&identity),
        );
        match self.queue.enqueue(Job::with_identity(class, deadline, identity, work)) {
            Enqueue::Admitted => Ok(receiver),
            Enqueue::Full(_) => Err(PersistenceError::WriterBusy { class: class.as_str() }),
            Enqueue::Closed(_) => Err(PersistenceError::WriterUnavailable),
        }
    }

    /// The identified dispatch that does not discard what its job produced.
    ///
    /// [`Self::submit_identified`] answers a caller whose wait ended with
    /// [`PersistenceError::WriterUnresponsive`] and keeps only a terminal record of the
    /// job - its value travelled through a channel that dies with that wait. That is the
    /// right answer for work whose only product is "the row is there now", and the wrong one
    /// for a business write whose caller is owed the value it wrote: a store that committed
    /// the write a moment after the caller's own deadline must be able to hand it over
    /// instead of throwing it away.
    ///
    /// So this entry publishes the job's own `Result` (the value, or the store's own error)
    /// before waking the caller, and a caller whose deadline expired keeps listening for
    /// that receipt for up to `settle_budget`. Three outcomes stay distinct:
    /// * the job answered inside the caller's own wait - unchanged, the value is returned;
    /// * the job produced its result after that wait but inside the receipt budget - the
    ///   value or the error is returned, and the caller settles the invocation with the
    ///   fact the store really produced;
    /// * nothing was produced inside the receipt budget - [`PersistenceError::WriterUnresponsive`],
    ///   the honest "admitted, not answered", never a claim that the write did not happen.
    ///
    /// Nothing is re-run for any of them: the retained result is the one the thread that
    /// ran the job produced, and the work happens exactly once.
    pub(crate) fn submit_settled<T, F>(
        &self,
        class: WorkClass,
        deadline: Deadline,
        identity: String,
        settle_budget: Duration,
        operation: F,
    ) -> Result<T, PersistenceError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, PersistenceError> + Send + 'static,
    {
        // The wake-up carries no value of its own: the value lives in the retained slot, so
        // whether the caller is still listening cannot decide whether it survives.
        let (sender, receiver) = std::sync::mpsc::sync_channel::<()>(1);
        let retained: Arc<RetainedSlot<T>> = Arc::new(RetainedSlot::new());
        let publishing = Arc::clone(&retained);
        let abandoned = Arc::new(AtomicBool::new(false));
        let left = Arc::clone(&abandoned);
        let work: WriterTask = Box::new(move |connection, _job_deadline| {
            let result = operation(connection);
            let succeeded = result.is_ok();
            // Published BEFORE the wake-up: a caller whose wait ends in the same instant
            // still finds the result, which is the whole point of this entry.
            publishing.publish(result);
            // "Answered" is observed where it happens rather than inferred later, and a
            // caller that had already ended its own wait is not counted as answered.
            let answered = !left.load(Ordering::Acquire) && sender.send(()).is_ok();
            (succeeded, answered)
        });
        // The same pre-enqueue hold point as the other entries, so a control document that
        // selects one case by identity parks this arrival like any other.
        #[cfg(debug_assertions)]
        let _holding = self.admission_barrier.arrive_identified(
            crate::barrier::Point::QueueAdmission,
            class.as_str(),
            Some(identity.as_str()),
        );
        let job = Job::with_identity(class, deadline, identity, work);
        match self.queue.enqueue(job) {
            Enqueue::Admitted => {}
            Enqueue::Full(_) => {
                return Err(PersistenceError::WriterBusy { class: class.as_str() });
            }
            Enqueue::Closed(_) => return Err(PersistenceError::WriterUnavailable),
        }
        let waited = deadline.map_or(WRITER_BUSY_TIMEOUT, |deadline| {
            deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1))
        });
        match receiver.recv_timeout(waited) {
            Ok(()) => retained.take().unwrap_or(Err(PersistenceError::WriterResponseClosed)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // The caller's own wait is over and the job's is not. Its result is
                // published by the thread that ran it, so this caller receives it here
                // rather than letting it die with the wait that just ended.
                abandoned.store(true, Ordering::Release);
                match retained.take_within(settle_budget) {
                    Some(outcome) => outcome,
                    None => Err(PersistenceError::WriterUnresponsive { class: class.as_str() }),
                }
            }
            // The job's sender is dropped only when the job closure is dropped, which
            // happens without running it only if the queue itself was dropped. The writer
            // was closed before it could answer.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(PersistenceError::WriterUnavailable)
            }
        }
    }

    /// The terminal records this writer has kept, with the counts behind them.
    #[must_use]
    pub(crate) fn settlements(&self) -> WriterSettlements {
        self.settlements.snapshot()
    }

    /// How much SQLite work this runtime's writer has done so far.
    #[must_use]
    pub(crate) fn measured(&self) -> PersistenceArtifactCounters {
        self.counters.snapshot()
    }

    pub(crate) fn measure_work(&self) {
        self.counters.measuring.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn health(&self) -> WriterHealth {
        // Whoever asks is the one who can observe the thread had finished: joining it
        // here means `joined` is true as soon as the writer really stopped, instead of
        // waiting for another explicit close.
        self.join_if_finished();
        let depth = self.queue.depth();
        WriterHealth {
            state: self.lifecycle.state(),
            sync_queued: depth.sync,
            index_queued: depth.index,
            sync_capacity: self.queue.capacities().0,
            index_capacity: self.queue.capacities().1,
            running: depth.running,
            refused_full: depth.refused_full,
            refused_closed: depth.refused_closed,
            executed: depth.executed,
            close_requests: self.lifecycle.close_requests.load(Ordering::Acquire) as u64,
            joined: self.lifecycle.joined.load(Ordering::Acquire),
            abandoned: self.lifecycle.abandoned.load(Ordering::Acquire),
        }
    }

    #[cfg(debug_assertions)]
    pub(crate) fn install_barrier(&self, barrier: Barrier) -> Barrier {
        self.barrier.install(barrier)
    }

    /// Installs a hold point on admission, before a job is enqueued.
    #[cfg(debug_assertions)]
    pub(crate) fn install_admission_barrier(&self, barrier: Barrier) -> Barrier {
        self.admission_barrier.install(barrier)
    }

    /// Arms a gate that the next writer job must pass through.
    ///
    /// Used by tests that need a real write to be in flight while they call the real
    /// read and control paths, so "the read queued behind the writer" is observed rather
    /// than assumed. The gate affects the very next job only; every later job runs
    /// normally even if the gate is never released.
    pub(crate) fn arm_dispatch_gate(&self) -> Arc<crate::coordinator::PersistenceDispatchGate> {
        let gate = Arc::new(crate::coordinator::PersistenceDispatchGate::new());
        if let Ok(mut slot) = self.gate.lock() {
            *slot = Some(Arc::clone(&gate));
        }
        gate
    }

    /// Closes admission once and waits a bounded time for the writer to stop.
    ///
    /// Returns the writer's health after the attempt. `joined` is the only thing that
    /// proves the thread ended; `abandoned` says a job was still running when the
    /// budget expired, which is reported rather than hidden. A repeated close returns
    /// the same state and does not settle twice.
    pub(crate) fn shutdown(&self, budget: Duration) -> WriterHealth {
        self.lifecycle.close_requests.fetch_add(1, Ordering::AcqRel);
        if self.queue.finish() {
            self.lifecycle.set(WriterState::Draining);
        }
        let deadline = Instant::now() + budget;
        loop {
            if self.lifecycle.joined.load(Ordering::Acquire) {
                break;
            }
            if matches!(self.lifecycle.state(), WriterState::Stopped | WriterState::Panicked) {
                break;
            }
            if Instant::now() >= deadline {
                self.lifecycle.abandoned.store(true, Ordering::Release);
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        self.join_if_finished();
        self.health()
    }

    /// Joins the writer thread if it has stopped. Never blocks on a running thread, and
    /// never joins from the writer thread itself.
    fn join_if_finished(&self) {
        if self.thread_id == Some(thread::current().id()) {
            return;
        }
        if !matches!(self.lifecycle.state(), WriterState::Stopped | WriterState::Panicked) {
            return;
        }
        let handle = self.lifecycle.thread.lock().ok().and_then(|mut slot| slot.take());
        if let Some(handle) = handle {
            let _ = handle.join();
            self.lifecycle.joined.store(true, Ordering::Release);
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // `Drop` never blocks indefinitely: it asks, waits the shutdown budget, and
        // records what it found. A writer still inside a job keeps its own thread and
        // its own handle rather than parking the process that is dropping it.
        if !self.lifecycle.joined.load(Ordering::Acquire) {
            let _ = self.shutdown(SHUTDOWN_JOIN_BUDGET);
        }
    }
}

fn writer_loop(
    connection: &mut Connection,
    queue: &WorkQueue<WriterTask>,
    counters: &PersistenceCounters,
    settlements: &SettlementLog,
    lifecycle: &Lifecycle,
    gate: &Mutex<Option<Arc<crate::coordinator::PersistenceDispatchGate>>>,
    #[cfg(debug_assertions)] barrier: &Slot,
) {
    while let Some(job) = queue.next_job() {
        lifecycle.set(WriterState::Executing);
        // The gate is checked before the hold point. A test that arms both means "park
        // this job here", and the hold point's arrival record must be the parked job's
        // own - if the hold point were reached first, a job that merely passed through
        // while the gate was still free would be counted as the held one.
        if let Ok(mut slot) = gate.lock()
            && let Some(armed) = slot.take()
        {
            armed.announce();
        }
        let identity = job.identity().map(str::to_owned);
        #[cfg(debug_assertions)]
        if let Some(_permit) = barrier.arrive_identified(
            crate::barrier::Point::WriterExecution,
            job.class.as_str(),
            identity.as_deref(),
        ) {
            // Held here: a real job, on the real connection, before it ran anything.
        }
        let deadline = job.work_deadline();
        // SQLite's own lock wait is part of the caller's budget, not a separate
        // unbounded stage: the handler is set from what is left of that deadline.
        let busy = deadline.map_or(WRITER_BUSY_TIMEOUT, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        if let Err(error) = connection.busy_timeout(busy) {
            crate::diagnostics::append_persistence_log(
                "persistence.busy_timeout_unavailable",
                serde_json::json!({ "reason": error.to_string() }),
            );
        }
        let identity = job.identity().map(str::to_owned);
        let class = job.class;
        let (succeeded, answered) = (job.work)(connection, deadline);
        counters.record(connection);
        // The job's own work is over — the transaction committed or it did not — so its
        // terminal state is recorded here, before the writer is held again. A caller that
        // stopped waiting reads this instead of being told "not executed"; the job's
        // identity is the caller's own.
        if let Some(identity) = identity.clone() {
            settlements.record(JobSettlement { class, identity, succeeded, answered });
        }
        #[cfg(debug_assertions)]
        if let Some(_permit) = barrier.arrive_identified(
            crate::barrier::Point::WriterPostCommit,
            class.as_str(),
            identity.as_deref(),
        ) {
            // Held here: the job already ran and released nothing yet, so the caller
            // answered and the connection is still this job's.
        }
        queue.finished_job();
        // The queue's own count of executed jobs is the authoritative one, so it is
        // copied into the work counters a caller reads: "how many jobs really ran" has
        // to be the same number whichever way it is asked for.
        counters.record_jobs(queue.depth().executed);
        lifecycle.set(if queue.is_closed() { WriterState::Draining } else { WriterState::Running });
    }
}
