//! The bounded, ownership-explicit boundary every synchronous section on an async Core
//! path runs on.
//!
//! WHY THIS EXISTS
//! The Core request path is asynchronous, but the work underneath several of its steps is
//! not: `broker/files.rs::facts` stats a path, walks a directory and hashes a whole file;
//! the broker journal runs SQLite statements; a session command writes through the one
//! persistence writer; the Workspace Files projection reads every online workspace. Those
//! are synchronous APIs, and calling one of them directly inside an async future parks a
//! Tokio **worker** for as long as it takes. The heartbeat, the startup progress, the
//! control lane and every other connection are scheduled on those same workers, so a
//! worker parked in file IO or in a lock wait is not a slow request: it is a Core that
//! cannot answer the peer at all.
//!
//! WHAT IS BOUNDED
//! * **How many run at once.** One fixed-size [`BlockingPool`] of [`SYNC_WORKERS`] permits.
//!   A section that cannot get a permit inside its budget is refused; it does not queue
//!   without limit and it does not grow the pool.
//! * **How long a caller waits.** The permit is taken under the caller's own budget, so a
//!   refusal is a proven "this section never ran".
//! * **Who owns the work.** The permit belongs to the blocking task, not to the caller. A
//!   caller that walks away leaves the work running and its permit held; the counters and
//!   the span are written by the thread that really ran the work, when it really finished.
//!
//! WHAT THIS DELIBERATELY IS NOT
//! The boundary is not a way to declare isolation by adding threads. The isolation the
//! Core needs comes from **never occupying a Tokio worker** with this work; the pool's
//! size only decides how much of it may block at once. The pool is therefore fixed and
//! small, and a saturation is an honest refusal that the caller can see.
//!
//! MEASUREMENT
//! Every completed section records a [`SyncSpan`]: which section, which workspace and
//! request it belonged to, the real OS thread that ran it, and the monotonic elapsed
//! time. Slow spans are appended to the Core log as `sync_boundary.slow_span` so a stall
//! in a real run can be attributed to a named section instead of guessed from timing.
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::blocking_pool::{BlockingAdmission, BlockingPool};

/// How many synchronous sections may really be blocking at once.
///
/// Fixed on purpose: the pool exists to keep this work off the reactor and to bound how
/// much of it the process does at once, not to run more of it in parallel.
pub(crate) const SYNC_WORKERS: usize = 4;

/// The production budget for one synchronous section.
///
/// It bounds how long an async caller waits for a section. It does **not** stop the
/// section: a section that started and did not answer inside this budget is reported as
/// [`BlockingAdmission::Unknown`] and keeps running under its own permit.
pub(crate) const SYNC_SECTION_BUDGET: Duration = Duration::from_secs(30);

/// A section slower than this is written to the Core log with its identity.
pub(crate) const SYNC_SLOW_SPAN_MS: u64 = 250;

/// How many spans are kept for inspection. A ring, so a long run cannot grow it.
pub(crate) const SYNC_SPAN_CAPACITY: usize = 256;

/// Which synchronous section a span belongs to.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SyncSection {
    /// Reading the pre-state facts of a dispatched operation (`broker/files.rs::facts`).
    Inspect,
    /// Reading the post-state facts and the domain journal after the handler returned.
    Evidence,
    /// Reserving, claiming or completing the broker ledger for an attempt.
    Ledger,
    /// A pure journal/storage read (`reconcile`, `verify_handle`).
    JournalRead,
    /// One admitted Agent Store section: a run's journal read or transaction.
    AgentStore,
    /// `session.create` / `session.rename`.
    SessionCommand,
    /// Rebuilding the Workspace Files projection.
    Projection,
    /// A configuration read or write performed by a recovery entry.
    ///
    /// `config.recovery.notice`, `.plan` and `.apply` do real
    /// synchronous file I/O — the settings facade opens, reads, validates, encodes and
    /// atomically replaces configuration files. They were called **directly from an
    /// async handler**, so that I/O ran on the request executor's own thread: a slow
    /// or blocked file (a network path, a filter driver, a large history directory)
    /// stalled the whole dispatch loop for every other request, including the status
    /// and stop calls the user is supposed to keep while configuration is broken.
    /// Routing them through the boundary puts them under the same bounded four-worker
    /// admission as every other real section, and makes the stall observable in the
    /// section counters instead of invisible.
    ConfigRecovery,
    /// Tool/Model Secret platform access and its owning journal transaction.
    CredentialSecret,
    /// Tool package import, preview, preparation and install: archive staging, file
    /// hashing and the one V2 transaction each step owns. Conformance runs elsewhere.
    PackageLifecycle,
    /// File-authoritative Vault management and frozen export snapshots.
    MemoryVault,
}

