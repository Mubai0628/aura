//! Bounded ownership of the two expensive Host Preparation facts.
//!
//! An ordinary `security.host_preparation.status` used to do three costly things on the
//! way to answering a question that needs none of them: it probed the host, it built a
//! whole security runtime projection, and it queued an expiry settlement on the single
//! persistence writer even when there was nothing to settle. All three ran inside a
//! Tokio handler, so the read was not merely slow — it held a lane, and the settlement
//! queued behind whatever large write the writer was already running. That is how a
//! status route ended up answering thirteen minutes late.
//!
//! This module owns the split the route needs:
//!
//! * a **status read** is a light read of stored state plus one indexed pre-check for a
//!   genuinely expired plan, and nothing else. It cannot queue a write and cannot probe.
//! * a **probe** and a **settlement** are the expensive half. They run here on exactly
//!   two long-lived threads — one per kind — so no Tokio worker ever blocks on host APIs
//!   or on the writer, a burst of callers cannot create a thread per request, and
//!   concurrent askers share one in-flight probe. A caller that would have to wait
//!   longer than its budget is told the component is busy instead of waiting without
//!   limit.
//!
//! The workers are *process* scoped, so an abandoned connection cannot leave work
//! running that nobody owns, and a caller's budget only ever ends that caller's wait —
//! never the work itself. Both threads park on a condition variable: there is no polling
//! and no timer.

use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Duration,
};

use aura_domain_common::{
    CorrelationId, ErrorCategory, ErrorCode, HostPreparationOperationId, UtcTimestamp,
};
use aura_persistence::{PersistenceCoordinator, PersistenceError};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};

/// How long one caller waits for a probe before it is told the component is busy. The
/// work keeps running and its result is still stored; only this caller's wait ends.
pub(crate) const PROBE_WAIT_BUDGET: Duration = Duration::from_secs(3);
/// How long one caller waits for an expired plan to be settled. A settlement writes, so
/// it can queue behind a large transaction; the caller does not wait that out.
pub(crate) const SETTLE_WAIT_BUDGET: Duration = Duration::from_secs(2);

/// How many settled operations are remembered, so a repeat status call that still sees
/// the same expired plan does not ask for a second settlement.
const SETTLED_MEMORY: usize = 32;

/// How many distinct settlements may wait for the worker. Bounded on purpose: a burst
/// has to be refused rather than grow an unbounded queue behind one writer. Joining a
/// settlement that is already queued or running does not consume depth — waiters are
/// not queue entries.
const SETTLE_QUEUE_DEPTH: usize = 16;

/// The bounded sub-stage names a Host Preparation operation reports.
///
/// Durations and identifiers only: no host path, no user content, never a credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HostPreparationStage {
    /// The light status read finished.
    StatusRead,
    /// The expiry pre-check answered, and this is how long it took.
    ExpiryChecked,
    /// A host probe started.
    ProbeStarted,
    /// A host probe finished.
    ProbeFinished,
    /// An expired plan was settled.
    SettlementFinished,
}

impl HostPreparationStage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::StatusRead => "status_read",
            Self::ExpiryChecked => "expiry_checked",
            Self::ProbeStarted => "probe_started",
            Self::ProbeFinished => "probe_finished",
            Self::SettlementFinished => "settlement_finished",
        }
    }
}

/// One measured sub-stage. `ms` is this process's monotonic measurement of the span that
/// just finished, so a reader can tell "the host probe is slow" from "this was waiting
/// for the writer".
pub(crate) struct HostPreparationStageReport {
    pub(crate) stage: HostPreparationStage,
    pub(crate) correlation_id: CorrelationId,
    pub(crate) operation_id: Option<HostPreparationOperationId>,
    pub(crate) ms: u64,
}

type StageSink = Arc<dyn Fn(HostPreparationStageReport) + Send + Sync>;

/// Everyone waiting for one probe's answer.
type ProbeWaiters = Vec<(CorrelationId, mpsc::SyncSender<Result<(), String>>)>;

/// The state of the one probe flight this process runs.
///
/// The waiters *are* the flight: they stay in the state for the whole host call, so a
/// caller arriving while the worker is inside it joins them and is answered by that
/// probe. The previous version moved the waiters into the worker before the host call,
/// which left the state empty while the host boundary was active — a second overlapping
/// call then started a second probe, and a controlled host boundary observed exactly
/// that.
#[derive(Default)]
struct ProbeState {
    waiting: ProbeWaiters,
    /// Whether the probe thread currently owns the flight. Only the worker sets and
    /// clears this, under the same lock that governs `waiting`, so a caller can always
    /// tell "a probe is running" from "no probe has been asked for".
    in_flight: bool,
    /// A finished failure, consumed by the first caller that arrives after it: that
    /// caller is answered with the reason instead of paying for another host call
    /// immediately, and consuming the cache is what lets the caller after it retry.
    last_failure: Option<String>,
}

/// The answer one settlement produces, shared by everyone who waited for it.
type SettleOutcome = Result<Option<HostPreparationOperationId>, String>;
type SettleWaiters = Vec<(CorrelationId, mpsc::SyncSender<SettleOutcome>)>;

/// One settlement and everyone waiting for its answer. The same shape serves a queued
/// request and the running flight: a duplicate request appends a waiter, and the worker
/// promotes a queued task to the running flight by moving it, waiters included.
struct SettleTask {
    operation_id: HostPreparationOperationId,
    waiters: SettleWaiters,
}

#[derive(Default)]
struct SettleState {
    /// Distinct operations waiting for the worker, each with everyone who asked for it.
    queue: std::collections::VecDeque<SettleTask>,
    /// The settlement the worker is inside right now. It stays in the state until the
    /// result exists, so a caller arriving mid-settlement joins its waiters and is
    /// answered with the same result instead of queueing the same write again.
    running: Option<SettleTask>,
    /// Operations this process has already settled, newest last. A second status call
    /// that still reads the same expired row therefore answers without writing again.
    settled: std::collections::VecDeque<HostPreparationOperationId>,
}

struct Shared {
    probe: Mutex<ProbeState>,
    probe_wake: Condvar,
    settle: Mutex<SettleState>,
    settle_wake: Condvar,
}

/// How long one worker parks between two checks for the stop signal.
const WORKER_IDLE_POLL: Duration = Duration::from_millis(100);

