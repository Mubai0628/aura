//! A workspace index that no longer describes its folder.
//!
//! Archiving, relocating or restoring a workspace leaves its stored index behind. Deleting a large
//! index inside the lifecycle's own transaction held the writer for as long as the delete took, so
//! every other registry write waited behind it. The transaction now only marks the index
//! `rebuilding` and moves its catalog revision and instance: an open does not trust the rows, a
//! write still in flight for the old index is refused at its next job, and the rows are removed in
//! jobs of [`BATCH`] — by the next whole write of an online workspace, or by
//! [`PersistenceCoordinator::clear_archived_workspace_index`] for an archived one.
use std::str::FromStr;

use aura_domain_common::{UtcTimestamp, WorkspaceId};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::catalog_write::{BATCH, delete_old};
use crate::{PersistenceCoordinator, PersistenceError, WorkClass};

/// Marks the workspace's stored index untrusted, inside the caller's transaction. The small
/// per-path tables go at once; the entries and their text rows are left for jobs of [`BATCH`].
pub(super) fn untrust(
    transaction: &Transaction<'_>,
    workspace_id: WorkspaceId,
) -> Result<(), PersistenceError> {
    for table in [
        "workspace_catalog_partitions",
        "workspace_path_recent_use",
        "workspace_deferred_mutations",
        "workspace_mutation_ownership",
        "file_references",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE workspace_id=?1"),
            [workspace_id.to_string()],
        )?;
    }
    transaction.execute(
        "UPDATE workspaces SET catalog_freshness='rebuilding',
            catalog_revision=catalog_revision+1, updated_at=?2
         WHERE workspace_id=?1",
        params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
    )?;
    // The open index's instance is replaced as well: its writer gives up instead of planning
    // again against the new revision. The next open claims a later instance.
    transaction.execute(
        "UPDATE workspace_catalog_instances SET instance_seq=instance_seq+1 WHERE workspace_id=?1",
        [workspace_id.to_string()],
    )?;
    Ok(())
}

/// Whether the workspace holds more stored entries than one job writes.
pub(super) fn more_than_one_batch(
    connection: &Connection,
    workspace_id: WorkspaceId,
) -> Result<bool, PersistenceError> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM workspace_catalog_entries WHERE workspace_id=?1 LIMIT 1 OFFSET ?2",
            params![workspace_id.to_string(), BATCH as i64],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

impl PersistenceCoordinator {
    /// Removes an archived workspace's stored index in jobs of [`BATCH`] rows on the index lane,
    /// so interactive writes run between them. It stops when nothing is left or the workspace is
    /// no longer archived (a restored workspace's walk removes the rest); returns the rows removed.
    pub fn clear_archived_workspace_index(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<u64, PersistenceError> {
        let mut removed = 0_u64;
        loop {
            let workspace = workspace_id.to_string();
            let deleted = self.dispatch_within(
                WorkClass::Index,
                self.index_deadline_instant(),
                move |connection| {
                    let transaction = connection.transaction()?;
                    let archived = transaction
                        .query_row(
                            "SELECT state='archived' FROM workspaces WHERE workspace_id=?1",
                            [&workspace],
                            |row| row.get::<_, bool>(0),
                        )
                        .optional()?
                        .unwrap_or(false);
                    if !archived {
                        return Ok(0);
                    }
                    let deleted = delete_old(&transaction, &workspace, Some(BATCH))?;
                    transaction.commit()?;
                    Ok(deleted)
                },
            )?;
            if deleted == 0 {
                return Ok(removed);
            }
            removed += deleted as u64;
        }
    }

    /// The archived workspaces that still hold stored index rows.
    pub fn archived_workspaces_with_index(&self) -> Result<Vec<WorkspaceId>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT w.workspace_id FROM workspaces AS w
                 WHERE w.state='archived' AND (
                    EXISTS (SELECT 1 FROM workspace_catalog_entries AS e
                            WHERE e.workspace_id=w.workspace_id)
                    OR EXISTS (SELECT 1 FROM workspace_content_index AS c
                               WHERE c.workspace_id=w.workspace_id))",
            )?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.iter().map(|id| Ok(WorkspaceId::from_str(id)?)).collect()
        })
    }
}
