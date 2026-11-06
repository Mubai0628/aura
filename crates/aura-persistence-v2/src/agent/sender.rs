//! Who sent a message the user did not type: a plugin's panel or background service. It is kept
//! by the message's id, which is its user event's once it is part of the conversation, and names
//! the Session, so it goes with it.
use super::facts;
use aura_agent_domain::MessageSender;
use aura_domain_common::{EventId, SessionId, UtcTimestamp};
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};

const KIND: &str = "agent.message_sender";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Sent {
    session_id: SessionId,
    sender: MessageSender,
}

/// Who sent the message; none for one the user typed.
pub fn of_message(db: &Connection, message: EventId) -> StoreResult<Option<MessageSender>> {
    Ok(facts::latest::<Sent>(db, &message.to_string(), KIND)?.map(|sent| sent.sender))
}

/// Records who sent a message, when it is sent.
pub fn set(
    tx: &Transaction<'_>,
    message: EventId,
    session: SessionId,
    sender: &MessageSender,
    at: UtcTimestamp,
) -> StoreResult<()> {
    facts::append(
        tx,
        EventId::new(),
        &message.to_string(),
        KIND,
        &Sent { session_id: session, sender: sender.clone() },
        at,
    )
}
