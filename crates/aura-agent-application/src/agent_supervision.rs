//! Per-run supervision: the real task owner, its generation and one teardown rule.
//!
//! WHY THIS EXISTS
//! A run used to be one unsupervised `tokio::spawn` whose teardown lived *after* the awaited
//! work. A normal return and an `Err` return reached it; a panic did not - the task died with
//! its `active` entry still in the map, so the run could never be cancelled, never be stopped
//! and never be started again, and its `stopped` signal never fired. "A cancel was sent" was
//! also the only evidence anyone had: there was no handle to ask whether the work had really
//! ended.
//!
//! WHAT IS KEPT APART
//! Admission, start, real end, cancellation and hand-over are five different facts, and this
//! module records them separately:
//!
//! * **admitted** - the entry exists, before anything runs. A second launch of the same run is
//!   refused here, so two tasks can never own one run.
//! * **started** - the task body really began.
//! * **cancel-requested** - the cancellation signals were fired. This is **not** an end.
//! * **really ended** - the task's own guard ran, however the task finished: normal return,
//!   `Err`, panic, abort or a dropped runtime.
//! * **handed over** - a run that did not end inside the stop budget is given to a bounded
//!   observation owner that keeps waiting for its real end and records it; the caller is told
//!   the run is still alive instead of being told `cleanup = true`.
//!
//! ONE TEARDOWN RULE
//! [`RunTaskGuard`]'s `Drop` is the only place an `active` entry is removed. It runs on a
//! normal return, on an `Err` return, on a panic (the future is dropped while unwinding), on an
//! abort and when the runtime is dropped. Removal is generation-checked, so a task from an
//! older generation can never delete or overwrite the entry a newer generation owns.
use aura_domain_common::AgentRunId;
use aura_domain_common::UtcTimestamp;
use aura_model_transport::ModelCancellationToken;
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, watch};

/// The monotonic generation of one run's task. Monotonic per run, never reused.
pub type RunGeneration = u64;

/// How long a stop request may wait for a run's real end before handing it to an observer.
pub(crate) const RUN_STOP_BUDGET: Duration = Duration::from_secs(45);

/// How long the bounded observation owner may keep waiting for a handed-over run.
pub(crate) const RUN_OBSERVATION_BUDGET: Duration = Duration::from_secs(300);

/// How many terminal facts are kept. A ring, so a long session cannot grow it.
const MAX_LEDGER: usize = 256;

/// The cancellation signals a run's real task consumes. A value, so a stop request never needs
/// a handle to be present.
#[derive(Clone)]
pub(crate) struct RunSignals {
    pub(crate) model: ModelCancellationToken,
    pub(crate) tools: watch::Sender<bool>,
}

impl RunSignals {
    /// Fires both independent cancellation sources. This is a request, not an end.
    pub(crate) fn request_cancel(&self) {
        self.model.cancel();
        self.tools.send_replace(true);
    }
}

/// The stage a run is really in, so a residue report names something.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunStage {
    Admitted,
    Starting,
    Preparing,
    Provider,
    Tools,
    Waiting,
    Recording,
    Stopping,
    Ended,
}

impl RunStage {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Starting => "starting",
            Self::Preparing => "preparing",
            Self::Provider => "provider",
            Self::Tools => "tools",
            Self::Waiting => "waiting",
            Self::Recording => "recording",
            Self::Stopping => "stopping",
            Self::Ended => "ended",
        }
    }
}

/// The shared stage cell one task writes and any reader observes.
#[derive(Clone)]
pub(crate) struct RunStageCell(Arc<Mutex<RunStage>>);

impl RunStageCell {
    #[must_use]
    fn new() -> Self {
        Self(Arc::new(Mutex::new(RunStage::Admitted)))
    }

    pub(crate) fn set(&self, stage: RunStage) {
        if let Ok(mut current) = self.0.lock() {
            *current = stage;
        }
    }

    #[must_use]
    pub(crate) fn get(&self) -> RunStage {
        self.0.lock().map(|stage| *stage).unwrap_or(RunStage::Admitted)
    }
}

/// How a run's task really finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskEnd {
    Completed,
    Failed,
    Panicked,
    Aborted,
}

impl TaskEnd {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Panicked => "panicked",
            Self::Aborted => "aborted",
        }
    }
}