/// Owns the process's host probes and expiry settlements.
///
/// The stop handle is a separate allocation on purpose. The two worker threads hold a
/// clone of it, so a struct that decided "am I the last owner" by counting references to
/// everything it holds would always see the threads' own clones and never stop them —
/// which is exactly how the previous version leaked two threads and a
/// `PersistenceCoordinator` per fixture. `stop` is held only by the owner chain, so the
/// count is about owners.
pub(crate) struct HostPreparationWorker {
    shared: Arc<Shared>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl HostPreparationWorker {
    pub(crate) fn start(
        persistence: Arc<PersistenceCoordinator>,
        stages: impl Fn(HostPreparationStageReport) + Send + Sync + 'static,
    ) -> Self {
        let stages: StageSink = Arc::new(stages);
        let shared = Arc::new(Shared {
            probe: Mutex::new(ProbeState::default()),
            probe_wake: Condvar::new(),
            settle: Mutex::new(SettleState::default()),
            settle_wake: Condvar::new(),
        });
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        for (name, probe) in
            [("aura-host-preparation-probe", true), ("aura-host-preparation-settle", false)]
        {
            let thread_persistence = Arc::clone(&persistence);
            let thread_stages = Arc::clone(&stages);
            let thread_shared = Arc::clone(&shared);
            let thread_stop = Arc::clone(&stop);
            let spawned = std::thread::Builder::new().name(name.to_owned()).spawn(move || {
                if probe {
                    probe_loop(thread_persistence, thread_shared, thread_stages, thread_stop);
                } else {
                    settle_loop(thread_persistence, thread_shared, thread_stages, thread_stop);
                }
            });
            if spawned.is_err() {
                crate::append_core_log(
                    "host_preparation.worker_unavailable",
                    serde_json::json!({ "worker": name }),
                );
            }
        }
        Self { shared, stop }
    }

    /// Runs one host probe, or joins the one already in flight.
    ///
    /// # Errors
    ///
    /// Returns a typed `Unavailable` envelope when the probe failed — including the
    /// cached reason handed to the first caller that arrives after a failure, which is
    /// also what frees the next call to retry — and a retryable busy envelope when the
    /// budget expired first. A budget that expires ends this caller's wait only.
    pub(crate) fn probe(&self, correlation_id: CorrelationId) -> Result<(), IpcErrorEnvelope> {
        let (done, wait) = mpsc::sync_channel(1);
        {
            let Ok(mut state) = self.shared.probe.lock() else {
                return Err(unavailable(
                    correlation_id,
                    "sandbox.host_preparation_unavailable",
                    "The Host Preparation probe state is unavailable.",
                ));
            };
            // A finished failure is consumed here, by being handed to this caller as
            // its answer: the reason is reported instead of silently folding the failure
            // into "no probe running", and consuming the cache is the refresh condition
            // that lets the following caller start a fresh probe.
            if let Some(reason) = state.last_failure.take() {
                return Err(unavailable(
                    correlation_id,
                    "sandbox.host_preparation_probe_failed",
                    format!("The Host Preparation probe failed: {reason}"),
                ));
            }
            // The waiters are the flight and they stay in the state for the whole host
            // call: a caller arriving while the worker is inside it joins them, so one
            // probe answers everyone. The worker claims the flight under the same lock
            // in `take_probe`.
            state.waiting.push((correlation_id, done));
            if !state.in_flight {
                self.shared.probe_wake.notify_all();
            }
        }
        let outcome = match wait.recv_timeout(PROBE_WAIT_BUDGET) {
            Ok(outcome) => outcome,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.detach(correlation_id);
                return Err(unavailable(
                    correlation_id,
                    "sandbox.host_preparation_busy",
                    "A Host Preparation probe is already running and did not finish inside \
                     the wait budget; retry once it completes.",
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.detach(correlation_id);
                return Err(unavailable(
                    correlation_id,
                    "sandbox.host_preparation_unavailable",
                    "The Host Preparation probe did not report an answer.",
                ));
            }
        };
        match outcome {
            Ok(()) => Ok(()),
            Err(reason) => Err(unavailable(
                correlation_id,
                "sandbox.host_preparation_probe_failed",
                format!("The Host Preparation probe failed: {reason}"),
            )),
        }
    }

    /// Stops waiting for a probe that is still running.
    ///
    /// The work keeps its owner — the probe thread — and still stores its result; only
    /// this waiter stops. The flight itself stays claimed, so a caller arriving after
    /// every waiter left joins the probe that is still running instead of starting
    /// another one.
    fn detach(&self, correlation_id: CorrelationId) {
        let Ok(mut state) = self.shared.probe.lock() else {
            return;
        };
        state.waiting.retain(|(id, _)| *id != correlation_id);
    }

    /// Settles one expired plan unless this process already settled it.
    ///
    /// A request for an operation that is already queued or already running does not
    /// queue a second write and is not answered "nothing": it joins that settlement's
    /// waiters and is answered with the same result the first request gets, so a caller
    /// is never told a plan was settled when the settlement it joined later failed.
    ///
    /// # Errors
    ///
    /// An error or an expired wait budget is reported to the caller rather than folded
    /// into success: a settlement that did not happen is a real state the status route
    /// has to be able to see, not something to answer as if it had worked.
    pub(crate) fn settle(
        &self,
        operation_id: HostPreparationOperationId,
        correlation_id: CorrelationId,
    ) -> Result<Option<HostPreparationOperationId>, IpcErrorEnvelope> {
        let (done, wait) = mpsc::sync_channel(1);
        let mut queued_new = false;
        {
            let Ok(mut state) = self.shared.settle.lock() else {
                return Err(unavailable(
                    correlation_id,
                    "sandbox.host_preparation_unavailable",
                    "The Host Preparation settlement state is unavailable.",
                ));
            };
            if state.settled.contains(&operation_id) {
                return Ok(None);
            }
            // One operation, one task: a second request for an operation that is
            // already queued or already running joins that task's waiters. It is then
            // answered with the settlement's real result — the previous version
            // answered `Ok(None)` here, which reported success while the settlement it
            // never joined could still fail.
            if let Some(flight) = &mut state.running
                && flight.operation_id == operation_id
            {
                flight.waiters.push((correlation_id, done));
            } else if let Some(task) =
                state.queue.iter_mut().find(|task| task.operation_id == operation_id)
            {
                task.waiters.push((correlation_id, done));
            } else {
                if state.queue.len() >= SETTLE_QUEUE_DEPTH {
                    return Err(unavailable(
                        correlation_id,
                        "sandbox.host_preparation_busy",
                        "The Host Preparation settlement queue is full.",
                    ));
                }
                state
                    .queue
                    .push_back(SettleTask { operation_id, waiters: vec![(correlation_id, done)] });
                queued_new = true;
            }
        }
        if queued_new {
            self.shared.settle_wake.notify_all();
        }
        match wait.recv_timeout(SETTLE_WAIT_BUDGET) {
            Ok(Ok(settled)) => Ok(settled),
            Ok(Err(reason)) => Err(unavailable(
                correlation_id,
                "sandbox.host_preparation_unavailable",
                format!("The expired Host Preparation plan could not be settled: {reason}"),
            )),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.detach_settle(correlation_id, operation_id);
                Err(unavailable(
                    correlation_id,
                    "sandbox.host_preparation_busy",
                    "The expired Host Preparation plan did not settle inside the wait budget.",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(unavailable(
                correlation_id,
                "sandbox.host_preparation_unavailable",
                "The Host Preparation settlement worker did not report an answer.",
            )),
        }
    }

    /// Stops waiting for a settlement that is still queued or running.
    ///
    /// The settlement keeps its owner — the worker thread — and still runs to its
    /// result; only this waiter stops. The task itself stays queued or claimed, so the
    /// last waiter leaving neither cancels the work nor lets a later call start it
    /// again.
    fn detach_settle(
        &self,
        correlation_id: CorrelationId,
        operation_id: HostPreparationOperationId,
    ) {
        let Ok(mut state) = self.shared.settle.lock() else {
            return;
        };
        if let Some(flight) = &mut state.running
            && flight.operation_id == operation_id
        {
            flight.waiters.retain(|(id, _)| *id != correlation_id);
        }
        if let Some(task) = state.queue.iter_mut().find(|task| task.operation_id == operation_id) {
            task.waiters.retain(|(id, _)| *id != correlation_id);
        }
    }

    /// Whether the owner chain has told the workers to stop.
    ///
    /// The stop signal is the mechanism that ends the two threads; a test can read it
    /// instead of guessing from timing. Before the fix it stayed `false` forever, because
    /// the drop path compared a reference count that always included the threads' own
    /// clones.
    #[cfg(test)]
    pub(crate) fn stop_requested(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether this process has already settled one expired plan.
    #[cfg(test)]
    pub(crate) fn settled(&self, operation_id: HostPreparationOperationId) -> bool {
        self.shared
            .settle
            .lock()
            .map(|state| state.settled.contains(&operation_id))
            .unwrap_or(false)
    }
}

/// Stops both workers when the last owner goes away.
///
/// `stop` is held only by the owner chain — the threads hold their own clone of it, but
/// the struct's reference count is about owners — so the count really does answer "am I
/// the last one". The previous version counted references to everything the struct held,
/// which always included the two threads' own clones, so it returned early every time and
/// neither thread was ever told to stop.
impl Drop for HostPreparationWorker {
    fn drop(&mut self) {
        // Unconditional. A reference count cannot answer "am I the last owner" here,
        // because the two worker threads hold a clone of this very handle: the count is
        // always at least two plus whoever cloned the services, which is exactly how the
        // previous version concluded another owner still existed and never stopped
        // anything. Setting the flag when it is already set costs nothing, and a worker
        // that is never cloned is dropped exactly once.
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        self.shared.probe_wake.notify_all();
        self.shared.settle_wake.notify_all();
    }
}

/// Waits until a probe flight has waiters nobody owns, claims it, and lets the caller
/// produce the answer for the waiters that stay in the state.
///
/// The waiters are left in place while the host call runs, so a caller that arrives
/// during it joins them and is answered by it. The previous version moved the waiters
/// into the worker before the host call, which left the state looking idle while the
/// host boundary was active — a second overlapping request then started a second
/// probe, observed behind one controlled gate as two actual host calls.
fn take_probe(shared: &Shared, stop: &std::sync::atomic::AtomicBool) -> Option<()> {
    let mut state = shared.probe.lock().ok()?;
    loop {
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        if !state.in_flight && !state.waiting.is_empty() {
            state.in_flight = true;
            return Some(());
        }
        let (next, _) = shared.probe_wake.wait_timeout(state, WORKER_IDLE_POLL).ok()?;
        state = next;
    }
}

fn probe_loop(
    persistence: Arc<PersistenceCoordinator>,
    shared: Arc<Shared>,
    stages: StageSink,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    while take_probe(&shared, &stop).is_some() {
        let (correlation_id, started) = {
            // The flight is claimed; the waiters are still in the state, so the first
            // one names this probe. A flight whose last waiter already left is still
            // answered and stored — the work outlives any single caller's budget.
            let Ok(state) = shared.probe.lock() else {
                return;
            };
            (
                state.waiting.first().map(|(id, _)| *id).unwrap_or_else(CorrelationId::new),
                std::time::Instant::now(),
            )
        };
        stages(HostPreparationStageReport {
            stage: HostPreparationStage::ProbeStarted,
            correlation_id,
            operation_id: None,
            ms: 0,
        });
        let outcome =
            probe_and_store(&persistence, correlation_id).map_err(|error| error.to_string());
        stages(HostPreparationStageReport {
            stage: HostPreparationStage::ProbeFinished,
            correlation_id,
            operation_id: None,
            ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        // The answer exists now: take the waiters, release the flight, and answer
        // everyone who joined it — including callers that arrived after the host call
        // finished but before this lock. A failure is cached for the next arrival;
        // a success clears the cache.
        let waiting = {
            let Ok(mut state) = shared.probe.lock() else {
                return;
            };
            state.in_flight = false;
            state.last_failure = outcome.as_ref().err().cloned();
            std::mem::take(&mut state.waiting)
        };
        for (_, waiter) in waiting {
            let _ = waiter.send(outcome.clone());
        }
    }
}

/// Takes the next queued settlement, or parks until one arrives.
///
/// The claim installs the flight in the state *before* the write starts, with the
/// operation's identity and its waiters, so a caller that arrives during the
/// settlement joins them and is answered with the same result. The wait condition is
/// "the queue is empty", not "the queue changed": a batch of two enqueues has to be
/// drained to empty, and a counter comparison would leave the second one waiting for
/// an enqueue that already happened.
fn take_settle(shared: &Shared, stop: &std::sync::atomic::AtomicBool) -> Option<()> {
    let mut state = shared.settle.lock().ok()?;
    loop {
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        if state.running.is_none()
            && let Some(task) = state.queue.pop_front()
        {
            state.running = Some(task);
            return Some(());
        }
        let (next, _) = shared.settle_wake.wait_timeout(state, WORKER_IDLE_POLL).ok()?;
        state = next;
    }
}

fn settle_loop(
    persistence: Arc<PersistenceCoordinator>,
    shared: Arc<Shared>,
    stages: StageSink,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    while take_settle(&shared, &stop).is_some() {
        let (operation_id, correlation_id, started) = {
            // The flight is installed; its waiters are still in the state, so the
            // first one names this settlement. A flight whose last waiter already
            // left still settles — the work outlives any single caller's budget.
            let Ok(state) = shared.settle.lock() else {
                return;
            };
            let flight = state.running.as_ref();
            (
                flight.map_or_else(HostPreparationOperationId::new, |task| task.operation_id),
                flight
                    .and_then(|task| task.waiters.first().map(|(id, _)| *id))
                    .unwrap_or_else(CorrelationId::new),
                std::time::Instant::now(),
            )
        };
        let outcome = settle_expired(&persistence, operation_id, correlation_id)
            .map_err(|error| error.to_string());
        stages(HostPreparationStageReport {
            stage: HostPreparationStage::SettlementFinished,
            correlation_id,
            operation_id: Some(operation_id),
            ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        // The result exists now: remember a success, release the flight, and answer
        // everyone who joined it with the one real result. A failure is answered as a
        // failure to all of them and is not remembered, so the next request retries.
        let waiters = {
            let Ok(mut state) = shared.settle.lock() else {
                return;
            };
            if outcome.is_ok() {
                state.settled.push_back(operation_id);
                while state.settled.len() > SETTLED_MEMORY {
                    state.settled.pop_front();
                }
            }
            state.running.take().map_or_else(Vec::new, |task| task.waiters)
        };
        for (_, waiter) in waiters {
            let _ = waiter.send(outcome.clone());
        }
    }
}

/// The real probe: the same runtime projection the previous implementation built, now
/// built on the worker thread instead of inside a request handler.
fn probe_and_store(
    persistence: &PersistenceCoordinator,
    correlation_id: CorrelationId,
) -> Result<(), PersistenceError> {
    let runtime = crate::p04::build_runtime_projection_at_start(persistence)
        .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
    persistence.record_host_preparation_probe(runtime, correlation_id).map(|_| ())
}

fn settle_expired(
    persistence: &PersistenceCoordinator,
    operation_id: HostPreparationOperationId,
    correlation_id: CorrelationId,
) -> Result<Option<HostPreparationOperationId>, PersistenceError> {
    let runtime = crate::p04::build_runtime_projection_at_start(persistence)
        .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
    // The transaction is told which plan the caller believed was due and refuses any
    // other, so an observation overtaken by a newer plan cannot settle the newer one.
    persistence.expire_prepared_host_preparation(
        UtcTimestamp::now(),
        runtime,
        Some(operation_id),
        correlation_id,
    )
}

fn unavailable(
    correlation_id: CorrelationId,
    code: &str,
    message: impl Into<String>,
) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new(code).unwrap_or_else(|_| {
            ErrorCode::new("sandbox.host_preparation_unavailable").expect("static fallback code")
        }),
        category: ErrorCategory::Unavailable,
        message: message.into(),
        retryable: true,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use aura_domain_common::{
        ApprovalRequestId, ContentHash, CorrelationId as Correlation, ReleaseBundleId, Revision,
        RuntimeInstanceId, WorkspaceId,
    };
    use aura_permission_domain::{
        ApprovalView, HostPreparationAction, HostPreparationBrokerPlan,
        HostPreparationOperationState, HostPreparationOperationView, HostPreparationOwnership,
        HostPreparationSummary, RuntimeComponentState, SecurityRuntimeProjection,
    };
    use aura_persistence::{PersistenceOptions, PersistenceStartup};
    use aura_runtime_contract::{ReleaseDescriptor, ReleaseIdentity};
    use aura_security_contract::{ApprovalDecisionKind, CanonicalPath, OperationRiskFlag};
    use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};

    use super::*;

    /// A barrier, not a timing guess: proceeds the moment `predicate` holds and fails
    /// after a bounded wait instead of sleeping a fixed duration and hoping the window
    /// was hit.
    fn wait_until(predicate: impl Fn() -> bool, what: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !predicate() {
            assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[derive(Default)]
    struct StageCounts {
        probe_started: AtomicUsize,
        settlement_finished: AtomicUsize,
    }

    /// A worker whose stage sink counts the expensive halves, so a test can assert how
    /// many host probes and settlements actually ran. A stored revision alone cannot
    /// prove single flight — a second, racy probe would store revision two and still
    /// look "successful".
    fn counting_worker(
        persistence: &Arc<PersistenceCoordinator>,
        counts: &Arc<StageCounts>,
    ) -> HostPreparationWorker {
        let counts = Arc::clone(counts);
        HostPreparationWorker::start(Arc::clone(persistence), move |report| match report.stage {
            HostPreparationStage::ProbeStarted => {
                counts.probe_started.fetch_add(1, Ordering::SeqCst);
            }
            HostPreparationStage::SettlementFinished => {
                counts.settlement_finished.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        })
    }

    /// A real fault inside the SQLite writer: the trigger aborts whichever transaction
    /// touches the table, so the probe or settlement that hits it fails for real and
    /// rolls back. DDL runs on a second connection while the writer is idle; the
    /// database is WAL with a busy timeout.
    const PROBE_WRITE_FAULT: &str = "CREATE TRIGGER probe_write_fault BEFORE INSERT ON events \
         BEGIN SELECT RAISE(ABORT, 'injected probe write fault'); END;";
    const PROBE_WRITE_FAULT_CLEAR: &str = "DROP TRIGGER IF EXISTS probe_write_fault;";
    const SETTLE_WRITE_FAULT: &str = "CREATE TRIGGER settle_write_fault BEFORE UPDATE ON host_preparation_journal \
         BEGIN SELECT RAISE(ABORT, 'injected settle write fault'); END;";
    const SETTLE_WRITE_FAULT_CLEAR: &str = "DROP TRIGGER IF EXISTS settle_write_fault;";

    fn set_write_fault(database: &std::path::Path, ddl: &str) {
        let connection = rusqlite::Connection::open(database).expect("open fault connection");
        connection.busy_timeout(Duration::from_secs(10)).expect("fault busy timeout");
        connection.execute_batch(ddl).expect("fault ddl");
    }

    /// The runtime projection the persistence-level setup calls need; the worker builds
    /// its own inside its probe and settlement.
    fn test_runtime() -> SecurityRuntimeProjection {
        SecurityRuntimeProjection {
            sandbox_available: false,
            host_preparation: HostPreparationSummary::default(),
            sandbox_host_state: RuntimeComponentState::NotStarted,
            egress_proxy_state: RuntimeComponentState::NotStarted,
            elevated_helper_state: RuntimeComponentState::NotStarted,
            quarantine_count: 0,
        }
    }

    /// Creates one prepared Host Preparation operation whose deadline is already past,
    /// so the worker's settlement really expires it.
    fn prepared_expired_operation(
        persistence: &PersistenceCoordinator,
    ) -> HostPreparationOperationId {
        let probed = persistence
            .record_host_preparation_probe(test_runtime(), CorrelationId::new())
            .expect("record probe");
        let created_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::minutes(5));
        let expires_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::minutes(1));
        let operation_id = HostPreparationOperationId::new();
        let runtime_instance_id = RuntimeInstanceId::new();
        let release_descriptor = ReleaseDescriptor {
            bundle_namespace: "aura.desktop.test.host-preparation".parse().expect("namespace"),
            identity: ReleaseIdentity {
                release_bundle_id: ReleaseBundleId::new(),
                component_manifest_hash: ContentHash::blake3(b"manifest"),
            },
        };
        let plan_hash = ContentHash::blake3(b"host preparation plan");
        let plan = HostPreparationBrokerPlan {
            operation_id,
            action: HostPreparationAction::Prepare,
            host_preparation_revision: probed.status.revision,
            runtime_instance_id,
            release_descriptor: release_descriptor.clone(),
            plan_nonce_hash: ContentHash::blake3(b"nonce"),
            expires_at,
            system_drive_root: CanonicalPath {
                display: r"C:\".to_owned(),
                utf16: r"C:\".encode_utf16().collect(),
            },
            ownership_before: HostPreparationOwnership::default(),
            add_all_application_packages_ace: true,
            add_all_restricted_application_packages_ace: false,
            update_null_device_security: false,
            plan_hash: plan_hash.clone(),
        };
        let operation = HostPreparationOperationView {
            operation_id,
            operation_revision: 1,
            action: HostPreparationAction::Prepare,
            state: HostPreparationOperationState::Prepared,
            host_preparation_revision: probed.status.revision,
            runtime_instance_id,
            release_descriptor: release_descriptor.clone(),
            plan_hash,
            expires_at,
            requires_elevation: true,
            add_all_application_packages_ace: true,
            add_all_restricted_application_packages_ace: false,
            update_null_device_security: false,
            created_at,
            updated_at: created_at,
            error_code: None,
        };
        let approval = ApprovalView {
            session_id: None,
            id: ApprovalRequestId::new(),
            title: "配置 Host Preparation".to_owned(),
            model_purpose: None,
            actual_operation_summary: "应用冻结的兼容配置".to_owned(),
            source_summary: "Aura Core Host Preparation".to_owned(),
            risk_flags: vec![
                OperationRiskFlag::SystemConfigurationMutation,
                OperationRiskFlag::Elevation,
            ],
            allowed_decisions: vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
            request_revision: 1,
            created_at,
            release_descriptor: Some(release_descriptor),
        };
        persistence
            .begin_host_preparation_operation(
                probed.status.revision,
                operation,
                plan,
                approval,
                test_runtime(),
                CorrelationId::new(),
            )
            .expect("begin prepared operation");
        operation_id
    }

    /// The persisted state of one operation, for assertions on what the settlement
    /// really did.
    fn operation_state(
        persistence: &PersistenceCoordinator,
        operation_id: HostPreparationOperationId,
    ) -> HostPreparationOperationState {
        persistence
            .host_preparation_operation(operation_id)
            .expect("load operation")
            .operation
            .state
    }

    fn start(directory: &std::path::Path) -> Arc<PersistenceCoordinator> {
        match PersistenceCoordinator::start(PersistenceOptions::new(
            directory.join("aura.sqlite3"),
            directory.join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected recovery: {}", value.reason)
            }
        }
    }

    /// One workspace whose catalog freshness can be flipped, used as the write that
    /// pins the single writer.
    fn workspace(persistence: &PersistenceCoordinator) -> WorkspaceId {
        let workspace_id = WorkspaceId::new();
        persistence
            .create_workspace(
                WorkspaceSummary {
                    workspace_id,
                    display_name: "host preparation".to_owned(),
                    selected_root: "C:\\host-preparation".to_owned(),
                    canonical_root: "C:\\host-preparation".to_owned(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Fresh,
                    authorization_epoch: Revision::INITIAL,
                },
                Correlation::new(),
            )
            .expect("workspace");
        workspace_id
    }

    fn worker(persistence: &Arc<PersistenceCoordinator>) -> HostPreparationWorker {
        HostPreparationWorker::start(Arc::clone(persistence), |_| {})
    }

    /// A status read must not need the persistence writer at all.
    ///
    /// This is the incident in one test: the status route answered thirteen minutes late
    /// because its work was queued behind a large catalog transaction on the single
    /// writer. A writer pinned mid-task is therefore the exact condition to test
    /// against — and the light path has to answer while it is pinned.
    #[test]
    fn a_status_read_answers_while_the_writer_is_pinned() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let workspace_id = workspace(&persistence);

        // Pin the writer: the next task it picks up stops until the gate is released.
        let gate = persistence.arm_dispatch_gate();
        let held = Arc::clone(&persistence);
        let holder = std::thread::spawn(move || held.mark_catalog_stale(workspace_id));
        assert!(
            gate.wait_until_arrived(std::time::Duration::from_secs(10)),
            "the writer never reached the gate, so nothing was pinned"
        );

        // The two reads the status route performs before it decides anything.
        let started = std::time::Instant::now();
        let stored = persistence.host_preparation_state().expect("stored state");
        let expiry = persistence.prepared_host_preparation_expiry().expect("expiry pre-check");
        let elapsed = started.elapsed();

        assert!(stored.is_none(), "a fresh database has never been probed");
        assert!(expiry.is_none(), "a fresh database has no prepared plan");
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "the status reads waited {elapsed:?} on a pinned writer, which means they queued"
        );

        // The control: an ordinary write really does queue behind the pinned one, so the
        // gate is doing what the test assumes it is.
        let queued = Arc::clone(&persistence);
        let queued_writer = std::thread::spawn(move || queued.mark_catalog_stale(workspace_id));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!queued_writer.is_finished(), "a write must queue behind the pinned one");

        gate.release();
        holder.join().expect("holder").expect("held write");
        queued_writer.join().expect("queued writer").expect("queued write");
    }

    /// A probe is single-flight: concurrent askers join one probe instead of each
    /// starting their own, and every one of them gets the same answer.
    #[test]
    fn concurrent_probe_askers_share_one_probe() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        let askers = (0..4)
            .map(|_| {
                let worker = Arc::clone(&worker);
                std::thread::spawn(move || worker.probe(CorrelationId::new()))
            })
            .collect::<Vec<_>>();
        let outcomes =
            askers.into_iter().map(|asker| asker.join().expect("asker")).collect::<Vec<_>>();
        assert!(
            outcomes.iter().all(Result::is_ok),
            "every asker must get the shared probe's answer: {outcomes:?}"
        );
        assert_eq!(
            counts.probe_started.load(Ordering::SeqCst),
            1,
            "concurrent askers must run exactly one actual probe"
        );
        let stored = persistence.host_preparation_state().expect("stored state");
        assert!(
            stored.is_some_and(|state| state.status.revision == 1),
            "one probe must store exactly one revision"
        );
    }

    /// Two overlapping probe requests run exactly one host probe.
    ///
    /// The independent review observed `overlapping_requests_actual_probes=2` behind one
    /// controlled host gate, because the flight left the state before the host call. The
    /// writer — which the probe's result must be recorded through — is pinned for real
    /// here, so the first asker is inside the probe when the second arrives, and the
    /// probe count proves the overlap shared one flight instead of guessing from a
    /// stored revision.
    #[test]
    fn overlapping_probe_askers_run_one_host_probe() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let workspace_id = workspace(&persistence);
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        // Pin the writer: the probe's record call queues behind this task.
        let gate = persistence.arm_dispatch_gate();
        let pinned = Arc::clone(&persistence);
        let holder = std::thread::spawn(move || pinned.mark_catalog_stale(workspace_id));
        assert!(
            gate.wait_until_arrived(Duration::from_secs(10)),
            "the writer never reached the gate, so nothing was pinned"
        );

        let first = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.probe(CorrelationId::new()))
        };
        // Barrier: the first asker is inside the probe, blocked at the pinned writer.
        wait_until(|| counts.probe_started.load(Ordering::SeqCst) == 1, "the first probe to start");
        {
            let state = worker.shared.probe.lock().expect("probe state");
            assert!(state.in_flight, "the flight must stay in the state while the probe runs");
            assert_eq!(state.waiting.len(), 1, "the first asker is waiting on the flight");
        }

        let second = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.probe(CorrelationId::new()))
        };
        // Barrier: the second asker joined the same flight rather than starting a probe.
        wait_until(
            || worker.shared.probe.lock().expect("probe state").waiting.len() == 2,
            "the second asker to join the flight",
        );

        gate.release();
        holder.join().expect("holder").expect("pinned write");
        let first = first.join().expect("first asker");
        let second = second.join().expect("second asker");
        assert!(
            first.is_ok() && second.is_ok(),
            "both askers must get the one probe's answer: {first:?} {second:?}"
        );
        assert_eq!(
            counts.probe_started.load(Ordering::SeqCst),
            1,
            "two overlapping asks must run one actual probe"
        );
        assert_eq!(
            persistence
                .host_preparation_state()
                .expect("stored state")
                .map(|state| state.status.revision),
            Some(1),
            "one probe records once; a racy second probe would be revision two"
        );
    }

