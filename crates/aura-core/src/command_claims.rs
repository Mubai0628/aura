//! Command identity: who may execute a side-effecting command, exactly once.
//!
//! A `CommandInvocationId` is the caller's stable name for one intended command.
//! The runtime has to answer two questions with it, and they are not the same:
//!
//! * **May this request execute now?** — the claim. It is taken before dispatch,
//!   atomically with the completed-outcome check, so two requests that carry the
//!   same id cannot both run.
//! * **What happened to the attempt?** — the outcome, which stays after the claim
//!   is gone so a replay conflicts instead of executing a second time.
//!
//! Ownership has to be precise to the *request*, not to the connection or to the
//! id alone: a connection can send two different requests that name the same id,
//! and only one of them may run. The credential that proves ownership
//! ([`CommandClaim`]) is therefore bound to the connection's request id, the
//! invocation id and a claim generation that changes whenever the id is claimed
//! again. It cannot be built from the wire (the module exposes no constructor and
//! no "already reserved" flag), it is not a bearer token for *any* request, and a
//! stale or borrowed one is refused.
//!
//! tool-mutation commands keep their own ledger: the runtime does not wrap
//! them in a second, conflicting claim.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::Result;
use aura_domain_common::{CommandInvocationId, CorrelationId, ErrorCategory, ErrorCode};
use aura_ipc::{AsyncPriorityQueue, JsonRpcRequest, JsonRpcResponse, QueuePriority};
use aura_runtime_contract::{
    CommandOutcome, CommandOutcomeUnknown, IpcErrorEnvelope, IpcRequestTerminal,
};

use crate::{RuntimeState, p07_commands, try_send_json};

/// How many settled outcomes are remembered for replay detection.
pub(crate) const MAX_COMMAND_OUTCOMES: usize = 4_096;

/// Proof that one specific request owns the right to execute its command.
///
/// Produced only by [`CommandOutcomeLedger::begin`], which is the single entry
/// point for taking a claim. The fields are private and there is no constructor:
/// a caller cannot mint one, and the type carries no boolean that would let a
/// caller declare itself authorised.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandClaim {
    invocation: CommandInvocationId,
    /// The connection request this claim was issued for.
    request: CorrelationId,
    /// Which claim of this invocation id this is. A released-and-retaken id gets a
    /// new generation, so a credential from the earlier attempt authorises
    /// nothing.
    generation: u64,
}

impl CommandClaim {
    /// The command this claim is for.
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn invocation(&self) -> CommandInvocationId {
        self.invocation
    }

    /// Whether this credential was issued for exactly this request and command.
    ///
    /// Copies of a credential are not extra authority: every copy still names one
    /// request and one generation, and the ledger checks that generation is the
    /// live one.
    #[must_use]
    pub(crate) fn covers(&self, request: CorrelationId, invocation: CommandInvocationId) -> bool {
        self.request == request && self.invocation == invocation
    }
}

/// The claims one connection holds, keyed by the request that owns them.
pub(crate) type CommandFlights = BTreeMap<CorrelationId, CommandClaim>;

/// The live claim of one invocation id.
struct InFlightClaim {
    request: CorrelationId,
    generation: u64,
}

#[derive(Default)]
pub(crate) struct CommandOutcomeLedger {
    pub(crate) entries: BTreeMap<CommandInvocationId, CommandOutcome>,
    order: VecDeque<CommandInvocationId>,
    /// Commands that are executing right now. A completed entry alone cannot
    /// stop a replay that arrives while the first attempt is still running.
    in_flight: BTreeMap<CommandInvocationId, InFlightClaim>,
    /// Commands a completion owner answers for right now: the work started, the caller may
    /// have stopped waiting, and the real result is still owed.
    ///
    /// This set exists for two properties an outcome row cannot give on its own:
    ///  * the row of a started-but-unsettled command is **never evicted** by the bounded
    ///    outcome ring, so a replay cannot be granted after the ring moved past it;
    ///  * the completion owner is the only writer of that row, so a departing caller's
    ///    "unknown" cannot overwrite a result that has already really landed.
    pending: BTreeSet<CommandInvocationId>,
    next_generation: u64,
}

impl CommandOutcomeLedger {
    pub(crate) fn insert(&mut self, id: CommandInvocationId, outcome: CommandOutcome) {
        self.in_flight.remove(&id);
        self.put(id, outcome);
    }

