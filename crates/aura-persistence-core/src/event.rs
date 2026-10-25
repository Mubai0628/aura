use crate::{StoreResult, record};
use aura_domain_common::{EventId, UtcTimestamp};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;

pub struct EventDraft<'a> {
    pub id: EventId,
    pub owner: &'a str,
    pub kind: &'a str,
    pub version: u32,
    pub payload: &'a Value,
    pub at: UtcTimestamp,
}

pub fn append(tx: &Transaction<'_>, event: &EventDraft<'_>, topic: &str) -> StoreResult<i64> {
    let payload = record::encode(event.payload)?;
    tx.execute("INSERT INTO events(event_id,owner,kind,version,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![event.id.to_string(),event.owner,event.kind,event.version,payload,event.at.to_string()])?;
    let revision: i64 = tx.query_row("INSERT INTO projection_revisions VALUES(?1,1) ON CONFLICT(topic) DO UPDATE SET revision=revision+1 RETURNING revision", [topic], |r| r.get(0))?;
    tx.execute(
        "INSERT INTO projection_outbox(event_id,topic,revision,payload) VALUES(?1,?2,?3,?4)",
        params![event.id.to_string(), topic, revision, payload],
    )?;
    Ok(tx.last_insert_rowid())
}

#[derive(Debug, PartialEq)]
pub struct OutboxEntry {
    pub sequence: i64,
    pub topic: String,
    pub revision: i64,
    pub payload: Value,
}

/// At-least-once delivery. Acknowledge only after consumers accept sequence and
/// revision. Reopening replays unacknowledged rows, never an execution action.
pub fn pending(connection: &Connection, limit: u32) -> StoreResult<Vec<OutboxEntry>> {
    let mut query=connection.prepare("SELECT sequence,topic,revision,payload FROM projection_outbox WHERE acknowledged_at IS NULL ORDER BY sequence LIMIT ?1")?;
    let rows = query.query_map([limit.min(1000)], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?))
    })?;
    rows.map(|r| {
        let (sequence, topic, revision, payload) = r?;
        Ok(OutboxEntry { sequence, topic, revision, payload: record::decode(&payload)? })
    })
    .collect()
}

pub fn acknowledge(tx: &Transaction<'_>, sequence: i64, at: UtcTimestamp) -> StoreResult<()> {
    let exists: Option<i64> = tx
        .query_row("SELECT sequence FROM projection_outbox WHERE sequence=?1", [sequence], |r| {
            r.get(0)
        })
        .optional()?;
    if exists.is_none() {
        return Err(crate::PersistenceCoreError::Conflict);
    }
    tx.execute("UPDATE projection_outbox SET acknowledged_at=COALESCE(acknowledged_at,?2) WHERE sequence=?1",params![sequence,at.to_string()])?;
    Ok(())
}