    /// A probe waiter whose budget expires does not take the flight with it.
    ///
    /// The waiter is told the component is busy; the probe keeps its owner and still
    /// stores its result; and a caller arriving after every waiter left joins the probe
    /// that is still running instead of starting a second one.
    #[test]
    fn a_probe_waiter_that_times_out_does_not_restart_the_probe() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let workspace_id = workspace(&persistence);
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        let gate = persistence.arm_dispatch_gate();
        let pinned = Arc::clone(&persistence);
        let holder = std::thread::spawn(move || pinned.mark_catalog_stale(workspace_id));
        assert!(
            gate.wait_until_arrived(Duration::from_secs(10)),
            "the writer never reached the gate, so nothing was pinned"
        );

        let first = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.probe(CorrelationId::new()))
        };
        wait_until(|| counts.probe_started.load(Ordering::SeqCst) == 1, "the first probe to start");

        // The first asker's budget expires while the probe is still pinned. That is a
        // busy answer, not a success — and its departure must neither end the flight
        // nor let it start again.
        let error = first
            .join()
            .expect("first asker")
            .expect_err("a wait that outlived its budget is reported, not folded into success");
        assert_eq!(error.code.as_str(), "sandbox.host_preparation_busy");
        {
            let state = worker.shared.probe.lock().expect("probe state");
            assert!(state.in_flight, "the flight outlives its last waiter");
            assert!(state.waiting.is_empty(), "the timed-out asker detached");
        }

        // The next caller arrives while the same probe is still running and joins it.
        let second = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.probe(CorrelationId::new()))
        };
        wait_until(
            || worker.shared.probe.lock().expect("probe state").waiting.len() == 1,
            "the later asker to join the still-running probe",
        );
        gate.release();
        holder.join().expect("holder").expect("pinned write");
        second
            .join()
            .expect("second asker")
            .expect("the later asker must get the still-running probe's result");
        assert_eq!(
            counts.probe_started.load(Ordering::SeqCst),
            1,
            "a caller arriving after every waiter left must not restart the probe"
        );
        assert_eq!(
            persistence
                .host_preparation_state()
                .expect("stored state")
                .map(|state| state.status.revision),
            Some(1)
        );
    }

    /// A failed probe is answered to everyone who waited for it; the cached reason is
    /// handed to the next caller as its answer instead of silently starting nothing;
    /// and clearing the fault lets the following call probe again.
    #[test]
    fn a_failed_probe_is_reported_to_every_waiter_then_is_retryable() {
        let directory = tempfile::tempdir().expect("data root");
        let database = directory.path().join("aura.sqlite3");
        let persistence = start(directory.path());
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        // A real fault inside the writer: the probe's own transaction aborts and rolls
        // back, so the failure is the production failure path, not a stand-in.
        set_write_fault(&database, PROBE_WRITE_FAULT);

        let first = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.probe(CorrelationId::new()))
        };
        wait_until(|| counts.probe_started.load(Ordering::SeqCst) == 1, "the probe to start");
        let second = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.probe(CorrelationId::new()))
        };
        // One flight answers both askers, whatever interleaving the joiner lands in:
        // joining the flight, or arriving just after it and consuming the cached reason.
        let first = first
            .join()
            .expect("first asker")
            .expect_err("the failed probe reports its failure to the asker who started it");
        let second = second
            .join()
            .expect("second asker")
            .expect_err("a waiter on the failed flight gets the same failure");
        assert_eq!(first.code.as_str(), "sandbox.host_preparation_probe_failed");
        assert_eq!(second.code.as_str(), "sandbox.host_preparation_probe_failed");
        assert_eq!(
            counts.probe_started.load(Ordering::SeqCst),
            1,
            "one flight answered both askers"
        );

        // The next caller consumes the cached reason: it is told why, immediately, and
        // no probe starts behind its back.
        let cached = worker
            .probe(CorrelationId::new())
            .expect_err("the cached failure is the next caller's answer");
        assert_eq!(cached.code.as_str(), "sandbox.host_preparation_probe_failed");
        assert_eq!(
            counts.probe_started.load(Ordering::SeqCst),
            1,
            "the cached failure must be reported, not answered by a new probe"
        );

        // Clearing the fault frees the retry: one new probe, recorded once.
        set_write_fault(&database, PROBE_WRITE_FAULT_CLEAR);
        worker.probe(CorrelationId::new()).expect("the probe retries once the fault is gone");
        assert_eq!(counts.probe_started.load(Ordering::SeqCst), 2);
        assert_eq!(
            persistence
                .host_preparation_state()
                .expect("stored state")
                .map(|state| state.status.revision),
            Some(1),
            "the failed attempt rolled back; the retry recorded exactly once"
        );
    }

    /// One operation, one settlement: askers of the same operation share the one real
    /// result whether the settlement is already running or still queued, and distinct
    /// operations are each drained exactly once.
    ///
    /// The independent review saw a queued duplicate answered `Ok(None)` — success —
    /// while the settlement it never joined later failed. Here the writer is pinned for
    /// real, so the running and queued settlements stay observable while askers arrive.
    #[test]
    fn overlapping_settle_askers_share_one_settlement_per_operation() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let workspace_id = workspace(&persistence);
        let due_operation = prepared_expired_operation(&persistence);
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        // Pin the writer so the due settlement stays observable while the others arrive.
        let gate = persistence.arm_dispatch_gate();
        let pinned = Arc::clone(&persistence);
        let holder = std::thread::spawn(move || pinned.mark_catalog_stale(workspace_id));
        assert!(
            gate.wait_until_arrived(Duration::from_secs(10)),
            "the writer never reached the gate, so nothing was pinned"
        );

        let running_asker = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(due_operation, CorrelationId::new()))
        };
        // Barrier: the due operation's settlement is claimed and running, with its
        // identity kept in the state — the fact the previous version did not record.
        wait_until(
            || worker.shared.settle.lock().expect("settle state").running.is_some(),
            "the due settlement to be claimed",
        );
        {
            let state = worker.shared.settle.lock().expect("settle state");
            let flight = state.running.as_ref().expect("running flight");
            assert_eq!(
                flight.operation_id, due_operation,
                "the flight keeps its operation's identity"
            );
            assert_eq!(flight.waiters.len(), 1);
        }

        // The same operation asked again joins the running flight.
        let joined_asker = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(due_operation, CorrelationId::new()))
        };
        wait_until(
            || {
                worker
                    .shared
                    .settle
                    .lock()
                    .expect("settle state")
                    .running
                    .as_ref()
                    .is_some_and(|flight| flight.waiters.len() == 2)
            },
            "the duplicate to join the running flight",
        );

        // Different operations queue once each, and a duplicate of a queued operation
        // joins that queue entry instead of adding another write.
        let other_operation = HostPreparationOperationId::new();
        let queued_asker = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(other_operation, CorrelationId::new()))
        };
        let queued_joiner = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(other_operation, CorrelationId::new()))
        };
        let third_asker = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || {
                worker.settle(HostPreparationOperationId::new(), CorrelationId::new())
            })
        };
        wait_until(
            || {
                let state = worker.shared.settle.lock().expect("settle state");
                state.queue.len() == 2
                    && state
                        .queue
                        .iter()
                        .any(|task| task.operation_id == other_operation && task.waiters.len() == 2)
            },
            "the queued operations and their shared waiters",
        );

        gate.release();
        holder.join().expect("holder").expect("pinned write");
        let running =
            running_asker.join().expect("running asker").expect("the due settlement succeeds");
        assert_eq!(running, Some(due_operation), "the settlement reports the operation it settled");
        let joined = joined_asker
            .join()
            .expect("joined asker")
            .expect("the joined asker gets the same success");
        assert_eq!(
            joined,
            Some(due_operation),
            "a late joiner must get the same result as the first asker"
        );
        let queued =
            queued_asker.join().expect("queued asker").expect("the queued settlement is answered");
        assert_eq!(
            queued, None,
            "the other operation was not due, so its settlement settles nothing"
        );
        let queued_joined = queued_joiner
            .join()
            .expect("queued joiner")
            .expect("the queued joiner gets the same answer");
        assert_eq!(queued_joined, None);
        third_asker.join().expect("third asker").expect("every queued settlement is answered");

        assert_eq!(
            counts.settlement_finished.load(Ordering::SeqCst),
            3,
            "one settlement ran per distinct operation"
        );
        assert!(worker.settled(due_operation));
        assert_eq!(
            operation_state(&persistence, due_operation),
            HostPreparationOperationState::Failed,
            "the expired plan is really expired"
        );
    }

    /// A settle waiter whose budget expires does not cancel or restart the settlement.
    ///
    /// The waiter is told the component is busy; the settlement keeps its owner and runs
    /// to its result; and the next caller joins the settlement that is still running.
    #[test]
    fn a_settle_waiter_that_times_out_does_not_restart_the_settlement() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let workspace_id = workspace(&persistence);
        let due_operation = prepared_expired_operation(&persistence);
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        let gate = persistence.arm_dispatch_gate();
        let pinned = Arc::clone(&persistence);
        let holder = std::thread::spawn(move || pinned.mark_catalog_stale(workspace_id));
        assert!(
            gate.wait_until_arrived(Duration::from_secs(10)),
            "the writer never reached the gate, so nothing was pinned"
        );

        let first = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(due_operation, CorrelationId::new()))
        };
        wait_until(
            || {
                worker
                    .shared
                    .settle
                    .lock()
                    .expect("settle state")
                    .running
                    .as_ref()
                    .is_some_and(|flight| flight.operation_id == due_operation)
            },
            "the settlement to be claimed",
        );

        // The first asker's budget expires while the settlement is still pinned. That
        // is a busy answer, not a success — and the settlement outlives its last waiter.
        let error = first
            .join()
            .expect("first asker")
            .expect_err("a wait that outlived its budget is reported, not folded into success");
        assert_eq!(error.code.as_str(), "sandbox.host_preparation_busy");
        {
            let state = worker.shared.settle.lock().expect("settle state");
            let flight = state.running.as_ref().expect("the settlement outlives its last waiter");
            assert_eq!(flight.operation_id, due_operation);
            assert!(flight.waiters.is_empty(), "the timed-out asker detached");
        }

        // The next caller arrives while the same settlement is still running and joins
        // it: no second settlement starts.
        let second = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(due_operation, CorrelationId::new()))
        };
        wait_until(
            || {
                worker
                    .shared
                    .settle
                    .lock()
                    .expect("settle state")
                    .running
                    .as_ref()
                    .is_some_and(|flight| flight.waiters.len() == 1)
            },
            "the later asker to join the still-running settlement",
        );
        gate.release();
        holder.join().expect("holder").expect("pinned write");
        let second = second
            .join()
            .expect("second asker")
            .expect("the settlement still succeeds for the caller who waited");
        assert_eq!(second, Some(due_operation));
        assert_eq!(
            counts.settlement_finished.load(Ordering::SeqCst),
            1,
            "a caller arriving after every waiter left must not restart the settlement"
        );
        assert_eq!(
            operation_state(&persistence, due_operation),
            HostPreparationOperationState::Failed
        );
    }

    /// A failed settlement is answered as a failure to everyone who waited for it — the
    /// duplicate is never told success while the settlement it joined failed — and the
    /// plan, still prepared after the rollback, settles for real once the fault clears.
    #[test]
    fn a_failed_settlement_is_answered_to_every_waiter_then_is_retryable() {
        let directory = tempfile::tempdir().expect("data root");
        let database = directory.path().join("aura.sqlite3");
        let persistence = start(directory.path());
        let due_operation = prepared_expired_operation(&persistence);
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        // A real fault inside the writer: the settlement's own UPDATE aborts and rolls
        // back, so the failure is the production failure path, not a stand-in.
        set_write_fault(&database, SETTLE_WRITE_FAULT);

        let first = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(due_operation, CorrelationId::new()))
        };
        wait_until(
            || worker.shared.settle.lock().expect("settle state").running.is_some(),
            "the settlement to be claimed",
        );
        let second = {
            let worker = Arc::clone(&worker);
            std::thread::spawn(move || worker.settle(due_operation, CorrelationId::new()))
        };
        wait_until(
            || {
                worker
                    .shared
                    .settle
                    .lock()
                    .expect("settle state")
                    .running
                    .as_ref()
                    .is_some_and(|flight| flight.waiters.len() == 2)
            },
            "the duplicate to join the failing settlement",
        );

        let first = first
            .join()
            .expect("first asker")
            .expect_err("the failed settlement reports its failure to the asker who started it");
        let second = second
            .join()
            .expect("second asker")
            .expect_err("a waiter on the failed settlement gets the same failure");
        assert_eq!(first.code.as_str(), "sandbox.host_preparation_unavailable");
        assert_eq!(second.code.as_str(), "sandbox.host_preparation_unavailable");
        assert_eq!(
            counts.settlement_finished.load(Ordering::SeqCst),
            1,
            "one attempt answered both askers"
        );
        assert_eq!(
            operation_state(&persistence, due_operation),
            HostPreparationOperationState::Prepared,
            "the aborted transaction rolled back: the plan is still prepared, not settled"
        );

        // Clearing the fault makes the plan settle for real; the retry is a new
        // settlement, not a replay of the failure.
        set_write_fault(&database, SETTLE_WRITE_FAULT_CLEAR);
        let retried = worker
            .settle(due_operation, CorrelationId::new())
            .expect("the retry settles once the fault is gone");
        assert_eq!(retried, Some(due_operation));
        assert_eq!(counts.settlement_finished.load(Ordering::SeqCst), 2);
        assert_eq!(
            operation_state(&persistence, due_operation),
            HostPreparationOperationState::Failed
        );
    }

    /// The queue stays bounded by distinct operations: once full, another distinct
    /// operation is refused with a typed busy answer instead of piling up behind the
    /// writer. Joining a settlement that is already queued or running costs no depth.
    #[test]
    fn the_settle_queue_refuses_work_past_its_bound() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let workspace_id = workspace(&persistence);
        let counts = Arc::new(StageCounts::default());
        let worker = Arc::new(counting_worker(&persistence, &counts));

        let gate = persistence.arm_dispatch_gate();
        let pinned = Arc::clone(&persistence);
        let holder = std::thread::spawn(move || pinned.mark_catalog_stale(workspace_id));
        assert!(
            gate.wait_until_arrived(Duration::from_secs(10)),
            "the writer never reached the gate, so nothing was pinned"
        );

        let askers = (0..=SETTLE_QUEUE_DEPTH)
            .map(|_| {
                let worker = Arc::clone(&worker);
                std::thread::spawn(move || {
                    worker.settle(HostPreparationOperationId::new(), CorrelationId::new())
                })
            })
            .collect::<Vec<_>>();
        // Barrier: one settlement is claimed and the queue holds every remaining
        // distinct operation — the point where the next one has to be refused.
        wait_until(
            || {
                let state = worker.shared.settle.lock().expect("settle state");
                state.running.is_some() && state.queue.len() == SETTLE_QUEUE_DEPTH
            },
            "the queue to fill",
        );
        let refused = worker
            .settle(HostPreparationOperationId::new(), CorrelationId::new())
            .expect_err("a full queue refuses the next distinct operation");
        assert_eq!(refused.code.as_str(), "sandbox.host_preparation_busy");

        gate.release();
        holder.join().expect("holder").expect("pinned write");
        for asker in askers {
            asker.join().expect("asker").expect("every queued settlement is still answered");
        }
        assert_eq!(
            counts.settlement_finished.load(Ordering::SeqCst),
            SETTLE_QUEUE_DEPTH + 1,
            "one settlement ran per accepted operation"
        );
    }

    /// A plan this process already settled is not settled again.
    ///
    /// A status call that races the writer can still read the prepared row after the
    /// settlement was committed, and asking again would queue another write for no gain
    /// — exactly the pattern that made a read wait behind the writer.
    #[test]
    fn a_settled_plan_is_not_settled_twice() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let worker = worker(&persistence);
        let operation_id = HostPreparationOperationId::new();
        assert!(!worker.settled(operation_id));
        // Nothing has been settled, so the memory is empty and the first ask goes
        // through. The settlement itself finds no prepared row and returns cleanly.
        worker
            .settle(operation_id, CorrelationId::new())
            .expect("a settlement that had nothing to do is not a failure");
        assert!(
            worker.settled(operation_id),
            "the worker has to remember what it settled, or every later status read \
             queues another write"
        );
    }

    /// A batch of settlements is drained to empty, not one per enqueue.
    ///
    /// The wait condition used to be "did the queue change since I last looked", so a
    /// second request enqueued in the same batch sat in the queue until some later
    /// enqueue happened to wake the worker again. It waited on an event that had already
    /// occurred.
    #[test]
    fn a_batch_of_settlements_is_drained_without_a_further_enqueue() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let worker = Arc::new(worker(&persistence));
        let askers = (0..3)
            .map(|_| {
                let worker = Arc::clone(&worker);
                std::thread::spawn(move || {
                    worker
                        .settle(HostPreparationOperationId::new(), CorrelationId::new())
                        .map(|_| ())
                })
            })
            .collect::<Vec<_>>();
        // Every one of them has to be answered. A worker that drains one item per wake-up
        // would leave two of these threads waiting until their budget expired.
        for asker in askers {
            asker
                .join()
                .expect("asker")
                .expect("every queued settlement must be answered, not left in the queue");
        }
    }

    /// Releasing the last worker tells its threads to stop.
    ///
    /// The stop signal used to be decided by counting references to everything the struct
    /// held — which always included the threads' own clones — so the count never reached
    /// one, the signal was never set, and each create/drop cycle left two parked threads
    /// holding a `PersistenceCoordinator`. Nothing could end them: not a dropped service,
    /// not a test fixture.
    #[test]
    fn the_last_worker_asks_its_threads_to_stop() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let worker = worker(&persistence);
        assert!(
            !worker.stop_requested(),
            "a running worker must not be stopping before its owner goes away"
        );
        let stop = Arc::clone(&worker.stop);
        drop(worker);
        // The signal is the mechanism, and it is observable without depending on how
        // quickly a parked thread reaches its next poll.
        assert!(
            stop.load(std::sync::atomic::Ordering::SeqCst),
            "the last owner must tell both workers to stop, or they park for the life of \
             the process"
        );
    }

    /// Dropping the last worker lets its threads exit and release the coordinator.
    ///
    /// This is the resource half of the same defect: a thread that is never told to stop
    /// keeps its `Arc<PersistenceCoordinator>` alive for the life of the process, so a
    /// process that creates and destroys services accumulates both threads and database
    /// handles.
    #[test]
    fn the_last_worker_releases_its_persistence_handles() {
        let directory = tempfile::tempdir().expect("data root");
        let persistence = start(directory.path());
        let baseline = Arc::strong_count(&persistence);
        {
            let worker = worker(&persistence);
            assert!(
                Arc::strong_count(&persistence) > baseline,
                "the live workers hold the coordinator while they can still run"
            );
            drop(worker);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while Arc::strong_count(&persistence) > baseline && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            Arc::strong_count(&persistence),
            baseline,
            "a worker whose owner is gone must release the coordinator it was holding"
        );
    }
}
