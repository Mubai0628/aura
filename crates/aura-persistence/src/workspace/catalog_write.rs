//! Writing a workspace's whole index.
//!
//! An index of up to [`BATCH`] entries is written in one transaction. A larger one is written in
//! jobs of at most [`BATCH`] entries on the writer's index lane, so interactive registry writes
//! (creating a conversation, choosing its model) run between them instead of waiting for the
//! whole index. The first job claims the next catalog revision and marks the index `rebuilding`;
//! every later job checks the claim still holds, and the last one marks the index fresh. An open
//! that finds an index still `rebuilding` does not trust it and walks the folder again.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use aura_domain_common::{CorrelationId, Revision, UtcTimestamp, WorkspaceId};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::json;

use super::{
    CatalogCommitOutcome, CatalogCommitPosition, CatalogCommitRefusal, CatalogEntryRecord,
    CatalogPartitionRecord, as_sql_u64, encoding_name, entry_kind_name, index_mode_name,
    parse_sql_u64,
};
use crate::coordinator::insert_event;
use crate::{PersistenceCoordinator, PersistenceError, PersistentEventDraft, WorkClass};

/// Entries one write job takes.
pub(crate) const BATCH: usize = 2_000;

/// Deletes the workspace's text rows and entries, at most `limit` of each; returns how many.
pub(super) fn delete_old(
    tx: &Transaction<'_>,
    workspace: &str,
    limit: Option<usize>,
) -> Result<usize, PersistenceError> {
    let limit = limit.map_or(-1, |limit| limit as i64);
    // The rowid map is the only way to address one text row. Reading it out before the delete
    // keeps every removal a primary-key lookup instead of a full-text scan.
    let rowids = {
        let mut statement = tx.prepare(
            "SELECT fts_rowid FROM workspace_content_index WHERE workspace_id=?1 LIMIT ?2",
        )?;
        statement
            .query_map(params![workspace, limit], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    {
        let mut index = tx.prepare(
            "DELETE FROM workspace_content_index WHERE workspace_id=?1 AND fts_rowid=?2",
        )?;
        let mut text = tx.prepare("DELETE FROM workspace_content_fts WHERE rowid=?1")?;
        for rowid in &rowids {
            index.execute(params![workspace, rowid])?;
            text.execute([rowid])?;
        }
    }
    let entries = tx.execute(
        "DELETE FROM workspace_catalog_entries WHERE rowid IN (
            SELECT rowid FROM workspace_catalog_entries WHERE workspace_id=?1 LIMIT ?2)",
        params![workspace, limit],
    )?;
    Ok(rowids.len() + entries)
}

pub(super) fn insert_partitions(
    tx: &Transaction<'_>,
    workspace_id: WorkspaceId,
    partitions: Vec<CatalogPartitionRecord>,
) -> Result<(), PersistenceError> {
    tx.execute(
        "DELETE FROM workspace_catalog_partitions WHERE workspace_id=?1",
        [workspace_id.to_string()],
    )?;
    for partition in partitions {
        tx.execute(
            "INSERT INTO workspace_catalog_partitions(
                workspace_id, partition_path, file_count, indexable_text_bytes, index_mode
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                workspace_id.to_string(),
                partition.partition_path.to_string(),
                as_sql_u64(partition.file_count)?,
                as_sql_u64(partition.indexable_text_bytes)?,
                index_mode_name(partition.index_mode),
            ],
        )?;
    }
    Ok(())
}

