//! A step sent through one of
//! the provider's backup connections, because the connection before it failed before answering. The
//! conversation shows it with the step.
use aura_domain_common::{EventId, ModelStepId, UtcTimestamp};
use aura_persistence_core::{StoreResult, event, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

const KIND: &str = "model.connection_backup";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    connection: String,
}

/// The step goes through `connection`, a backup.
pub fn record_backup(
    tx: &Transaction<'_>,
    step: ModelStepId,
    connection: &str,
    at: UtcTimestamp,
) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &step.to_string(),
            kind: KIND,
            version: 1,
            payload: &serde_json::to_value(Backup { connection: connection.to_owned() })?,
            at,
        },
        "agent",
    )?;
    Ok(())
}

/// How many of a step's first events are read for its backup connection.
const FIRST_EVENTS: u32 = 64;

/// The backup connection the step went through, if it did.
///
/// The choice is recorded while the step is
/// prepared, before its request and anything it streams, so it is among the step's first events.
/// Only those are read: looking from the newest down read every streamed piece of a step that had
/// none (13,000 in the owner's longest), for each step a conversation showed — most of the first
/// open of a long conversation.
pub fn backup_of(db: &Connection, step: ModelStepId) -> StoreResult<Option<String>> {
    let raw: Option<String> = db
        .query_row(
            // The window reads kinds only; the one payload wanted is read by its place.
            "SELECT payload FROM events WHERE owner=?1 AND sequence=(SELECT sequence FROM \
             (SELECT sequence, kind FROM events WHERE owner=?1 ORDER BY sequence ASC LIMIT ?3) \
             WHERE kind=?2 ORDER BY sequence DESC LIMIT 1)",
            params![step.to_string(), KIND, FIRST_EVENTS],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw.map(|text| record::decode::<Backup>(&text)).transpose()?.map(|b| b.connection))
}
