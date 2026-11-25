use aura_agent_domain::TurnTask;
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
    collect_inactive_snapshots(tx, snapshot)
}

/// How many older snapshots one new snapshot clears at most. A store that kept every step's
/// snapshot is cleared as a whole before Core serves (`slimming`); here it is one at a time.
const CLEARED_PER_SNAPSHOT: i64 = 4;

/// A failed or uncounted request must not release the last adopted context. The context of
/// the newest queued step and the last context with measured usage serve different callers.
fn collect_inactive_snapshots(
    tx: &Transaction<'_>,
    newest: &ContextProjectionSnapshot,
) -> StoreResult<()> {
    let counted = crate::agent::counted_request::latest_counted_snapshot(tx, newest.session_id)?;
    let adopted = latest_adopted_snapshot(tx, newest.session_id)?;
    let regular = latest_regular_adopted_snapshot(tx, newest.session_id)?;
    let mut query = tx.prepare(
        "SELECT id, json_extract(payload,'$.model_step_id'), json_extract(payload,'$.sections') \
         FROM context_snapshots WHERE session_id=?1 AND id<>?2 AND (?3 IS NULL OR id<>?3) \
         AND (?4 IS NULL OR id<>?4) AND (?5 IS NULL OR id<>?5) ORDER BY rowid LIMIT ?6",
    )?;
    let older = query
        .query_map(
            params![
                newest.session_id.to_string(),
                newest.id.to_string(),
                counted,
                adopted,
                regular,
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

/// Only the committed, normalized outcome adopts a projection. Provider usage is optional.
pub(crate) fn latest_adopted_snapshot(
    db: &Connection,
    session: SessionId,
) -> StoreResult<Option<String>> {
    adopted_snapshot(db, session, None, false)
}

pub(crate) fn latest_regular_adopted_snapshot(
    db: &Connection,
    session: SessionId,
) -> StoreResult<Option<String>> {
    adopted_snapshot(db, session, None, true)
}

pub(crate) fn latest_adopted_in_scope(
    db: &Connection,
    turn: &TurnTask,
) -> StoreResult<Option<String>> {
    adopted_snapshot(db, turn.session_id, Some(turn), true)
}

fn adopted_snapshot(
    db: &Connection,
    session: SessionId,
    turn: Option<&TurnTask>,
    check_scope: bool,
) -> StoreResult<Option<String>> {
    Ok(db
        .query_row(
            "SELECT c.id FROM turn_tasks t
         JOIN agent_runs r ON r.turn_task_id=t.id
         JOIN model_steps s ON s.agent_run_id=r.id AND s.state='completed'
         JOIN model_execution_snapshots e ON e.id=s.snapshot_id
         JOIN context_snapshots c ON c.id=json_extract(e.payload,'$.context_snapshot_id')
         WHERE t.session_id=?1 AND c.session_id=t.session_id
           AND (?2 IS NULL OR t.rowid<=(SELECT rowid FROM turn_tasks WHERE id=?2))
           AND (NOT ?4 OR r.id=?3 OR NOT EXISTS(
                SELECT 1 FROM agent_collaboration_deliveries WHERE target_run_id IN (?3,r.id)))
         ORDER BY s.rowid DESC LIMIT 1",
            params![
                session.to_string(),
                turn.map(|t| t.turn_task_id.to_string()),
                turn.map(|t| t.agent_run_id.to_string()),
                check_scope,
            ],
            |row| row.get(0),
        )
        .optional()?)
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
