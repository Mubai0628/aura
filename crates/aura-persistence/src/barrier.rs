//! Default-off hold points on the real storage path, for isolation evidence.
//!
//! WHY THIS EXISTS
//! S03-A has to show what a *real* read, control or independent write does while the
//! storage layer is genuinely occupied. A `sleep` helper proves that a test waited, and
//! holding a request before authorization proves nothing about storage: the question is
//! what happens once a write transaction is actually open on the writer connection, or
//! once a read connection is actually held. Only a hold *inside* the storage layer makes
//! that observable, and only an armed one is allowed to slow anything down.
//!
//! DEFAULT-OFF, THREE WAYS
//! * A hold is created by [`Barrier::arm`]. Nothing in production ever calls it, so in
//!   every product configuration the hook is `None` and the check below is one
//!   `Option::is_none` on a value the calling thread already owns.
//! * The whole module is `#[cfg(debug_assertions)]`: a release build does not contain
//!   it, so it cannot be reached from a shipped Core.
//! * An armed hold still only fires when the arriving case *matches*: the point has to
//!   be the armed one, and - for the writer points - the work class has to be the
//!   armed one. Everything else passes through untouched.
//!
//! WHAT IT MAY DO
//! * Block the arriving thread until the driver releases it, or until a monotonic
//!   fallback budget expires. It cannot change a result, fabricate one or reorder
//!   anything.
//!
//! ONE ARRIVAL, ONE LOCK
//! An earlier version of this module kept a single "who is being held" slot and made a
//! later arrival wait for it. That is a deadlock waiting to happen: the later arrival
//! had to take the slot lock while the held thread was parked, and any other lock the
//! held thread needed on the way out closed the cycle. It did happen - the read pool's
//! second arrival blocked on the records lock the held arrival was still holding - so
//! the slot is gone. Each arrival now takes the state lock once, decides whether it is
//! the armed case, and parks on its own; nothing an arrival holds is needed by another
//! arriving thread, so no cycle can form.
//!
//! WHAT AN ARRIVAL RECORDS
//! * Counts, identifiers and milliseconds only - no path, no file content, no
//!   credential. `arrivals()` is what lets a test assert the barrier *reached the
//!   target layer* before it draws any conclusion from the concurrency that follows.
//!
//! THE ENVIRONMENT-BOUND CONTROL (what lets a driver outside this process hold one job)
//! * `AURA_S03_BARRIER_FILE` names the control document. Setting it is what binds the
//!   barrier; the document itself is read lazily, so a driver can arm, re-arm or disarm
//!   while the process is running without a restart. The document's identity (length and
//!   modification time) is what decides whether it has to be re-read, so an unchanged
//!   document costs one `stat` per arrival.
//! * `{"point":"writer_execution","label":"index","step_millis":0,"once":true}` arms a
//!   hold for the first matching arrival only. `"point":"none"` (or an absent document)
//!   holds nothing. An unreadable or unparsable document is recorded in the journal as a
//!   real control failure and holds nothing; it is never read as "no hold was asked for".
//! * `AURA_S03_BARRIER_JOURNAL` (default `<document directory>/barrier.jsonl`) is where
//!   arrivals, control changes and releases are appended, so a driver can see that the
//!   hold really reached the storage layer instead of guessing from a sleep.
//! * A driver ends a park by writing `{"release":true,"at_ms":<epoch ms>}` to
//!   `AURA_S03_BARRIER_RELEASE` (default `<document directory>/barrier-release.json`).
//!   `at_ms` has to be at or after the parked arrival's own timestamp, so a release left
//!   behind by an earlier hold cannot end a later one. The [`DEFAULT_FALLBACK_HOLD`]
//!   budget still applies underneath: a driver that disappears is a slow case, not a
//!   hung process.
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Which storage-layer hold point a barrier sits on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Point {
    /// The writer thread, after it took a job off the queue and before it runs it.
    /// A task held here is a real write that has not touched the connection yet.
    WriterExecution,
    /// The writer thread, after the job returned and the connection counters were
    /// recorded. A task held here has already committed and is still holding the
    /// writer, which is the state a second write really queues behind.
    WriterPostCommit,
    /// One read connection, taken out of the pool and before the caller used it.
    ReadConnection,
    /// Admission, inside the queue, before the job was enqueued.
    QueueAdmission,
    /// The Agent Tool dispatch, after the call's reservation committed and before any
    /// backend was dispatched.
    ///
    /// NOT A STORAGE POINT, and deliberately absent from the storage capability this module
    /// announces (`announce_capability`): the Agent acceptance adapter owns this name, and a
    /// driver that only knows the storage protocol must not be told the storage layer has a
    /// point it never reaches. It is carried by the same [`Barrier`] because the hold has
    /// exactly the same semantics - a bounded park of the arriving thread that can delay work
    /// and can change nothing else - and because a second, near-identical primitive would be a
    /// second thing to keep honest.
    AgentToolReservation,
    /// The Agent Tool backend, **after its real side effect and before the terminal receipt**.
    ///
    /// WHY THIS POINT IS DIFFERENT FROM `AgentToolReservation`. A hold at the reservation point
    /// parks a call that has not executed anything: at expiry there is a reserved Attempt and no
    /// side effect, which is "reserved but never executed". D7 needs the opposite case - the
    /// backend really ran, the operation/attempt/runtime binding is real, and only the terminal
    /// receipt is missing when the production budget expires. The two are different facts and are
    /// held at different places on purpose, so neither can be reported as the other.
    ///
    /// A task held here has already mutated the workspace; the hold only delays the return, and the
    /// ledger, the router and the authorisation have all already run.
    AgentToolPostExecution,
    /// Acceptance-only file caller, after actual complete-set qualification, before journal/effect.
    FileMutationCommit,
    /// Acceptance-only P04 overwrite, real bytes committed while journal still says applying.
    FilePublicationApplying,
    /// Acceptance-only P04 overwrite, real bytes committed and published journal durable.
    FilePublicationPublished,
}