pub(super) fn insert_entries(
    tx: &Transaction<'_>,
    workspace_id: WorkspaceId,
    entries: Vec<CatalogEntryRecord>,
) -> Result<(), PersistenceError> {
    for entry in entries {
        tx.execute(
            "INSERT INTO workspace_catalog_entries(
                workspace_id, relative_path, parent_path, file_name, entry_kind,
                partition_path, byte_length, modified_at, content_hash, text_encoding,
                ignored, generated, hidden_from_search, reparse_tag, change_usn,
                volume_serial, file_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                       ?16, ?17)",
            params![
                workspace_id.to_string(),
                entry.path.to_string(),
                entry.parent_path.to_string(),
                entry.file_name,
                entry_kind_name(entry.kind),
                entry.partition_path.to_string(),
                entry.byte_length.map(as_sql_u64).transpose()?,
                entry.modified_at,
                entry.content_hash.as_ref().map(ToString::to_string),
                entry.text_encoding.map(encoding_name),
                entry.ignored,
                entry.generated,
                entry.hidden_from_search,
                entry.reparse_tag.map(i64::from),
                entry.change_usn.map(as_sql_u64).transpose()?,
                entry.volume_serial,
                entry.file_id,
            ],
        )?;
        if let Some(content) = entry.content {
            let content_hash = entry.content_hash.as_ref().map(ToString::to_string);
            tx.execute(
                "INSERT INTO workspace_content_fts(workspace_id, relative_path, content)
                 VALUES (?1, ?2, ?3)",
                params![workspace_id.to_string(), entry.path.to_string(), content],
            )?;
            tx.execute(
                "INSERT INTO workspace_content_index(
                    workspace_id, relative_path, fts_rowid, content_hash, needs_reindex
                 ) VALUES (?1, ?2, ?3, ?4, 0)",
                params![
                    workspace_id.to_string(),
                    entry.path.to_string(),
                    tx.last_insert_rowid(),
                    content_hash
                ],
            )?;
        }
    }
    Ok(())
}

