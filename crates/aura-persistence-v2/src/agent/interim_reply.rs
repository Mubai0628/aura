//! A user-visible reply and its tool result commit in the same unit of work.
use super::{facts, turns};
use aura_agent_domain::{SessionReplyInput, SessionReplyReceipt};
use aura_domain_common::{EventId, ModelStepId, ToolCallId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_projection_contract::AgentInterimReplyView;
use aura_tool_contract::CoreInvocationIdentityV1;
use rusqlite::{Connection, Transaction, params};

const KIND: &str = "agent.interim_reply";

pub(super) fn send(
    tx: &Transaction<'_>,
    identity: &CoreInvocationIdentityV1,
    call_id: ToolCallId,
    input: SessionReplyInput,
    at: UtcTimestamp,
) -> StoreResult<SessionReplyReceipt> {
    if turns::load_run(tx, identity.agent_run_id)?.parent_agent_run_id.is_some() {
        return Err(Error::Invalid("session.reply.parent_only"));
    }
    if !input.valid() {
        return Err(Error::Invalid("session.reply.invalid_text"));
    }
    let event_id = EventId::from_bytes(call_id.to_bytes());
    let message = AgentInterimReplyView { tool_call_id: call_id, text: input.text, at };
    facts::append(tx, event_id, &identity.model_step_id.to_string(), KIND, &message, at)?;
    Ok(SessionReplyReceipt { event_id })
}

pub(super) fn for_step(
    db: &Connection,
    step: ModelStepId,
) -> StoreResult<Vec<AgentInterimReplyView>> {
    let mut stmt = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=1 ORDER BY sequence",
    )?;
    stmt.query_map(params![step.to_string(), KIND], |row| row.get::<_, String>(0))?
        .map(|raw| record::decode(&raw?))
        .collect()
}