impl Point {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WriterExecution => "writer_execution",
            Self::WriterPostCommit => "writer_post_commit",
            Self::ReadConnection => "read_connection",
            Self::QueueAdmission => "queue_admission",
            Self::AgentToolReservation => "agent_tool_reservation",
            Self::AgentToolPostExecution => "agent_tool_post_execution",
            Self::FileMutationCommit => "file_mutation_commit",
            Self::FilePublicationApplying => "file_publication_applying",
            Self::FilePublicationPublished => "file_publication_published",
        }
    }
}

/// How long an armed hold may park an arriving case if the driver never releases it.
/// Bounded so a lost driver is a slow test, never a hung process.
pub const DEFAULT_FALLBACK_HOLD: Duration = Duration::from_secs(20);

/// How often a parked arrival re-checks a driver's release document. In-process releases
/// wake the arrival at once through the condvar; this only bounds the file case.
const RELEASE_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The journal's own schema, so a reader can tell these lines from another protocol's.
pub const JOURNAL_SCHEMA: &str = "aura.s03.barrier.v1";

/// The environment variable that names the barrier control document.
pub const CONTROL_DOCUMENT_ENV: &str = "AURA_S03_BARRIER_FILE";
/// The environment variable that names the driver's release document.
pub const RELEASE_DOCUMENT_ENV: &str = "AURA_S03_BARRIER_RELEASE";
/// The environment variable that names the arrival journal.
pub const JOURNAL_ENV: &str = "AURA_S03_BARRIER_JOURNAL";

/// One arriving case the barrier let through, as identity plus timing only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Arrival {
    pub point: Point,
    /// Wall-clock milliseconds, for display only. Never used as a budget.
    pub at_ms: u64,
    /// Whether this case was the armed one and therefore passed through the hold.
    pub held: bool,
    /// Monotonic microseconds the case spent inside the hold. Zero while it is still
    /// held, and zero for a case the hold did not match.
    pub hold_micros: u64,
    /// The same duration in nanoseconds. A hold that is released the instant it is
    /// observed can round to zero microseconds, and "was this case really held" has to
    /// stay answerable, so the precise value is recorded as well.
    pub hold_nanos: u64,
}

struct State {
    point: Option<Point>,
    label: Option<String>,
    step_millis: u64,
    fallback: Duration,
    /// Once released, every later arrival passes without waiting.
    released: bool,
    /// When set, the hold applies to the **first** matching arrival only. Later
    /// arrivals pass through untouched.
    ///
    /// A hold that keeps firing makes it impossible to measure what a *second* call
    /// costs while the first one is parked - and that second call is exactly what
    /// "a read is served while a connection is held" is about. The writer already has
    /// this property through [`crate::PersistenceDispatchGate`], which takes effect
    /// once; this gives the read pool the same one-shot semantics.
    once: bool,
    /// How many matching arrivals have been held.
    holds: u64,
    /// The control document this barrier follows, when it was bound to the environment
    /// rather than armed in process. While it is set, the hold's state is re-read from
    /// that document whenever the document changes, so an external driver can arm,
    /// re-arm or disarm a running process.
    control_document: Option<PathBuf>,
    /// Where that driver writes its release for the current hold.
    release_document: Option<PathBuf>,
    /// The identity of the control document the current state was last read from.
    /// `None` means no document has been read yet.
    applied: Option<ControlStamp>,
    /// An explicit case selection from the control document.
    ///
    /// `None` holds whichever matching case arrives first. `Some(text)` holds **only** a case
    /// whose own identity contains that text, so a driver can name the one invocation it
    /// means instead of holding whichever thread got there first. A case that carries no
    /// identity is never selected: it cannot be the named one.
    select: Option<String>,
    /// The last real control-document failure, for diagnostics. Never a hold.
    control_error: Option<String>,
}

/// What the control document looked like the last time its state was applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlStamp {
    /// There is no control document: the barrier holds nothing.
    Absent,
    /// There is one, at this length and modification time.
    Present { len: u64, modified_ms: u64 },
}

fn control_stamp(path: &Path) -> ControlStamp {
    match std::fs::metadata(path) {
        Ok(metadata) => ControlStamp::Present {
            len: metadata.len(),
            modified_ms: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
        },
        Err(_) => ControlStamp::Absent,
    }
}

struct Inner {
    state: Mutex<State>,
    changed: Condvar,
    /// One preallocated slot per arrival. Written only by the arriving thread for its
    /// own index, so the record lock is never held while a thread is parked.
    records: Mutex<Vec<Option<Arrival>>>,
    reached: AtomicU64,
    journal: Option<PathBuf>,
}

/// The armed hold. Every barrier is `Arc`-shared with the storage layer it belongs to.
pub struct Barrier {
    inner: Option<Arc<Inner>>,
}

/// The arriving case is inside the hold. Dropping it publishes the hold's duration;
/// the hold itself ends when the driver releases it or the fallback expires, so a
/// panicking test cannot park the storage layer for the next one.
pub struct Permit {
    inner: Option<Arc<Inner>>,
    record: usize,
    started: Instant,
    published: bool,
}

impl Permit {
    /// How long this case has been inside the hold so far.
    #[must_use]
    pub fn held_for(&self) -> Duration {
        self.started.elapsed()
    }