    /// Write a row without touching the in-flight record.
    ///
    /// `claim_pending` uses this: a command that was claimed and handed to a handler is *still
    /// running*, and the start of a completion record must not erase that fact.
    fn put(&mut self, id: CommandInvocationId, outcome: CommandOutcome) {
        if self.entries.insert(id, outcome).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > MAX_COMMAND_OUTCOMES {
            let Some(expired) = self.pop_evictable() else {
                // Every remaining entry is a started-but-unsettled command. The ring is
                // allowed to stay over its size rather than evict the one row a real result
                // is still owed to.
                break;
            };
            self.entries.remove(&expired);
        }
    }

    /// The oldest entry that may be evicted, skipping the ones a completion owner holds.
    fn pop_evictable(&mut self) -> Option<CommandInvocationId> {
        for _ in 0..self.order.len() {
            let id = self.order.pop_front()?;
            if self.pending.contains(&id) {
                self.order.push_back(id);
                continue;
            }
            return Some(id);
        }
        None
    }

    /// Take responsibility for a started command's row.
    ///
    /// The row is the honest `Unknown` from the moment the work exists: the command has
    /// started and no result is recorded yet. The pending mark is what keeps this row from
    /// being evicted and what makes a replay a conflict instead of a second execution.
    pub(crate) fn claim_pending(&mut self, id: CommandInvocationId, outcome: CommandOutcome) {
        self.pending.insert(id);
        self.put(id, outcome);
    }

    /// Whether a completion owner is currently answerable for `id`.
    #[must_use]
    pub(crate) fn is_pending(&self, id: CommandInvocationId) -> bool {
        self.pending.contains(&id)
    }

    /// The owner recorded the real outcome: the row is final and the id is no longer owed.
    pub(crate) fn settle_pending(&mut self, id: CommandInvocationId, outcome: CommandOutcome) {
        self.pending.remove(&id);
        self.insert(id, outcome);
    }

    /// The caller reported the operation's own answer: the caller owns the row from here.
    pub(crate) fn clear_pending(&mut self, id: CommandInvocationId) {
        self.pending.remove(&id);
    }

    /// The work provably never started: drop the placeholder row and free the id.
    ///
    /// Only reachable from a proven pre-execution refusal, which is exactly the case where a
    /// retry is honest. It never applies to work that started.
    pub(crate) fn abandon_pending(&mut self, id: CommandInvocationId) {
        if !self.pending.remove(&id) {
            return;
        }
        if matches!(self.entries.get(&id), Some(CommandOutcome::Unknown { .. }))
            && !self.in_flight.contains_key(&id)
        {
            self.entries.remove(&id);
            self.order.retain(|candidate| *candidate != id);
        }
    }

    /// Claim the right to execute `invocation` for `request`.
    ///
    /// `None` means the id is already in flight, is being settled by a completion owner, or
    /// already finished, so this request must not run. Otherwise the returned credential is
    /// the only proof that satisfies [`Self::owns`].
    pub(crate) fn begin(
        &mut self,
        invocation: CommandInvocationId,
        request: CorrelationId,
    ) -> Option<CommandClaim> {
        if self.entries.contains_key(&invocation)
            || self.in_flight.contains_key(&invocation)
            || self.pending.contains(&invocation)
        {
            return None;
        }
        self.next_generation = self.next_generation.wrapping_add(1);
        let generation = self.next_generation;
        self.in_flight.insert(invocation, InFlightClaim { request, generation });
        Some(CommandClaim { invocation, request, generation })
    }

    /// Whether `claim` is the live claim for its invocation id.
    ///
    /// False for a credential that was never issued, for one issued to another
    /// request, and for one whose id has since been claimed again or settled.
    #[must_use]
    pub(crate) fn owns(&self, claim: &CommandClaim) -> bool {
        self.in_flight.get(&claim.invocation).is_some_and(|live| {
            live.request == claim.request && live.generation == claim.generation
        })
    }

    /// Release a claim without recording an outcome, for a request that never
    /// reached a handler. Nothing could have happened, so the id is free again —
    /// but only the credential that owns the live claim may do this.
    pub(crate) fn abandon(&mut self, claim: &CommandClaim) {
        if self.owns(claim) {
            self.in_flight.remove(&claim.invocation);
        }
    }