/// One run's real task, as the supervisor holds it.
struct RunHandle {
    generation: RunGeneration,
    signals: RunSignals,
    stopped: Arc<Notify>,
    /// Written by the teardown guard itself: a real end is never inferred from a signal.
    #[allow(dead_code)] // the real-end fact, kept beside the entry the guard removes
    ended: Arc<AtomicBool>,
    /// Admission and start are different facts; the regression reads this one.
    started: Arc<AtomicBool>,
    stage: RunStageCell,
    task: Option<tokio::task::JoinHandle<()>>,
}

/// One terminal fact, recorded by the owner that observed it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalFact {
    pub run: AgentRunId,
    pub(crate) generation: RunGeneration,
    pub end: &'static str,
    pub(crate) stage: &'static str,
    pub(crate) at: UtcTimestamp,
}

/// A run that did not end inside a stop budget, handed to a bounded observation owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunResidue {
    pub run: AgentRunId,
    pub(crate) generation: RunGeneration,
    pub stage: &'static str,
    /// Whether a real observation owner is still waiting for this run's end.
    pub observed: bool,
    /// The budget that owner was given.
    pub observation_budget_ms: u64,
}

/// What one stop request really achieved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StopOutcome {
    pub(crate) run: AgentRunId,
    pub(crate) generation: RunGeneration,
    /// Whether the run's task really ended inside the budget. Never inferred from a cancel.
    pub real_end: bool,
    pub(crate) waited_ms: u64,
    pub(crate) stage: &'static str,
    pub(crate) residue: Option<RunResidue>,
}

/// What a Core shutdown really achieved, with residue listed separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShutdownOutcome {
    /// Runs whose real task ended inside the shared deadline.
    pub ended: Vec<AgentRunId>,
    /// Runs that were still alive when the deadline expired, each with its own fact.
    pub residue: Vec<RunResidue>,
    pub waited_ms: u64,
}

/// The admission a launch receives before anything runs.
pub(crate) struct RunAdmission {
    pub(crate) run: AgentRunId,
    pub(crate) generation: RunGeneration,
    signals: RunSignals,
    stopped: Arc<Notify>,
    ended: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    stage: RunStageCell,
}

impl RunAdmission {
    #[must_use]
    pub(crate) fn signals(&self) -> RunSignals {
        self.signals.clone()
    }

    /// The tool-cancellation receiver belongs to the run's task.
    #[must_use]
    pub(crate) fn tool_receiver(&self) -> watch::Receiver<bool> {
        self.signals.tools.subscribe()
    }

    #[must_use]
    pub(crate) fn stage_cell(&self) -> RunStageCell {
        self.stage.clone()
    }

    /// The one teardown rule, as a value the task owns.
    #[must_use]
    pub(crate) fn guard(self, supervisor: RunSupervisor) -> RunTaskGuard {
        RunTaskGuard {
            supervisor: Some(supervisor),
            run: self.run,
            generation: self.generation,
            stopped: self.stopped,
            ended: self.ended,
            started: self.started,
            stage: self.stage,
            end: None,
        }
    }
}

/// The only owner of an `active` entry's removal.
pub(crate) struct RunTaskGuard {
    supervisor: Option<RunSupervisor>,
    run: AgentRunId,
    generation: RunGeneration,
    stopped: Arc<Notify>,
    ended: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    stage: RunStageCell,
    end: Option<TaskEnd>,
}

impl RunTaskGuard {
    pub(crate) fn mark_started(&self) {
        self.started.store(true, Ordering::Release);
    }

    /// Records how the task really finished, on every exit path that can name one.
    pub(crate) fn finished(&mut self, end: TaskEnd) {
        self.end = Some(end);
    }

    #[allow(dead_code)] // the identity the guard tears down
    #[must_use]
    pub(crate) fn run(&self) -> AgentRunId {
        self.run
    }

    #[allow(dead_code)] // the generation the guard checks before it removes anything
    #[must_use]
    pub(crate) fn generation(&self) -> RunGeneration {
        self.generation
    }
}

impl Drop for RunTaskGuard {
    fn drop(&mut self) {
        // A panic that reaches this guard without having been named is still a panic: the run
        // really unwound, and calling it an abort would understate what happened.
        let end = self.end.unwrap_or(if std::thread::panicking() {
            TaskEnd::Panicked
        } else {
            TaskEnd::Aborted
        });
        // The real end is recorded before the entry is released, so a reader that no longer
        // sees an active entry can already see the terminal fact.
        self.ended.store(true, Ordering::Release);
        if let Some(supervisor) = self.supervisor.take() {
            supervisor.finish(self.run, self.generation, end);
        }
        self.stage.set(RunStage::Ended);
        self.stopped.notify_waiters();
    }
}