    fn publish(&mut self) {
        if self.published {
            return;
        }
        self.published = true;
        let elapsed = self.started.elapsed();
        let hold_micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        let hold_nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        if let Some(inner) = self.inner.as_ref()
            && let Ok(mut records) = inner.records.lock()
            && let Some(Some(record)) = records.get_mut(self.record)
        {
            record.hold_micros = hold_micros;
            record.hold_nanos = hold_nanos;
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.publish();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

/// Write one JSON line to the configured journal, if a driver asked for one.
fn append(inner: &Inner, document: &serde_json::Value) {
    let Some(path) = inner.journal.as_deref() else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(file, "{document}");
}

impl Barrier {
    /// A barrier that is not armed. The storage layer holds one of these in every
    /// production configuration.
    #[must_use]
    pub const fn disabled() -> Self {
        Self { inner: None }
    }

    /// Arms a hold at `point`.
    ///
    /// `step_millis` of zero means the hold ends only when the driver releases it (or
    /// the fallback budget expires). A non-zero step makes each arrival wait that long
    /// and then pass, which is how a test measures what an unrelated entry point really
    /// costs while the storage layer is occupied - without anybody deciding when the
    /// hold ends.
    ///
    /// `label` restricts the hold to the work class (writer points) it names. `None`
    /// holds whichever case arrives first.
    #[must_use]
    pub fn arm(point: Point, label: Option<&str>, step_millis: u64) -> Self {
        Self::arm_with(point, label, step_millis, false)
    }

    /// The same hold, but only for the first matching arrival.
    ///
    /// Every later arrival at the same point passes through without waiting, so a test
    /// can park one real write or one real read and still measure what the next caller
    /// costs.
    #[must_use]
    pub fn arm_once(point: Point, label: Option<&str>, step_millis: u64) -> Self {
        Self::arm_with(point, label, step_millis, true)
    }

    /// The same hold, restricted to the case whose own identity contains `select`.
    ///
    /// A control document can name this selection too; this form is for a driver inside the
    /// process. An arrival without an identity is never selected.
    #[must_use]
    pub fn arm_selected(
        point: Point,
        label: Option<&str>,
        select: Option<&str>,
        step_millis: u64,
    ) -> Self {
        let barrier = Self::arm_with(point, label, step_millis, false);
        if let Some(inner) = barrier.inner.as_ref()
            && let Ok(mut state) = inner.state.lock()
        {
            state.select = select.map(ToOwned::to_owned);
        }
        barrier
    }

    fn arm_with(point: Point, label: Option<&str>, step_millis: u64, once: bool) -> Self {
        Self::arm_with_journal(
            point,
            label,
            step_millis,
            once,
            std::env::var_os(JOURNAL_ENV).map(PathBuf::from),
        )
    }

    fn arm_with_journal(
        point: Point,
        label: Option<&str>,
        step_millis: u64,
        once: bool,
        journal: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: Some(Arc::new(Inner {
                state: Mutex::new(State {
                    point: Some(point),
                    label: label.map(ToOwned::to_owned),
                    step_millis,
                    fallback: DEFAULT_FALLBACK_HOLD,
                    released: false,
                    once,
                    holds: 0,
                    control_document: None,
                    release_document: None,
                    applied: None,
                    select: None,
                    control_error: None,
                }),
                changed: Condvar::new(),
                records: Mutex::new(Vec::new()),
                reached: AtomicU64::new(0),
                journal,
            })),
        }
    }

    /// A barrier bound to a control document a driver outside this process writes.
    ///
    /// It starts with no hold at all: the document decides. Its state is re-read whenever
    /// the document's identity changes, so one running process can be armed and disarmed
    /// as often as the driver needs. Nothing here reads the environment - the paths are
    /// the caller's.
    #[must_use]
    pub fn environment_bound(
        document: PathBuf,
        release: Option<PathBuf>,
        journal: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: Some(Arc::new(Inner {
                state: Mutex::new(State {
                    point: None,
                    label: None,
                    step_millis: 0,
                    fallback: DEFAULT_FALLBACK_HOLD,
                    released: false,
                    once: false,
                    holds: 0,
                    control_document: Some(document),
                    release_document: release,
                    applied: None,
                    select: None,
                    control_error: None,
                }),
                changed: Condvar::new(),
                records: Mutex::new(Vec::new()),
                reached: AtomicU64::new(0),
                journal,
            })),
        }
    }

    /// The barrier control named by `AURA_S03_BARRIER_FILE`, read once per process.
    ///
    /// This is what lets a test that only has the public persistence API - an
    /// integration test, or the acceptance driver - reach a hold point the coordinator
    /// owns privately. Setting the variable binds the barrier to that document; the
    /// document's content is re-read lazily, so a driver can arm it after this process
    /// started. It never arms a hold by itself.
    #[must_use]
    pub fn from_environment() -> Self {
        static FILE: OnceLock<Option<Barrier>> = OnceLock::new();
        FILE.get_or_init(environment_bound_barrier).clone().unwrap_or_else(Self::disabled)
    }

    /// The same environment reader, without the process-wide cache: a test that writes
    /// a new barrier document between two cases uses this.
    #[must_use]
    pub fn reload_from_environment() -> Self {
        environment_bound_barrier().unwrap_or_else(Self::disabled)
    }

    /// Copies the environment barrier into a coordinator, so the slot holds one of
    /// these and later readers of that slot never touch the environment.
    fn into_slot(self) -> Option<Arc<Inner>> {
        self.inner
    }

    /// Whether this barrier can hold anything at all.
    #[must_use]
    pub const fn is_armed(&self) -> bool {
        self.inner.is_some()
    }

    /// Whether `point`/`label` is the armed case, and therefore would be held.
    #[must_use]
    pub fn matches(&self, point: Point, label: &str) -> bool {
        self.armed(point, label, None).is_some()
    }

    /// `Some(step_millis, fallback)` when this case is the armed one.
    ///
    /// `identity` is the arriving case's own name, when the layer has one: a workspace
    /// catalogue write names its workspace and correlation, an unnamed job does not. When the
    /// control document selects a case, only an arrival whose identity contains the selection
    /// is armed - so "the hold was reached" can be attributed to the invocation the driver
    /// meant rather than to whichever thread arrived first.
    fn armed(&self, point: Point, label: &str, identity: Option<&str>) -> Option<(u64, Duration)> {
        let inner = self.inner.as_ref()?;
        // An environment-bound barrier follows its control document: a driver that writes
        // (or removes) it changes what the next arrival does, without a restart.
        refresh_control(inner);
        let state = inner.state.lock().ok()?;
        let matches = match (state.point, state.label.as_deref()) {
            (Some(armed), Some(wanted)) => armed == point && wanted == label,
            (Some(armed), None) => armed == point,
            (None, _) => false,
        };
        let selected = match state.select.as_deref() {
            None => true,
            Some(wanted) => identity.is_some_and(|identity| identity.contains(wanted)),
        };
        // A one-shot hold that already held its case lets every later arrival through.
        let spent = state.once && state.holds > 0;
        (matches && selected && !spent).then_some((state.step_millis, state.fallback))
    }

    /// Every case that reached an armed point, in arrival order.
    #[must_use]
    pub fn arrivals(&self) -> Vec<Arrival> {
        self.inner.as_ref().map_or_else(Vec::new, |inner| {
            inner
                .records
                .lock()
                .map(|records| records.iter().flatten().copied().collect())
                .unwrap_or_default()
        })
    }

    /// Cases that reached the armed point and were actually held.
    #[must_use]
    pub fn held_count(&self) -> u64 {
        self.arrivals().iter().filter(|arrival| arrival.held).count() as u64
    }

    /// Whether the armed point has been reached by any case at all.
    #[must_use]
    pub fn was_reached(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.reached.load(Ordering::Acquire) > 0)
    }

