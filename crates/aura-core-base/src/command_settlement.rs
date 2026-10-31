//! Who owns a started synchronous business operation's result after its caller walked away.
//!
//! WHY THIS EXISTS
//! `routes/workspace.rs` runs a real `session.create`/`session.rename` on the bounded
//! synchronous boundary. When the caller's budget expires while that work is still running,
//! the caller is answered `command.outcome_unknown` and returns to the connection. The work,
//! its permit and its command claim keep running — which is correct — but nothing was left
//! that could ever *settle* it: the blocking task's value had no reader, and the outcome
//! ledger kept whatever the departing caller had written. A real commit could then be
//! permanently recorded as a confirmed failure, and the same invocation could be replayed
//! once the ledger ring evicted the row.
//!
//! THE THREE ENDINGS THIS KEEPS APART (the requirement, not a naming preference):
//!  * **the caller stopped waiting** — a fact about one waiter, [`CommandSettlements::caller_departed`];
//!  * **the real task finished** — a fact about the business, [`CommandSettlements::task_finished`];
//!  * **evidence and settlement finished** — a fact about the ledger, written only here, and
//!    only once one of the two above has delivered a real result.
//!
//! THE OWNER
//! A slot is claimed by [`CommandSettlements::begin`] *before* the work starts, so there is no
//! window in which a result exists and nothing could receive it. The slot is bound to the
//! original `CommandInvocationId`, the connection request (`CorrelationId`) it was sent for,
//! the method, and this Core runtime instance — the "operation/attempt" binding where one
//! exists is carried by the dispatch itself, not re-invented here. Exactly one of the two
//! deliveries takes the ending out of the record and writes the ledger row, so a late result
//! settles exactly once and can never be attributed to another request, another invocation or
//! another runtime.
//!
//! WHAT IT DELIBERATELY DOES NOT DO
//! It never aborts, evicts, replaces or releases held work: the permit and the claim belong to
//! the blocking task and to the ledger, and a caller timing out changes neither. It never
//! fabricates a success: an ending it never received leaves the honest `Unknown` row in place.
//! It does not retry or resend anything.
//!
//! BOUNDS
//!  * At most [`MAX_PENDING_SETTLEMENTS`] started-but-unsettled operations per process.
//!    `begin` refuses at the cap, and that refusal happens **before** any business runs, so it
//!    is a real "it did not run" answer instead of a lost result.
//!  * [`PENDING_SETTLEMENT_BUDGET`] marks an overdue owner for diagnostics. Time passing does
//!    not prove that work stopped, so an overdue owner keeps its slot and its pinned ledger
//!    row. Admission refuses new work at the cap rather than losing a live task's result.
//!
//! WHERE IT IS REACHED FROM
//! The Core entry point (`routes/workspace.rs`) claims the slot, hands a sink to the bounded
//! boundary, and reports which of the three endings happened. No other module keeps a result
//! monitor per method, and there is no per-method copy of this state machine.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use aura_domain_common::{
    CommandInvocationId, CorrelationId, ErrorCategory, ErrorCode, RuntimeInstanceId,
};
use aura_ipc::JsonRpcResponse;
use aura_runtime_contract::{
    CommandOutcome, CommandOutcomeUnknown, IpcErrorEnvelope, IpcRequestTerminal,
};
use serde_json::Value;

use crate::command_claims::CommandOutcomeLedger;

/// How many started-but-unsettled synchronous operations one Core holds at once.
///
/// Bounded on purpose and independent of the outcome ring: this cap is what a burst of
/// abandoned callers can exhaust, and exhausting it must produce an honest refusal, not a
/// result nobody owns.
pub const MAX_PENDING_SETTLEMENTS: usize = 64;

/// When an unsettled owner first becomes overdue for diagnostics.
///
/// Reaching this threshold records a warning; it never releases the completion owner, its
/// admission slot or its ledger pin. Only an actual ending can discharge that responsibility.
pub(crate) const PENDING_SETTLEMENT_BUDGET: Duration = Duration::from_secs(120);

/// The contract's own declaration, inside `details`, that a failed command may already have
/// reached the writer and must be resolved through `runtime.command_outcome`, never replayed.
///
/// The same value the transport puts on its own timeout envelope
/// (`apps/desktop/shared/ipc-request-failure.ts`), so Main's existing confirmation entry and
/// the Renderer's existing pending-confirmation protection recognise a Core-produced Unknown
/// by the contract's own recovery field instead of by a message.
pub const OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY: &str =
    "query_runtime.command_outcome_without_replay";