    /// Settle a claim for a command that ran but whose result was never recorded:
    /// the effect is *unknown*, and it stays that way.
    ///
    /// The record is what a later replay conflicts with, so this is the honest
    /// answer to `runtime.command_outcome` and the reason a replay cannot run the
    /// same command a second time.
    pub(crate) fn record_unknown(&mut self, claim: &CommandClaim) {
        if !self.owns(claim) {
            // The handler already recorded an outcome for this attempt, or this
            // credential is stale. Either way, overwriting would invent a result.
            return;
        }
        self.insert(
            claim.invocation,
            CommandOutcome::Unknown {
                command_invocation_id: claim.invocation,
                reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
            },
        );
    }

    /// Whether `id` is executing right now, so a refusal can say which it is.
    #[must_use]
    pub(crate) fn is_in_flight(&self, id: CommandInvocationId) -> bool {
        self.in_flight.contains_key(&id)
    }

    /// Whether the recorded outcome for `id` exists but says nothing about the
    /// result, so a refusal can tell the caller to query instead of replaying.
    #[must_use]
    pub(crate) fn recorded_outcome_is_unknown(&self, id: CommandInvocationId) -> bool {
        matches!(self.entries.get(&id), Some(CommandOutcome::Unknown { .. }))
    }

    pub(crate) fn query(&self, id: CommandInvocationId) -> CommandOutcome {
        self.entries.get(&id).cloned().unwrap_or(CommandOutcome::Unknown {
            command_invocation_id: id,
            reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
        })
    }

    /// Why a request may not execute this id, when it may not.
    fn refusal_for(&self, id: CommandInvocationId) -> Option<CommandInvocationRefusal> {
        if self.is_in_flight(id) || self.pending.contains(&id) {
            // A started command whose result is still owed is still executing as far as a
            // second attempt is concerned: its refusal must say "await its outcome", not
            // "it already completed".
            Some(CommandInvocationRefusal::InFlight)
        } else if self.recorded_outcome_is_unknown(id) {
            Some(CommandInvocationRefusal::Unknown)
        } else if self.entries.contains_key(&id) {
            Some(CommandInvocationRefusal::Settled)
        } else {
            None
        }
    }
}

/// How a claimed command ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandSettlement {
    /// The command was handed to a handler: it may have had an effect, so an
    /// unrecorded result must stay *unknown* rather than be released.
    Ran,
    /// The request never reached a handler (refused for capacity, or dropped
    /// while still queued): nothing could have happened, so the claim is free.
    NeverRan,
}

/// Close out a command claim.
///
/// This is the rule that keeps a replay from executing a command twice while
/// still not inventing a result: a claim for work that never ran is released, a
/// claim for work that ran without a recorded outcome becomes an explicit
/// `Unknown` — which is both the honest answer to `runtime.command_outcome` and
/// what makes the next replay a conflict instead of a second execution.
pub(crate) fn settle_command_claim(
    state: &RuntimeState,
    claim: &CommandClaim,
    settlement: CommandSettlement,
) {
    let Ok(mut outcomes) = state.command_outcomes.lock() else {
        // A poisoned ledger cannot be trusted to record anything; leaving the
        // claim in place fails closed.
        return;
    };
    match settlement {
        CommandSettlement::NeverRan => outcomes.abandon(claim),
        CommandSettlement::Ran => outcomes.record_unknown(claim),
    }
}

/// Outcome of claiming a side-effecting request's CommandInvocationId.
pub(crate) enum CommandReservation {
    /// The request may run. The credential is present when this command needs one.
    Granted(Option<CommandClaim>),
    /// The id cannot be executed by this request, and the refusal has been
    /// answered. The request must not run.
    Refused,
}

