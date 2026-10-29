use aura_context_domain::{
    ContextProjectionSnapshot, HistoryAtomicGroupPreimageV1, PromptBundleSnapshot,
};
use aura_domain_common::{ContextProjectionSnapshotId, SessionId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
pub mod compaction;
pub mod manual_compaction;
pub mod read;
pub mod remediation;

pub fn insert_snapshot(
    tx: &Transaction<'_>,
    snapshot: &ContextProjectionSnapshot,
) -> StoreResult<()> {
    snapshot.validate_contract_shape().map_err(|_| Error::Invalid("context snapshot"))?;
    tx.execute(
        "INSERT INTO context_snapshots VALUES(?1,?2,?3,?4)",
        params![
            snapshot.id.to_string(),
            snapshot.session_id.to_string(),
            snapshot.tool_schema_set.registry_snapshot_ref.tool_registry_snapshot_id.to_string(),
            record::encode(snapshot)?
        ],
    )?;
    keep_two(tx, snapshot)
}

/// How many older snapshots one new snapshot clears at most. A store that kept every step's
/// snapshot is cleared as a whole before Core serves (`slimming`); here it is one at a time.
const CLEARED_PER_SNAPSHOT: i64 = 4;

/// A conversation keeps two full snapshots: the newest, and the one behind its newest counted
/// request, which the next request is measured by (`agent::counted_request`). An older step's
/// context details show the figures its inspector record keeps; its section texts go with it.
fn keep_two(tx: &Transaction<'_>, newest: &ContextProjectionSnapshot) -> StoreResult<()> {
    let counted = crate::agent::counted_request::latest_counted_snapshot(tx, newest.session_id)?;
    let mut query = tx.prepare(
        "SELECT id, json_extract(payload,'$.model_step_id'), json_extract(payload,'$.sections') \
         FROM context_snapshots WHERE session_id=?1 AND id<>?2 AND (?3 IS NULL OR id<>?3) \
         ORDER BY rowid LIMIT ?4",
    )?;
    let older = query
        .query_map(
            params![
                newest.session_id.to_string(),
                newest.id.to_string(),
                counted,
                CLEARED_PER_SNAPSHOT
            ],
            |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    drop(query);
    if older.is_empty() {
        return Ok(());
    }
    for (id, step, sections) in older {
        clear_snapshot(tx, &id, &step, &sections)?;
    }
    aura_persistence_core::artifact::collect(tx)?;
    Ok(())
}

/// Deletes one snapshot and gives up its step's hold on the section texts it was built from.
pub(crate) fn clear_snapshot(
    tx: &Transaction<'_>,
    id: &str,
    step: &str,
    sections: &str,
) -> StoreResult<()> {
    let sections: Vec<serde_json::Value> = serde_json::from_str(sections)?;
    for section in sections {
        let Some(artifact) = section["content_artifact_id"].as_str() else { continue };
        let artifact = artifact.parse().map_err(|_| Error::Invalid("section artifact"))?;
        aura_persistence_core::artifact::release_owner(tx, artifact, step)?;
    }
    tx.execute("DELETE FROM context_snapshots WHERE id=?1", [id])?;
    Ok(())
}
pub fn snapshot(
    connection: &Connection,
    id: ContextProjectionSnapshotId,
) -> StoreResult<Option<ContextProjectionSnapshot>> {
    let v: Option<String> = connection
        .query_row("SELECT payload FROM context_snapshots WHERE id=?1", [id.to_string()], |r| {
            r.get(0)
        })
        .optional()?;
    v.map(|v| {
        let s: ContextProjectionSnapshot = record::decode(&v)?;
        s.validate_contract_shape().map_err(|_| Error::Invalid("context snapshot"))?;
        Ok(s)
    })
    .transpose()
}
pub fn insert_bundle(
    tx: &Transaction<'_>,
    session: SessionId,
    bundle: &PromptBundleSnapshot,
) -> StoreResult<()> {
    bundle.validate_contract_shape().map_err(|_| Error::Invalid("prompt bundle"))?;
    tx.execute(
        "INSERT INTO prompt_bundles VALUES(?1,?2,?3,?4)",
        params![
            bundle.id.to_string(),
            session.to_string(),
            record::encode(&bundle.bundle_manifest_hash)?,
            record::encode(bundle)?
        ],
    )?;
    Ok(())
}
pub fn append_history(
    tx: &Transaction<'_>,
    group: &HistoryAtomicGroupPreimageV1,
) -> StoreResult<()> {
    let key = group.group_ref().map_err(|_| Error::Invalid("history atomic group"))?;
    let previous: Option<String> = tx.query_row("SELECT payload FROM history_atomic_groups WHERE json_extract(payload,'$.tool_batch_id')=?1", [group.tool_batch_id.to_string()], |r| r.get(0)).optional()?;
    if let Some(previous) = previous {
        if record::decode::<HistoryAtomicGroupPreimageV1>(&previous)? == *group {
            return Ok(());
        }
        return Err(Error::Conflict);
    }
    tx.execute("INSERT INTO history_atomic_groups SELECT ?1,COALESCE(MAX(ordinal),0)+1,?2,?3 FROM history_atomic_groups WHERE session_id=?1",params![group.session_id.to_string(),record::encode(&key)?,record::encode(group)?])?;
    Ok(())
}
pub fn history(
    connection: &Connection,
    session: SessionId,
) -> StoreResult<Vec<HistoryAtomicGroupPreimageV1>> {
    let mut query = connection.prepare(
        "SELECT payload FROM history_atomic_groups WHERE session_id=?1 ORDER BY ordinal",
    )?;
    query
        .query_map([session.to_string()], |r| r.get::<_, String>(0))?
        .map(|v| record::decode(&v?))
        .collect()
}