/// The `details` key that carries the original invocation, the same one the transport uses.
pub(crate) const COMMAND_INVOCATION_DETAIL: &str = "command_invocation_id";
/// The `details` key that carries the recovery instruction.
pub const OUTCOME_RECOVERY_DETAIL: &str = "outcome_recovery";

/// The frozen contract code for "this command started and its outcome is not known".
pub(crate) const COMMAND_OUTCOME_UNKNOWN_CODE: &str = "command.outcome_unknown";

/// The persistence layer's own frozen code for "the write was admitted and did not answer".
///
/// It is a *code*, not a message: `PersistenceError::WriterUnresponsive` is the one structured
/// answer that says the job may still run and may already have committed, so an envelope carrying
/// it is an unconfirmed outcome like any other — the recovery instruction is a contract value and
/// the settlement must not record it as a definite failure.
pub const PERSISTENCE_OUTCOME_UNKNOWN_CODE: &str = "persistence.outcome_unknown";

/// The typed envelope for a started command whose outcome is not known.
///
/// It carries the existing contract's recovery field and the original invocation, so a reader
/// can tell "the outcome is unknown, query the ledger" from "this definitely failed" without
/// reading the message — and so `error.terminal = failed` cannot erase the pending meaning.
pub fn outcome_unknown_envelope(
    correlation_id: CorrelationId,
    command_invocation_id: Option<CommandInvocationId>,
    message: String,
) -> IpcErrorEnvelope {
    let mut details = BTreeMap::new();
    if let Some(command_invocation_id) = command_invocation_id {
        details
            .insert(COMMAND_INVOCATION_DETAIL.to_owned(), serde_json::json!(command_invocation_id));
    }
    details.insert(
        OUTCOME_RECOVERY_DETAIL.to_owned(),
        serde_json::json!(OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY),
    );
    IpcErrorEnvelope {
        code: ErrorCode::new(COMMAND_OUTCOME_UNKNOWN_CODE).expect("static code"),
        category: ErrorCategory::Unavailable,
        message,
        retryable: false,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details,
    }
}

/// Whether this envelope says "the command started and its outcome is not known".
///
/// Read from the contract's own recovery field and from the two frozen codes that mean it, never
/// from the message: the message is display text, and a decision made from it would break the
/// moment it is reworded. The persistence layer's own "admitted, not answered" code is one of
/// them, because a write that may still commit is exactly the case this must not call a failure.
#[must_use]
pub fn envelope_is_unconfirmed(envelope: &IpcErrorEnvelope) -> bool {
    envelope.details.get(OUTCOME_RECOVERY_DETAIL).and_then(Value::as_str)
        == Some(OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY)
        || matches!(
            envelope.code.as_str(),
            COMMAND_OUTCOME_UNKNOWN_CODE | PERSISTENCE_OUTCOME_UNKNOWN_CODE
        )
}

/// Whether a wire response means "unconfirmed", by the same typed rule.
#[must_use]
pub fn response_is_unconfirmed(response: &JsonRpcResponse) -> bool {
    response.error.as_ref().map(|error| &error.data).is_some_and(envelope_is_unconfirmed)
}

/// What a started operation's own thread really produced.
///
/// It is owned and `Send` because it is built on the blocking thread and consumed by the
/// owner: the value is handed over by value, so the real result cannot be lost in a channel
/// that nobody reads.
#[derive(Clone, Debug)]
pub enum TaskEnding {
    /// The business returned a value.
    Succeeded(Value),
    /// The business returned a typed failure.
    Failed(IpcErrorEnvelope),
}

impl TaskEnding {
    /// The typed ending of one dispatch result, as the operation's own thread saw it.
    #[must_use]
    pub fn from_result(result: &Result<Value, IpcErrorEnvelope>) -> Self {
        match result {
            Ok(value) => Self::Succeeded(value.clone()),
            Err(envelope) => Self::Failed(envelope.clone()),
        }
    }