    /// Lets every currently held case continue, and every later arrival pass.
    pub fn release_all(&self) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        if let Ok(mut state) = inner.state.lock() {
            state.released = true;
        }
        inner.changed.notify_all();
    }

    /// Lets the case held at `point` with this exact label continue.
    ///
    /// Recorded as a release of `point`, which is what a driver that names the point it
    /// armed means.
    pub fn release(&self, point: Point, label: &str) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let _ = label;
        if let Ok(mut state) = inner.state.lock()
            && state.point == Some(point)
        {
            state.released = true;
        }
        inner.changed.notify_all();
    }

    /// Waits until at least `count` cases have reached the armed point.
    ///
    /// Returns `false` on timeout, which is how a test refuses to draw a conclusion
    /// from a hold that never happened.
    #[must_use]
    pub fn wait_for_arrivals(&self, count: u64, budget: Duration) -> bool {
        let Some(inner) = self.inner.as_ref() else {
            return false;
        };
        let deadline = Instant::now() + budget;
        loop {
            if inner.reached.load(Ordering::Acquire) >= count {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return inner.reached.load(Ordering::Acquire) >= count;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(2)));
        }
    }

    /// The arriving case reaches `point`. `Some(permit)` means it is the armed case and
    /// has passed through the hold.
    ///
    /// `label` is the case identity the hold matches on - a work class for the writer
    /// points, a fixed name for the read point.
    #[must_use]
    pub fn arrive(&self, point: Point, label: &str) -> Option<Permit> {
        self.arrive_identified(point, label, None)
    }

    /// The same arrival, carrying the arriving case's own identity.
    ///
    /// `identity` is what makes a selected hold attributable: the journal records it, so a
    /// reader can tell that the parked case is the one the control document named and not a
    /// lookalike that happened to arrive first.
    #[must_use]
    pub fn arrive_identified(
        &self,
        point: Point,
        label: &str,
        identity: Option<&str>,
    ) -> Option<Permit> {
        let (step_millis, fallback) = self.armed(point, label, identity)?;
        let inner = self.inner.as_ref()?;
        let started = Instant::now();
        let at_ms = now_ms();
        let record = match inner.records.lock() {
            Ok(mut records) => {
                records.push(Some(Arrival {
                    point,
                    at_ms,
                    held: true,
                    hold_micros: 0,
                    hold_nanos: 0,
                }));
                records.len() - 1
            }
            Err(_) => return None,
        };
        if let Ok(mut state) = inner.state.lock() {
            state.holds = state.holds.saturating_add(1);
        }
        inner.reached.fetch_add(1, Ordering::AcqRel);
        append(
            inner,
            &serde_json::json!({
                "schema": JOURNAL_SCHEMA,
                "event": "arrived",
                "point": point.as_str(),
                "label": label,
                "identity": identity,
                "at_ms": at_ms,
                "thread": std::thread::current().name().unwrap_or("unnamed").to_owned(),
                "pid": std::process::id(),
            }),
        );
        let permit = Permit { inner: Some(Arc::clone(inner)), record, started, published: false };
        // Release conditions, checked on a monotonic clock only.
        let deadline =
            started + if step_millis == 0 { fallback } else { Duration::from_millis(step_millis) };
        let release_document =
            inner.state.lock().ok().and_then(|state| state.release_document.clone());
        let end: &str;
        loop {
            if Instant::now() >= deadline {
                end = if step_millis == 0 { "fallback" } else { "step" };
                break;
            }
            {
                let Ok(state) = inner.state.lock() else {
                    end = "control_unreadable";
                    break;
                };
                if state.released {
                    end = "released";
                    break;
                }
            }
            // A driver outside this process releases the hold with its own document. It has
            // to be at or after this arrival's own timestamp, so a release left behind by an
            // earlier hold cannot end a later one.
            if let Some(path) = release_document.as_deref()
                && release_document_ends(path, at_ms)
            {
                end = "released_by_driver";
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                end = if step_millis == 0 { "fallback" } else { "step" };
                break;
            }
            let Ok(state) = inner.state.lock() else {
                end = "control_unreadable";
                break;
            };
            let _ = inner.changed.wait_timeout(state, remaining.min(RELEASE_POLL_INTERVAL));
        }
        append(
            inner,
            &serde_json::json!({
                "schema": JOURNAL_SCHEMA,
                "event": "released",
                "point": point.as_str(),
                "label": label,
                "at_ms": at_ms,
                "ended_by": end,
                "thread": std::thread::current().name().unwrap_or("unnamed").to_owned(),
                "pid": std::process::id(),
            }),
        );
        // The permit goes back to the caller, which publishes its duration when it drops it.
        Some(permit)
    }
}

