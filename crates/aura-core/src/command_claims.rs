//! Taking and settling the command claims of the requests on a connection. The ledger the
//! claims are taken from is `aura_core_base::command_claims`.
use std::collections::BTreeMap;

use anyhow::Result;
use aura_domain_common::{CommandInvocationId, CorrelationId, ErrorCategory, ErrorCode};
use aura_ipc::{AsyncPriorityQueue, JsonRpcRequest, JsonRpcResponse, QueuePriority};
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};

#[cfg(test)]
pub(crate) use aura_core_base::command_claims::MAX_COMMAND_OUTCOMES;
pub(crate) use aura_core_base::command_claims::{
    CommandClaim, CommandFlights, CommandInvocationRefusal, CommandOutcomeLedger,
};

use crate::{RuntimeState, try_send_json};
use aura_tool_service::p07_commands;

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