    /// The wire response this ending is reported as, addressed to `correlation_id`.
    ///
    /// Built here rather than re-derived by a reader, so the settled row holds the same shape
    /// the caller's own reply would have had: one JSON-RPC terminal, the original correlation
    /// id and a typed envelope. Main's confirmation verifies exactly those three things.
    #[must_use]
    pub(crate) fn into_response(self, correlation_id: CorrelationId) -> JsonRpcResponse {
        match self {
            Self::Succeeded(value) => JsonRpcResponse::success(correlation_id, value),
            Self::Failed(envelope) => JsonRpcResponse::failure(correlation_id, envelope),
        }
    }
}

/// Proof that one operation holds a settlement slot.
///
/// Issued only by [`CommandSettlements::begin`]; the fields are private and there is no
/// constructor, so a caller cannot present a ticket for an operation it did not claim.
#[derive(Clone, Debug)]
pub struct SettlementTicket {
    runtime: RuntimeInstanceId,
    claim: Arc<()>,
    correlation: CorrelationId,
    invocation: CommandInvocationId,
}

impl SettlementTicket {
    /// The invocation this ticket settles.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) const fn invocation(&self) -> CommandInvocationId {
        self.invocation
    }

    /// The connection request this ticket was issued for.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) const fn correlation(&self) -> CorrelationId {
        self.correlation
    }
}

/// One claimed slot.
struct Pending {
    claim: Arc<()>,
    correlation: CorrelationId,
    method: String,
    started_at: Instant,
    deadline: Instant,
    overdue_reported: bool,
    /// The real result, once the task produced one. Taken out exactly once, by whichever of
    /// the two deliveries becomes able to settle.
    ending: Option<TaskEnding>,
    /// Set by the caller when it stopped waiting. Until this is set, a task that finished is
    /// the caller's own reply to record, not this owner's.
    caller_departed: bool,
}

#[derive(Default)]
struct Registry {
    records: BTreeMap<CommandInvocationId, Pending>,
}

/// The process-wide completion owner. Cheap to clone; every clone shares one registry.
#[derive(Clone)]
pub struct CommandSettlements {
    /// The Core runtime instance this owner belongs to. A record from another runtime can
    /// never be settled here, which is what keeps a new process from confirming an old one.
    runtime: RuntimeInstanceId,
    /// The authoritative ledger rows are written through the same lock the runtime uses, so
    /// the pending mark and the settled row can never be seen apart.
    outcomes: Arc<Mutex<CommandOutcomeLedger>>,
    inner: Arc<Mutex<Registry>>,
    /// Regression-only fault injection: behave as the code did before this owner existed, i.e.
    /// let a late result be discarded instead of settling its invocation. It exists so a
    /// regression can show that the property it checks is about the ownership rather than about
    /// timing, and it is compiled out of every product build.
    #[cfg(any(test, feature = "test-hooks"))]
    inert: Arc<std::sync::atomic::AtomicBool>,
}

/// One caller's interest, separate from the lifetime of the admitted work. Aborting an IPC
/// handler drops this guard and transfers its reply to the existing completion owner.
pub struct SettlementWaiter {
    settlements: CommandSettlements,
    ticket: Option<SettlementTicket>,
}

impl SettlementWaiter {
    pub fn new(settlements: CommandSettlements, ticket: Option<SettlementTicket>) -> Self {
        Self { settlements, ticket }
    }

    pub fn received(mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.settlements.caller_settled(&ticket);
        }
    }
}

impl Drop for SettlementWaiter {
    fn drop(&mut self) {
        if let Some(ticket) = &self.ticket {
            self.settlements.caller_departed(ticket);
        }
    }
}

/// Last-resort teardown for a completion task which unwinds before it can publish a result.
/// This guard lives with the actual task, never with a caller's timeout.
pub struct SettlementTask {
    settlements: CommandSettlements,
    ticket: Option<SettlementTicket>,
}

impl SettlementTask {
    pub fn new(settlements: CommandSettlements, ticket: Option<SettlementTicket>) -> Self {
        Self { settlements, ticket }
    }

    pub fn finished(mut self) {
        self.ticket.take();
    }
}

impl Drop for SettlementTask {
    fn drop(&mut self) {
        if let Some(ticket) = &self.ticket {
            self.settlements.unresolved(ticket);
        }
    }
}