/// Whether a driver's release document ends a hold that started at `not_before_ms`.
///
/// `at_ms` has to be at or after that moment: a release document left behind by a previous
/// hold must not end this one, and a document that cannot be read is not a release.
fn release_document_ends(path: &Path, not_before_ms: u64) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<ReleaseDocument>(&text).ok())
        .is_some_and(|document| document.release && document.at_ms >= not_before_ms)
}

#[derive(serde::Deserialize)]
struct ReleaseDocument {
    #[serde(default)]
    release: bool,
    #[serde(default)]
    at_ms: u64,
}

impl Clone for Barrier {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl std::fmt::Debug for Barrier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            None => formatter.write_str("Barrier(disabled)"),
            Some(inner) => {
                let point = inner.state.lock().ok().and_then(|state| state.point);
                write!(formatter, "Barrier({:?})", point.map(Point::as_str))
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct Document {
    point: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    step_millis: u64,
    /// When true, the hold applies to the first matching arrival only.
    #[serde(default)]
    once: bool,
    /// An explicit case selection: hold only an arrival whose own identity contains this.
    ///
    /// This is what makes an arrival attributable to the invocation a driver meant. Without
    /// it, a hold is "whichever matching case arrived first" - which, for a point that more
    /// than one thread can reach, is not the same thing as "the case under test".
    #[serde(default)]
    select: Option<String>,
}

/// The barrier the environment names, or `None` when it names none.
///
/// Setting `AURA_S03_BARRIER_FILE` binds the barrier to that document. The document's
/// content is not read here: an absent, half-written or not-yet-armed document is not a
/// hold, and a driver may arm it later without restarting the process.
fn environment_bound_barrier() -> Option<Barrier> {
    let document = std::env::var_os(CONTROL_DOCUMENT_ENV).map(PathBuf::from)?;
    if document.as_os_str().is_empty() {
        return None;
    }
    // The document's own directory is where a driver writes its release and arrivals, so a
    // driver that only wrote the control document finds both beside it.
    let directory = document.parent().map(Path::to_path_buf);
    let release = std::env::var_os(RELEASE_DOCUMENT_ENV)
        .map(PathBuf::from)
        .or_else(|| directory.as_ref().map(|parent| parent.join("barrier-release.json")));
    let journal = std::env::var_os(JOURNAL_ENV)
        .map(PathBuf::from)
        .or_else(|| directory.as_ref().map(|parent| parent.join("barrier.jsonl")));
    announce_capability(&document, &journal);
    Some(Barrier::environment_bound(document, release, journal))
}

/// The Core's own statement that THIS image really carries the storage hold points.
///
/// A driver that only has the file protocol cannot otherwise tell "this image has no such
/// hold point" from "the hold was never reached". It is written next to the control
/// document, once per process, before anything can be held; a release image does not contain
/// this module at all, so it writes nothing and a driver reports the hold as not exercised
/// instead of waiting for a boundary that cannot exist.
fn announce_capability(document: &Path, journal: &Option<PathBuf>) {
    static ANNOUNCED: OnceLock<()> = OnceLock::new();
    if ANNOUNCED.set(()).is_err() {
        return;
    }
    let Some(directory) = document.parent() else {
        return;
    };
    let points = [
        Point::WriterExecution,
        Point::WriterPostCommit,
        Point::ReadConnection,
        Point::QueueAdmission,
    ]
    .map(Point::as_str);
    let capability = serde_json::json!({
        "schema": "aura.s03.storage-barrier.capability.v1",
        "protocol": JOURNAL_SCHEMA,
        "feature": "debug-assertions storage hold points",
        "compiled_under": "cfg(debug_assertions): a release image contains none of this module",
        "pid": std::process::id(),
        "started_at_ms": now_ms(),
        "control_document": document.display().to_string(),
        "journal": journal.as_ref().map(|path| path.display().to_string()),
        "fallback_hold_ms": u64::try_from(DEFAULT_FALLBACK_HOLD.as_millis()).unwrap_or(u64::MAX),
        "fallback_clock": "instant_monotonic",
        "points": points,
    });
    let path = directory.join("storage-barrier-capability.json");
    if let Err(error) = std::fs::write(&path, format!("{capability}\n")) {
        eprintln!(
            "the storage barrier capability could not be announced at {}: {error}",
            path.display()
        );
    }
}

/// Applies the control document to an environment-bound barrier, when it changed.
///
/// This is what makes one running process armable: the document's identity (length and
/// modification time) is compared with the last one applied, and an unchanged document
/// costs one `stat`. A document that is absent, unreadable or unparsable holds nothing and
/// is journalled as a real control failure - never silently treated as "no hold asked for".
fn refresh_control(inner: &Inner) {
    let (document, release) = {
        let Ok(state) = inner.state.lock() else {
            return;
        };
        let Some(document) = state.control_document.clone() else {
            return;
        };
        (document, state.release_document.clone())
    };
    let stamp = control_stamp(&document);
    {
        let Ok(state) = inner.state.lock() else {
            return;
        };
        if state.applied == Some(stamp) {
            return;
        }
    }
    let mut failure = None;
    let (point, label, step_millis, once, select) = match std::fs::read_to_string(&document) {
        Ok(text) => match serde_json::from_str::<Document>(&text) {
            // "none" is how a driver disarms a control it has finished with.
            Ok(document) if document.point == "none" => (None, None, 0, false, None),
            Ok(document) => match parse_point(&document.point) {
                Some(point) => (
                    Some(point),
                    document.label,
                    document.step_millis,
                    document.once,
                    document.select,
                ),
                None => {
                    failure = Some(format!(
                        "the barrier control document names an unknown point {:?}",
                        document.point
                    ));
                    (None, None, 0, false, None)
                }
            },
            Err(error) => {
                failure =
                    Some(format!("the barrier control document could not be parsed: {error}"));
                (None, None, 0, false, None)
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None, 0, false, None),
        Err(error) => {
            failure = Some(format!("the barrier control document could not be read: {error}"));
            (None, None, 0, false, None)
        }
    };
    if let Ok(mut state) = inner.state.lock() {
        state.point = point;
        state.label = label.clone();
        state.step_millis = step_millis;
        state.once = once;
        state.select = select.clone();
        // A new control document starts a new hold: arrival and release records from the
        // previous one must not decide this one.
        state.holds = 0;
        state.released = false;
        state.applied = Some(stamp);
        state.release_document = release;
        state.control_error = failure.clone();
    }
    append(
        inner,
        &serde_json::json!({
            "schema": JOURNAL_SCHEMA,
            "event": "control",
            "point": point.map(Point::as_str),
            "label": label,
            "step_millis": step_millis,
            "once": once,
            "select": select,
            "failure": failure,
            "at_ms": now_ms(),
            "pid": std::process::id(),
        }),
    );
}

fn parse_point(value: &str) -> Option<Point> {
    match value {
        "writer_execution" => Some(Point::WriterExecution),
        "writer_post_commit" => Some(Point::WriterPostCommit),
        "read_connection" => Some(Point::ReadConnection),
        "queue_admission" => Some(Point::QueueAdmission),
        // Named by the S04-E acceptance adapter's own control document, never by the storage
        // protocol's capability; see the variant's documentation.
        "agent_tool_reservation" => Some(Point::AgentToolReservation),
        // The post-execution point. `announce_capability` on the Core side and
        // `AGENT_HOLD_POINTS` on the driver side both already carried this name, and the variant
        // itself existed - but this parser did not accept it, so a control document that armed D7
        // was refused here with "unknown point" and D7 could not be armed at all. The name is
        // accepted exactly like its reservation sibling, and like it, it is only ever named by the
        // acceptance adapter's own document.
        "agent_tool_post_execution" => Some(Point::AgentToolPostExecution),
        "file_mutation_commit" => Some(Point::FileMutationCommit),
        "file_publication_applying" => Some(Point::FilePublicationApplying),
        "file_publication_published" => Some(Point::FilePublicationPublished),
        _ => None,
    }
}

/// A barrier slot the storage layer reads on every arrival.
///
/// The slot is `Arc<Mutex<Option<Arc<Inner>>>>` so a test can install a hold at any
/// time and the arriving thread picks it up without the storage layer having to be
/// rebuilt. Reading it costs one uncontended lock on a value the thread owns.
#[derive(Clone, Default)]
pub(crate) struct Slot {
    inner: Arc<Mutex<Option<Arc<Inner>>>>,
    /// Finalised barriers, kept alive so their arrival records survive an install.
    kept: Arc<Mutex<Vec<Barrier>>>,
}

impl Slot {
    /// Installs a barrier and returns it for inspection.
    ///
    /// A disabled barrier installs nothing: a slot that held a disabled hold would still
    /// be a slot the arriving thread has to lock on every job, for no observation at all.
    pub(crate) fn install(&self, barrier: Barrier) -> Barrier {
        if let Ok(mut kept) = self.kept.lock() {
            kept.push(barrier.clone());
        }
        if barrier.is_armed()
            && let Ok(mut slot) = self.inner.lock()
        {
            *slot = barrier.clone().into_slot();
        }
        barrier
    }

    fn with<T>(&self, run: impl FnOnce(&Barrier) -> T) -> Option<T> {
        let inner = self.inner.lock().ok()?.clone()?;
        Some(run(&Barrier { inner: Some(inner) }))
    }

    pub(crate) fn arrive(&self, point: Point, label: &str) -> Option<Permit> {
        self.with(|barrier| barrier.arrive(point, label)).flatten()
    }

    /// The same arrival, with the arriving case's identity for a selected hold.
    pub(crate) fn arrive_identified(
        &self,
        point: Point,
        label: &str,
        identity: Option<&str>,
    ) -> Option<Permit> {
        self.with(|barrier| barrier.arrive_identified(point, label, identity)).flatten()
    }

    /// Whether a hold is installed. Used by the slot's own test.
    #[cfg(test)]
    pub(crate) fn is_armed(&self) -> bool {
        self.with(Barrier::is_armed).unwrap_or(false)
    }
}

/// Whether `AURA_S03_BARRIER_FILE` names a barrier control at all.
///
/// Read once, at coordinator start, so the hot path never touches the environment.
#[must_use]
pub fn environment_requests_barrier() -> bool {
    std::env::var_os(CONTROL_DOCUMENT_ENV).is_some_and(|value| !value.is_empty())
}

/// The journal path a driver reads arrivals from, when it configured one.
#[must_use]
pub fn environment_journal() -> Option<PathBuf> {
    std::env::var_os(JOURNAL_ENV).map(PathBuf::from)
}

/// Where the barrier document for one test/driver directory lives.
#[must_use]
pub fn document_path(directory: &Path) -> PathBuf {
    directory.join("barrier.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_arrival_does_not_wait_for_the_first() {
        // A step hold keeps every arrival bounded, so what is measured is the second
        // arrival's own hold DECISION rather than the length of the first one's park (a
        // step-0 hold parks the calling thread until it is released or its fallback ends,
        // which is the module's documented behaviour and not something a second arrival
        // can shortcut).
        let barrier = Barrier::arm(Point::ReadConnection, None, 150);
        let first = std::thread::spawn({
            let barrier = barrier.clone();
            move || barrier.arrive(Point::ReadConnection, "read_connection").is_some()
        });
        assert!(barrier.wait_for_arrivals(1, Duration::from_secs(5)), "the first arrival");
        // The first arrival is parked inside its own hold. A second one must still get
        // through the hold decision and park on its own, because nothing it needs is held
        // by the first: the arrival is recorded while the first is still parked.
        let started = Instant::now();
        let second = std::thread::spawn({
            let barrier = barrier.clone();
            move || barrier.arrive(Point::ReadConnection, "read_connection").is_some()
        });
        assert!(barrier.wait_for_arrivals(2, Duration::from_secs(5)), "the second arrival");
        assert!(second.join().expect("the second arrival"), "the second arrival is held too");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(first.join().expect("the first arrival must not block the second"), "held");
        barrier.release_all();
        let third = barrier.arrive(Point::ReadConnection, "read_connection");
        assert!(third.is_some(), "a released barrier still reports the arrival");
        assert_eq!(barrier.arrivals().len(), 3);
        assert_eq!(barrier.held_count(), 3);
    }

    #[test]
    fn a_disabled_barrier_holds_nothing_and_never_arms() {
        let barrier = Barrier::disabled();
        assert!(!barrier.is_armed());
        assert!(barrier.arrive(Point::WriterExecution, "index").is_none());
        assert!(!barrier.was_reached());
        assert!(barrier.arrivals().is_empty());
    }

    #[test]
    fn a_hold_that_matches_nothing_passes_the_case_through() {
        let barrier = Barrier::arm(Point::WriterExecution, Some("index"), 0);
        assert!(barrier.matches(Point::WriterExecution, "index"));
        assert!(!barrier.matches(Point::WriterExecution, "sync"));
        assert!(!barrier.matches(Point::ReadConnection, "index"));
        assert!(barrier.arrive(Point::ReadConnection, "read_connection").is_none());
        assert!(!barrier.was_reached(), "a case the hold did not match is not an arrival");
    }

    #[test]
    fn a_step_hold_ends_by_itself_on_a_monotonic_budget() {
        let barrier = Barrier::arm(Point::WriterExecution, None, 40);
        let started = Instant::now();
        let permit = barrier.arrive(Point::WriterExecution, "index").expect("held");
        let held = started.elapsed();
        assert!(held >= Duration::from_millis(30), "the step has to be a real wait: {held:?}");
        assert!(held < Duration::from_secs(2));
        drop(permit);
        assert!(barrier.arrivals()[0].hold_micros > 0, "the hold duration is recorded");
    }

    /// A selected hold parks the named case and nothing else.
    ///
    /// This is the property that makes "the hold was reached" attributable: without a
    /// selection, the first matching arrival wins - and at a point more than one thread can
    /// reach, that is not the same thing as the case under test.
    #[test]
    fn a_selected_hold_parks_only_the_named_case() {
        let barrier =
            Barrier::arm_selected(Point::QueueAdmission, Some("index"), Some("cor_named"), 1);
        assert!(
            barrier.arrive_identified(Point::QueueAdmission, "index", None).is_none(),
            "an arrival that carries no identity cannot be the named one"
        );
        assert!(
            barrier.arrive_identified(Point::QueueAdmission, "index", Some("cor_other")).is_none(),
            "a different identity is not the named one"
        );
        assert!(
            barrier.arrive(Point::QueueAdmission, "index").is_none(),
            "the un-identified entry point is never selected"
        );
        assert!(
            barrier.arrive_identified(Point::QueueAdmission, "sync", Some("cor_named")).is_none(),
            "the class still has to match"
        );
        assert_eq!(barrier.held_count(), 0, "nothing unrelated was held");
        let permit =
            barrier.arrive_identified(Point::QueueAdmission, "index", Some("wsp_1/cor_named"));
        assert!(permit.is_some(), "the named case really is parked");
        assert_eq!(barrier.held_count(), 1);
        drop(permit);
    }

    /// A control document can select a case, and the journal carries that identity.
    #[test]
    fn a_control_document_can_select_the_case_it_means() {
        let directory = tempfile::tempdir().expect("control directory");
        let document = directory.path().join("barrier.json");
        let journal = directory.path().join("barrier.jsonl");
        let barrier = Barrier::environment_bound(document.clone(), None, Some(journal.clone()));
        std::fs::write(
            &document,
            r#"{"point":"queue_admission","label":"index","select":"cor_named","step_millis":1}"#,
        )
        .expect("arm");
        assert!(barrier.arrive(Point::QueueAdmission, "index").is_none());
        assert!(
            barrier.arrive_identified(Point::QueueAdmission, "index", Some("cor_other")).is_none()
        );
        let permit = barrier
            .arrive_identified(Point::QueueAdmission, "index", Some("wsp_1/cor_named"))
            .expect("the selected case is held");
        drop(permit);
        let text = std::fs::read_to_string(&journal).expect("journal");
        assert!(text.contains(r#""identity":"wsp_1/cor_named""#), "{text}");
        assert!(text.contains(r#""select":"cor_named""#), "{text}");
    }

    #[test]
    fn a_slot_that_was_never_armed_installs_nothing() {
        let slot = Slot::default();
        slot.install(Barrier::disabled());
        assert!(!slot.is_armed());
        assert!(slot.arrive(Point::WriterExecution, "index").is_none());
        slot.install(Barrier::arm(Point::WriterExecution, None, 0));
        assert!(slot.is_armed());
    }

    #[test]
    fn a_document_names_the_point_the_label_and_the_step() {
        let document: Document = serde_json::from_str(
            r#"{"point":"writer_post_commit","label":"index","step_millis":25}"#,
        )
        .expect("document");
        assert_eq!(parse_point(&document.point), Some(Point::WriterPostCommit));
        assert_eq!(document.label.as_deref(), Some("index"));
        assert_eq!(document.step_millis, 25);
        assert!(!document.once, "a document that names no one-shot is not one");
        assert_eq!(parse_point("nowhere"), None);
    }

    /// A control document written after the barrier exists arms it, and "none" disarms it
    /// again - the property an outside driver needs, without restarting the process.
    #[test]
    fn an_environment_bound_barrier_follows_its_control_document() {
        let directory = tempfile::tempdir().expect("control directory");
        let document = directory.path().join("barrier.json");
        let journal = directory.path().join("barrier.jsonl");
        let barrier = Barrier::environment_bound(document.clone(), None, Some(journal.clone()));
        // Nothing is armed yet: an absent document is not a hold.
        assert!(barrier.arrive(Point::WriterExecution, "index").is_none());
        std::fs::write(
            &document,
            r#"{"point":"writer_execution","label":"index","step_millis":100,"once":true}"#,
        )
        .expect("arm");
        let permit = barrier.arrive(Point::WriterExecution, "index").expect("armed");
        // One-shot: the next matching arrival passes.
        assert!(barrier.arrive(Point::WriterExecution, "index").is_none());
        // A different class never matches.
        assert!(barrier.arrive(Point::WriterExecution, "sync").is_none());
        drop(permit);
        std::fs::write(&document, r#"{"point":"none"}"#).expect("disarm");
        assert!(
            barrier.arrive(Point::WriterExecution, "index").is_none(),
            "a disarmed control holds nothing"
        );
        let journalled = std::fs::read_to_string(&journal).expect("journal");
        assert!(journalled.contains("\"event\":\"arrived\""), "{journalled}");
        assert!(journalled.contains("\"event\":\"control\""), "{journalled}");
    }

    /// A driver's release document ends a parked arrival; a stale one does not.
    #[test]
    fn a_driver_release_document_ends_a_park_but_a_stale_one_does_not() {
        let directory = tempfile::tempdir().expect("control directory");
        let document = directory.path().join("barrier.json");
        let release = directory.path().join("barrier-release.json");
        let barrier = Barrier::environment_bound(
            document.clone(),
            Some(release.clone()),
            Some(directory.path().join("barrier.jsonl")),
        );
        std::fs::write(&document, r#"{"point":"queue_admission","label":"index"}"#).expect("arm");
        // A release written BEFORE the arrival must not end it.
        std::fs::write(&release, r#"{"release":true,"at_ms":1}"#).expect("stale release");
        let holder = std::thread::spawn({
            let barrier = barrier.clone();
            move || barrier.arrive(Point::QueueAdmission, "index")
        });
        assert!(barrier.wait_for_arrivals(1, Duration::from_secs(5)), "the arrival was recorded");
        std::thread::sleep(Duration::from_millis(60));
        assert!(!holder.is_finished(), "a stale release must not end the hold");
        std::fs::write(&release, format!("{{\"release\":true,\"at_ms\":{}}}", now_ms() + 1_000))
            .expect("release");
        let permit = holder.join().expect("holder");
        assert!(permit.is_some(), "the arrival is the armed case");
        let journalled =
            std::fs::read_to_string(directory.path().join("barrier.jsonl")).expect("journal");
        assert!(journalled.contains("released_by_driver"), "{journalled}");
    }

    /// A control document that cannot be parsed holds nothing and is recorded as a failure
    /// instead of being read as "no hold was asked for".
    #[test]
    fn an_unreadable_control_document_is_a_recorded_failure_not_a_silent_pass() {
        let directory = tempfile::tempdir().expect("control directory");
        let document = directory.path().join("barrier.json");
        let journal = directory.path().join("barrier.jsonl");
        let barrier = Barrier::environment_bound(document.clone(), None, Some(journal.clone()));
        std::fs::write(&document, "{ not json").expect("broken document");
        assert!(barrier.arrive(Point::WriterExecution, "index").is_none());
        let journalled = std::fs::read_to_string(&journal).expect("journal");
        assert!(journalled.contains("could not be parsed"), "{journalled}");
        assert!(journalled.contains("\"point\":null"), "{journalled}");
    }
}
