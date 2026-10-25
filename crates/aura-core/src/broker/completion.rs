//! Who owns a broker dispatch's POST-EXECUTION tail.
//!
//! WHY THIS EXISTS
//! A broker dispatch is: `inspect` → ledger `claim` → **the handler** → evidence → ledger
//! completion. The last two steps run after the side effect may already exist, and they used to
//! be awaited by the same future that answers the caller. When the caller's section budget
//! expired, `broker/port.rs` returned early and the background evidence work finished with
//! *nobody* left to write the completion or to publish the command's real result: the effect was
//! on disk, the invocation stayed `unknown`, and the only thing that had really ended was one
//! caller's wait.
//!
//! THE OWNER
//! [`PostExecutionOwners::claim`] takes the completion responsibility **before** the handler is
//! entered, from the same bounded registry the session route uses
//! ([`crate::command_settlement::CommandSettlements`]): admission is finite, and a refusal is a
//! proven "it did not run" taken before any business exists. From then on the tail is
//! [`PostExecution`]: it holds the handler's own value, the original
//! invocation/correlation/attempt/runtime identity, the evidence closure and the journal, and it
//! runs the two post-execution steps on the bounded [`SyncBoundary`].
//!
//! FOUR ENDINGS THIS KEEPS APART (the requirement, not a naming preference)
//!  * **the handler finished** — a fact about the business; it produces the value the tail owns;
//!  * **the evidence step finished** — a fact about the post-state projection of that value;
//!  * **the journal completion is durable** — a fact about the ledger, written once here;
//!  * **the caller stopped waiting** — a fact about one waiter. Crossing it moves the remainder of
//!    the tail into its own detached continuation; it never destroys an input, a permit or the
//!    responsibility for a later stage.
//!
//! WHAT A LATE VALUE DOES
//! A step that started and did not answer inside the caller's budget still delivers its real value
//! to this owner, through [`crate::sync_boundary::SyncBoundary::run_settled`]'s sink, from the
//! thread that really ran the work. A step that provably did not run, or whose section unwound
//! without producing a value, is a *definite* "not established" and is never awaited again.
//! The tail never re-enters the handler, never repeats the file operation and never re-dispatches
//! the command. One step may be projected again, and only one: the evidence step is a **read-only**
//! post-state projection, and the single case where it is re-projected is the persistence writer's
//! own structured "the job was admitted and has not answered" answer, where the value of the first
//! projection was dropped by the layer below and a read is what can recover it.
//!
//! WHAT IT DELIBERATELY DOES NOT DO
//! It never fabricates a terminal: an ending it did not establish leaves the honest `Unknown` row
//! in place and records the real teardown fact. It never evicts a live tail on a timer. It never
//! blocks a Tokio worker: every step runs on the bounded boundary, and a tail that outlives its
//! caller is one detached task awaiting that boundary — at most one per claimed slot, and the
//! slots are capped at [`crate::command_settlement::MAX_PENDING_SETTLEMENTS`].

use super::journal::{BrokerJournal, CompletionVerdict};
use crate::blocking_pool::BlockingAdmission;
use crate::command_settlement::{
    CommandSettlements, MAX_PENDING_SETTLEMENTS, SettlementTicket, TaskEnding,
    outcome_unknown_envelope,
};
use crate::sync_boundary::{SyncBoundary, SyncSection, SyncSubject};
use aura_domain_common::{
    CommandInvocationId, CorrelationId, ErrorCategory, ErrorCode, RuntimeInstanceId,
};
use aura_execution_contract::{AttemptRef, BrokerEvidence, DispatchRequest};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

/// How many post-execution tails one Core answers for at once.
///
/// The same bound the completion registry uses: one tail per claimed settlement slot, so a burst
/// of abandoned callers cannot grow this without bound.
pub(crate) const MAX_PENDING_TAILS: usize = MAX_PENDING_SETTLEMENTS;