impl SyncSection {
    #[allow(dead_code)]
    pub(crate) const ALL: [Self; 11] = [
        Self::Inspect,
        Self::Evidence,
        Self::Ledger,
        Self::JournalRead,
        Self::AgentStore,
        Self::SessionCommand,
        Self::Projection,
        Self::ConfigRecovery,
        Self::CredentialSecret,
        Self::PackageLifecycle,
        Self::MemoryVault,
    ];

    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Evidence => "evidence",
            Self::Ledger => "ledger",
            Self::JournalRead => "journal_read",
            Self::AgentStore => "agent_store",
            Self::SessionCommand => "session_command",
            Self::Projection => "projection",
            Self::ConfigRecovery => "config_recovery",
            Self::CredentialSecret => "credential_secret",
            Self::PackageLifecycle => "package_lifecycle",
            Self::MemoryVault => "memory_vault",
        }
    }
}

/// The identity a span is attributed to.
///
/// Built from what the caller already has. It is what makes a stall nameable: a slow span
/// says which workspace and which request it belonged to, instead of only how long it was.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SyncSubject {
    workspace: Option<String>,
    request: Option<String>,
    method: Option<String>,
}

impl SyncSubject {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn workspace(mut self, workspace: impl std::fmt::Display) -> Self {
        self.workspace = Some(workspace.to_string());
        self
    }

    #[must_use]
    pub(crate) fn request(mut self, request: impl std::fmt::Display) -> Self {
        self.request = Some(request.to_string());
        self
    }

    #[must_use]
    pub(crate) fn method(mut self, method: &str) -> Self {
        self.method = Some(method.to_owned());
        self
    }
}

/// One synchronous section that reached this boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SyncSpan {
    pub(crate) section: &'static str,
    pub(crate) workspace: Option<String>,
    pub(crate) request: Option<String>,
    pub(crate) method: Option<String>,
    /// The real OS thread that ran the work. Empty for a section that never ran.
    pub(crate) thread: String,
    /// Wall-clock milliseconds. Display only, never a budget.
    pub(crate) at_ms: u64,
    /// Monotonic time the work itself took. Zero when the work never ran.
    pub(crate) elapsed_ms: u64,
    /// Monotonic time the caller spent before it got its answer.
    pub(crate) caller_waited_ms: u64,
    /// `completed`, `refused` or `unknown`. A refusal means the work did not run; an
    /// unknown answer means it started and may still be running.
    pub(crate) outcome: &'static str,
}

/// What one section has done so far.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct SyncSectionCounters {
    /// Sections that took a permit without waiting.
    pub(crate) admitted: u64,
    /// Sections that waited for a permit and then got one.
    pub(crate) waited: u64,
    /// Sections refused because no permit came free inside the budget. They did not run.
    pub(crate) refused: u64,
    /// Sections that started and did not answer inside the budget.
    pub(crate) unknown: u64,
    /// Sections whose work really finished, counted by the thread that ran it.
    pub(crate) completed: u64,
    /// Completed sections slower than [`SYNC_SLOW_SPAN_MS`].
    pub(crate) slow: u64,
    /// Sections whose work is really running right now. Released by the work itself.
    pub(crate) in_flight: u64,
    /// The slowest completed section.
    pub(crate) max_elapsed_ms: u64,
    /// The sum of every completed section's own time.
    pub(crate) total_elapsed_ms: u64,
}

#[derive(Default)]
struct Shared {
    counters: BTreeMap<&'static str, SyncSectionCounters>,
    spans: VecDeque<SyncSpan>,
}

/// The bounded boundary itself. Cheap to clone; every clone shares one pool and one record.
#[derive(Clone)]
pub(crate) struct SyncBoundary {
    pool: BlockingPool,
    budget_ms: Arc<AtomicU64>,
    shared: Arc<Mutex<Shared>>,
    /// Regression-only fault injection: run the section on the calling thread instead of on
    /// the pool, which is exactly the behaviour that existed before this boundary. It exists
    /// so a regression can show that the property it checks is real rather than assumed, and
    /// it is compiled out of every product build.
    #[cfg(test)]
    inline: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for SyncBoundary {
    fn default() -> Self {
        Self::open()
    }
}

impl SyncBoundary {
    /// The production boundary: [`SYNC_WORKERS`] permits, [`SYNC_SECTION_BUDGET`] per call.
    #[must_use]
    pub(crate) fn open() -> Self {
        Self::with_budget(SYNC_SECTION_BUDGET)
    }