#[derive(Default)]
struct Shared {
    inner: Mutex<BTreeMap<AgentRunId, RunHandle>>,
    generations: Mutex<BTreeMap<AgentRunId, RunGeneration>>,
    ledger: Mutex<VecDeque<TerminalFact>>,
}

/// The per-run supervisor. One per Core; cheap to clone, every clone shares one record.
#[derive(Clone, Default)]
pub struct RunSupervisor {
    shared: Arc<Shared>,
}

impl RunSupervisor {
    #[must_use]
    pub fn new() -> Self {
        Self { shared: Arc::new(Shared::default()) }
    }

    /// Admits one run's task. `None` means this run already has a live task.
    pub(crate) fn admit(&self, run: AgentRunId) -> Option<RunAdmission> {
        let mut inner = self.shared.inner.lock().ok()?;
        if inner.contains_key(&run) {
            return None;
        }
        let generation = {
            let mut generations = self.shared.generations.lock().ok()?;
            let next = generations.get(&run).copied().unwrap_or(0).saturating_add(1);
            generations.insert(run, next);
            next
        };
        let (tools, _receiver) = watch::channel(false);
        let admission = RunAdmission {
            run,
            generation,
            signals: RunSignals { model: ModelCancellationToken::default(), tools },
            stopped: Arc::new(Notify::new()),
            ended: Arc::new(AtomicBool::new(false)),
            started: Arc::new(AtomicBool::new(false)),
            stage: RunStageCell::new(),
        };
        inner.insert(
            run,
            RunHandle {
                generation,
                signals: admission.signals.clone(),
                stopped: Arc::clone(&admission.stopped),
                ended: Arc::clone(&admission.ended),
                started: Arc::clone(&admission.started),
                stage: admission.stage.clone(),
                task: None,
            },
        );
        Some(admission)
    }

    /// Attaches the real task handle to the admitted run.
    pub(crate) fn install(
        &self,
        run: AgentRunId,
        generation: RunGeneration,
        task: tokio::task::JoinHandle<()>,
    ) -> bool {
        let Ok(mut inner) = self.shared.inner.lock() else {
            task.abort();
            return false;
        };
        let Some(handle) = inner.get_mut(&run) else {
            task.abort();
            return false;
        };
        if handle.generation != generation {
            task.abort();
            return false;
        }
        handle.task = Some(task);
        true
    }

    /// The generation-checked teardown. An older generation cannot remove a newer entry.
    fn finish(&self, run: AgentRunId, generation: RunGeneration, end: TaskEnd) {
        let stage = {
            let Ok(inner) = self.shared.inner.lock() else {
                return;
            };
            match inner.get(&run) {
                Some(handle) if handle.generation == generation => handle.stage.get(),
                // A newer generation owns this run: the old task must not delete or overwrite it.
                _ => return,
            }
        };
        self.record(TerminalFact {
            run,
            generation,
            end: end.as_str(),
            stage: stage.as_str(),
            at: UtcTimestamp::now(),
        });
        if let Ok(mut inner) = self.shared.inner.lock()
            && inner.get(&run).is_some_and(|handle| handle.generation == generation)
        {
            inner.remove(&run);
        }
    }

    fn record(&self, fact: TerminalFact) {
        if let Ok(mut ledger) = self.shared.ledger.lock() {
            if ledger.len() >= MAX_LEDGER {
                ledger.pop_front();
            }
            ledger.push_back(fact);
        }
    }

    #[must_use]
    pub fn active_runs(&self) -> Vec<AgentRunId> {
        self.shared.inner.lock().map(|inner| inner.keys().copied().collect()).unwrap_or_default()
    }

    #[must_use]
    pub fn is_active(&self, run: AgentRunId) -> bool {
        self.shared.inner.lock().is_ok_and(|inner| inner.contains_key(&run))
    }

    #[must_use]
    pub fn generation_of(&self, run: AgentRunId) -> Option<RunGeneration> {
        self.shared.generations.lock().ok().and_then(|generations| generations.get(&run).copied())
    }