/// How many times the read-only evidence step may be projected in total for one dispatch.
///
/// The first projection plus two. The only case that uses a second one is the persistence writer's
/// own structured "admitted, not answered" answer, where the first projection's value was dropped
/// by the layer below; a writer that never answers gives up here and leaves the honest `Unknown`.
const MAX_EVIDENCE_PROJECTIONS: u32 = 3;

/// The completion responsibility for one dispatch's business call.
///
/// Built by [`PostExecutionOwners::claim`] *before* the handler is entered, and handed to the port
/// so the tail it builds cannot exist without a claimed slot. The ticket is the registry's own
/// proof of one claim: it is bound to the runtime and to this claim, so no later tail can settle
/// another owner's invocation.
pub(super) struct Responsibility {
    runtime: RuntimeInstanceId,
    ticket: Option<SettlementTicket>,
    invocation: Option<CommandInvocationId>,
    correlation: CorrelationId,
    method: String,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Responsibility {
    /// The registry's own proof of this claim, for the two calls the caller makes about its own
    /// wait (`caller_settled` / `caller_departed`). A copy authorises nothing new: the registry
    /// checks the claim's identity on every one of them.
    pub(super) fn ticket(&self) -> Option<SettlementTicket> {
        self.ticket.clone()
    }
}

/// Releases a claim only while it is still in the pre-handler slot. Once the port takes the
/// responsibility, this guard has nothing to release: admitted work keeps its own owner.
pub(super) struct UnenteredClaim {
    slot: Arc<Mutex<Option<Responsibility>>>,
    settlements: CommandSettlements,
}

impl UnenteredClaim {
    pub(super) fn new(
        slot: Arc<Mutex<Option<Responsibility>>>,
        settlements: CommandSettlements,
    ) -> Self {
        Self { slot, settlements }
    }
}

impl Drop for UnenteredClaim {
    fn drop(&mut self) {
        let untouched = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(responsibility) = untouched
            && let Some(ticket) = responsibility.ticket()
        {
            self.settlements.never_ran(&ticket);
        }
    }
}

/// The process-wide post-execution owner: its record, and the one place a tail is claimed.
///
/// Cheap to clone; every clone shares one record. It holds no per-method state and no thread: the
/// steps themselves run on the boundary the runtime already owns.
#[derive(Clone)]
pub(crate) struct PostExecutionOwners {
    runtime: RuntimeInstanceId,
    counters: Arc<TailCounters>,
    slots: Arc<tokio::sync::Semaphore>,
    /// Regression-only fault injection: the behaviour that existed before this owner, where the
    /// caller's timeout left the remainder of the tail with nobody to run it. Compiled out of
    /// every product build and never reachable from a product path.
    #[cfg(test)]
    no_continuation: Arc<AtomicBool>,
}

#[derive(Default)]
struct TailCounters {
    started: AtomicU64,
    finished_in_time: AtomicU64,
    continued: AtomicU64,
    settled_after_departure: AtomicU64,
    unresolved: AtomicU64,
    evidence_reprojected: AtomicU64,
}

/// Where the route reads one dispatch's single result from.
///
/// The tail writes it exactly once, whether or not the caller was still waiting for it; the route
/// takes the handler's value out of it by value.
pub(super) type PublishedResult<T> = Arc<Mutex<Option<Arc<Result<T, IpcErrorEnvelope>>>>>;

/// What the post-execution owner has really done, as counts.
///
/// Read by the regressions and by the acceptance report. No product decision is made from it, so
/// no product path reads it.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PostExecutionFacts {
    /// Tails that were claimed.
    pub started: u64,
    /// Tails whose two steps had both really finished before their caller stopped waiting.
    pub finished_in_time: u64,
    /// Tails whose caller stopped waiting and whose remainder moved to a continuation.
    pub continued: u64,
    /// Tails that settled their invocation after their caller had stopped waiting.
    pub settled_after_departure: u64,
    /// Tails that could not establish the durable settlement and left the honest `Unknown`.
    pub unresolved: u64,
    /// Read-only evidence projections made again after a structured writer answer.
    pub evidence_reprojected: u64,
}