    /// The same boundary with a caller-chosen budget.
    ///
    /// A regression that has to observe a refusal must not wait the production budget, and
    /// the budget is the only thing it changes: the pool, the ownership and the recording
    /// are the production ones.
    #[must_use]
    pub(crate) fn with_budget(budget: Duration) -> Self {
        Self {
            pool: BlockingPool::new(SYNC_WORKERS),
            budget_ms: Arc::new(AtomicU64::new(
                u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
            )),
            shared: Arc::new(Mutex::new(Shared::default())),
            #[cfg(test)]
            inline: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Regression-only: makes this boundary run sections on the calling thread.
    ///
    /// This is the pre-boundary behaviour, injected so that the isolation regression can
    /// show it really distinguishes the two. It is never reachable from a product path.
    #[cfg(test)]
    pub(crate) fn force_inline_for_test(&self, inline: bool) {
        self.inline.store(inline, Ordering::Release);
    }

    /// Replaces the budget of every clone of this boundary.
    ///
    /// Used by the isolation regressions, which must observe a refusal without waiting the
    /// production budget. Only the budget moves; the pool, the ownership and the recording
    /// are the production ones.
    #[allow(dead_code)]
    pub(crate) fn set_budget(&self, budget: Duration) {
        self.budget_ms
            .store(u64::try_from(budget.as_millis()).unwrap_or(u64::MAX), Ordering::Release);
    }

    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn budget(&self) -> Duration {
        Duration::from_millis(self.budget_ms.load(Ordering::Acquire))
    }

    /// How many sections may block at once.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn capacity(&self) -> usize {
        self.pool.capacity()
    }

    /// How many permits are free right now.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn available(&self) -> usize {
        self.pool.available()
    }

    /// One section's counters. A section that never ran reports zeroes.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn counters(&self, section: SyncSection) -> SyncSectionCounters {
        self.shared
            .lock()
            .ok()
            .and_then(|shared| shared.counters.get(section.as_str()).copied())
            .unwrap_or_default()
    }

