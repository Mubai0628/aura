//! Who decided an approval on a chat platform, through one of a plugin's channels. It is kept on
//! the tool call the approval was for, so the conversation shows it on that call's row, and names
//! the Session, so it goes with it.
use super::{approval, facts, turns};
use aura_agent_domain::{AgentRunState, MessageSender};
use aura_domain_common::{ApprovalRequestId, EventId, SessionId, ToolCallId, UtcTimestamp};
use aura_execution_contract::ExecutionApprovalRequest;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_security_contract::ApprovalDecisionKind;
use aura_tool_projection::AgentToolRemoteDecision;
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

const KIND: &str = "agent.approval_decider";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Decided {
    session_id: SessionId,
    sender: MessageSender,
    decision: ApprovalDecisionKind,
}

/// The tool call the approval is for, while its run waits for the answer or runs the calls the
/// answer let go on.
pub fn call_of(
    db: &Connection,
    session: SessionId,
    approval: ApprovalRequestId,
) -> StoreResult<ToolCallId> {
    let view = approval::view(
        db,
        &ExecutionApprovalRequest { session_id: session, approval_id: approval },
    )?;
    let run = view.operation.content.identity.agent_run_id.ok_or(Error::Conflict)?;
    let (AgentRunState::AwaitingApproval { tool_batch_id }
    | AgentRunState::ToolsRunning { tool_batch_id }) = turns::load_run(db, run)?.state
    else {
        return Err(Error::Conflict);
    };
    let admitted = crate::tool::admission::load(db, tool_batch_id)?.ok_or(Error::Conflict)?;
    admitted
        .operations
        .iter()
        .find(|(_, operation)| **operation == view.operation)
        .map(|(call, _)| *call)
        .ok_or(Error::Conflict)
}

/// Records who decided the approval, and how, in the transaction that records the decision.
pub fn set(
    tx: &Transaction<'_>,
    session: SessionId,
    approval: ApprovalRequestId,
    sender: &MessageSender,
    decision: ApprovalDecisionKind,
    at: UtcTimestamp,
) -> StoreResult<()> {
    let call = call_of(tx, session, approval)?;
    let decided = Decided { session_id: session, sender: sender.clone(), decision };
    facts::append(tx, EventId::new(), &call.to_string(), KIND, &decided, at)
}

/// Who decided the call's approval on a chat platform, and how; none for an approval decided in
/// Aura's window, or a call that asked none.
pub fn of_call(db: &Connection, call: ToolCallId) -> StoreResult<Option<AgentToolRemoteDecision>> {
    Ok(facts::latest::<Decided>(db, &call.to_string(), KIND)?.map(|decided| {
        AgentToolRemoteDecision { sender: decided.sender, decision: decided.decision }
    }))
}