impl PostExecutionOwners {
    /// The owner for one Core runtime instance.
    #[must_use]
    pub(crate) fn new(runtime: RuntimeInstanceId) -> Self {
        Self {
            runtime,
            counters: Arc::new(TailCounters::default()),
            slots: Arc::new(tokio::sync::Semaphore::new(MAX_PENDING_TAILS)),
            #[cfg(test)]
            no_continuation: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The real counts behind this owner.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn facts(&self) -> PostExecutionFacts {
        PostExecutionFacts {
            started: self.counters.started.load(Ordering::Acquire),
            finished_in_time: self.counters.finished_in_time.load(Ordering::Acquire),
            continued: self.counters.continued.load(Ordering::Acquire),
            settled_after_departure: self.counters.settled_after_departure.load(Ordering::Acquire),
            unresolved: self.counters.unresolved.load(Ordering::Acquire),
            evidence_reprojected: self.counters.evidence_reprojected.load(Ordering::Acquire),
        }
    }

    /// Regression-only: discard the remainder of a tail whose caller stopped waiting, exactly as
    /// the code did before this owner existed. Never reachable from a product path.
    #[cfg(test)]
    pub(crate) fn force_no_continuation_for_test(&self, on: bool) {
        self.no_continuation.store(on, Ordering::Release);
    }

    #[cfg(test)]
    fn keeps_no_continuation(&self) -> bool {
        self.no_continuation.load(Ordering::Acquire)
    }

    #[cfg(not(test))]
    const fn keeps_no_continuation(&self) -> bool {
        false
    }

    /// Claim the completion responsibility for a command that is about to be dispatched.
    ///
    /// A refusal is a proven "it did not run" taken **before** the handler: the caller gets a
    /// definite answer it may retry, and no effect exists to be lost. The row of the claimed
    /// invocation is the honest `Unknown` from this moment, so the work can never be observed
    /// without something that answers for its result.
    pub(super) fn claim(
        &self,
        settlements: &CommandSettlements,
        invocation: Option<CommandInvocationId>,
        correlation: CorrelationId,
        method: &str,
    ) -> Result<Responsibility, IpcErrorEnvelope> {
        let permit = Arc::clone(&self.slots)
            .try_acquire_owned()
            .map_err(|_| no_completion_owner_envelope(correlation, invocation))?;
        let Some(invocation) = invocation else {
            // A dispatch that carries no CommandInvocationId has no command row to settle: the
            // tail still runs, and nothing here invents an invocation for it.
            return Ok(Responsibility {
                runtime: self.runtime,
                ticket: None,
                invocation: None,
                correlation,
                method: method.to_owned(),
                _permit: permit,
            });
        };
        match settlements.begin(invocation, correlation, method) {
            Some(ticket) => {
                self.counters.started.fetch_add(1, Ordering::AcqRel);
                Ok(Responsibility {
                    runtime: self.runtime,
                    ticket: Some(ticket),
                    invocation: Some(invocation),
                    correlation,
                    method: method.to_owned(),
                    _permit: permit,
                })
            }
            None => Err(no_completion_owner_envelope(correlation, Some(invocation))),
        }
    }

    /// Give back a responsibility whose business provably never ran.
    ///
    /// This is the only path on which a claimed invocation returns to "never attempted", and it is
    /// taken only when the port never entered the handler.
    pub(super) fn never_ran(
        &self,
        settlements: &CommandSettlements,
        responsibility: &Responsibility,
    ) {
        if let Some(ticket) = &responsibility.ticket {
            settlements.never_ran(ticket);
        }
    }

    /// A responsibility for a dispatch that reached the port without one.
    ///
    /// Only reachable when the port is driven outside the assembled Core entry, which is a unit
    /// fixture: the product path always claims before the handler. It invents no invocation but
    /// still holds a bounded tail slot, and can publish a result without settling a command row.
    pub(super) fn unclaimed(
        &self,
        correlation: CorrelationId,
        method: &str,
    ) -> Result<Responsibility, IpcErrorEnvelope> {
        let permit = Arc::clone(&self.slots)
            .try_acquire_owned()
            .map_err(|_| no_completion_owner_envelope(correlation, None))?;
        Ok(Responsibility {
            runtime: self.runtime,
            ticket: None,
            invocation: None,
            correlation,
            method: method.to_owned(),
            _permit: permit,
        })
    }
}

/// The answer for a command that was not started because no completion owner was free.
///
/// It is a definite not-run answer, and it deliberately carries **no** recovery field: there is
/// nothing to query, because nothing ran. The invariant it protects is that a business call is
/// never entered without something that can answer for its result.
fn no_completion_owner_envelope(
    correlation: CorrelationId,
    invocation: Option<CommandInvocationId>,
) -> IpcErrorEnvelope {
    let mut details = BTreeMap::new();
    details.insert("command_invocation_id".to_owned(), serde_json::json!(invocation));
    IpcErrorEnvelope {
        code: ErrorCode::new("persistence.busy").expect("static code"),
        category: ErrorCategory::Unavailable,
        message: format!(
            "the command did not run: all {MAX_PENDING_TAILS} completion owners are still answering \
             for work that has not settled"
        ),
        retryable: true,
        correlation_id: correlation,
        terminal: IpcRequestTerminal::Failed,
        details,
    }
}

/// How long the caller is still willing to wait, extended by the tail after each step that really
/// finished.
///
/// This is a *wait* budget, not a work budget: crossing it changes who is waiting and nothing else.
/// The work keeps its permit, its owner and its responsibility.
struct CallerWait {
    started: tokio::time::Instant,
    budget: Duration,
    deadline_ms: AtomicU64,
    finished: AtomicBool,
    departed: AtomicBool,
}

impl CallerWait {
    fn new(budget: Duration) -> Arc<Self> {
        let budget_ms = u64::try_from(budget.as_millis()).unwrap_or(u64::MAX);
        Arc::new(Self {
            started: tokio::time::Instant::now(),
            budget,
            deadline_ms: AtomicU64::new(budget_ms),
            finished: AtomicBool::new(false),
            departed: AtomicBool::new(false),
        })
    }

    fn deadline(&self) -> tokio::time::Instant {
        self.started + Duration::from_millis(self.deadline_ms.load(Ordering::Acquire))
    }

    /// One step really finished while this caller was still waiting: it may wait for the next one
    /// under its own budget, exactly as it did when the two steps were awaited separately.
    fn step_finished(&self) {
        self.deadline_ms.fetch_add(
            u64::try_from(self.budget.as_millis()).unwrap_or(u64::MAX),
            Ordering::AcqRel,
        );
    }

    fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }

    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    fn depart(&self) {
        self.departed.store(true, Ordering::Release);
    }

    fn has_departed(&self) -> bool {
        self.departed.load(Ordering::Acquire)
    }
}

/// What one post-execution step produced.
enum Step<P> {
    /// The step really ran and produced this value.
    Produced(Arc<P>),
    /// The boundary refused the step inside the budget: it did not run.
    NotRun { waited_ms: u64 },
    /// The step started, did not answer inside the budget, and its section ended without a value.
    /// Nothing can arrive later, which is what separates this from a late delivery.
    NoValue { waited_ms: u64 },
}

/// Runs one post-execution step on the bounded boundary and returns its **real** value.
///
/// The caller's budget decides when the *caller* stops waiting; it never decides whether the step
/// keeps running. A step that started hands its value to this function through the boundary's own
/// settlement sink, so a late value is received here instead of being dropped with the caller's
/// future.
async fn run_step<P, W>(
    boundary: &SyncBoundary,
    section: SyncSection,
    subject: &SyncSubject,
    work: W,
) -> Step<P>
where
    P: Send + Sync + 'static,
    W: FnOnce() -> P + Send + 'static,
{
    let (sender, receiver) = oneshot::channel::<Arc<P>>();
    let admission = boundary
        .run_settled(
            section,
            subject.clone(),
            move || Arc::new(work()),
            move |value: &Arc<P>| {
                // Delivered by the thread that really ran the work, before the section is counted as
                // finished. A caller that already gave up cannot make this disappear.
                let _ = sender.send(Arc::clone(value));
            },
        )
        .await;
    match admission {
        BlockingAdmission::Completed(value) => Step::Produced(value),
        BlockingAdmission::Refused { waited_ms } => Step::NotRun { waited_ms },
        BlockingAdmission::Unknown { waited_ms } => match receiver.await {
            Ok(value) => Step::Produced(value),
            // The section unwound: the sink was dropped without ever being called, so no value
            // exists and none will arrive. This is a definite fact, not a wait.
            Err(_) => Step::NoValue { waited_ms },
        },
    }
}

/// Whether a post-execution step's own failure is one whose underlying storage job may still be
/// running.
///
/// Classified from the persistence layer's **typed** error and from the stage that produced it,
/// never from a message: `WriterUnresponsive` is the one answer that means "the job was admitted
/// and has not answered", so the value of this step may still exist below and must not be treated
/// as established failure material.
pub(crate) fn writer_may_still_answer(error: &anyhow::Error) -> bool {
    error.downcast_ref::<aura_persistence::PersistenceError>().is_some_and(|error| {
        matches!(error, aura_persistence::PersistenceError::WriterUnresponsive { .. })
    })
}

/// What a dispatched command's own result carries for its caller.
///
/// A broker dispatch does not always answer with a bare JSON value: a model or tool-mutation
/// command answers with the value *and* the projection changes that go with it, and the route —
/// not the ledger — is what applies those. The one thing the settlement owner has to be able to
/// state is the value the original caller's reply would have carried, so that a late settlement
/// publishes the real result rather than a shape invented here.
pub(crate) trait DispatchOutcome {
    /// The value this dispatch's own reply carries, or `None` when it has none to state.
    fn reply_value(&self) -> Option<serde_json::Value>;
}

impl DispatchOutcome for serde_json::Value {
    fn reply_value(&self) -> Option<serde_json::Value> {
        Some(self.clone())
    }
}

impl DispatchOutcome for crate::p05::P05CommandDispatch {
    fn reply_value(&self) -> Option<serde_json::Value> {
        Some(self.value.clone())
    }
}

impl DispatchOutcome for crate::p07_commands::ToolMutationDispatchV1 {
    fn reply_value(&self) -> Option<serde_json::Value> {
        Some(self.value.clone())
    }
}

/// The whole post-execution tail of one dispatch, and the identity it belongs to.
pub(super) struct PostExecution<T> {
    owners: PostExecutionOwners,
    settlements: CommandSettlements,
    responsibility: Responsibility,
    attempt: AttemptRef,
    request: Arc<DispatchRequest>,
    /// The handler's real result, shared with the evidence closure rather than copied: the port
    /// hands the caller the very value the handler produced.
    output: Arc<Result<T, IpcErrorEnvelope>>,
    evidence: super::OwnedEvidence<T>,
    journal: Arc<BrokerJournal>,
    boundary: SyncBoundary,
    subject: SyncSubject,
    /// The port's own slot, read by the route once the dispatch is answered. The tail publishes
    /// exactly one result into it, whether or not the caller was still there to read it.
    published: PublishedResult<T>,
    wait: Arc<CallerWait>,
}

/// Which of the two endings the caller's own wait reached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TailOutcome {
    /// Both post-execution steps had really finished while the caller was still waiting.
    Finished,
    /// The caller stopped waiting. The tail owns the rest and will settle the invocation.
    Continued,
}