    /// Every section's counters, for a report that has to show all of them at once.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn all_counters(&self) -> BTreeMap<&'static str, SyncSectionCounters> {
        self.shared.lock().map(|shared| shared.counters.clone()).unwrap_or_default()
    }

    /// The recorded spans, oldest first. Bounded by [`SYNC_SPAN_CAPACITY`].
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn spans(&self) -> Vec<SyncSpan> {
        self.shared.lock().map(|shared| shared.spans.iter().cloned().collect()).unwrap_or_default()
    }

    /// Runs `work` as one bounded synchronous section.
    ///
    /// The work is moved to the boundary whole and owns everything it needs; nothing about
    /// it is re-implemented here. The permit, the counters and the span belong to the
    /// blocking task: they are released and written when the work really ends, whether or
    /// not the caller is still waiting.
    pub(crate) async fn run<T, F>(
        &self,
        section: SyncSection,
        subject: SyncSubject,
        work: F,
    ) -> BlockingAdmission<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        self.run_settled(section, subject, work, |_value: &T| {}).await
    }

    /// Await an admitted section's actual result from an independently owned completion task.
    /// A caller's timed wait must wrap that task, not this receiver. The four-worker admission
    /// limit still applies; once admitted, an asynchronous receipt retains no boundary permit.
    pub(crate) async fn run_owned<T, F>(
        &self,
        section: SyncSection,
        subject: SyncSubject,
        work: F,
    ) -> BlockingAdmission<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let admission = self
            .run(section, subject, move || {
                let _ = sender.send(work());
            })
            .await;
        match admission {
            BlockingAdmission::Refused { waited_ms } => BlockingAdmission::Refused { waited_ms },
            other => match receiver.await {
                Ok(value) => BlockingAdmission::Completed(value),
                Err(_) => BlockingAdmission::Unknown {
                    waited_ms: match other {
                        BlockingAdmission::Unknown { waited_ms } => waited_ms,
                        _ => 0,
                    },
                },
            },
        }
    }

    /// The same section, with the result handed to `settle` when the work really ends.
    ///
    /// WHY THIS EXISTS. `run` returns the value to the caller and nothing else; when the
    /// caller's budget expires the value had no reader at all. A section that is a real
    /// business operation must instead hand its result to the owner that answers for it, and
    /// this is the one seam every such section goes through. The sink is invoked exactly once
    /// by the thread that really ran the work — through the pool on the pool path, and inline
    /// on the injected test path — never by the departing caller.
    pub(crate) async fn run_settled<T, F, S>(
        &self,
        section: SyncSection,
        subject: SyncSubject,
        work: F,
        settle: S,
    ) -> BlockingAdmission<T>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
        S: FnOnce(&T) + Send + 'static,
    {
        let started = Instant::now();
        let deadline = started + self.budget();
        let shared = Arc::clone(&self.shared);
        let admission_shared = Arc::clone(&self.shared);
        let section_name = section.as_str();
        #[cfg(test)]
        if self.inline.load(Ordering::Acquire) {
            // The injected pre-boundary behaviour: the work runs right here, on whatever
            // thread called this - which for a Core request is a reactor worker.
            return {
                bump(&shared, section_name, |counters| counters.in_flight += 1);
                let work_started = Instant::now();
                let value = work();
                let elapsed = elapsed_ms(work_started);
                settle(&value);
                bump(&shared, section_name, |counters| {
                    counters.admitted += 1;
                    counters.completed += 1;
                    counters.in_flight = counters.in_flight.saturating_sub(1);
                    counters.max_elapsed_ms = counters.max_elapsed_ms.max(elapsed);
                    counters.total_elapsed_ms = counters.total_elapsed_ms.saturating_add(elapsed);
                });
                record(
                    &shared,
                    SyncSpan {
                        section: section_name,
                        workspace: subject.workspace.clone(),
                        request: subject.request.clone(),
                        method: subject.method.clone(),
                        thread: format!(
                            "inline:{}",
                            std::thread::current().name().unwrap_or("unnamed")
                        ),
                        at_ms: now_ms(),
                        elapsed_ms: elapsed,
                        caller_waited_ms: elapsed,
                        outcome: "completed",
                    },
                );
                BlockingAdmission::Completed(value)
            };
        }
        // Whether this call had to queue is observed before the permit is asked for, in the
        // same way the pool itself decides; it is attributed to the section here.
        let had_free_worker = self.pool.available() > 0;
        bump(&shared, section_name, |counters| counters.in_flight += 1);
        let owned_subject = subject.clone();
        let admission = self
            .pool
            .run_settled(
                deadline,
                move || {
                    let work_started = Instant::now();
                    let value = work();
                    let elapsed = elapsed_ms(work_started);
                    let thread = std::thread::current().name().unwrap_or("unnamed").to_owned();
                    bump(&shared, section_name, |counters| {
                        counters.completed += 1;
                        counters.in_flight = counters.in_flight.saturating_sub(1);
                        counters.max_elapsed_ms = counters.max_elapsed_ms.max(elapsed);
                        counters.total_elapsed_ms =
                            counters.total_elapsed_ms.saturating_add(elapsed);
                        if elapsed >= SYNC_SLOW_SPAN_MS {
                            counters.slow += 1;
                        }
                    });
                    record(
                        &shared,
                        SyncSpan {
                            section: section_name,
                            workspace: owned_subject.workspace.clone(),
                            request: owned_subject.request.clone(),
                            method: owned_subject.method.clone(),
                            thread,
                            at_ms: now_ms(),
                            elapsed_ms: elapsed,
                            caller_waited_ms: elapsed_ms(started),
                            outcome: "completed",
                        },
                    );
                    if elapsed >= SYNC_SLOW_SPAN_MS {
                        crate::append_core_log(
                            "sync_boundary.slow_span",
                            serde_json::json!({
                                "section": section_name,
                                "workspace": owned_subject.workspace,
                                "request": owned_subject.request,
                                "method": owned_subject.method,
                                "elapsed_ms": elapsed,
                                "thread": std::thread::current().name().unwrap_or("unnamed"),
                                "workers": SYNC_WORKERS,
                            }),
                        );
                    }
                    value
                },
                settle,
            )
            .await;
        match &admission {
            BlockingAdmission::Completed(_) => {
                bump(&admission_shared, section_name, |counters| {
                    if had_free_worker {
                        counters.admitted += 1;
                    } else {
                        counters.waited += 1;
                    }
                });
            }
            BlockingAdmission::Refused { .. } => {
                bump(&admission_shared, section_name, |counters| {
                    counters.refused += 1;
                    counters.in_flight = counters.in_flight.saturating_sub(1);
                });
                self.record_terminal(section, subject, started, "refused");
            }
            BlockingAdmission::Unknown { .. } => {
                bump(&admission_shared, section_name, |counters| counters.unknown += 1);
                self.record_terminal(section, subject, started, "unknown");
            }
        }
        admission
    }

    fn record_terminal(
        &self,
        section: SyncSection,
        subject: SyncSubject,
        started: Instant,
        outcome: &'static str,
    ) {
        let shared = Arc::clone(&self.shared);
        record(
            &shared,
            SyncSpan {
                section: section.as_str(),
                workspace: subject.workspace.clone(),
                request: subject.request.clone(),
                method: subject.method.clone(),
                thread: String::new(),
                at_ms: now_ms(),
                elapsed_ms: 0,
                caller_waited_ms: elapsed_ms(started),
                outcome,
            },
        );
        // A refusal is a real fact about a real request and is worth a log line: it is the
        // difference between "the Core was slow" and "the Core refused because it was full".
        crate::append_core_log(
            "sync_boundary.section_not_run",
            serde_json::json!({
                "section": section.as_str(),
                "outcome": outcome,
                "workspace": subject.workspace,
                "request": subject.request,
                "method": subject.method,
                "waited_ms": elapsed_ms(started),
                "workers": SYNC_WORKERS,
            }),
        );
    }
}

