//! A tool registry snapshot never changes once stored, but turning one into its
//! checked form costs about 38 ms in a debug build, and every read of a tool call's row, its
//! result, its admission and its hooks did it again. Reading a long conversation's tool rows held
//! the store for seconds. The last few decoded snapshots are kept by identity and fingerprint; a
//! row that is gone, or holds another fingerprint, is read from the store again.
use aura_domain_common::ToolRegistrySnapshotId;
use aura_persistence_core::{StoreResult, record};
use aura_tool_contract::ToolRegistrySnapshotV1;
use rusqlite::{Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

const KEPT: usize = 16;

type Kept = Vec<(ToolRegistrySnapshotId, String, Arc<ToolRegistrySnapshotV1>)>;
static DECODED: Mutex<Kept> = Mutex::new(Vec::new());

pub(super) fn registry(
    connection: &Connection,
    id: ToolRegistrySnapshotId,
) -> StoreResult<Option<Arc<ToolRegistrySnapshotV1>>> {
    let fingerprint: Option<String> = connection
        .query_row(
            "SELECT fingerprint FROM tool_registry_snapshots WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(fingerprint) = fingerprint else { return Ok(None) };
    if let Ok(mut kept) = DECODED.lock()
        && let Some(position) = kept.iter().position(|(i, f, _)| *i == id && *f == fingerprint)
    {
        let hit = kept.remove(position);
        let value = Arc::clone(&hit.2);
        kept.insert(0, hit);
        return Ok(Some(value));
    }
    let payload: String = connection.query_row(
        "SELECT payload FROM tool_registry_snapshots WHERE id=?1",
        [id.to_string()],
        |row| row.get(0),
    )?;
    let value = Arc::new(record::decode::<ToolRegistrySnapshotV1>(&payload)?);
    if let Ok(mut kept) = DECODED.lock() {
        kept.insert(0, (id, fingerprint, Arc::clone(&value)));
        kept.truncate(KEPT);
    }
    Ok(Some(value))
}