impl<T> PostExecution<T>
where
    T: DispatchOutcome + Send + Sync + 'static,
{
    /// The tail for one dispatch whose handler has returned.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        owners: PostExecutionOwners,
        settlements: CommandSettlements,
        responsibility: Responsibility,
        attempt: AttemptRef,
        request: Arc<DispatchRequest>,
        output: Arc<Result<T, IpcErrorEnvelope>>,
        evidence: super::OwnedEvidence<T>,
        journal: Arc<BrokerJournal>,
        boundary: SyncBoundary,
        subject: SyncSubject,
        published: PublishedResult<T>,
    ) -> Self {
        let budget = boundary.budget();
        Self {
            owners,
            settlements,
            responsibility,
            attempt,
            request,
            output,
            evidence,
            journal,
            boundary,
            subject,
            published,
            wait: CallerWait::new(budget),
        }
    }

    /// Drives the tail from the caller's own wait, and hands it over if that wait ends first.
    ///
    /// The caller's budget is per step — the same budget each step was awaited with before this
    /// owner existed — and it is extended here only after a step really finished. Once it is
    /// crossed, the tail keeps everything (the value, the permit, the ticket and the invocation)
    /// and continues in its own task.
    pub(super) async fn run_within_caller_wait(self) -> TailOutcome {
        let wait = Arc::clone(&self.wait);
        let owners = self.owners.clone();
        // Start ownership independently before waiting. Dropping this caller at any await point
        // must not destroy the tail, including before the caller's own timeout fires.
        let mut tail = tokio::spawn(self.run());
        loop {
            let deadline = wait.deadline();
            match tokio::time::timeout_at(deadline, &mut tail).await {
                Ok(Ok(())) => return TailOutcome::Finished,
                Ok(Err(error)) => {
                    crate::append_core_log(
                        "broker.post_execution_task_lost",
                        serde_json::json!({"panicked": error.is_panic(), "cancelled": error.is_cancelled()}),
                    );
                    // A lost continuation is not an established business failure. The task guard
                    // has released its real ownership while retaining the honest Unknown row.
                    return TailOutcome::Continued;
                }
                Err(_) => {
                    if wait.is_finished() {
                        return TailOutcome::Finished;
                    }
                    if wait.deadline() == deadline {
                        break;
                    }
                    // A step finished exactly as the budget ended: the caller may wait for the
                    // next one under its own budget rather than being told Unknown one step early.
                }
            }
        }
        // FROM HERE THE CALLER STOPS WAITING. Nothing else changes: the tail keeps its inputs, its
        // permit and its responsibility, and it is the only thing that can still settle this
        // invocation. Dropping the future here is the pre-owner behaviour and exists only as a
        // regression injection.
        wait.depart();
        owners.counters.continued.fetch_add(1, Ordering::AcqRel);
        if owners.keeps_no_continuation() {
            tail.abort();
        }
        TailOutcome::Continued
    }

    /// The tail itself: evidence, then the durable completion, then the single delivery.
    async fn run(self) {
        let task = crate::command_settlement::SettlementTask::new(
            self.settlements.clone(),
            self.responsibility.ticket(),
        );
        let Some(evidence) = self.evidence_step().await else {
            self.finish_unresolved("evidence");
            task.finished();
            return;
        };
        if let Some(reason) = self.completion_step(evidence).await {
            self.finish_unresolved(&reason);
            task.finished();
            return;
        }
        self.finish_settled();
        task.finished();
    }

    /// The evidence step: the read-only post-state projection of the handler's result.
    ///
    /// A projection whose own storage job was admitted and did not answer is projected again,
    /// because the value below may still exist and the projection is a read. Everything else is
    /// decided by what the step really produced: an established projection, a step that did not
    /// run, or a step that will never produce anything.
    async fn evidence_step(&self) -> Option<BrokerEvidence> {
        let mut projections = 0_u32;
        loop {
            projections += 1;
            let evidence = Arc::clone(&self.evidence);
            let request = Arc::clone(&self.request);
            let output = Arc::clone(&self.output);
            let step = run_step(
                &self.boundary,
                SyncSection::Evidence,
                &self.subject,
                move || -> anyhow::Result<BrokerEvidence> { evidence(&request, &output) },
            )
            .await;
            let produced = match step {
                Step::Produced(value) => value,
                Step::NotRun { waited_ms } => {
                    self.record(
                        "evidence_not_run",
                        &format!("no synchronous worker was free after {waited_ms} ms"),
                    );
                    return None;
                }
                Step::NoValue { waited_ms } => {
                    self.record(
                        "evidence_no_value",
                        &format!("the evidence section ended without a value after {waited_ms} ms"),
                    );
                    return None;
                }
            };
            match produced.as_ref() {
                Ok(evidence) => {
                    self.wait.step_finished();
                    return Some(evidence.clone());
                }
                Err(error) => {
                    // The projection itself failed. The one structured case in which the value
                    // may still exist below is the writer's own unresponsive answer: its job was
                    // admitted, so the read is made again rather than being read as a failure.
                    if writer_may_still_answer(error) && projections < MAX_EVIDENCE_PROJECTIONS {
                        self.owners.counters.evidence_reprojected.fetch_add(1, Ordering::AcqRel);
                        self.record(
                            "evidence_reprojected",
                            &format!(
                                "projection {projections} met a writer that was admitted and had \
                                 not answered"
                            ),
                        );
                        // Let the writer's own deadline and the parked job settle before reading
                        // again, so the next projection is a real read rather than a repeated
                        // queue wait.
                        tokio::time::sleep(self.boundary.budget()).await;
                        continue;
                    }
                    self.record("evidence_not_established", &format!("{error:#}"));
                    return None;
                }
            }
        }
    }

    /// The ledger completion: the one step that makes the attempt's end durable.
    ///
    /// It receives the real evidence and the handler's own typed ending. The verdict is read from
    /// the handler's envelope, never from `is_ok()`: an outcome the Core itself declared unknown
    /// is recorded as `Unknown`, not as a failed attempt.
    async fn completion_step(&self, evidence: BrokerEvidence) -> Option<String> {
        let journal = Arc::clone(&self.journal);
        let request = Arc::clone(&self.request);
        let verdict = self.completion_verdict();
        let step = run_step(
            &self.boundary,
            SyncSection::Ledger,
            &self.subject,
            move || -> anyhow::Result<()> { journal.complete(&request, evidence, verdict) },
        )
        .await;
        let produced = match step {
            Step::Produced(value) => value,
            Step::NotRun { waited_ms } => {
                return Some(format!(
                    "completion did not run: no synchronous worker was free after {waited_ms} ms"
                ));
            }
            Step::NoValue { waited_ms } => {
                return Some(format!(
                    "completion produced no value: its section ended after {waited_ms} ms"
                ));
            }
        };
        match produced.as_ref() {
            Ok(()) => {
                self.wait.step_finished();
                None
            }
            Err(error) => Some(format!("completion did not commit: {error:#}")),
        }
    }

    /// How the ledger is allowed to record this attempt's end.
    fn completion_verdict(&self) -> CompletionVerdict {
        match self.output.as_ref() {
            Ok(_) => CompletionVerdict::Succeeded,
            Err(envelope) => {
                if crate::command_settlement::envelope_is_unconfirmed(envelope) {
                    CompletionVerdict::Unknown
                } else {
                    CompletionVerdict::Failed
                }
            }
        }
    }

    /// Both post-execution steps really finished: the invocation is settled with the real value.
    fn finish_settled(&self) {
        let ending = match self.output.as_ref() {
            Ok(value) => match value.reply_value() {
                Some(value) => TaskEnding::Succeeded(value),
                None => TaskEnding::Failed(outcome_unknown_envelope(
                    self.responsibility.correlation,
                    self.responsibility.invocation,
                    "the command produced a result with no value this Core can state, so its \
                     outcome is not established"
                        .to_owned(),
                )),
            },
            Err(envelope) => TaskEnding::Failed(envelope.clone()),
        };
        self.deliver(Arc::clone(&self.output), ending, "settled");
    }

    /// The durable settlement could not be established. The invocation keeps the honest `Unknown`
    /// and the real teardown fact is recorded; nothing is fabricated and nothing is re-run.
    fn finish_unresolved(&self, stage: &str) {
        let reason = format!(
            "the command's post-execution settlement could not be completed at the {stage} stage; \
             its outcome stays unknown and it must be resolved by querying the original invocation"
        );
        let envelope = outcome_unknown_envelope(
            self.responsibility.correlation,
            self.responsibility.invocation,
            reason,
        );
        if let Some(ticket) = &self.responsibility.ticket {
            // A real teardown fact: no further fact about this invocation can be established, so
            // the slot and its resources go back while the row stays the honest Unknown.
            self.settlements.unresolved(ticket);
        }
        self.owners.counters.unresolved.fetch_add(1, Ordering::AcqRel);
        self.deliver(Arc::new(Err(envelope.clone())), TaskEnding::Failed(envelope), "unresolved");
        self.record("unresolved", stage);
    }

    /// Publishes the one result this tail produced and delivers the ending to its owner.
    ///
    /// The delivery is the registry's own single point: an ending that arrives while the caller is
    /// still waiting is stored for the caller's reply, and one that arrives after the caller left
    /// settles the invocation there. Both are exactly-once, because the registry takes the record
    /// out under its own lock.
    fn deliver(
        &self,
        published: Arc<Result<T, IpcErrorEnvelope>>,
        ending: TaskEnding,
        disposition: &str,
    ) {
        if let Ok(mut slot) = self.published.lock() {
            *slot = Some(published);
        }
        let departed = self.wait.has_departed()
            || self
                .responsibility
                .ticket
                .as_ref()
                .is_some_and(|ticket| self.settlements.has_departed(ticket));
        if departed && !self.wait.has_departed() {
            self.owners.counters.continued.fetch_add(1, Ordering::AcqRel);
        }
        if let Some(ticket) = &self.responsibility.ticket
            && self.responsibility.runtime == self.owners.runtime
        {
            match &ending {
                // An unconfirmed ending is the one case in which waiting could never end: the
                // handler has returned and both post-execution steps have ended, so no later value
                // can be delivered to this owner. That is a real teardown fact, and it is what
                // gives the slot back - the honest `Unknown` row stays, the id stays refused for a
                // replay, and nothing is evicted on a timer.
                TaskEnding::Failed(envelope)
                    if crate::command_settlement::envelope_is_unconfirmed(envelope) =>
                {
                    self.settlements.task_finished(ticket, ending);
                    self.settlements.unresolved(ticket);
                }
                _ => self.settlements.task_finished(ticket, ending),
            }
            if departed {
                self.owners.counters.settled_after_departure.fetch_add(1, Ordering::AcqRel);
            }
        }
        self.wait.finish();
        if !departed {
            self.owners.counters.finished_in_time.fetch_add(1, Ordering::AcqRel);
        }
        self.record(disposition, &self.responsibility.method);
    }

    /// The real teardown fact of one tail, written where a run can be attributed.
    fn record(&self, disposition: &str, detail: &str) {
        crate::append_core_log(
            "broker.post_execution_tail",
            serde_json::json!({
                "disposition": disposition,
                "detail": detail,
                "method": self.responsibility.method,
                "correlation_id": self.responsibility.correlation,
                "command_invocation_id": self.responsibility.invocation,
                "operation_id": self.attempt.operation_id,
                "ordinal": self.attempt.ordinal.get(),
                "runtime_instance_id": self.owners.runtime,
            }),
        );
    }
}
