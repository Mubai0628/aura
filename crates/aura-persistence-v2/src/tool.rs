use aura_domain_common::{
    SecurityOperationId, SessionId, ToolBatchId, ToolCallId, ToolRegistrySnapshotId,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_tool_contract::ToolRegistrySnapshotV1;
use rusqlite::{Connection, Transaction, params};
pub mod activity;
pub mod admission;
pub(crate) mod call_name;
pub mod configuration;
pub mod definitions;
pub mod entry;
pub mod execution;
pub mod exposure;
pub mod external_change;
pub mod preparing;
mod registry_cache;
pub mod result;
pub mod session_tools;

pub fn insert_registry(tx: &Transaction<'_>, snapshot: &ToolRegistrySnapshotV1) -> StoreResult<()> {
    snapshot.validate().map_err(|_| Error::Invalid("tool registry snapshot"))?;
    tx.execute(
        "INSERT INTO tool_registry_snapshots VALUES(?1,?2,?3)",
        params![
            snapshot.snapshot_ref.tool_registry_snapshot_id.to_string(),
            record::encode(&snapshot.snapshot_ref.content_hash)?,
            record::encode(snapshot)?
        ],
    )?;
    Ok(())
}
/// A stored registry snapshot, shared: it never changes once stored.
pub fn registry(
    connection: &Connection,
    id: ToolRegistrySnapshotId,
) -> StoreResult<Option<std::sync::Arc<ToolRegistrySnapshotV1>>> {
    registry_cache::registry(connection, id)
}
pub fn insert_batch(
    tx: &Transaction<'_>,
    id: ToolBatchId,
    session: SessionId,
    registry: ToolRegistrySnapshotId,
    payload: &serde_json::Value,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO tool_batches VALUES(?1,?2,?3,'prepared',1,?4)",
        params![
            id.to_string(),
            session.to_string(),
            registry.to_string(),
            record::encode(payload)?
        ],
    )?;
    Ok(())
}
pub fn insert_call(
    tx: &Transaction<'_>,
    id: ToolCallId,
    batch: ToolBatchId,
    ordinal: u32,
    operation: SecurityOperationId,
    payload: &serde_json::Value,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO tool_calls VALUES(?1,?2,?3,?4,?5)",
        params![
            id.to_string(),
            batch.to_string(),
            ordinal,
            operation.to_string(),
            record::encode(payload)?
        ],
    )?;
    Ok(())
}