fn bump(
    shared: &Arc<Mutex<Shared>>,
    section: &'static str,
    update: impl FnOnce(&mut SyncSectionCounters),
) {
    let Ok(mut shared) = shared.lock() else {
        return;
    };
    update(shared.counters.entry(section).or_default());
}

fn record(shared: &Arc<Mutex<Shared>>, span: SyncSpan) {
    let Ok(mut shared) = shared.lock() else {
        return;
    };
    if shared.spans.len() >= SYNC_SPAN_CAPACITY {
        shared.spans.pop_front();
    }
    shared.spans.push_back(span);
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_full_boundary_refuses_without_running_the_section_and_recovers() {
        let boundary = SyncBoundary::with_budget(Duration::from_millis(60));
        assert_eq!(boundary.capacity(), SYNC_WORKERS);
        let release = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut holders = Vec::new();
        for _ in 0..SYNC_WORKERS {
            let held = boundary.clone();
            let release = Arc::clone(&release);
            holders.push(tokio::spawn(async move {
                held.run(SyncSection::Inspect, SyncSubject::new(), move || {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while release.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                })
                .await
            }));
        }
        for _ in 0..2_000 {
            if boundary.available() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(boundary.available(), 0, "every worker is really occupied");
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let outcome = boundary
            .run(SyncSection::Evidence, SyncSubject::new().workspace("wsp_x"), move || {
                flag.store(true, Ordering::SeqCst);
            })
            .await;
        assert!(
            matches!(outcome, BlockingAdmission::Refused { .. }),
            "a full boundary must refuse inside the budget"
        );
        assert!(!ran.load(Ordering::SeqCst), "a refused section must not have run");
        let refused = boundary.counters(SyncSection::Evidence);
        assert_eq!(refused.refused, 1);
        assert_eq!(refused.completed, 0);
        let in_flight = boundary.counters(SyncSection::Inspect).in_flight;
        assert_eq!(in_flight, SYNC_WORKERS as u64, "the running work still owns its permits");
        for _ in 0..SYNC_WORKERS {
            release.fetch_add(1, Ordering::AcqRel);
        }
        for holder in holders {
            let _ = holder.await;
        }
        for _ in 0..2_000 {
            if boundary.available() == SYNC_WORKERS {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(boundary.available(), SYNC_WORKERS, "the permits came back");
        assert_eq!(boundary.counters(SyncSection::Inspect).in_flight, 0);
        assert_eq!(boundary.counters(SyncSection::Inspect).completed, SYNC_WORKERS as u64);
        let recovered = boundary.run(SyncSection::Inspect, SyncSubject::new(), || 7).await;
        assert!(matches!(recovered, BlockingAdmission::Completed(7)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_finished_section_is_attributed_to_its_thread_and_subject() {
        let boundary = SyncBoundary::with_budget(Duration::from_secs(5));
        let outcome = boundary
            .run(
                SyncSection::Projection,
                SyncSubject::new().workspace("wsp_1").request("cor_1").method("projection"),
                || 11,
            )
            .await;
        assert!(matches!(outcome, BlockingAdmission::Completed(11)));
        let spans = boundary.spans();
        let span = spans.last().expect("a completed section records a span");
        assert_eq!(span.section, "projection");
        assert_eq!(span.outcome, "completed");
        assert_eq!(span.workspace.as_deref(), Some("wsp_1"));
        assert_eq!(span.request.as_deref(), Some("cor_1"));
        assert!(!span.thread.is_empty(), "the real thread that ran the work is recorded");
        assert_eq!(boundary.counters(SyncSection::Projection).completed, 1);
    }
}