/// Claim the right to execute a side-effecting request before it is dispatched.
///
/// This is the single entry point for taking a claim: the read loop uses it
/// before admitting a request, and an internal caller that reaches the handler
/// directly must use it too. Two requests that carry the same
/// CommandInvocationId cannot both execute — the second is refused with a
/// Conflict that says which of the two situations it is — and a duplicate's
/// refusal neither releases nor overwrites the original attempt.
pub(crate) async fn reserve_command_invocation(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    request: &JsonRpcRequest,
) -> Result<CommandReservation> {
    if !needs_command_claim(&request.method) {
        return Ok(CommandReservation::Granted(None));
    }
    let Some(command_invocation_id) = request.command_invocation_id else {
        let error = IpcErrorEnvelope {
            code: ErrorCode::new("ipc.command_invocation_id_required")?,
            category: ErrorCategory::Validation,
            message: "这次请求缺少操作编号，Aura Core 没有执行它；请重试。".to_owned(),
            retryable: false,
            correlation_id: request.id,
            terminal: IpcRequestTerminal::Failed,
            details: BTreeMap::new(),
        };
        // Non-blocking like every other reader-side answer: a full partition
        // must end the connection, not park the reader.
        try_send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request.id, error))?;
        return Ok(CommandReservation::Refused);
    };
    let claim = {
        let mut outcomes = state
            .command_outcomes
            .lock()
            .map_err(|_| anyhow::anyhow!("command outcome ledger was poisoned"))?;
        outcomes.begin(command_invocation_id, request.id)
    };
    let Some(claim) = claim else {
        let refusal = state
            .command_outcomes
            .lock()
            .map_err(|_| anyhow::anyhow!("command outcome ledger was poisoned"))?
            .refusal_for(command_invocation_id);
        let error = command_invocation_refusal(
            request.id,
            command_invocation_id,
            refusal.unwrap_or(CommandInvocationRefusal::InFlight),
        )?;
        try_send_json(queue, QueuePriority::P0, &JsonRpcResponse::failure(request.id, error))?;
        return Ok(CommandReservation::Refused);
    };
    Ok(CommandReservation::Granted(Some(claim)))
}

/// Whether this method's execution must be claimed before it may run.
///
/// tool-mutation commands are excluded: they own their own command ledger,
/// and a second claim around them would be two answers to one question.
#[must_use]
pub(crate) fn needs_command_claim(method_name: &str) -> bool {
    aura_ipc::routes::records_command_outcome(method_name)
        && !p07_commands::ToolMutationControlServiceV1::handles_main_command(method_name)
}

/// Which frozen refusal a repeated CommandInvocationId gets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandInvocationRefusal {
    /// The original command is still executing.
    InFlight,
    /// The original command finished and its outcome was recorded.
    Settled,
    /// The original command finished without a recorded outcome.
    Unknown,
}

/// The refusal for a CommandInvocationId that cannot be used again.
///
/// Only two codes exist for this, and their meanings are frozen: an id that is
/// still executing is ipc.command_invocation_in_flight (Conflict), and an id
/// whose attempt is over is ipc.command_invocation_already_completed
/// (Conflict). The message distinguishes a recorded outcome from one that is
/// unknown, so the caller knows whether to query a result or treat the effect as
/// undetermined — without inventing a third code.
pub(crate) fn command_invocation_refusal(
    request_id: CorrelationId,
    command_invocation_id: CommandInvocationId,
    refusal: CommandInvocationRefusal,
) -> Result<IpcErrorEnvelope> {
    let mut details = BTreeMap::new();
    details
        .insert("command_invocation_id".to_owned(), serde_json::to_value(command_invocation_id)?);
    let (code, message) = match refusal {
        CommandInvocationRefusal::InFlight => {
            ("ipc.command_invocation_in_flight", "这项操作正在执行，不会再开始一次；请等它的结果。")
        }
        CommandInvocationRefusal::Settled => (
            "ipc.command_invocation_already_completed",
            "这项操作已经完成过，不会重复执行；请刷新查看结果。",
        ),
        CommandInvocationRefusal::Unknown => (
            "ipc.command_invocation_already_completed",
            "这项操作已经尝试过，但结果没有记录下来，是否生效未知；请先刷新核对当前状态，\
             不要另起一次同样的操作。",
        ),
    };
    Ok(IpcErrorEnvelope {
        code: ErrorCode::new(code)?,
        category: ErrorCategory::Conflict,
        message: message.to_owned(),
        retryable: false,
        correlation_id: request_id,
        terminal: IpcRequestTerminal::Failed,
        details,
    })
}

/// The refusal for a request that reached the handler carrying no valid claim.
///
/// This is an internal invariant, not a client-facing state: every execution path
/// takes its claim through [`reserve_command_invocation`], so a request without
/// one did not come through the runtime's own dispatch.
pub(crate) fn missing_claim_error(request_id: CorrelationId) -> Result<IpcErrorEnvelope> {
    Ok(IpcErrorEnvelope {
        code: ErrorCode::new("internal.unexpected")?,
        category: ErrorCategory::Internal,
        message: "This command reached its handler without the claim that authorises it; it was \
                  not executed."
            .to_owned(),
        retryable: false,
        correlation_id: request_id,
        terminal: IpcRequestTerminal::Failed,
        details: BTreeMap::new(),
    })
}