impl CommandSettlements {
    /// The owner for one Core runtime instance and its outcome ledger.
    #[must_use]
    pub fn new(runtime: RuntimeInstanceId, outcomes: Arc<Mutex<CommandOutcomeLedger>>) -> Self {
        Self {
            runtime,
            outcomes,
            inner: Arc::new(Mutex::new(Registry::default())),
            #[cfg(any(test, feature = "test-hooks"))]
            inert: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Regression-only: make this owner discard every late result, exactly as the pre-owner code
    /// did when the timeout dropped the blocking handle. Never reachable from a product path.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn force_inert_for_test(&self, inert: bool) {
        self.inert.store(inert, std::sync::atomic::Ordering::Release);
    }

    #[cfg(any(test, feature = "test-hooks"))]
    fn is_inert(&self) -> bool {
        self.inert.load(std::sync::atomic::Ordering::Acquire)
    }

    /// The runtime instance this owner settles for.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) const fn runtime(&self) -> RuntimeInstanceId {
        self.runtime
    }

    /// How many slots are claimed right now. For diagnostics and regressions.
    #[must_use]
    #[allow(dead_code)]
    pub fn pending(&self) -> usize {
        self.lock().map(|registry| registry.records.len()).unwrap_or_default()
    }

    /// Whether one invocation currently has a claimed slot.
    ///
    /// Read by the outcome ledger before it writes a row and before it lets an id execute
    /// again: while a slot is claimed, this owner is the only writer of that invocation's row
    /// and a second execution of it must be refused.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_pending(&self, invocation: CommandInvocationId) -> bool {
        self.lock().map(|registry| registry.records.contains_key(&invocation)).unwrap_or(false)
    }

    pub fn has_pending_method(&self, method: &str) -> bool {
        self.lock()
            .map(|registry| registry.records.values().any(|pending| pending.method == method))
            .unwrap_or(true) // unavailable ownership is not a confirmed idle state
    }

    /// Claim the settlement responsibility for an operation that is about to start.
    ///
    /// `None` is an honest refusal **before** any business runs; the caller must not start the
    /// work. The ledger row is set to `Unknown` here rather than by the departing caller, so
    /// the moment the work exists its queryable state already says "started, not settled".
    pub fn begin(
        &self,
        invocation: CommandInvocationId,
        correlation: CorrelationId,
        method: &str,
    ) -> Option<SettlementTicket> {
        let now = Instant::now();
        let claim = Arc::new(());
        {
            let mut registry = self.lock().ok()?;
            observe_overdue(&mut registry, now);
            if registry.records.len() >= MAX_PENDING_SETTLEMENTS
                || registry.records.contains_key(&invocation)
            {
                return None;
            }
            registry.records.insert(
                invocation,
                Pending {
                    claim: Arc::clone(&claim),
                    correlation,
                    method: method.to_owned(),
                    started_at: now,
                    deadline: now + PENDING_SETTLEMENT_BUDGET,
                    overdue_reported: false,
                    ending: None,
                    caller_departed: false,
                },
            );
        }
        let recorded = self.outcomes.lock().is_ok_and(|mut outcomes| {
            outcomes.claim_pending(
                invocation,
                CommandOutcome::Unknown {
                    command_invocation_id: invocation,
                    reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
                },
            );
            true
        });
        if !recorded {
            // A ledger that cannot be written cannot be settled honestly, so the slot is given
            // back and the caller refuses before starting anything.
            self.forget(invocation);
            return None;
        }
        Some(SettlementTicket { runtime: self.runtime, claim, correlation, invocation })
    }

    /// The real result of the started operation, delivered by the thread that produced it.
    ///
    /// This is the delivery that replaces "the JoinHandle's result was discarded": it is called
    /// from inside the work, so a late value reaches the owner no matter whether the caller is
    /// still there. If the caller already departed, the ending settles here.
    pub fn task_finished(&self, ticket: &SettlementTicket, ending: TaskEnding) {
        let settled = {
            let Ok(mut registry) = self.lock() else {
                return;
            };
            let Some(record) = registry.records.get_mut(&ticket.invocation) else {
                // The slot was already settled or proven never started. A duplicate delivery
                // cannot recreate its responsibility.
                return;
            };
            if !self.matches_ticket(record, ticket) {
                return;
            }
            record.ending = Some(ending);
            if record.caller_departed { record.ending.take() } else { None }
        };
        if let Some(ending) = settled {
            self.write_settlement(ticket, ending);
        }
    }

    /// Whether this claim's caller has departed, as recorded by its waiter guard.
    pub fn has_departed(&self, ticket: &SettlementTicket) -> bool {
        self.lock().is_ok_and(|registry| {
            registry
                .records
                .get(&ticket.invocation)
                .is_some_and(|record| self.matches_ticket(record, ticket) && record.caller_departed)
        })
    }

    /// The caller stopped waiting for an operation that is already running.
    ///
    /// From here the owner answers for the result: a task that already finished settles
    /// immediately, and one that finishes later settles on its own delivery. Nothing is
    /// aborted, released or replaced by this call.
    pub fn caller_departed(&self, ticket: &SettlementTicket) {
        let settled = {
            let Ok(mut registry) = self.lock() else {
                return;
            };
            let Some(record) = registry.records.get_mut(&ticket.invocation) else {
                return;
            };
            if !self.matches_ticket(record, ticket) {
                return;
            }
            record.caller_departed = true;
            record.ending.take()
        };
        if let Some(ending) = settled {
            self.write_settlement(ticket, ending);
        }
    }

    /// The caller received the task's own answer and reported it.
    ///
    /// The slot is given back so the caller's own reply is what the ledger records; discarding
    /// any stored ending here is deliberate, because the caller holds the same value.
    pub(crate) fn caller_settled(&self, ticket: &SettlementTicket) {
        self.release_owned(ticket, false);
    }

    /// The work never started, so nothing is owed and the id is free again.
    ///
    /// This is the only path on which a claimed invocation returns to "never attempted": a
    /// proven refusal before the business ran is exactly the case where a retry is honest.
    pub fn never_ran(&self, ticket: &SettlementTicket) {
        self.release_owned(ticket, true);
    }

    /// The owner reached a real teardown fact: no further fact about this invocation can be
    /// established, so its slot, its permit-independent record and its ledger pin go back.
    ///
    /// The row itself stays the honest `Unknown` and the invocation stays refused for execution,
    /// so this is not a release into "it never happened": it is the end of *this owner's*
    /// responsibility, taken only from a teardown the owner really observed (a post-execution step
    /// that provably did not run, or one whose section ended without a value). Nothing here
    /// fabricates a terminal and nothing is re-executed.
    pub fn unresolved(&self, ticket: &SettlementTicket) {
        self.release_owned(ticket, false);
    }

    /// Write the real ending for one invocation, exactly once.
    fn write_settlement(&self, ticket: &SettlementTicket, ending: TaskEnding) {
        let invocation = ticket.invocation;
        let correlation = ticket.correlation;
        #[cfg(any(test, feature = "test-hooks"))]
        if self.is_inert() {
            // The injected pre-owner behaviour: the result reaches nothing and the invocation is
            // left exactly as the departing caller left it.
            return;
        }
        let response = ending.into_response(correlation);
        if response_is_unconfirmed(&response) {
            // A completed wrapper can still report that its underlying write has not settled.
            // Keep the pending Unknown; the wire's failed terminal is not a business verdict.
            return;
        }
        let (terminal, stored) = match (&response.result, &response.error) {
            (Some(result), _) => (result.terminal, serde_json::to_value(&response).ok()),
            (None, Some(error)) => (error.data.terminal, serde_json::to_value(&response).ok()),
            (None, None) => return,
        };
        let Some(stored) = stored else {
            return;
        };
        let Ok(mut registry) = self.lock() else {
            return;
        };
        let Some(record) = registry.records.get(&invocation) else {
            return;
        };
        if !self.matches_ticket(record, ticket) {
            return;
        }
        let method = record.method.clone();
        let Ok(mut outcomes) = self.outcomes.lock() else {
            return;
        };
        // The row is written only if this invocation still has a claimed slot: an abandoned or
        // already-settled record must not be overwritten by a value that arrived after its
        // responsibility ended. This is also the exactly-once guard, because settling clears
        // the pending mark under the same lock.
        if !outcomes.is_pending(invocation) {
            return;
        }
        outcomes.settle_pending(
            invocation,
            CommandOutcome::Confirmed {
                command_invocation_id: invocation,
                terminal,
                response: stored,
            },
        );
        registry.records.remove(&invocation);
        drop(outcomes);
        drop(registry);
        crate::append_core_log(
            "command_settlement.settled",
            serde_json::json!({
                "command_invocation_id": invocation,
                "correlation_id": correlation,
                "method": method,
                "runtime_instance_id": self.runtime,
                "terminal": terminal,
            }),
        );
    }

    fn matches_ticket(&self, record: &Pending, ticket: &SettlementTicket) -> bool {
        self.runtime == ticket.runtime
            && record.correlation == ticket.correlation
            && Arc::ptr_eq(&record.claim, &ticket.claim)
    }

    fn release_owned(&self, ticket: &SettlementTicket, never_ran: bool) {
        let Ok(mut registry) = self.lock() else {
            return;
        };
        if !registry
            .records
            .get(&ticket.invocation)
            .is_some_and(|record| self.matches_ticket(record, ticket))
        {
            return;
        }
        let Ok(mut outcomes) = self.outcomes.lock() else {
            return;
        };
        if never_ran {
            outcomes.abandon_pending(ticket.invocation);
        } else {
            outcomes.clear_pending(ticket.invocation);
        }
        registry.records.remove(&ticket.invocation);
    }

    fn forget(&self, invocation: CommandInvocationId) {
        let _ = self.lock().map(|mut registry| registry.records.remove(&invocation));
    }

    fn lock(&self) -> Result<MutexGuard<'_, Registry>, ()> {
        self.inner.lock().map_err(|_| ())
    }
}

