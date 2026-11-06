//! What a message referred to with "@", as Core resolved it when the message was sent. It is
//! kept by the message's id, which is its user event's once it is part of the conversation, and
//! names the Session, so a queued message that never became part of it goes with it. Another
//! conversation is named by its handle, so deleting that one leaves this record alone.
use super::facts;
use aura_agent_domain::ResolvedMessageReference;
use aura_domain_common::{ConversationHandle, EventId, SessionId, UtcTimestamp};
use aura_persistence_core::{StoreResult, record};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

const KIND: &str = "agent.message_references";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Referred {
    session_id: SessionId,
    references: Vec<ResolvedMessageReference>,
}

/// What a message referred to when it was sent; none for a message without.
pub fn of_message(db: &Connection, message: EventId) -> StoreResult<Vec<ResolvedMessageReference>> {
    Ok(facts::latest::<Referred>(db, &message.to_string(), KIND)?
        .map(|value| value.references)
        .unwrap_or_default())
}

/// Records what a message refers to, when it is sent.
pub fn set(
    tx: &Transaction<'_>,
    message: EventId,
    session: SessionId,
    references: &[ResolvedMessageReference],
    at: UtcTimestamp,
) -> StoreResult<()> {
    if references.is_empty() {
        return Ok(());
    }
    facts::append(
        tx,
        EventId::new(),
        &message.to_string(),
        KIND,
        &Referred { session_id: session, references: references.to_vec() },
        at,
    )
}

/// Whether a user message of `session` referred to the conversation `named`: one that is part of
/// the conversation, not one still queued or removed from the queue.
pub fn refers_to(
    db: &Connection,
    session: SessionId,
    named: &ConversationHandle,
) -> StoreResult<bool> {
    let mut statement = db.prepare(
        "SELECT r.payload FROM events u \
         JOIN events r ON r.owner = u.event_id AND r.kind = ?2 AND r.version = 1 \
         WHERE u.owner = ?1 AND u.kind = 'agent.user_message' AND u.version = 1 \
         AND instr(r.payload, ?3) > 0",
    )?;
    let rows = statement.query_map(
        rusqlite::params![format!("agent-user:{session}"), KIND, named.as_str()],
        |row| row.get::<_, String>(0),
    )?;
    for raw in rows {
        let referred: Referred = record::decode(&raw?)?;
        if referred.session_id == session
            && referred.references.iter().any(|reference| reference.conversation() == Some(named))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