/// Why the handler refused a command, if it did.
pub(crate) enum ClaimVerdict {
    /// The request owns the live claim: it may execute.
    Owned,
    /// The id is executing for another request, or already finished.
    Refused(CommandInvocationRefusal),
    /// No valid claim was presented and nothing is in flight for the id.
    Missing,
}

/// Decide whether `request` may execute, given the credential it arrived with.
///
/// `Some(claim)` must be the live claim for exactly this request and id; every
/// other case falls back to the ledger's own answer, so a borrowed, stale,
/// invented or absent credential can never authorise an execution.
#[must_use]
pub(crate) fn claim_verdict(
    ledger: &CommandOutcomeLedger,
    request_id: CorrelationId,
    command_invocation_id: CommandInvocationId,
    presented: Option<&CommandClaim>,
) -> ClaimVerdict {
    if presented
        .is_some_and(|claim| claim.covers(request_id, command_invocation_id) && ledger.owns(claim))
    {
        return ClaimVerdict::Owned;
    }
    match ledger.refusal_for(command_invocation_id) {
        Some(refusal) => ClaimVerdict::Refused(refusal),
        None => ClaimVerdict::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unknown(id: CommandInvocationId) -> CommandOutcome {
        CommandOutcome::Unknown {
            command_invocation_id: id,
            reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
        }
    }

    /// A started command's row is NOT evicted by the bounded outcome ring, and its id cannot run
    /// again while a completion owner is answerable for it.
    ///
    /// Both properties are what keep "the caller stopped waiting" from becoming "run it again": an
    /// evicted row would let `begin` grant the id a second time, which is the replay hazard the
    /// pending mark exists to close.
    #[test]
    fn a_started_commands_row_survives_the_ring_and_its_id_cannot_run_twice() {
        let mut ledger = CommandOutcomeLedger::default();
        let started = CommandInvocationId::new();
        let request = CorrelationId::new();
        let claim = ledger.begin(started, request).expect("the id starts free");
        ledger.claim_pending(started, unknown(started));
        assert!(ledger.is_pending(started));
        assert!(ledger.owns(&claim));

        // Fill the ring well past its capacity with unrelated settled commands.
        for _ in 0..(MAX_COMMAND_OUTCOMES + 64) {
            let other = CommandInvocationId::new();
            ledger.insert(
                other,
                CommandOutcome::Confirmed {
                    command_invocation_id: other,
                    terminal: IpcRequestTerminal::Succeeded,
                    response: serde_json::json!({"settled": true}),
                },
            );
        }
        assert!(
            matches!(ledger.query(started), CommandOutcome::Unknown { .. }),
            "the started command's own row must still be there after the ring moved past it"
        );
        assert!(
            ledger.begin(started, CorrelationId::new()).is_none(),
            "an id a completion owner answers for must not be granted a second execution"
        );
        assert_eq!(ledger.refusal_for(started), Some(CommandInvocationRefusal::InFlight));

        // The owner writes the real result: the row becomes final and the id stays refused.
        ledger.settle_pending(
            started,
            CommandOutcome::Confirmed {
                command_invocation_id: started,
                terminal: IpcRequestTerminal::Succeeded,
                response: serde_json::json!({"settled": true}),
            },
        );
        assert!(!ledger.is_pending(started));
        assert!(ledger.begin(started, CorrelationId::new()).is_none());
        assert_eq!(ledger.refusal_for(started), Some(CommandInvocationRefusal::Settled));
    }

    /// A proven pre-execution refusal returns the id to "never attempted"; a started command never
    /// does, and its row stays an honest Unknown.
    #[test]
    fn only_a_proven_refusal_frees_the_id_again() {
        let mut ledger = CommandOutcomeLedger::default();
        let never_ran = CommandInvocationId::new();
        ledger.claim_pending(never_ran, unknown(never_ran));
        ledger.abandon_pending(never_ran);
        assert!(ledger.begin(never_ran, CorrelationId::new()).is_some(), "a refusal frees the id");

        let started = CommandInvocationId::new();
        ledger.claim_pending(started, unknown(started));
        ledger.clear_pending(started);
        assert!(
            ledger.begin(started, CorrelationId::new()).is_none(),
            "a started command keeps its attempt: releasing it would run the business twice"
        );
        assert_eq!(ledger.refusal_for(started), Some(CommandInvocationRefusal::Unknown));
    }
}