/// The workspace's catalog revision and catalog instance.
fn position(
    tx: &Transaction<'_>,
    workspace_id: WorkspaceId,
) -> Result<Option<CatalogCommitPosition>, PersistenceError> {
    let observed = tx
        .query_row(
            "SELECT w.catalog_revision, COALESCE(i.instance_seq, 0)
             FROM workspaces AS w
             LEFT JOIN workspace_catalog_instances AS i ON i.workspace_id = w.workspace_id
             WHERE w.workspace_id=?1",
            [workspace_id.to_string()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    observed
        .map(|(revision, instance)| {
            Ok(CatalogCommitPosition {
                catalog_revision: Revision::new(parse_sql_u64(revision)?),
                instance_seq: parse_sql_u64(instance)?,
            })
        })
        .transpose()
}

/// Why a job may not write: the workspace is gone, or another writer moved its catalog.
fn moved(
    observed: Option<CatalogCommitPosition>,
    expected: CatalogCommitPosition,
) -> Option<CatalogCommitRefusal> {
    match observed {
        None => Some(CatalogCommitRefusal::WorkspaceMissing),
        Some(observed) if observed.instance_seq != expected.instance_seq => {
            Some(CatalogCommitRefusal::InstanceReplaced { current: observed.instance_seq })
        }
        Some(observed) if observed.catalog_revision != expected.catalog_revision => {
            Some(CatalogCommitRefusal::RevisionMoved { current: observed.catalog_revision })
        }
        Some(_) => None,
    }
}

/// One write job: `Some` says why it may not write, and then nothing it did is kept.
type Job = Box<
    dyn FnOnce(&Transaction<'_>) -> Result<Option<CatalogCommitRefusal>, PersistenceError> + Send,
>;

/// Writes a large index in jobs; see the module documentation.
pub(super) fn replace_in_batches(
    coordinator: &PersistenceCoordinator,
    workspace_id: WorkspaceId,
    expected: Option<CatalogCommitPosition>,
    mut entries: Vec<CatalogEntryRecord>,
    partitions: Vec<CatalogPartitionRecord>,
    correlation_id: CorrelationId,
    stop: Option<Arc<AtomicBool>>,
) -> Result<CatalogCommitOutcome, PersistenceError> {
    // Checked before every job after the claim: a stopped write leaves the index `rebuilding`.
    let stopped = move || stop.as_ref().is_some_and(|stop| stop.load(Ordering::Acquire));
    let refuse_stopped = || Ok(CatalogCommitOutcome::Refused(CatalogCommitRefusal::Stopped));
    let job = |work: Job| {
        let identity =
            PersistenceCoordinator::workspace_catalog_write_identity(workspace_id, correlation_id);
        coordinator.dispatch_class_identified(
            WorkClass::Index,
            Some(coordinator.index_deadline_instant()),
            identity,
            move |connection| {
                let tx = connection.transaction()?;
                let refused = work(&tx)?;
                if refused.is_none() {
                    tx.commit()?;
                }
                Ok(refused)
            },
        )
    };
    // The claim: the next revision, and an index no open trusts until the last job.
    let (claim_tx, claim_rx) = std::sync::mpsc::channel();
    if let Some(refused) = job(Box::new(move |tx| {
        let Some(observed) = position(tx, workspace_id)? else {
            return Ok(Some(CatalogCommitRefusal::WorkspaceMissing));
        };
        if let Some(expected) = expected
            && let Some(refused) = moved(Some(observed), expected)
        {
            return Ok(Some(refused));
        }
        tx.execute(
            "UPDATE workspaces SET catalog_freshness='rebuilding',
                catalog_revision=catalog_revision+1, updated_at=?2
             WHERE workspace_id=?1",
            params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
        )?;
        let _ = claim_tx.send(position(tx, workspace_id)?);
        Ok(None)
    }))? {
        return Ok(CatalogCommitOutcome::Refused(refused));
    }
    let claimed = claim_rx.recv().ok().flatten().ok_or(PersistenceError::WorkspaceNotFound)?;
    let held =
        move |tx: &Transaction<'_>| -> Result<Option<CatalogCommitRefusal>, PersistenceError> {
            Ok(moved(position(tx, workspace_id)?, claimed))
        };
    let workspace = workspace_id.to_string();
    loop {
        if stopped() {
            return refuse_stopped();
        }
        let workspace = workspace.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        if let Some(refused) = job(Box::new(move |tx| {
            if let Some(refused) = held(tx)? {
                return Ok(Some(refused));
            }
            let _ = done_tx.send(delete_old(tx, &workspace, Some(BATCH))? == 0);
            Ok(None)
        }))? {
            return Ok(CatalogCommitOutcome::Refused(refused));
        }
        if done_rx.recv().unwrap_or(true) {
            break;
        }
    }
    if stopped() {
        return refuse_stopped();
    }
    if let Some(refused) = job(Box::new(move |tx| {
        if let Some(refused) = held(tx)? {
            return Ok(Some(refused));
        }
        insert_partitions(tx, workspace_id, partitions)?;
        Ok(None)
    }))? {
        return Ok(CatalogCommitOutcome::Refused(refused));
    }
    while !entries.is_empty() {
        if stopped() {
            return refuse_stopped();
        }
        let rest = entries.split_off(entries.len().min(BATCH));
        let batch = std::mem::replace(&mut entries, rest);
        if let Some(refused) = job(Box::new(move |tx| {
            if let Some(refused) = held(tx)? {
                return Ok(Some(refused));
            }
            insert_entries(tx, workspace_id, batch)?;
            Ok(None)
        }))? {
            return Ok(CatalogCommitOutcome::Refused(refused));
        }
    }
    if let Some(refused) = job(Box::new(move |tx| {
        if let Some(refused) = held(tx)? {
            return Ok(Some(refused));
        }
        tx.execute(
            "UPDATE workspaces SET catalog_freshness='fresh', revision=revision+1, updated_at=?2
             WHERE workspace_id=?1",
            params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
        )?;
        let mut event = PersistentEventDraft::system(
            "workspace.catalog.rebuilt",
            correlation_id,
            json!({ "workspace_id": workspace_id, "catalog_revision": claimed.catalog_revision }),
        );
        event.workspace_id = Some(workspace_id);
        insert_event(tx, &event)?;
        Ok(None)
    }))? {
        return Ok(CatalogCommitOutcome::Refused(refused));
    }
    Ok(CatalogCommitOutcome::Applied(claimed.catalog_revision))
}