    #[must_use]
    pub(crate) fn stage_of(&self, run: AgentRunId) -> Option<RunStage> {
        self.shared.inner.lock().ok().and_then(|inner| inner.get(&run).map(|h| h.stage.get()))
    }

    /// The "started" fact, read by the regression tests.
    #[allow(dead_code)]
    #[must_use]
    pub fn started(&self, run: AgentRunId) -> bool {
        self.shared
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.get(&run).map(|h| h.started.load(Ordering::Acquire)))
            .unwrap_or(false)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn abort_task_for_test(&self, run: AgentRunId) -> bool {
        let Ok(inner) = self.shared.inner.lock() else { return false };
        let Some(task) = inner.get(&run).and_then(|handle| handle.task.as_ref()) else {
            return false;
        };
        task.abort();
        true
    }

    #[must_use]
    pub(crate) fn signals_of(&self, run: AgentRunId) -> Option<RunSignals> {
        self.shared.inner.lock().ok().and_then(|inner| inner.get(&run).map(|h| h.signals.clone()))
    }

    /// The terminal facts a report or a regression reads.
    #[allow(dead_code)]
    #[must_use]
    pub fn ledger(&self) -> Vec<TerminalFact> {
        self.shared.ledger.lock().map(|ledger| ledger.iter().cloned().collect()).unwrap_or_default()
    }

    /// Fires the cancellation sources of one run. A request, never a terminal fact.
    pub(crate) fn request_cancel(&self, run: AgentRunId) -> bool {
        match self.signals_of(run) {
            Some(signals) => {
                signals.request_cancel();
                true
            }
            None => false,
        }
    }

    /// Waits for one run's real end inside an absolute deadline. Never cancels anything.
    pub async fn wait_ended(&self, run: AgentRunId, deadline: Instant) -> StopOutcome {
        let started = Instant::now();
        let (generation, stopped, stage) = {
            let Ok(inner) = self.shared.inner.lock() else {
                return StopOutcome {
                    run,
                    generation: 0,
                    real_end: false,
                    waited_ms: 0,
                    stage: RunStage::Ended.as_str(),
                    residue: None,
                };
            };
            match inner.get(&run) {
                Some(handle) => {
                    (handle.generation, Arc::clone(&handle.stopped), handle.stage.get())
                }
                // No live entry: the real end already happened, or nothing was ever admitted.
                None => {
                    return StopOutcome {
                        run,
                        generation: self.generation_of(run).unwrap_or(0),
                        real_end: true,
                        waited_ms: 0,
                        stage: RunStage::Ended.as_str(),
                        residue: None,
                    };
                }
            }
        };
        let notified = stopped.notified();
        tokio::pin!(notified);
        // Enabled before the second check, so an end that happens in between is not lost.
        notified.as_mut().enable();
        if !self.is_active(run) || Instant::now() >= deadline {
            let ended = !self.is_active(run);
            return StopOutcome {
                run,
                generation,
                real_end: ended,
                waited_ms: elapsed_ms(started),
                stage: if ended { RunStage::Ended } else { stage }.as_str(),
                residue: None,
            };
        }
        let ended = tokio::time::timeout_at(deadline.into(), notified).await.is_ok();
        let waited = elapsed_ms(started);
        if ended || !self.is_active(run) {
            StopOutcome {
                run,
                generation,
                real_end: true,
                waited_ms: waited,
                stage: RunStage::Ended.as_str(),
                residue: None,
            }
        } else {
            StopOutcome {
                run,
                generation,
                real_end: false,
                waited_ms: waited,
                stage: stage.as_str(),
                residue: Some(RunResidue {
                    run,
                    generation,
                    stage: stage.as_str(),
                    observed: false,
                    observation_budget_ms: 0,
                }),
            }
        }
    }

    /// Stops one run: request cancellation, then wait for its real end inside one deadline.
    pub(crate) async fn stop_within(&self, run: AgentRunId, deadline: Instant) -> StopOutcome {
        self.request_cancel(run);
        let mut outcome = self.wait_ended(run, deadline).await;
        if !outcome.real_end {
            // The side effect's terminal state is not proven, so the run is handed to a bounded
            // observation owner instead of being reported as cleaned up.
            let observed = self.observe(run, outcome.generation);
            outcome.residue = Some(RunResidue {
                run,
                generation: outcome.generation,
                stage: outcome.stage,
                observed,
                observation_budget_ms: millis(RUN_OBSERVATION_BUDGET),
            });
        }
        outcome
    }

