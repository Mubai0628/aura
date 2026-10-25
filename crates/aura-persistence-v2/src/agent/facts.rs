//! Versioned Agent facts use the existing immutable Event/Outbox protocol.
//! Payloads are domain-owned; no second database or startup schema reset.
use aura_domain_common::{EventId, UtcTimestamp};
use aura_persistence_core::{StoreResult, event, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};

pub(super) fn latest<T: DeserializeOwned>(
    db: &Connection,
    owner: &str,
    kind: &str,
) -> StoreResult<Option<T>> {
    let raw: Option<String> = db.query_row(
        "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=1 ORDER BY sequence DESC LIMIT 1",
        params![owner, kind], |r| r.get(0)).optional()?;
    raw.map(|s| record::decode(&s)).transpose()
}
pub(super) fn append<T: Serialize>(
    tx: &Transaction<'_>,
    id: EventId,
    owner: &str,
    kind: &str,
    value: &T,
    at: UtcTimestamp,
) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft {
            id,
            owner,
            kind,
            version: 1,
            payload: &serde_json::to_value(value)?,
            at,
        },
        "agent",
    )?;
    Ok(())
}
pub(super) fn exact<T: DeserializeOwned>(
    db: &Connection,
    id: EventId,
    owner: &str,
    kind: &str,
) -> StoreResult<T> {
    let raw: String = db.query_row(
        "SELECT payload FROM events WHERE event_id=?1 AND owner=?2 AND kind=?3 AND version=1",
        params![id.to_string(), owner, kind],
        |r| r.get(0),
    )?;
    record::decode(&raw)
}