/// Diagnose overdue work once without discarding its owner or bypassing the admission cap.
fn observe_overdue(registry: &mut Registry, now: Instant) {
    for (invocation, record) in &mut registry.records {
        if now < record.deadline || record.overdue_reported {
            continue;
        }
        record.overdue_reported = true;
        crate::append_core_log(
            "command_settlement.overdue",
            serde_json::json!({
                "command_invocation_id": invocation,
                "correlation_id": record.correlation,
                "method": record.method,
                "waited_ms": u64::try_from(record.started_at.elapsed().as_millis())
                    .unwrap_or(u64::MAX),
                "budget_ms": u64::try_from(PENDING_SETTLEMENT_BUDGET.as_millis())
                    .unwrap_or(u64::MAX),
                "outcome": "unknown",
            }),
        );
    }
}

#[cfg(test)]
#[path = "command_settlement_tests.rs"]
mod ownership_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unknown_envelope_carries_the_contracts_recovery_field_and_the_invocation() {
        let invocation = CommandInvocationId::new();
        let correlation = CorrelationId::new();
        let envelope =
            outcome_unknown_envelope(correlation, Some(invocation), "started, not settled".into());
        assert_eq!(envelope.code.as_str(), COMMAND_OUTCOME_UNKNOWN_CODE);
        assert_eq!(envelope.terminal, IpcRequestTerminal::Failed);
        assert_eq!(
            envelope.details.get(OUTCOME_RECOVERY_DETAIL).and_then(Value::as_str),
            Some(OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY)
        );
        assert_eq!(
            envelope
                .details
                .get(COMMAND_INVOCATION_DETAIL)
                .and_then(|value| serde_json::from_value::<CommandInvocationId>(value.clone()).ok()),
            Some(invocation)
        );
        assert!(envelope_is_unconfirmed(&envelope));
        // The typed rule is not the message: a different sentence is still recognised.
        let reworded = outcome_unknown_envelope(correlation, None, "something else".into());
        assert!(envelope_is_unconfirmed(&reworded));
    }

    #[test]
    fn a_definite_failure_is_not_recognised_as_unconfirmed() {
        let correlation = CorrelationId::new();
        let envelope = IpcErrorEnvelope {
            code: ErrorCode::new("persistence.busy").expect("static code"),
            category: ErrorCategory::Unavailable,
            message: "no synchronous worker was free after 80 ms".into(),
            retryable: true,
            correlation_id: correlation,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        };
        assert!(!envelope_is_unconfirmed(&envelope));
        assert!(!response_is_unconfirmed(&JsonRpcResponse::failure(correlation, envelope)));
    }

    #[test]
    fn an_ending_is_reported_as_a_response_addressed_to_the_original_request() {
        let correlation = CorrelationId::new();
        let success = TaskEnding::Succeeded(serde_json::json!({"session_id": "ses_x"}))
            .into_response(correlation);
        assert_eq!(success.id, correlation);
        assert_eq!(
            success.result.as_ref().map(|result| result.terminal),
            Some(IpcRequestTerminal::Succeeded)
        );
        assert!(!response_is_unconfirmed(&success));

        let failure = TaskEnding::Failed(outcome_unknown_envelope(
            correlation,
            None,
            "started, not settled".into(),
        ))
        .into_response(correlation);
        assert_eq!(failure.id, correlation);
        assert!(failure.result.is_none());
        assert!(response_is_unconfirmed(&failure));
    }
}
