use crate::{StoreResult, record};
use aura_domain_common::{EventId, UtcTimestamp};
use rusqlite::{Transaction, params};
use serde_json::Value;

pub struct EventDraft<'a> {
    pub id: EventId,
    pub owner: &'a str,
    pub kind: &'a str,
    pub version: u32,
    pub payload: &'a Value,
    pub at: UtcTimestamp,
}

/// Appends one audit event. It is written once: nothing reads a second copy, so the outbox an
/// older version still fills is left empty here.
pub fn append(tx: &Transaction<'_>, event: &EventDraft<'_>, _topic: &str) -> StoreResult<i64> {
    let payload = record::encode(event.payload)?;
    tx.execute("INSERT INTO events(event_id,owner,kind,version,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![event.id.to_string(),event.owner,event.kind,event.version,payload,event.at.to_string()])?;
    Ok(tx.last_insert_rowid())
}
