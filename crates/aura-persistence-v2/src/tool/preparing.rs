//! When Core began preparing each call of a batch, as a real per-call fact.
//!
//! Preparation happens before an Attempt is reserved — reserving earlier would make
//! readiness, resource locks and preparation itself hold execution capacity, which the
//! scheduler deliberately avoids. Without a per-call fact for that window, a call Core is
//! already working on is indistinguishable from one still waiting for a slot, and that is
//! what the projection showed during a slow backend selection.
//!
//! This is a timestamp, not an Attempt: it claims no capacity, is written before anything
//! is dispatched, and never lets a caller treat preparation as execution. Loading is the
//! latest mark per call, so a restart or a repeated prepare refines the fact instead of
//! duplicating it.
use aura_domain_common::{EventId, ToolBatchId, ToolCallId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event};
use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const KIND: &str = "tool.call_preparing";
/// The mark written when a real execution slot is activated for the call.
///
/// Preparing and executing are different facts, and the difference is what the projection
/// has to show: backend selection, readiness and a wait for capacity all happen *before* an
/// Attempt is reserved, and a call in that window has not started executing — it is still
/// waiting its turn. Reporting Running from the preparing mark alone made a queued call look
/// like a running one for as long as the wait lasted.
const EXECUTING_KIND: &str = "tool.call_executing";
const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PreparingMark {
    call_id: ToolCallId,
    at: UtcTimestamp,
}

/// Records that preparation of one call has begun, keeping the earliest mark.
pub fn mark(
    tx: &Transaction<'_>,
    batch: ToolBatchId,
    call_id: ToolCallId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if load(tx, batch)?.contains_key(&call_id) {
        // The first mark is the start fact; a later prepare for the same call does not
        // make it have started later.
        return Ok(());
    }
    let value = PreparingMark { call_id, at };
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &batch.to_string(),
            kind: KIND,
            version: VERSION,
            payload: &serde_json::to_value(value)?,
            at,
        },
        "tool",
    )?;
    Ok(())
}

/// The earliest preparation mark of every call of one batch.
pub fn load(
    db: &Connection,
    batch: ToolBatchId,
) -> StoreResult<BTreeMap<ToolCallId, UtcTimestamp>> {
    load_kind(db, batch, KIND)
}

/// Records that a real execution slot was activated for one call, keeping the first mark.
///
/// This is the fact that means "executing": an Attempt has been reserved for the call, so it
/// is no longer waiting for capacity or for a resource.
pub fn mark_executing(
    tx: &Transaction<'_>,
    batch: ToolBatchId,
    call_id: ToolCallId,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if load_executing(tx, batch)?.contains_key(&call_id) {
        return Ok(());
    }
    let value = PreparingMark { call_id, at };
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &batch.to_string(),
            kind: EXECUTING_KIND,
            version: VERSION,
            payload: &serde_json::to_value(value)?,
            at,
        },
        "tool",
    )?;
    Ok(())
}

/// The first executing mark of every call of one batch.
pub fn load_executing(
    db: &Connection,
    batch: ToolBatchId,
) -> StoreResult<BTreeMap<ToolCallId, UtcTimestamp>> {
    load_kind(db, batch, EXECUTING_KIND)
}

fn load_kind(
    db: &Connection,
    batch: ToolBatchId,
    kind: &str,
) -> StoreResult<BTreeMap<ToolCallId, UtcTimestamp>> {
    let mut statement = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=?3 ORDER BY sequence",
    )?;
    let rows = statement
        .query_map([batch.to_string(), kind.to_owned(), VERSION.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
    let mut marks = BTreeMap::new();
    for row in rows {
        let mark: PreparingMark = serde_json::from_str(&row?).map_err(|_| Error::Conflict)?;
        marks.entry(mark.call_id).or_insert(mark.at);
    }
    Ok(marks)
}