    /// Hands one still-running task to a bounded observation owner.
    ///
    /// The owner keeps the task's real handle, waits a bounded time for its actual end and
    /// records the terminal fact. Returns whether an owner was really started: with no runtime
    /// left there is nothing to observe with, and that is reported as `false` rather than as a
    /// cleaned-up run.
    pub(crate) fn observe(&self, run: AgentRunId, generation: RunGeneration) -> bool {
        let task = {
            let Ok(mut inner) = self.shared.inner.lock() else {
                return false;
            };
            let Some(handle) = inner.get_mut(&run) else {
                return false;
            };
            if handle.generation != generation {
                return false;
            }
            handle.task.take()
        };
        let Some(task) = task else {
            return false;
        };
        let supervisor = self.clone();
        let spawned = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tokio::spawn(async move {
                match tokio::time::timeout(RUN_OBSERVATION_BUDGET, task).await {
                    // The task's own guard already recorded its end; awaiting the real handle
                    // here is what makes the hand-over a real observation.
                    Ok(_) => {}
                    Err(_) => supervisor.record(TerminalFact {
                        run,
                        generation,
                        end: "unobserved",
                        stage: RunStage::Stopping.as_str(),
                        at: UtcTimestamp::now(),
                    }),
                }
            })
        }));
        spawned.is_ok()
    }

    /// Stops every live run under one monotonic deadline.
    ///
    /// The product `Drop` path uses [`Self::shutdown_now`], which cannot wait; this is the
    /// bounded entry a caller that can await uses.
    #[allow(dead_code)]
    ///
    /// The deadline is computed once by the caller and never reset per run or per stage: a
    /// second run does not buy the first one more time.
    pub(crate) async fn stop_all_within(&self, deadline: Instant) -> ShutdownOutcome {
        let started = Instant::now();
        let runs = self.active_runs();
        for run in &runs {
            self.request_cancel(*run);
        }
        let mut ended = Vec::new();
        let mut residue = Vec::new();
        for run in runs {
            let generation = self.generation_of(run).unwrap_or(0);
            let outcome = self.wait_ended(run, deadline).await;
            if outcome.real_end {
                ended.push(run);
            } else {
                let observed = self.observe(run, generation);
                residue.push(RunResidue {
                    run,
                    generation,
                    stage: outcome.stage,
                    observed,
                    observation_budget_ms: millis(RUN_OBSERVATION_BUDGET),
                });
            }
        }
        ShutdownOutcome { ended, residue, waited_ms: elapsed_ms(started) }
    }

    /// The shutdown entry a `Drop` can call: fires every cancellation, hands every live run to a
    /// bounded observation owner when a runtime is still available, and reports every live run as
    /// residue. Nothing here claims the work ended.
    pub(crate) fn shutdown_now(&self) -> ShutdownOutcome {
        let started = Instant::now();
        let runs = self.active_runs();
        for run in &runs {
            self.request_cancel(*run);
        }
        let residue = runs
            .into_iter()
            .map(|run| {
                let generation = self.generation_of(run).unwrap_or(0);
                // With a runtime still alive the run is handed over for real; with none there is
                // nothing to observe with, and that is reported as `false` rather than as a
                // cleaned-up run.
                let observed = self.observe(run, generation);
                RunResidue {
                    run,
                    generation,
                    stage: self.stage_of(run).map_or("unknown", RunStage::as_str),
                    observed,
                    observation_budget_ms: if observed {
                        millis(RUN_OBSERVATION_BUDGET)
                    } else {
                        0
                    },
                }
            })
            .collect();
        ShutdownOutcome { ended: Vec::new(), residue, waited_ms: elapsed_ms(started) }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// A run's own future, with one panic turned into a value.
///
/// WHY THIS EXISTS: teardown must run on **every** exit, and the honest fact that a handler
/// panicked has to be available before the entry is released. `Drop` gives the first and not the
/// second. This wrapper hands `Poll` to `catch_unwind` once, exactly like
/// `futures::FutureExt::catch_unwind`, and the future is abandoned as soon as it panics - it is
/// never polled again - so nothing ever observes a broken future.
///
/// It is written here rather than pulled in as a dependency: the whole of it is the pin
/// projection of an already-boxed future plus one `catch_unwind`.
pub(crate) struct CatchUnwind<F: Future> {
    inner: Pin<Box<F>>,
}

impl<F: Future> CatchUnwind<F> {
    pub(crate) fn new(future: F) -> Self {
        Self { inner: Box::pin(future) }
    }
}

impl<F: Future> Future for CatchUnwind<F> {
    type Output = Result<F::Output, Box<dyn std::any::Any + Send>>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match std::panic::catch_unwind(AssertUnwindSafe(|| this.inner.as_mut().poll(context))) {
            Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(payload) => Poll::Ready(Err(payload)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panicking_run_task_still_releases_its_entry_and_signals_the_real_end() {
        let supervisor = RunSupervisor::new();
        let run = AgentRunId::new();
        let admission = supervisor.admit(run).expect("admitted");
        let generation = admission.generation;
        let stopped = supervisor.active_runs();
        assert_eq!(stopped, vec![run], "admission is a fact before anything runs");
        let task = {
            let supervisor = supervisor.clone();
            tokio::spawn(async move {
                let guard = admission.guard(supervisor);
                assert_eq!(guard.run(), run);
                panic!("injected: the real run task unwound");
            })
        };
        assert!(supervisor.install(run, generation, task));
        // Wait for the panic to unwind the real task.
        for _ in 0..500 {
            if !supervisor.is_active(run) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(4)).await;
        }
        assert!(!supervisor.is_active(run), "a panic must not leave an active entry");
        let ledger = supervisor.ledger();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].end, "panicked", "{ledger:?}");
        let outcome = supervisor.wait_ended(run, Instant::now() + RUN_STOP_BUDGET).await;
        assert!(outcome.real_end, "the real end is already a fact");
    }

    #[tokio::test]
    async fn an_old_generation_cannot_tear_down_a_new_one() {
        let supervisor = RunSupervisor::new();
        let run = AgentRunId::new();
        let first = supervisor.admit(run).expect("first admission");
        let first_generation = first.generation;
        let guard = first.guard(supervisor.clone());
        supervisor.finish(run, first_generation, TaskEnd::Completed);
        drop(guard);
        let second = supervisor.admit(run).expect("second admission");
        assert!(second.generation > first_generation);
        // A late teardown from the old generation must not remove the new entry.
        supervisor.finish(run, first_generation, TaskEnd::Failed);
        assert!(supervisor.is_active(run), "the new generation still owns its entry");
        assert_eq!(supervisor.generation_of(run), Some(second.generation));
        supervisor.finish(run, second.generation, TaskEnd::Completed);
        assert!(!supervisor.is_active(run));
    }

    #[tokio::test]
    async fn a_run_that_does_not_end_in_the_budget_is_handed_over_not_called_clean() {
        let supervisor = RunSupervisor::new();
        let run = AgentRunId::new();
        let admission = supervisor.admit(run).expect("admitted");
        let generation = admission.generation;
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let task = {
            let supervisor = supervisor.clone();
            tokio::spawn(async move {
                let mut guard = admission.guard(supervisor);
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = wait.recv_timeout(Duration::from_secs(20));
                })
                .await;
                guard.finished(TaskEnd::Completed);
            })
        };
        assert!(supervisor.install(run, generation, task));
        let outcome =
            supervisor.stop_within(run, Instant::now() + Duration::from_millis(120)).await;
        assert!(!outcome.real_end, "{outcome:?}");
        let residue = outcome.residue.expect("residue is reported, not hidden");
        assert!(residue.observed, "the run was handed to a real observation owner");
        assert_eq!(residue.stage, RunStage::Admitted.as_str());
        let _ = release.send(());
        for _ in 0..500 {
            if !supervisor.is_active(run) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(!supervisor.is_active(run), "the observation owner really saw the end");
        assert_eq!(supervisor.ledger().last().map(|fact| fact.end), Some("completed"));
    }

    #[test]
    fn the_shutdown_entry_lists_residue_instead_of_claiming_cleanup() {
        let supervisor = RunSupervisor::new();
        let run = AgentRunId::new();
        let admission = supervisor.admit(run).expect("admitted");
        let outcome = supervisor.shutdown_now();
        assert!(outcome.ended.is_empty());
        assert_eq!(outcome.residue.len(), 1);
        assert_eq!(outcome.residue[0].run, run);
        assert!(!outcome.residue[0].observed);
        drop(admission);
    }
}
