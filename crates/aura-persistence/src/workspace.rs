mod catalog_write;
mod lifecycle_record;
mod stale_index;
use lifecycle_record::load_lifecycle_record;
use std::{
    collections::{BTreeSet, HashSet},
    str::FromStr,
    sync::{Arc, atomic::AtomicBool},
};

use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, CorrelationId, DeferredMutationId, FileReferenceId,
    MutationJournalId, RendererInstanceId, Revision, SessionId, UtcTimestamp, WorkspaceId,
    WorkspaceLifecycleOperationId, WorkspacePath,
};
use aura_permission_domain::{ApprovalRequest, PermissionGrant, ResolvedScope};
use aura_security_contract::ApprovalState;
use aura_workspace_domain::{
    CatalogEntryKind, CatalogIndexMode, DeferredMutationSummary, TextEncoding,
    WorkspaceLifecycleOperation, WorkspaceLifecycleOperationKind, WorkspaceLifecyclePreflight,
    WorkspaceLifecycleState, WorkspaceLifecycleSummary, WorkspaceSessionProjection, WorkspaceState,
    WorkspaceSummary,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};

use crate::WorkClass;
use crate::coordinator::{
    catalog_freshness_name, insert_event, load_workspace_session_projection,
    next_projection_revision, parse_catalog_freshness, parse_workspace_state, workspace_state_name,
};
use crate::{CommittedProjection, PersistenceCoordinator, PersistenceError, PersistentEventDraft};

/// One **execution's** identity inside the workspace mutation-ownership record.
///
/// The row this keys used to be identified by the workspace alone, which made one session's two
/// independent executions a single owner that one release ended — and made every other session in
/// the workspace a conflict. An execution id separates them: two executions of one session are two
/// rows, so releasing one leaves the other's ownership intact.
///
/// It is deliberately a persistence-layer value rather than a domain contract type: the id never
/// crosses a process or IPC boundary, it is only a row key, and a new public id would be a new
/// public contract for no gain.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MutationOwnershipExecutionId(String);

impl MutationOwnershipExecutionId {
    /// A fresh execution id.
    #[must_use]
    pub fn new() -> Self {
        Self(ulid::Ulid::new().to_string())
    }

    /// The id a row written by the pre-`execution_id` shape carries after migration.
    ///
    /// A release through the workspace-scoped API addresses this, so an in-flight release issued
    /// before an upgrade still finds the row it wrote.
    #[must_use]
    pub fn legacy_default() -> Self {
        Self("legacy".to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for MutationOwnershipExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MutationOwnershipExecutionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug)]
pub struct CatalogPartitionRecord {
    pub partition_path: WorkspacePath,
    pub file_count: u64,
    pub indexable_text_bytes: u64,
    pub index_mode: CatalogIndexMode,
}

#[derive(Clone, Debug)]
pub struct CatalogEntryRecord {
    pub path: WorkspacePath,
    pub parent_path: WorkspacePath,
    pub file_name: String,
    pub kind: CatalogEntryKind,
    pub partition_path: WorkspacePath,
    pub byte_length: Option<u64>,
    pub modified_at: Option<String>,
    pub content_hash: Option<ContentHash>,
    pub text_encoding: Option<TextEncoding>,
    pub ignored: bool,
    pub generated: bool,
    pub hidden_from_search: bool,
    pub reparse_tag: Option<u32>,
    pub content: Option<String>,
    /// The volume's change generation for this file at the moment its text was
    /// read, through a handle that excluded writers, and the object identity it
    /// was read from. All three together are the proof a later open needs before
    /// it reuses this text; any of them missing means the file is read again
    /// rather than trusted.
    pub change_usn: Option<u64>,
    pub volume_serial: Option<String>,
    pub file_id: Option<String>,
}

/// One incremental catalog change. `upserts` replace exactly one entry each,
/// `removed_paths` are applied after them, `partitions` carries the recomputed
/// summary for every partition the change touched, and `removed_partitions` names
/// the partitions whose last file disappeared. `reason` records which bounded
/// scope produced the delta so a recovery is auditable and never looks like a
/// routine full rebuild.
#[derive(Clone, Debug)]
pub struct CatalogDeltaRecord {
    pub upserts: Vec<CatalogEntryRecord>,
    pub removed_paths: Vec<WorkspacePath>,
    pub partitions: Vec<CatalogPartitionRecord>,
    /// A partition that no longer has any file must leave the stored summary,
    /// otherwise a later read reports a partition the catalog no longer has.
    pub removed_partitions: Vec<WorkspacePath>,
    /// Paths whose re-derivation mark this delta settles.
    ///
    /// A mark is cleared only by the committed conclusion for exactly that path:
    /// its text row written, or its file removed. A path whose text a repair
    /// re-read but still could not establish keeps its mark, so the next open
    /// re-derives it instead of calling an unproven path indexed. Because the
    /// mark and the conclusion settle in this one transaction, a failed write
    /// leaves every mark exactly as it was.
    pub repaired_paths: Vec<WorkspacePath>,
    pub reason: String,
}

/// The durable position a catalog commit has to still hold to be allowed to write.
///
/// Both halves are checked inside the writing transaction, so "the plan I am publishing
/// was formed against this revision, and this instance still owns the workspace's index"
/// is one condition with the modification rather than a check that another writer can
/// invalidate between the check and the write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogCommitPosition {
    /// The workspace's durable `catalog_revision` the plan was derived from.
    pub catalog_revision: Revision,
    /// The catalog instance generation that claimed this workspace's index.
    pub instance_seq: u64,
}

/// Why a guarded catalog commit was refused.
///
/// The two refusals mean different things to the caller, which is why they are reported
/// separately rather than as one "no": a revision that moved can be answered by re-reading
/// the committed facts and deriving a new plan from them, while a replaced instance can
/// only be answered by giving up — re-reading would not make this instance the owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogCommitRefusal {
    /// The durable catalog revision is no longer the one the caller's plan was derived
    /// from. Nothing was written.
    RevisionMoved { current: Revision },
    /// A newer catalog instance has claimed this workspace's index. Nothing was written,
    /// and no amount of re-reading makes the caller that instance.
    InstanceReplaced { current: u64 },
    /// The workspace row is gone, so there is nothing to write into.
    WorkspaceMissing,
    /// The caller stopped a write in jobs between two jobs. The index stays `rebuilding`, so
    /// the next open walks the folder again.
    Stopped,
}

/// What a guarded catalog commit did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogCommitOutcome {
    /// The write committed, at the revision it reports.
    Applied(Revision),
    /// The guard refused: the transaction rolled back and nothing was written.
    Refused(CatalogCommitRefusal),
}

impl CatalogCommitOutcome {
    #[must_use]
    pub const fn applied(self) -> Option<Revision> {
        match self {
            Self::Applied(revision) => Some(revision),
            Self::Refused(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeferredMutationRecord {
    pub deferred_mutation_id: DeferredMutationId,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub path: WorkspacePath,
    pub expected_hash: Option<ContentHash>,
    pub reason: String,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug)]
pub struct MutationAuditRecord {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub operation_kind: String,
    pub path: WorkspacePath,
    pub ignored: bool,
    pub generated: bool,
    pub before_hash: Option<ContentHash>,
    pub after_hash: Option<ContentHash>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceLifecycleRecord {
    pub summary: WorkspaceLifecycleSummary,
    pub operation: WorkspaceLifecycleOperation,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceBlockers {
    pub running_objects: u64,
    pub runtime_leases: u64,
    pub mutation_ownership: u64,
    pub deferred_mutations: u64,
    pub unfinished_journals: u64,
}

impl WorkspaceBlockers {
    #[must_use]
    pub const fn is_clear(self) -> bool {
        self.running_objects == 0
            && self.runtime_leases == 0
            && self.mutation_ownership == 0
            && self.deferred_mutations == 0
            && self.unfinished_journals == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceDeleteReport {
    pub ended_sessions: Vec<SessionId>,
    pub artifacts_queued_for_gc: u64,
}

impl PersistenceCoordinator {
    pub fn create_file_reference(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        path: &WorkspacePath,
    ) -> Result<FileReferenceId, PersistenceError> {
        let file_reference_id = FileReferenceId::new();
        let path = path.to_string();
        self.dispatch(move |connection| {
            let belongs = connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sessions WHERE session_id=?1 AND workspace_id=?2
                 )",
                params![session_id.to_string(), workspace_id.to_string()],
                |row| row.get::<_, bool>(0),
            )?;
            if !belongs {
                return Err(PersistenceError::InvalidRequest(
                    "file reference session does not belong to workspace".to_owned(),
                ));
            }
            connection.execute(
                "INSERT INTO file_references(
                    file_reference_id, session_id, workspace_id, relative_path, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    file_reference_id.to_string(),
                    session_id.to_string(),
                    workspace_id.to_string(),
                    path,
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            Ok(file_reference_id)
        })
    }

    pub fn resolve_file_reference(
        &self,
        session_id: SessionId,
        file_reference_id: FileReferenceId,
    ) -> Result<Option<(WorkspaceId, WorkspacePath)>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT workspace_id, relative_path FROM file_references
                     WHERE file_reference_id=?1 AND session_id=?2",
                    params![file_reference_id.to_string(), session_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .map(|(workspace_id, path)| {
                    Ok((
                        WorkspaceId::from_str(&workspace_id)?,
                        WorkspacePath::from_str(&path).map_err(invalid_path)?,
                    ))
                })
                .transpose()
        })
    }

    pub fn workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<WorkspaceSummary, PersistenceError> {
        self.with_reader(|connection| load_workspace(connection, workspace_id))
    }

    pub fn find_workspace_by_root(
        &self,
        canonical_root: &str,
        volume_serial_number: Option<&str>,
        file_id: Option<&str>,
    ) -> Result<Option<WorkspaceSummary>, PersistenceError> {
        let canonical_root = canonical_root.to_owned();
        let volume_serial_number = volume_serial_number.map(ToOwned::to_owned);
        let file_id = file_id.map(ToOwned::to_owned);
        self.with_reader(|connection| {
            let id = connection
                .query_row(
                    "SELECT workspace_id FROM workspaces
                     WHERE canonical_root=?1 COLLATE NOCASE
                        OR (?2 IS NOT NULL AND ?3 IS NOT NULL
                            AND root_volume_serial=?2 AND root_file_id=?3)
                     ORDER BY created_at LIMIT 1",
                    params![canonical_root, volume_serial_number, file_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            id.map(|value| load_workspace(connection, WorkspaceId::from_str(&value)?)).transpose()
        })
    }

    pub fn create_workspace(
        &self,
        workspace: WorkspaceSummary,
        correlation_id: CorrelationId,
    ) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
        validate_workspace(&workspace)?;
        let committed = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let duplicate = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM workspaces
                    WHERE canonical_root=?1 COLLATE NOCASE
                       OR (?2 IS NOT NULL AND ?3 IS NOT NULL
                           AND root_volume_serial=?2 AND root_file_id=?3)
                )",
                params![
                    workspace.canonical_root,
                    workspace.root_identity.as_ref().map(|value| &value.volume_serial_number),
                    workspace.root_identity.as_ref().map(|value| &value.file_id),
                ],
                |row| row.get::<_, bool>(0),
            )?;
            if duplicate {
                return Err(PersistenceError::WorkspaceRootConflict);
            }
            insert_workspace(&transaction, &workspace)?;
            let mut event = PersistentEventDraft::system(
                "workspace.created",
                correlation_id,
                json!({ "workspace_id": workspace.workspace_id }),
            );
            event.workspace_id = Some(workspace.workspace_id);
            insert_event(&transaction, &event)?;
            let committed = commit_projection(&transaction)?;
            transaction.commit()?;
            Ok(committed)
        })?;
        self.publish_and_mark(committed)
    }

    pub fn update_workspace(
        &self,
        workspace: WorkspaceSummary,
        event_type: &'static str,
        correlation_id: CorrelationId,
        invalidate_paths: bool,
    ) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
        validate_workspace(&workspace)?;
        let committed = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let updated = transaction.execute(
                "UPDATE workspaces SET
                    display_name=?2, selected_root=?3, canonical_root=?4,
                    root_volume_serial=?5, root_file_id=?6, state=?7, revision=?8,
                    catalog_freshness=?9, authorization_epoch=?10,
                    archived_at=CASE WHEN ?7='archived' THEN COALESCE(archived_at, ?11) ELSE NULL END,
                    updated_at=?11
                 WHERE workspace_id=?1",
                params![
                    workspace.workspace_id.to_string(),
                    workspace.display_name,
                    workspace.selected_root,
                    workspace.canonical_root,
                    workspace.root_identity.as_ref().map(|value| &value.volume_serial_number),
                    workspace.root_identity.as_ref().map(|value| &value.file_id),
                    workspace_state_name(workspace.state),
                    as_sql_u64(workspace.revision.value())?,
                    catalog_freshness_name(workspace.catalog_freshness),
                    as_sql_u64(workspace.authorization_epoch.value())?,
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            if updated != 1 {
                return Err(PersistenceError::WorkspaceNotFound);
            }
            if invalidate_paths {
                stale_index::untrust(&transaction, workspace.workspace_id)?;
            }
            let mut event = PersistentEventDraft::system(
                event_type,
                correlation_id,
                json!({ "workspace_id": workspace.workspace_id }),
            );
            event.workspace_id = Some(workspace.workspace_id);
            insert_event(&transaction, &event)?;
            let committed = commit_projection(&transaction)?;
            transaction.commit()?;
            Ok(committed)
        })?;
        self.publish_and_mark(committed)
    }

    pub fn commit_workspace_lifecycle(
        &self,
        workspace: WorkspaceSummary,
        lifecycle: WorkspaceLifecycleRecord,
        event_type: &'static str,
        correlation_id: CorrelationId,
        invalidate_paths: bool,
    ) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
        validate_workspace(&workspace)?;
        validate_lifecycle_record(&lifecycle)?;
        if lifecycle.summary.workspace_id != workspace.workspace_id
            || lifecycle.summary.state != WorkspaceLifecycleState::Completed
        {
            return Err(PersistenceError::InvalidRequest(
                "completed lifecycle record does not match the workspace".to_owned(),
            ));
        }
        let committed = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let now = UtcTimestamp::now().to_string();
            if transaction.execute(
                "UPDATE workspaces SET
                    display_name=?2, selected_root=?3, canonical_root=?4,
                    root_volume_serial=?5, root_file_id=?6, state=?7, revision=?8,
                    catalog_freshness=?9, authorization_epoch=?10,
                    archived_at=CASE WHEN ?7='archived' THEN COALESCE(archived_at, ?11) ELSE NULL END,
                    updated_at=?11
                 WHERE workspace_id=?1",
                params![
                    workspace.workspace_id.to_string(),
                    workspace.display_name,
                    workspace.selected_root,
                    workspace.canonical_root,
                    workspace.root_identity.as_ref().map(|value| &value.volume_serial_number),
                    workspace.root_identity.as_ref().map(|value| &value.file_id),
                    workspace_state_name(workspace.state),
                    as_sql_u64(workspace.revision.value())?,
                    catalog_freshness_name(workspace.catalog_freshness),
                    as_sql_u64(workspace.authorization_epoch.value())?,
                    now,
                ],
            )? != 1
            {
                return Err(PersistenceError::WorkspaceNotFound);
            }
            if invalidate_paths {
                stale_index::untrust(&transaction, workspace.workspace_id)?;
            }
            let (external_grants_changed, revoked_external_grant_ids) =
                match lifecycle.summary.operation {
                WorkspaceLifecycleOperationKind::Archive => (transaction.execute(
                    "UPDATE external_grants SET
                        lifecycle='suspended', suspended_at=?2, updated_at=?2,
                        revalidation_revision=revalidation_revision+1
                     WHERE workspace_id=?1 AND lifecycle='active'",
                    params![workspace.workspace_id.to_string(), &now],
                )?, Vec::new()),
                WorkspaceLifecycleOperationKind::Relocate => {
                    let mut statement = transaction.prepare(
                        "SELECT external_grant_id FROM external_grants
                         WHERE workspace_id=?1 AND lifecycle!='revoked'
                           AND workspace_authorization_epoch IS NOT NULL
                           AND workspace_authorization_epoch<?2",
                    )?;
                    let ids = statement
                        .query_map(
                            params![
                                workspace.workspace_id.to_string(),
                                as_sql_u64(workspace.authorization_epoch.value())?
                            ],
                            |row| row.get::<_, String>(0),
                        )?
                        .collect::<Result<Vec<_>, _>>()?;
                    drop(statement);
                    let changed = transaction.execute(
                        "UPDATE external_grants SET
                            lifecycle='revoked', revoked_at=?3, suspended_at=NULL,
                            updated_at=?3, revalidation_revision=revalidation_revision+1
                         WHERE workspace_id=?1 AND lifecycle!='revoked'
                           AND workspace_authorization_epoch IS NOT NULL
                           AND workspace_authorization_epoch<?2",
                        params![
                            workspace.workspace_id.to_string(),
                            as_sql_u64(workspace.authorization_epoch.value())?,
                            &now
                        ],
                    )?;
                    (changed, ids)
                }
                WorkspaceLifecycleOperationKind::PermanentDelete => (0, Vec::new()),
            };
            let (permission_grants_revoked, approvals_invalidated) =
                if lifecycle.summary.operation == WorkspaceLifecycleOperationKind::Relocate {
                    invalidate_relocated_workspace_authorizations(
                        &transaction,
                        workspace.workspace_id,
                        &now,
                        &revoked_external_grant_ids,
                    )?
                } else {
                    (0, 0)
                };
            let operation_json = serde_json::to_string(&lifecycle.operation)?;
            let preflight_json = lifecycle
                .summary
                .preflight
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            if transaction.execute(
                "UPDATE workspace_lifecycle_operations SET
                    state='completed', lifecycle_revision=?2, operation_json=?3,
                    preflight_json=?4, error_code=NULL, updated_at=?5, terminal_at=?5
                 WHERE operation_id=?1 AND workspace_id=?6
                   AND state='committing'",
                params![
                    lifecycle.summary.operation_id.to_string(),
                    as_sql_u64(lifecycle.summary.lifecycle_revision.value())?,
                    operation_json,
                    preflight_json,
                    now,
                    workspace.workspace_id.to_string(),
                ],
            )? != 1
            {
                return Err(PersistenceError::InvalidRequest(
                    "workspace lifecycle operation was not committing".to_owned(),
                ));
            }
            let mut event = PersistentEventDraft::system(
                event_type,
                correlation_id,
                json!({
                    "workspace_id": workspace.workspace_id,
                    "operation_id": lifecycle.summary.operation_id,
                    "external_grants_changed": external_grants_changed,
                    "permission_grants_revoked": permission_grants_revoked,
                    "approvals_invalidated": approvals_invalidated,
                }),
            );
            event.workspace_id = Some(workspace.workspace_id);
            insert_event(&transaction, &event)?;
            let committed = commit_projection(&transaction)?;
            transaction.commit()?;
            Ok(committed)
        })?;
        self.publish_and_mark(committed)
    }

    pub fn workspace_blockers(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<WorkspaceBlockers, PersistenceError> {
        self.with_reader(|connection| query_blockers(connection, workspace_id))
    }

    pub fn create_workspace_lifecycle_operation(
        &self,
        record: WorkspaceLifecycleRecord,
    ) -> Result<(), PersistenceError> {
        validate_lifecycle_record(&record)?;
        self.dispatch(move |connection| {
            let now = UtcTimestamp::now().to_string();
            let operation_json = serde_json::to_string(&record.operation)?;
            let preflight_json =
                record.summary.preflight.as_ref().map(serde_json::to_string).transpose()?;
            let terminal_at = record.summary.state.is_terminal().then(|| now.clone());
            connection.execute(
                "INSERT INTO workspace_lifecycle_operations(
                    operation_id, workspace_id, renderer_instance_id, operation_kind,
                    operation_json, state, lifecycle_revision, ack_deadline_at,
                    preflight_json, error_code, created_at, updated_at, terminal_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)",
                params![
                    record.summary.operation_id.to_string(),
                    record.summary.workspace_id.to_string(),
                    record.summary.renderer_instance_id.to_string(),
                    lifecycle_operation_kind_name(record.summary.operation),
                    operation_json,
                    lifecycle_state_name(record.summary.state),
                    as_sql_u64(record.summary.lifecycle_revision.value())?,
                    record.summary.ack_deadline_at.as_ref().map(ToString::to_string),
                    preflight_json,
                    record.summary.error_code,
                    now,
                    terminal_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_workspace_lifecycle_operation(
        &self,
        record: WorkspaceLifecycleRecord,
    ) -> Result<(), PersistenceError> {
        validate_lifecycle_record(&record)?;
        self.dispatch(move |connection| {
            let now = UtcTimestamp::now().to_string();
            let operation_json = serde_json::to_string(&record.operation)?;
            let preflight_json =
                record.summary.preflight.as_ref().map(serde_json::to_string).transpose()?;
            let terminal_at = record.summary.state.is_terminal().then(|| now.clone());
            let updated = connection.execute(
                "UPDATE workspace_lifecycle_operations SET
                    renderer_instance_id=?3, operation_kind=?4, operation_json=?5,
                    state=?6, lifecycle_revision=?7, ack_deadline_at=?8,
                    preflight_json=?9, error_code=?10, updated_at=?11, terminal_at=?12
                 WHERE operation_id=?1 AND workspace_id=?2",
                params![
                    record.summary.operation_id.to_string(),
                    record.summary.workspace_id.to_string(),
                    record.summary.renderer_instance_id.to_string(),
                    lifecycle_operation_kind_name(record.summary.operation),
                    operation_json,
                    lifecycle_state_name(record.summary.state),
                    as_sql_u64(record.summary.lifecycle_revision.value())?,
                    record.summary.ack_deadline_at.as_ref().map(ToString::to_string),
                    preflight_json,
                    record.summary.error_code,
                    now,
                    terminal_at,
                ],
            )?;
            if updated != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "workspace lifecycle operation not found".to_owned(),
                ));
            }
            Ok(())
        })
    }

    pub fn workspace_lifecycle_operation(
        &self,
        operation_id: WorkspaceLifecycleOperationId,
    ) -> Result<WorkspaceLifecycleRecord, PersistenceError> {
        self.with_reader(|connection| load_lifecycle_record(connection, operation_id))
    }

    pub fn workspace_lifecycle_operations(
        &self,
    ) -> Result<Vec<WorkspaceLifecycleSummary>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT operation_id FROM workspace_lifecycle_operations
                 ORDER BY created_at, operation_id",
            )?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|value| {
                    load_lifecycle_record(
                        connection,
                        WorkspaceLifecycleOperationId::from_str(&value)?,
                    )
                    .map(|record| record.summary)
                })
                .collect()
        })
    }

    pub fn recover_interrupted_workspace_lifecycles(
        &self,
    ) -> Result<Vec<WorkspaceLifecycleOperationId>, PersistenceError> {
        self.dispatch(|connection| {
            let transaction = connection.transaction()?;
            let rows = {
                let mut statement = transaction.prepare(
                    "SELECT operation_id, lifecycle_revision
                     FROM workspace_lifecycle_operations
                     WHERE state NOT IN ('completed', 'cancelled', 'failed')",
                )?;
                statement
                    .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let now = UtcTimestamp::now().to_string();
            let mut recovered = Vec::with_capacity(rows.len());
            for (operation_id, revision) in rows {
                let next = parse_sql_u64(revision)?.checked_add(1).ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "workspace lifecycle revision exhausted".to_owned(),
                    )
                })?;
                transaction.execute(
                    "UPDATE workspace_lifecycle_operations
                     SET state='failed', lifecycle_revision=?2,
                         error_code='workspace.lifecycle_core_restarted',
                         updated_at=?3, terminal_at=?3
                     WHERE operation_id=?1",
                    params![operation_id, as_sql_u64(next)?, now],
                )?;
                recovered.push(WorkspaceLifecycleOperationId::from_str(&operation_id)?);
            }
            transaction.commit()?;
            Ok(recovered)
        })
    }

    pub fn register_workspace_runtime_object(
        &self,
        workspace_id: WorkspaceId,
        object_kind: impl Into<String>,
        object_id: impl Into<String>,
    ) -> Result<(), PersistenceError> {
        let object_kind = object_kind.into();
        let object_id = object_id.into();
        if object_kind.trim().is_empty() || object_id.trim().is_empty() {
            return Err(PersistenceError::InvalidRequest(
                "workspace runtime object identity must not be empty".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            ensure_workspace_accepts_new_activity(connection, workspace_id)?;
            let now = UtcTimestamp::now().to_string();
            connection.execute(
                "INSERT INTO workspace_runtime_objects(
                    workspace_id, object_kind, object_id, state, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, 'active', ?4, ?4)",
                params![workspace_id.to_string(), object_kind, object_id, now],
            )?;
            Ok(())
        })
    }

    pub fn finish_workspace_runtime_object(
        &self,
        workspace_id: WorkspaceId,
        object_kind: &str,
        object_id: &str,
        cancelled: bool,
    ) -> Result<bool, PersistenceError> {
        let object_kind = object_kind.to_owned();
        let object_id = object_id.to_owned();
        self.dispatch(move |connection| {
            let changed = connection.execute(
                "UPDATE workspace_runtime_objects
                 SET state=?4, updated_at=?5
                 WHERE workspace_id=?1 AND object_kind=?2 AND object_id=?3 AND state='active'",
                params![
                    workspace_id.to_string(),
                    object_kind,
                    object_id,
                    if cancelled { "cancelled" } else { "completed" },
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            Ok(changed == 1)
        })
    }

    pub(crate) fn publish_and_mark(
        &self,
        committed: CommittedProjection<WorkspaceSessionProjection>,
    ) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
        self.publish_workspace_session(committed.clone())?;
        let outbox_id = committed.outbox_id;
        self.dispatch(move |connection| {
            connection.execute(
                "UPDATE projection_outbox SET published_at=?1 WHERE outbox_id=?2",
                params![UtcTimestamp::now().to_string(), outbox_id],
            )?;
            Ok(())
        })?;
        Ok(committed)
    }
}

fn invalidate_relocated_workspace_authorizations(
    transaction: &Transaction<'_>,
    workspace_id: WorkspaceId,
    now: &str,
    revoked_external_grant_ids: &[String],
) -> Result<(u64, u64), PersistenceError> {
    let revoked_external_grant_ids =
        revoked_external_grant_ids.iter().cloned().collect::<HashSet<_>>();
    let mut grants = Vec::new();
    {
        let mut statement = transaction.prepare(
            "SELECT grant_json FROM permission_grants
             WHERE workspace_id=?1 AND revoked_at IS NULL AND consumed_at IS NULL",
        )?;
        let rows =
            statement.query_map([workspace_id.to_string()], |row| row.get::<_, String>(0))?;
        for row in rows {
            grants.push(serde_json::from_str::<PermissionGrant>(&row?)?);
        }
    }
    let revoked_at = UtcTimestamp::from_str(now).map_err(|error| {
        PersistenceError::InvalidRequest(format!("invalid lifecycle timestamp: {error}"))
    })?;
    let mut revoked = 0_u64;
    for mut grant in grants {
        let references_revoked_external_grant = grant.scopes.iter().any(|scope| {
            matches!(scope, ResolvedScope::ExternalGrant { grant_id }
                if revoked_external_grant_ids.contains(&grant_id.to_string()))
        });
        if !grant.scopes.iter().any(|scope| scope_binds_workspace_path(scope, workspace_id))
            && !references_revoked_external_grant
        {
            continue;
        }
        grant.revoked_at = Some(revoked_at);
        if transaction.execute(
            "UPDATE permission_grants SET grant_json=?2, revoked_at=?3
             WHERE permission_grant_id=?1 AND revoked_at IS NULL AND consumed_at IS NULL",
            params![grant.id.to_string(), serde_json::to_string(&grant)?, now],
        )? == 1
        {
            revoked = revoked.saturating_add(1);
        }
    }

    let mut approvals = Vec::new();
    {
        let mut statement = transaction
            .prepare("SELECT request_json FROM approval_requests WHERE state='pending'")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        for row in rows {
            approvals.push(serde_json::from_str::<ApprovalRequest>(&row?)?);
        }
    }
    let mut invalidated = 0_u64;
    for mut request in approvals {
        let references_revoked_external_grant = request.scopes.iter().any(|scope| {
            matches!(scope, ResolvedScope::ExternalGrant { grant_id }
                if revoked_external_grant_ids.contains(&grant_id.to_string()))
        });
        if request.identity.workspace_id != Some(workspace_id)
            || (!request.scopes.iter().any(|scope| scope_binds_workspace_path(scope, workspace_id))
                && !references_revoked_external_grant)
        {
            continue;
        }
        request.state = ApprovalState::Invalidated;
        request.request_revision = request.request_revision.checked_add(1).ok_or_else(|| {
            PersistenceError::InvalidRequest("approval revision exhausted".to_owned())
        })?;
        request.resolved_at = Some(revoked_at);
        request.invalidation_reason =
            Some("workspace_relocated_authorization_epoch_changed".to_owned());
        if transaction.execute(
            "UPDATE approval_requests SET request_json=?2, request_revision=?3,
                 state='invalidated', resolved_at=?4, invalidation_reason=?5
             WHERE approval_request_id=?1 AND state='pending'",
            params![
                request.id.to_string(),
                serde_json::to_string(&request)?,
                as_sql_u64(request.request_revision)?,
                now,
                "workspace_relocated_authorization_epoch_changed",
            ],
        )? == 1
        {
            invalidated = invalidated.saturating_add(1);
        }
    }
    Ok((revoked, invalidated))
}

fn scope_binds_workspace_path(scope: &ResolvedScope, workspace_id: WorkspaceId) -> bool {
    matches!(
        scope,
        ResolvedScope::WorkspaceRoot { workspace_id: bound }
            | ResolvedScope::WorkspacePath { workspace_id: bound, .. }
            | ResolvedScope::GitRepository { workspace_id: bound }
            if *bound == workspace_id
    )
}

impl PersistenceCoordinator {
    pub fn recover_interrupted_workspace_mutations(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<Vec<WorkspaceId>, PersistenceError> {
        let current_runtime = self.runtime_instance_id();
        let (workspace_ids, committed) = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let workspace_ids = {
                let mut statement = transaction.prepare(
                    "SELECT DISTINCT journal.workspace_id
                     FROM workspace_operation_journals AS journal
                     JOIN runtime_instances AS runtime
                       ON runtime.runtime_instance_id=journal.runtime_instance_id
                     WHERE journal.runtime_instance_id<>?1
                       AND runtime.ended_at IS NOT NULL
                       AND journal.state IN ('preparing', 'applying', 'compensating')
                     ORDER BY journal.workspace_id",
                )?;
                statement
                    .query_map([current_runtime.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|value| WorkspaceId::from_str(&value))
                    .collect::<Result<Vec<_>, _>>()?
            };
            transaction.execute(
                "DELETE FROM workspace_mutation_ownership
                 WHERE runtime_instance_id<>?1
                   AND runtime_instance_id IN (
                       SELECT runtime_instance_id FROM runtime_instances
                       WHERE ended_at IS NOT NULL
                   )",
                [current_runtime.to_string()],
            )?;
            for workspace_id in &workspace_ids {
                transaction.execute(
                    "UPDATE workspace_operation_journals
                     SET state='reconciliation_required', updated_at=?2
                     WHERE workspace_id=?1
                       AND runtime_instance_id<>?3
                       AND state IN ('preparing', 'applying', 'compensating')",
                    params![
                        workspace_id.to_string(),
                        UtcTimestamp::now().to_string(),
                        current_runtime.to_string()
                    ],
                )?;
                transaction.execute(
                    "UPDATE workspaces SET
                        state='reconciliation_required', catalog_freshness='stale',
                        revision=revision+1, authorization_epoch=authorization_epoch+1,
                        updated_at=?2
                     WHERE workspace_id=?1",
                    params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
                )?;
                let mut event = PersistentEventDraft::system(
                    "workspace.mutation.reconciliation_required",
                    correlation_id,
                    json!({
                        "workspace_id": workspace_id,
                        "reason": "interrupted_workspace_journal",
                        "automatic_replay": false,
                    }),
                );
                event.workspace_id = Some(*workspace_id);
                insert_event(&transaction, &event)?;
            }
            let committed = if workspace_ids.is_empty() {
                None
            } else {
                Some(commit_projection(&transaction)?)
            };
            transaction.commit()?;
            Ok((workspace_ids, committed))
        })?;
        if let Some(committed) = committed {
            self.publish_and_mark(committed)?;
        }
        Ok(workspace_ids)
    }

    /// Acquires the workspace's mutation-ownership record for one **execution**.
    ///
    /// # What this does and does not exclude
    ///
    /// This table is the workspace's own bookkeeping record: which session, on which runtime
    /// instance, currently has a mutation registered against this workspace. It is deliberately
    /// **not** the file-coordination boundary — that is
    /// `aura_workspace_domain::EligibilityOwner`, keyed by the real resource, which is what lets
    /// two sessions write unrelated files in one workspace at the same time.
    ///
    /// The row used to be taken as an exclusive claim by *workspace plus session*, so a second
    /// session writing an unrelated file in the same workspace was refused with
    /// `MutationOwnedByOtherSession`, and one session's two independent executions were treated as
    /// one owner that a single release would end. Both were wrong, and neither is what the table
    /// is for. What remains is the record itself:
    ///
    /// * a **different session** is no longer refused here. Unrelated files must run in parallel,
    ///   and whether two executions really touch the same file is decided by the resource owner
    ///   from real identities, not by a workspace-wide guess.
    /// * the row is keyed by `(workspace_id, session_id, execution_id)`, so one session's two
    ///   executions are two rows and releasing one does not release the other.
    ///
    /// The workspace lifecycle still uses this table: `ensure_workspace_accepts_new_activity` runs
    /// first and refuses during a lifecycle transition, and `release_mutation_ownership` for a
    /// workspace is still the right thing for a workspace that is closing. Deleting the table or
    /// the check would drop that coverage, which is why the check keeps its place and only its
    /// exclusion rule changed.
    /// The workspace-scoped record, for callers that name one registration per workspace.
    ///
    /// This is the **legacy** shape: one row per `(workspace, session)`, addressed by a fixed
    /// execution id so an acquire and its release still pair up. A caller that runs two independent
    /// executions must use [`Self::acquire_mutation_ownership_for_execution`] with two ids — with
    /// this API they would share one row, which is exactly the "one release ends both" behaviour
    /// this batch removes for the real mutation path.
    pub fn acquire_mutation_ownership(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
    ) -> Result<(), PersistenceError> {
        self.acquire_mutation_ownership_for_execution(
            workspace_id,
            session_id,
            MutationOwnershipExecutionId::legacy_default(),
        )
    }

    /// The same record, for an execution whose identity the caller already holds.
    ///
    /// A caller that runs two independent executions passes two ids and gets two rows, so one
    /// `release` cannot end the other's ownership.
    pub fn acquire_mutation_ownership_for_execution(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        execution_id: MutationOwnershipExecutionId,
    ) -> Result<(), PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            ensure_workspace_accepts_new_activity(&transaction, workspace_id)?;
            transaction.execute(
                "INSERT INTO workspace_mutation_ownership(
                    workspace_id, session_id, runtime_instance_id, acquired_at, execution_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(workspace_id, session_id, execution_id) DO NOTHING",
                params![
                    workspace_id.to_string(),
                    session_id.to_string(),
                    runtime_instance_id.to_string(),
                    UtcTimestamp::now().to_string(),
                    execution_id.to_string()
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Releases **this execution's** ownership row.
    ///
    /// Returns whether a row was really removed. Only this execution's row goes: another
    /// execution of the same session, and every other session, keep theirs.
    pub fn release_mutation_ownership(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
    ) -> Result<bool, PersistenceError> {
        self.release_mutation_ownership_for_execution(
            workspace_id,
            session_id,
            MutationOwnershipExecutionId::legacy_default(),
        )
    }

    /// Releases one execution's ownership row.
    pub fn release_mutation_ownership_for_execution(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        execution_id: MutationOwnershipExecutionId,
    ) -> Result<bool, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            Ok(connection.execute(
                "DELETE FROM workspace_mutation_ownership
                 WHERE workspace_id=?1 AND session_id=?2 AND runtime_instance_id=?3
                   AND execution_id=?4",
                params![
                    workspace_id.to_string(),
                    session_id.to_string(),
                    runtime_instance_id.to_string(),
                    execution_id.to_string()
                ],
            )? == 1)
        })
    }

    /// Releases **every** ownership row this runtime instance holds for one workspace.
    ///
    /// This is the workspace-lifecycle release: closing or migrating a workspace really does end
    /// every execution's registration inside it, which is the deliberate exception to per-resource
    /// coordination.
    pub fn release_workspace_mutation_ownership(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<usize, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            Ok(connection.execute(
                "DELETE FROM workspace_mutation_ownership
                 WHERE workspace_id=?1 AND runtime_instance_id=?2",
                params![workspace_id.to_string(), runtime_instance_id.to_string()],
            )?)
        })
    }

    pub fn record_deferred_mutation(
        &self,
        record: DeferredMutationRecord,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            ensure_workspace_accepts_new_activity(connection, record.workspace_id)?;
            connection.execute(
                "INSERT INTO workspace_deferred_mutations(
                    deferred_mutation_id, workspace_id, session_id, relative_path,
                    expected_hash, reason, state, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending_reread', ?7)",
                params![
                    record.deferred_mutation_id.to_string(),
                    record.workspace_id.to_string(),
                    record.session_id.to_string(),
                    record.path.to_string(),
                    record.expected_hash.as_ref().map(ToString::to_string),
                    record.reason,
                    record.created_at.to_string(),
                ],
            )?;
            Ok(())
        })
    }

    pub fn resolve_deferred_mutations_for_path(
        &self,
        workspace_id: WorkspaceId,
        path: &WorkspacePath,
        correlation_id: CorrelationId,
    ) -> Result<u64, PersistenceError> {
        let path = path.to_string();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let changed = transaction.execute(
                "UPDATE workspace_deferred_mutations
                 SET state='superseded', resolved_at=?3
                 WHERE workspace_id=?1 AND relative_path=?2 AND state='pending_reread'",
                params![workspace_id.to_string(), path, UtcTimestamp::now().to_string()],
            )?;
            if changed > 0 {
                let mut event = PersistentEventDraft::system(
                    "workspace.deferred_mutation.reread_required",
                    correlation_id,
                    json!({
                        "workspace_id": workspace_id,
                        "relative_path": path,
                        "superseded_intents": changed,
                        "requires_model_reread": true,
                    }),
                );
                event.workspace_id = Some(workspace_id);
                insert_event(&transaction, &event)?;
            }
            transaction.commit()?;
            u64::try_from(changed).map_err(|_| {
                PersistenceError::InvalidRequest("deferred mutation count overflow".to_owned())
            })
        })
    }

    pub fn pending_deferred_mutations(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<DeferredMutationSummary>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT deferred_mutation_id, session_id, relative_path, reason, created_at
                 FROM workspace_deferred_mutations
                 WHERE workspace_id=?1 AND state='pending_reread'
                 ORDER BY created_at, deferred_mutation_id",
            )?;
            let rows = statement
                .query_map([workspace_id.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|row| {
                    Ok(DeferredMutationSummary {
                        deferred_mutation_id: DeferredMutationId::from_str(&row.0)?,
                        session_id: SessionId::from_str(&row.1)?,
                        path: WorkspacePath::from_str(&row.2).map_err(invalid_path)?,
                        reason: row.3,
                        created_at: row.4,
                    })
                })
                .collect()
        })
    }

    pub fn begin_workspace_journal(
        &self,
        journal_id: MutationJournalId,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        operation_kind: &str,
        trace: Value,
    ) -> Result<(), PersistenceError> {
        let operation_kind = operation_kind.to_owned();
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            ensure_workspace_accepts_new_activity(connection, workspace_id)?;
            let now = UtcTimestamp::now().to_string();
            connection.execute(
                "INSERT INTO workspace_operation_journals(
                    journal_id, workspace_id, session_id, runtime_instance_id,
                    operation_kind, state, trace_json, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 'preparing', ?6, ?7, ?7)",
                params![
                    journal_id.to_string(),
                    workspace_id.to_string(),
                    session_id.to_string(),
                    runtime_instance_id.to_string(),
                    operation_kind,
                    serde_json::to_string(&trace)?,
                    now,
                ],
            )?;
            Ok(())
        })
    }

    pub fn update_workspace_journal(
        &self,
        journal_id: MutationJournalId,
        state: &str,
        trace: Value,
    ) -> Result<(), PersistenceError> {
        let state = state.to_owned();
        if !matches!(
            state.as_str(),
            "applying" | "compensating" | "completed" | "compensated" | "reconciliation_required"
        ) {
            return Err(PersistenceError::InvalidRequest("invalid journal state".to_owned()));
        }
        self.dispatch(move |connection| {
            if connection.execute(
                "UPDATE workspace_operation_journals
                 SET state=?2,
                     trace_json=json_set(?3, '$.correlation_id', json_extract(trace_json, '$.correlation_id')),
                     updated_at=?4 WHERE journal_id=?1",
                params![
                    journal_id.to_string(),
                    state,
                    serde_json::to_string(&trace)?,
                    UtcTimestamp::now().to_string(),
                ],
            )? != 1
            {
                return Err(PersistenceError::InvalidRequest("journal not found".to_owned()));
            }
            Ok(())
        })
    }

    pub fn audit_workspace_mutation(
        &self,
        audit: MutationAuditRecord,
    ) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            connection.execute(
                "INSERT INTO workspace_mutation_audit(
                    workspace_id, session_id, operation_kind, relative_path,
                    ignored, generated, before_hash, after_hash, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    audit.workspace_id.to_string(),
                    audit.session_id.to_string(),
                    audit.operation_kind,
                    audit.path.to_string(),
                    audit.ignored,
                    audit.generated,
                    audit.before_hash.as_ref().map(ToString::to_string),
                    audit.after_hash.as_ref().map(ToString::to_string),
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            Ok(())
        })
    }

    pub fn workspace_mutation_audit_count(
        &self,
        workspace_id: WorkspaceId,
        path: &WorkspacePath,
    ) -> Result<u64, PersistenceError> {
        let path = path.to_string();
        self.with_reader(|connection| {
            let count = connection.query_row(
                "SELECT COUNT(*) FROM workspace_mutation_audit
                 WHERE workspace_id=?1 AND relative_path=?2",
                params![workspace_id.to_string(), path],
                |row| row.get::<_, i64>(0),
            )?;
            parse_sql_u64(count)
        })
    }

    pub fn load_workspace_mutation_audits(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<MutationAuditRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT session_id, operation_kind, relative_path, ignored, generated,
                        before_hash, after_hash
                 FROM workspace_mutation_audit
                 WHERE workspace_id=?1 ORDER BY audit_id",
            )?;
            let rows = statement
                .query_map([workspace_id.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, bool>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|row| {
                    Ok(MutationAuditRecord {
                        workspace_id,
                        session_id: SessionId::from_str(&row.0)?,
                        operation_kind: row.1,
                        path: WorkspacePath::from_str(&row.2).map_err(invalid_path)?,
                        ignored: row.3,
                        generated: row.4,
                        before_hash: row.5.map(parse_hash).transpose()?,
                        after_hash: row.6.map(parse_hash).transpose()?,
                    })
                })
                .collect()
        })
    }
}

impl PersistenceCoordinator {
    /// Claims the next catalog instance generation for one workspace.
    ///
    /// Every open of a workspace catalog claims one. The generation is what lets an
    /// instance that has been replaced — by a close and reopen, or by a second runtime
    /// over the same database — be told so by the database rather than by a convention:
    /// its commits present the generation they claimed and are refused once a newer one
    /// exists. The claim is one row update and does not touch the catalog revision,
    /// freshness or any entry, so an open that writes nothing still changes who owns the
    /// index.
    ///
    /// Returns the generation this call claimed, which is strictly greater than every
    /// generation claimed before it.
    pub fn claim_workspace_catalog_instance(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<u64, PersistenceError> {
        let runtime = self.runtime_instance_id().to_string();
        self.dispatch(move |connection| {
            let updated = connection.execute(
                "INSERT INTO workspace_catalog_instances(
                    workspace_id, instance_seq, runtime_instance_id, claimed_at
                 ) VALUES (?1, 1, ?2, ?3)
                 ON CONFLICT(workspace_id) DO UPDATE SET
                    instance_seq = workspace_catalog_instances.instance_seq + 1,
                    runtime_instance_id = ?2,
                    claimed_at = ?3",
                params![workspace_id.to_string(), runtime, UtcTimestamp::now().to_string()],
            )?;
            if updated != 1 {
                return Err(PersistenceError::WorkspaceNotFound);
            }
            let claimed = connection.query_row(
                "SELECT instance_seq FROM workspace_catalog_instances WHERE workspace_id=?1",
                [workspace_id.to_string()],
                |row| row.get::<_, i64>(0),
            )?;
            parse_sql_u64(claimed)
        })
    }

    /// The current catalog instance generation of one workspace, or zero when no catalog
    /// has ever been opened for it.
    pub fn workspace_catalog_instance(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<u64, PersistenceError> {
        self.workspace_catalog_commit_position(workspace_id).map(|position| position.instance_seq)
    }

    /// The durable commit position of one workspace's catalog, read in **one** statement.
    ///
    /// The revision and the owning instance generation are two halves of one condition,
    /// so they are read together: sampling them separately is how a caller ends up
    /// deciding against two different points in time.
    pub fn workspace_catalog_commit_position(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<CatalogCommitPosition, PersistenceError> {
        self.with_reader(move |connection| {
            let position = connection
                .query_row(
                    "SELECT w.catalog_revision, COALESCE(i.instance_seq, 0)
                     FROM workspaces AS w
                     LEFT JOIN workspace_catalog_instances AS i
                        ON i.workspace_id = w.workspace_id
                     WHERE w.workspace_id=?1",
                    [workspace_id.to_string()],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let Some((revision, instance)) = position else {
                return Err(PersistenceError::WorkspaceNotFound);
            };
            Ok(CatalogCommitPosition {
                catalog_revision: Revision::new(parse_sql_u64(revision)?),
                instance_seq: parse_sql_u64(instance)?,
            })
        })
    }

    /// Whether one transaction still holds the commit right it is about to use.
    ///
    /// This is the first statement of a catalog-writing transaction: it answers
    /// "is the durable revision still the one the plan was derived from, and is the
    /// instance presenting the plan still the one that owns this workspace's index?".
    /// A `false` answer means the caller must not write anything, and because the check
    /// runs inside the transaction that would have written, no other writer can slip
    /// between the answer and the modification.
    /// The commit right's own answer, read inside the transaction that would use it.
    ///
    /// One statement returns both halves of the durable position, so the decision is taken
    /// against a single point in time and the refusal can say which half moved.
    fn catalog_commit_guard(
        transaction: &Transaction<'_>,
        workspace_id: WorkspaceId,
        expected: CatalogCommitPosition,
    ) -> Result<Result<(), CatalogCommitRefusal>, PersistenceError> {
        let observed = transaction
            .query_row(
                "SELECT w.catalog_revision, COALESCE(i.instance_seq, 0)
                 FROM workspaces AS w
                 LEFT JOIN workspace_catalog_instances AS i
                    ON i.workspace_id = w.workspace_id
                 WHERE w.workspace_id=?1",
                [workspace_id.to_string()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((revision, instance)) = observed else {
            return Ok(Err(CatalogCommitRefusal::WorkspaceMissing));
        };
        let revision = Revision::new(parse_sql_u64(revision)?);
        let instance = parse_sql_u64(instance)?;
        if instance != expected.instance_seq {
            return Ok(Err(CatalogCommitRefusal::InstanceReplaced { current: instance }));
        }
        if revision != expected.catalog_revision {
            return Ok(Err(CatalogCommitRefusal::RevisionMoved { current: revision }));
        }
        Ok(Ok(()))
    }

    pub fn replace_workspace_catalog(
        &self,
        workspace_id: WorkspaceId,
        entries: Vec<CatalogEntryRecord>,
        partitions: Vec<CatalogPartitionRecord>,
        correlation_id: CorrelationId,
    ) -> Result<Revision, PersistenceError> {
        match self.replace_workspace_catalog_inner(
            workspace_id,
            None,
            entries,
            partitions,
            correlation_id,
            None,
        )? {
            CatalogCommitOutcome::Applied(revision) => Ok(revision),
            CatalogCommitOutcome::Refused(_) => Err(PersistenceError::InvalidRequest(
                "an unguarded catalog replacement cannot be refused".to_owned(),
            )),
        }
    }

    /// Replaces a workspace's whole catalog **only if** the expected durable position
    /// still holds.
    ///
    /// [`CatalogCommitOutcome::Refused`] means the guard refused the write: the catalog
    /// revision moved, or a newer catalog instance claimed this workspace, and the
    /// transaction rolled back without touching an entry, a partition, a text row or the
    /// revision.
    /// A write in jobs checks `stop` between jobs and is refused with
    /// [`CatalogCommitRefusal::Stopped`] once it is set.
    pub fn replace_workspace_catalog_guarded(
        &self,
        workspace_id: WorkspaceId,
        expected: CatalogCommitPosition,
        entries: Vec<CatalogEntryRecord>,
        partitions: Vec<CatalogPartitionRecord>,
        correlation_id: CorrelationId,
        stop: Option<Arc<AtomicBool>>,
    ) -> Result<CatalogCommitOutcome, PersistenceError> {
        self.replace_workspace_catalog_inner(
            workspace_id,
            Some(expected),
            entries,
            partitions,
            correlation_id,
            stop,
        )
    }

    fn replace_workspace_catalog_inner(
        &self,
        workspace_id: WorkspaceId,
        expected: Option<CatalogCommitPosition>,
        entries: Vec<CatalogEntryRecord>,
        partitions: Vec<CatalogPartitionRecord>,
        correlation_id: CorrelationId,
        stop: Option<Arc<AtomicBool>>,
    ) -> Result<CatalogCommitOutcome, PersistenceError> {
        // A rebuild is derived index work on its own lane: it must not take the
        // interactive lane's capacity, and an interactive write must not wait behind the
        // whole rebuild. Its own budget is finite (see `INDEX_DEADLINE`), and the job
        // carries the caller's identity so a replacement whose caller stopped waiting
        // leaves a terminal record instead of an unresolvable unknown.
        // A large index, new or stored, is written in jobs, so interactive writes run between
        // them: deleting a large stored index is as long as writing one.
        if entries.len() > catalog_write::BATCH
            || self.with_reader(|connection| {
                stale_index::more_than_one_batch(connection, workspace_id)
            })?
        {
            return catalog_write::replace_in_batches(
                self,
                workspace_id,
                expected,
                entries,
                partitions,
                correlation_id,
                stop,
            );
        }
        let identity = Self::workspace_catalog_write_identity(workspace_id, correlation_id);
        self.dispatch_class_identified(
            WorkClass::Index,
            Some(self.index_deadline_instant()),
            identity,
            move |connection| {
                let transaction = connection.transaction()?;
                if let Some(expected) = expected
                    && let Err(refusal) =
                        Self::catalog_commit_guard(&transaction, workspace_id, expected)?
                {
                    // Nothing has been written yet, and the transaction is rolled back by
                    // dropping it: a refused plan leaves no trace at all.
                    return Ok(CatalogCommitOutcome::Refused(refusal));
                }
                catalog_write::delete_old(&transaction, &workspace_id.to_string(), None)?;
                catalog_write::insert_partitions(&transaction, workspace_id, partitions)?;
                catalog_write::insert_entries(&transaction, workspace_id, entries)?;
                // The revision the whole-set replacement publishes is claimed, not assumed:
                // for a guarded commit the update also carries the revision the plan was
                // derived from, so the condition and the modification are one statement as
                // well as one transaction.
                let updated = match expected {
                    Some(expected) => transaction.execute(
                        "UPDATE workspaces SET
                        catalog_freshness='fresh', catalog_revision=catalog_revision+1,
                        revision=revision+1, updated_at=?3
                     WHERE workspace_id=?1 AND catalog_revision=?2",
                        params![
                            workspace_id.to_string(),
                            as_sql_u64(expected.catalog_revision.value())?,
                            UtcTimestamp::now().to_string()
                        ],
                    )?,
                    None => transaction.execute(
                        "UPDATE workspaces SET
                        catalog_freshness='fresh', catalog_revision=catalog_revision+1,
                        revision=revision+1, updated_at=?2
                     WHERE workspace_id=?1",
                        params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
                    )?,
                };
                if updated != 1 {
                    return Err(PersistenceError::WorkspaceNotFound);
                }
                let revision = transaction.query_row(
                    "SELECT catalog_revision FROM workspaces WHERE workspace_id=?1",
                    [workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )?;
                let mut event = PersistentEventDraft::system(
                    "workspace.catalog.rebuilt",
                    correlation_id,
                    json!({ "workspace_id": workspace_id, "catalog_revision": revision }),
                );
                event.workspace_id = Some(workspace_id);
                insert_event(&transaction, &event)?;
                transaction.commit()?;
                Ok(CatalogCommitOutcome::Applied(Revision::new(parse_sql_u64(revision)?)))
            },
        )
    }

    pub fn mark_catalog_stale(&self, workspace_id: WorkspaceId) -> Result<(), PersistenceError> {
        self.dispatch(move |connection| {
            if connection.execute(
                "UPDATE workspaces SET catalog_freshness='stale', updated_at=?2
                 WHERE workspace_id=?1",
                params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
            )? != 1
            {
                return Err(PersistenceError::WorkspaceNotFound);
            }
            Ok(())
        })
    }

    /// Paths whose indexed text could not be adopted and has to be re-derived.
    ///
    /// The migration marks a path when several text rows existed for it and none could
    /// be proven current; those rows are dropped, so the path is not stale in the
    /// ordinary sense — it has no text at all. The catalog re-reads exactly these paths
    /// when it opens, which repairs the derived index from the filesystem instead of
    /// from a guess about which row was newer.
    pub fn workspace_content_rebuild_paths(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<WorkspacePath>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT relative_path FROM workspace_content_index
                 WHERE workspace_id=?1 AND needs_reindex=1
                 ORDER BY relative_path",
            )?;
            let rows = statement
                .query_map([workspace_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|path| WorkspacePath::from_str(&path).map_err(invalid_path))
                .collect()
        })
    }

    /// Paths whose text the durable index really holds.
    ///
    /// The catalog uses this to tell "every recorded fact matches" from "the text is
    /// there": a path whose text row was dropped — by an upgrade that could not prove
    /// which generation was current — has to be written again even though the file did
    /// not change.
    pub fn workspace_content_paths(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<WorkspacePath>, PersistenceError> {
        // A whole index's read: its size, not an interactive wait, sets how long it takes.
        self.read_within(self.index_deadline(), |connection| {
            let mut statement = connection.prepare(
                "SELECT relative_path FROM workspace_content_index
                 WHERE workspace_id=?1 AND fts_rowid IS NOT NULL AND needs_reindex=0
                 ORDER BY relative_path",
            )?;
            let rows = statement
                .query_map([workspace_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|path| WorkspacePath::from_str(&path).map_err(invalid_path))
                .collect()
        })
    }

    /// Applies one incremental catalog delta inside a single transaction.
    ///
    /// This is the ordinary update path. It never deletes and rewrites the whole
    /// workspace catalog, so a single-directory change no longer costs a full
    /// rescan and unrelated directory cursors keep their meaning. A caller that
    /// cannot describe the change locally still uses
    /// [`Self::replace_workspace_catalog`] and records why.
    ///
    /// Re-derivation marks settle in this same transaction: a repaired path gives
    /// its mark up only with the committed conclusion for that exact path, and a
    /// failed write leaves every mark untouched.
    pub fn apply_workspace_catalog_delta(
        &self,
        workspace_id: WorkspaceId,
        delta: CatalogDeltaRecord,
        correlation_id: CorrelationId,
    ) -> Result<Revision, PersistenceError> {
        match self.apply_workspace_catalog_delta_with_fault_inner(
            workspace_id,
            None,
            delta,
            correlation_id,
            None,
        )? {
            CatalogCommitOutcome::Applied(revision) => Ok(revision),
            CatalogCommitOutcome::Refused(_) => Err(PersistenceError::InvalidRequest(
                "an unguarded catalog delta cannot be refused".to_owned(),
            )),
        }
    }

    /// The same delta, but only if the expected durable revision and the claiming
    /// catalog instance still hold, checked inside the writing transaction.
    ///
    /// [`CatalogCommitOutcome::Refused`] is the honest refusal: the revision moved, or a
    /// newer instance claimed this workspace's index, and the transaction rolled back
    /// without writing an entry, a partition, a text row, a mark or a revision.
    pub fn apply_workspace_catalog_delta_guarded(
        &self,
        workspace_id: WorkspaceId,
        expected: CatalogCommitPosition,
        delta: CatalogDeltaRecord,
        correlation_id: CorrelationId,
    ) -> Result<CatalogCommitOutcome, PersistenceError> {
        self.apply_workspace_catalog_delta_with_fault_inner(
            workspace_id,
            Some(expected),
            delta,
            correlation_id,
            None,
        )
    }

    /// The same delta with a real injected failure point, so a caller can prove what a
    /// rolled-back index write leaves behind: nothing. The failure is raised inside the
    /// transaction, after every statement has run and before the commit, which is the
    /// point at which a partial write would become visible if the boundary were wrong.
    pub fn apply_workspace_catalog_delta_with_fault(
        &self,
        workspace_id: WorkspaceId,
        delta: CatalogDeltaRecord,
        correlation_id: CorrelationId,
    ) -> Result<Revision, PersistenceError> {
        match self.apply_workspace_catalog_delta_with_fault_inner(
            workspace_id,
            None,
            delta,
            correlation_id,
            Some(crate::PersistenceFaultPoint::BeforeCommit),
        )? {
            CatalogCommitOutcome::Applied(revision) => Ok(revision),
            CatalogCommitOutcome::Refused(_) => Err(PersistenceError::InvalidRequest(
                "an unguarded catalog delta cannot be refused".to_owned(),
            )),
        }
    }

    fn apply_workspace_catalog_delta_with_fault_inner(
        &self,
        workspace_id: WorkspaceId,
        expected: Option<CatalogCommitPosition>,
        delta: CatalogDeltaRecord,
        correlation_id: CorrelationId,
        fault: Option<crate::PersistenceFaultPoint>,
    ) -> Result<CatalogCommitOutcome, PersistenceError> {
        // One bounded catalog delta is index work: it rebuilds derived facts and is
        // scheduled on the index lane, so an interactive write that is already waiting is
        // taken first. The job carries the caller's identity, so a delta whose caller
        // stopped waiting leaves a terminal record instead of an unresolvable unknown.
        let identity = Self::workspace_catalog_write_identity(workspace_id, correlation_id);
        self.dispatch_class_identified(
            WorkClass::Index,
            Some(self.index_deadline_instant()),
            identity,
            move |connection| {
                let transaction = connection.transaction()?;
                // The guard is the first statement of the transaction, before any
                // non-revertible write: a plan whose revision moved, or whose instance no
                // longer owns this workspace's index, is refused without touching anything.
                if let Some(expected) = expected
                    && let Err(refusal) =
                        Self::catalog_commit_guard(&transaction, workspace_id, expected)?
                {
                    return Ok(CatalogCommitOutcome::Refused(refusal));
                }
                let CatalogDeltaRecord {
                    upserts,
                    removed_paths,
                    partitions,
                    removed_partitions,
                    repaired_paths,
                    reason,
            } = delta;
            let repaired = repaired_paths.into_iter().collect::<BTreeSet<_>>();
            for path in &removed_partitions {
                transaction.execute(
                    "DELETE FROM workspace_catalog_partitions
                     WHERE workspace_id=?1 AND partition_path=?2",
                    params![workspace_id.to_string(), path.to_string()],
                )?;
            }
            for path in &removed_paths {
                remove_workspace_text_row(&transaction, workspace_id, path)?;
                transaction.execute(
                    "DELETE FROM workspace_catalog_entries WHERE workspace_id=?1 AND relative_path=?2",
                    params![workspace_id.to_string(), path.to_string()],
                )?;
            }
            for entry in upserts {
                // An upsert replaces exactly one entry. A moved or retyped path
                // cannot keep a stale kind, partition or text index. The old text row
                // is dropped first — writing new text must not be able to leave the
                // previous generation's row behind.
                remove_workspace_text_row(&transaction, workspace_id, &entry.path)?;
                // A repaired file whose text this write still cannot establish keeps
                // its mark: the row above just dropped it, and the recoverable fact
                // that the index owes the path a text row has to survive the write.
                // A path that no longer is a file holds no text by conclusion, so
                // its mark is settled by the same drop.
                let text_unproven =
                    repaired.contains(&entry.path) && entry.kind == CatalogEntryKind::File;
                transaction.execute(
                    "DELETE FROM workspace_catalog_entries WHERE workspace_id=?1 AND relative_path=?2",
                    params![workspace_id.to_string(), entry.path.to_string()],
                )?;
                transaction.execute(
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
                    transaction.execute(
                        "INSERT INTO workspace_content_fts(workspace_id, relative_path, content)
                         VALUES (?1, ?2, ?3)",
                        params![workspace_id.to_string(), entry.path.to_string(), content],
                    )?;
                    transaction.execute(
                        "INSERT INTO workspace_content_index(
                            workspace_id, relative_path, fts_rowid, content_hash, needs_reindex
                         ) VALUES (?1, ?2, ?3, ?4, 0)",
                        params![
                            workspace_id.to_string(),
                            entry.path.to_string(),
                            transaction.last_insert_rowid(),
                            content_hash
                        ],
                    )?;
                } else if text_unproven {
                    transaction.execute(
                        "INSERT INTO workspace_content_index(
                            workspace_id, relative_path, fts_rowid, content_hash, needs_reindex
                         ) VALUES (?1, ?2, NULL, NULL, 1)",
                        params![workspace_id.to_string(), entry.path.to_string()],
                    )?;
                }
            }
            for partition in partitions {
                transaction.execute(
                    "DELETE FROM workspace_catalog_partitions
                     WHERE workspace_id=?1 AND partition_path=?2",
                    params![workspace_id.to_string(), partition.partition_path.to_string()],
                )?;
                transaction.execute(
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
            // Freshness is only restored by the operation that can restore it. One
            // bounded delta makes the paths it describes current; it says nothing about
            // the rest of the workspace, so a catalog that was `stale` (a lost watcher
            // event, a rebuild that has not finished) stays stale until the full rebuild
            // commits. Marking it fresh here is what let a single-path update hide an
            // unfinished rebuild behind an authoritative-looking answer.
            let updated = match expected {
                Some(expected) => transaction.execute(
                    "UPDATE workspaces SET
                        catalog_freshness=CASE
                            WHEN catalog_freshness='stale' THEN 'stale'
                            WHEN catalog_freshness='rebuilding' THEN 'rebuilding'
                            ELSE 'fresh'
                        END,
                        catalog_revision=catalog_revision+1,
                        revision=revision+1, updated_at=?3
                     WHERE workspace_id=?1 AND catalog_revision=?2",
                    params![
                        workspace_id.to_string(),
                        as_sql_u64(expected.catalog_revision.value())?,
                        UtcTimestamp::now().to_string()
                    ],
                )?,
                None => transaction.execute(
                    "UPDATE workspaces SET
                        catalog_freshness=CASE
                            WHEN catalog_freshness='stale' THEN 'stale'
                            WHEN catalog_freshness='rebuilding' THEN 'rebuilding'
                            ELSE 'fresh'
                        END,
                        catalog_revision=catalog_revision+1,
                        revision=revision+1, updated_at=?2
                     WHERE workspace_id=?1",
                    params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
                )?,
            };
            if updated != 1 {
                return Err(PersistenceError::WorkspaceNotFound);
            }
            let revision = transaction.query_row(
                "SELECT catalog_revision FROM workspaces WHERE workspace_id=?1",
                [workspace_id.to_string()],
                |row| row.get::<_, i64>(0),
            )?;
            let mut event = PersistentEventDraft::system(
                "workspace.catalog.updated",
                correlation_id,
                json!({ "workspace_id": workspace_id, "catalog_revision": revision, "scope": reason }),
            );
            event.workspace_id = Some(workspace_id);
            insert_event(&transaction, &event)?;
            if fault == Some(crate::PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected persistence failure before commit".to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(CatalogCommitOutcome::Applied(Revision::new(parse_sql_u64(revision)?)))
            },
        )
    }

    pub fn record_path_use(
        &self,
        workspace_id: WorkspaceId,
        path: &WorkspacePath,
    ) -> Result<(), PersistenceError> {
        let path = path.to_string();
        self.dispatch(move |connection| {
            connection.execute(
                "INSERT INTO workspace_path_recent_use(workspace_id, relative_path, used_at, use_count)
                 VALUES (?1, ?2, ?3, 1)
                 ON CONFLICT(workspace_id, relative_path) DO UPDATE SET
                    used_at=excluded.used_at, use_count=use_count+1",
                params![workspace_id.to_string(), path, UtcTimestamp::now().to_string()],
            )?;
            Ok(())
        })
    }

    /// How many catalog events of one type this workspace has recorded.
    ///
    /// A bounded update and a full rebuild are deliberately different event
    /// types, so a caller can prove which path ran instead of inferring it from
    /// timing.
    pub fn workspace_catalog_event_count(
        &self,
        workspace_id: WorkspaceId,
        event_type: &str,
    ) -> Result<u64, PersistenceError> {
        let event_type = event_type.to_owned();
        self.with_reader(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM events WHERE workspace_id=?1 AND event_type=?2",
                params![workspace_id.to_string(), event_type],
                |row| row.get(0),
            )?;
            parse_sql_u64(count)
        })
    }

    pub fn load_path_recent_use(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<(WorkspacePath, u64)>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT relative_path, use_count
                 FROM workspace_path_recent_use
                 WHERE workspace_id=?1
                 ORDER BY used_at, relative_path",
            )?;
            let rows = statement
                .query_map([workspace_id.to_string()], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(|(path, count)| {
                    Ok((
                        WorkspacePath::from_str(&path).map_err(invalid_path)?,
                        parse_sql_u64(count)?,
                    ))
                })
                .collect()
        })
    }

    pub fn complete_workspace_file_mutation(
        &self,
        workspace_id: WorkspaceId,
        correlation_id: CorrelationId,
        operation_kind: &str,
    ) -> Result<Revision, PersistenceError> {
        let operation_kind = operation_kind.to_owned();
        let committed = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            if transaction.execute(
                "UPDATE workspaces SET
                    revision=revision+1,
                    catalog_freshness='stale',
                    updated_at=?2
                 WHERE workspace_id=?1",
                params![workspace_id.to_string(), UtcTimestamp::now().to_string()],
            )? != 1
            {
                return Err(PersistenceError::WorkspaceNotFound);
            }
            let revision = transaction.query_row(
                "SELECT revision FROM workspaces WHERE workspace_id=?1",
                [workspace_id.to_string()],
                |row| row.get::<_, i64>(0),
            )?;
            let mut event = PersistentEventDraft::system(
                "workspace.files.mutated",
                correlation_id,
                json!({
                    "workspace_id": workspace_id,
                    "workspace_revision": revision,
                    "operation_kind": operation_kind,
                }),
            );
            event.workspace_id = Some(workspace_id);
            insert_event(&transaction, &event)?;
            let committed = commit_projection(&transaction)?;
            transaction.commit()?;
            Ok((committed, Revision::new(parse_sql_u64(revision)?)))
        })?;
        let (projection, revision) = committed;
        self.publish_and_mark(projection)?;
        Ok(revision)
    }
}

impl PersistenceCoordinator {
    pub fn load_workspace_catalog(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(Vec<CatalogEntryRecord>, Vec<CatalogPartitionRecord>, Revision), PersistenceError>
    {
        // A whole index's read: its size, not an interactive wait, sets how long it takes.
        self.read_within(self.index_deadline(), |connection| {
            let revision = connection
                .query_row(
                    "SELECT catalog_revision FROM workspaces WHERE workspace_id=?1",
                    [workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .ok_or(PersistenceError::WorkspaceNotFound)?;
            let mut statement = connection.prepare(
                "SELECT relative_path, parent_path, file_name, entry_kind, partition_path,
                        byte_length, modified_at, content_hash, text_encoding, ignored,
                        generated, hidden_from_search, reparse_tag, change_usn,
                        volume_serial, file_id
                 FROM workspace_catalog_entries WHERE workspace_id=?1 ORDER BY relative_path",
            )?;
            let rows = statement
                .query_map([workspace_id.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, bool>(9)?,
                        row.get::<_, bool>(10)?,
                        row.get::<_, bool>(11)?,
                        row.get::<_, Option<i64>>(12)?,
                        row.get::<_, Option<i64>>(13)?,
                        row.get::<_, Option<String>>(14)?,
                        row.get::<_, Option<String>>(15)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut entries = Vec::with_capacity(rows.len());
            for row in rows {
                entries.push(CatalogEntryRecord {
                    path: WorkspacePath::from_str(&row.0).map_err(invalid_path)?,
                    parent_path: WorkspacePath::from_str(&row.1).map_err(invalid_path)?,
                    file_name: row.2,
                    kind: parse_entry_kind(&row.3)?,
                    partition_path: WorkspacePath::from_str(&row.4).map_err(invalid_path)?,
                    byte_length: row.5.map(parse_sql_u64).transpose()?,
                    modified_at: row.6,
                    content_hash: row.7.map(parse_hash).transpose()?,
                    text_encoding: row.8.map(|value| parse_encoding(&value)).transpose()?,
                    ignored: row.9,
                    generated: row.10,
                    hidden_from_search: row.11,
                    reparse_tag: row
                        .12
                        .map(|value| {
                            u32::try_from(value).map_err(|_| {
                                PersistenceError::InvalidRequest("invalid reparse tag".to_owned())
                            })
                        })
                        .transpose()?,
                    content: None,
                    change_usn: row.13.map(parse_sql_u64).transpose()?,
                    volume_serial: row.14,
                    file_id: row.15,
                });
            }
            let mut statement = connection.prepare(
                "SELECT partition_path, file_count, indexable_text_bytes, index_mode
                 FROM workspace_catalog_partitions WHERE workspace_id=?1 ORDER BY partition_path",
            )?;
            let rows = statement
                .query_map([workspace_id.to_string()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut partitions = Vec::with_capacity(rows.len());
            for (path, count, bytes, mode) in rows {
                partitions.push(CatalogPartitionRecord {
                    partition_path: WorkspacePath::from_str(&path).map_err(invalid_path)?,
                    file_count: parse_sql_u64(count)?,
                    indexable_text_bytes: parse_sql_u64(bytes)?,
                    index_mode: parse_index_mode(&mode)?,
                });
            }
            Ok((entries, partitions, Revision::new(parse_sql_u64(revision)?)))
        })
    }

    pub fn permanently_delete_workspace(
        &self,
        workspace_id: WorkspaceId,
        lifecycle: WorkspaceLifecycleRecord,
    ) -> Result<WorkspaceDeleteReport, PersistenceError> {
        validate_lifecycle_record(&lifecycle)?;
        if lifecycle.summary.workspace_id != workspace_id
            || lifecycle.summary.operation != WorkspaceLifecycleOperationKind::PermanentDelete
            || lifecycle.summary.state != WorkspaceLifecycleState::Completed
        {
            return Err(PersistenceError::InvalidRequest(
                "permanent-delete lifecycle record is invalid".to_owned(),
            ));
        }
        // What only whole-table reads find, and the rows there are thousands of, go first in
        // short background jobs, so the transaction below holds the writer briefly.
        let subject = workspace_id.to_string();
        let mut scanned = self.with_reader(|connection| {
            let state = connection
                .query_row(
                    "SELECT state FROM workspaces WHERE workspace_id=?1",
                    [&subject],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(PersistenceError::WorkspaceNotFound)?;
            if parse_workspace_state(&state)? != WorkspaceState::Archived {
                return Err(PersistenceError::WorkspaceBusy);
            }
            crate::registry_purge::of_workspace(connection, &subject)
        })?;
        scanned.delete_text(self)?;
        crate::registry_purge::delete_catalog(self, &subject)?;
        let (report, committed) = self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let state = transaction
                .query_row(
                    "SELECT state FROM workspaces WHERE workspace_id=?1",
                    [workspace_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or(PersistenceError::WorkspaceNotFound)?;
            if parse_workspace_state(&state)? != WorkspaceState::Archived
                || !query_blockers(&transaction, workspace_id)?.is_clear()
            {
                return Err(PersistenceError::WorkspaceBusy);
            }
            let session_ids = {
                let mut statement = transaction.prepare(
                    "SELECT session_id FROM sessions WHERE workspace_id=?1 ORDER BY created_at",
                )?;
                statement
                    .query_map([workspace_id.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let artifact_ids = {
                let mut statement = transaction.prepare(
                    "SELECT artifact_id FROM artifact_owners
                     WHERE owner_kind='session'
                       AND owner_id IN (
                           SELECT session_id FROM sessions WHERE workspace_id=?1
                       )",
                )?;
                statement
                    .query_map([workspace_id.to_string()], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let now = UtcTimestamp::now().to_string();
            if transaction.execute(
                "UPDATE workspace_lifecycle_operations SET
                    state='completed', lifecycle_revision=?2, preflight_json=?3,
                    error_code=NULL, updated_at=?4, terminal_at=?4
                 WHERE operation_id=?1 AND workspace_id=?5 AND state='committing'",
                params![
                    lifecycle.summary.operation_id.to_string(),
                    as_sql_u64(lifecycle.summary.lifecycle_revision.value())?,
                    lifecycle.summary.preflight.as_ref().map(serde_json::to_string).transpose()?,
                    now,
                    workspace_id.to_string(),
                ],
            )? != 1
            {
                return Err(PersistenceError::InvalidRequest(
                    "permanent-delete lifecycle operation was not committing".to_owned(),
                ));
            }
            crate::session_lifecycle::record_removal(
                &transaction,
                workspace_id,
                &session_ids,
                &now,
            )?;
            transaction.execute(
                "DELETE FROM workspaces WHERE workspace_id=?1",
                [workspace_id.to_string()],
            )?;
            let mut queued = 0_u64;
            for artifact_id in artifact_ids {
                queued += u64::from(
                    transaction.execute(
                        "INSERT OR IGNORE INTO artifact_gc_queue(
                            artifact_id, enqueued_at, reason
                         ) VALUES (?1, ?2, 'workspace_permanently_deleted')",
                        params![artifact_id, now],
                    )? == 1,
                );
            }
            scanned.delete_outbox(
                &transaction,
                &workspace_id.to_string(),
                "projection_kind='workspace_session' AND ?1 IS NOT NULL",
            )?;
            let committed = commit_projection(&transaction)?;
            transaction.commit()?;
            Ok((
                WorkspaceDeleteReport {
                    ended_sessions: session_ids
                        .into_iter()
                        .map(|value| SessionId::from_str(&value))
                        .collect::<Result<Vec<_>, _>>()?,
                    artifacts_queued_for_gc: queued,
                },
                committed,
            ))
        })?;
        self.publish_and_mark(committed)?;
        Ok(report)
    }

    pub fn cleanup_notification_count(
        &self,
        notification_kind: &str,
        subject_id: &str,
    ) -> Result<u64, PersistenceError> {
        let notification_kind = notification_kind.to_owned();
        let subject_id = subject_id.to_owned();
        self.with_reader(|connection| {
            let count = connection.query_row(
                "SELECT COUNT(*) FROM cleanup_notifications
                 WHERE notification_kind=?1 AND subject_id=?2",
                params![notification_kind, subject_id],
                |row| row.get::<_, i64>(0),
            )?;
            parse_sql_u64(count)
        })
    }
}

fn validate_lifecycle_record(record: &WorkspaceLifecycleRecord) -> Result<(), PersistenceError> {
    if record.summary.lifecycle_revision.value() == 0
        || record.summary.operation != record.operation.kind()
    {
        return Err(PersistenceError::InvalidRequest(
            "workspace lifecycle record is inconsistent".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_workspace_accepts_new_activity_by_id(
    connection: &Connection,
    workspace_id: &str,
) -> Result<(), PersistenceError> {
    let active = connection
        .query_row(
            "SELECT 1 FROM workspace_lifecycle_operations
             WHERE workspace_id=?1
               AND state NOT IN ('completed', 'cancelled', 'failed')
             LIMIT 1",
            [workspace_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if active { Err(PersistenceError::WorkspaceBusy) } else { Ok(()) }
}

fn ensure_workspace_accepts_new_activity(
    connection: &Connection,
    workspace_id: WorkspaceId,
) -> Result<(), PersistenceError> {
    ensure_workspace_accepts_new_activity_by_id(connection, &workspace_id.to_string())
}

fn lifecycle_state_name(state: WorkspaceLifecycleState) -> &'static str {
    match state {
        WorkspaceLifecycleState::Preparing => "preparing",
        WorkspaceLifecycleState::Blocked => "blocked",
        WorkspaceLifecycleState::Saving => "saving",
        WorkspaceLifecycleState::Discarding => "discarding",
        WorkspaceLifecycleState::Committing => "committing",
        WorkspaceLifecycleState::Completed => "completed",
        WorkspaceLifecycleState::Cancelled => "cancelled",
        WorkspaceLifecycleState::Failed => "failed",
    }
}

fn parse_lifecycle_state(value: &str) -> Result<WorkspaceLifecycleState, PersistenceError> {
    match value {
        "preparing" => Ok(WorkspaceLifecycleState::Preparing),
        "blocked" => Ok(WorkspaceLifecycleState::Blocked),
        "saving" => Ok(WorkspaceLifecycleState::Saving),
        "discarding" => Ok(WorkspaceLifecycleState::Discarding),
        "committing" => Ok(WorkspaceLifecycleState::Committing),
        "completed" => Ok(WorkspaceLifecycleState::Completed),
        "cancelled" => Ok(WorkspaceLifecycleState::Cancelled),
        "failed" => Ok(WorkspaceLifecycleState::Failed),
        _ => {
            Err(PersistenceError::InvalidRequest("persisted lifecycle state is invalid".to_owned()))
        }
    }
}

fn lifecycle_operation_kind_name(kind: WorkspaceLifecycleOperationKind) -> &'static str {
    match kind {
        WorkspaceLifecycleOperationKind::Archive => "archive",
        WorkspaceLifecycleOperationKind::Relocate => "relocate",
        WorkspaceLifecycleOperationKind::PermanentDelete => "permanent_delete",
    }
}

fn parse_lifecycle_operation_kind(
    value: &str,
) -> Result<WorkspaceLifecycleOperationKind, PersistenceError> {
    match value {
        "archive" => Ok(WorkspaceLifecycleOperationKind::Archive),
        "relocate" => Ok(WorkspaceLifecycleOperationKind::Relocate),
        "permanent_delete" => Ok(WorkspaceLifecycleOperationKind::PermanentDelete),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted lifecycle operation kind is invalid".to_owned(),
        )),
    }
}

fn validate_workspace(workspace: &WorkspaceSummary) -> Result<(), PersistenceError> {
    if workspace.display_name.trim().is_empty()
        || workspace.selected_root.trim().is_empty()
        || workspace.canonical_root.trim().is_empty()
        || workspace.revision.value() == 0
        || workspace.authorization_epoch.value() == 0
    {
        return Err(PersistenceError::InvalidRequest(
            "workspace fields and revisions must be valid".to_owned(),
        ));
    }
    Ok(())
}

fn insert_workspace(
    transaction: &Transaction<'_>,
    workspace: &WorkspaceSummary,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO workspaces(
            workspace_id, display_name, selected_root, canonical_root,
            root_volume_serial, root_file_id, state, revision, catalog_freshness,
            authorization_epoch, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            workspace.workspace_id.to_string(),
            workspace.display_name,
            workspace.selected_root,
            workspace.canonical_root,
            workspace.root_identity.as_ref().map(|value| &value.volume_serial_number),
            workspace.root_identity.as_ref().map(|value| &value.file_id),
            workspace_state_name(workspace.state),
            as_sql_u64(workspace.revision.value())?,
            catalog_freshness_name(workspace.catalog_freshness),
            as_sql_u64(workspace.authorization_epoch.value())?,
            UtcTimestamp::now().to_string(),
        ],
    )?;
    Ok(())
}

fn load_workspace(
    connection: &rusqlite::Connection,
    workspace_id: WorkspaceId,
) -> Result<WorkspaceSummary, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT display_name, selected_root, canonical_root, root_volume_serial,
                    root_file_id, state, revision, catalog_freshness, authorization_epoch
             FROM workspaces WHERE workspace_id=?1",
            [workspace_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::WorkspaceNotFound)?;
    Ok(WorkspaceSummary {
        workspace_id,
        display_name: row.0,
        selected_root: row.1,
        canonical_root: row.2,
        root_identity: match (row.3, row.4) {
            (Some(volume_serial_number), Some(file_id)) => {
                Some(aura_domain_common::FileIdentity { volume_serial_number, file_id })
            }
            (None, None) => None,
            _ => {
                return Err(PersistenceError::InvalidRequest(
                    "persisted workspace identity is incomplete".to_owned(),
                ));
            }
        },
        state: parse_workspace_state(&row.5)?,
        revision: Revision::new(parse_sql_u64(row.6)?),
        catalog_freshness: parse_catalog_freshness(&row.7)?,
        authorization_epoch: Revision::new(parse_sql_u64(row.8)?),
    })
}

pub(crate) fn commit_projection(
    transaction: &Transaction<'_>,
) -> Result<CommittedProjection<WorkspaceSessionProjection>, PersistenceError> {
    let payload = load_workspace_session_projection(transaction)?;
    let source_revision = next_projection_revision(transaction, "workspace_session", "global")?;
    transaction.execute(
        "INSERT INTO projection_outbox(
            projection_kind, projection_key, revision, payload_json, created_at
         ) VALUES ('workspace_session', 'global', ?1, ?2, ?3)",
        params![
            as_sql_u64(source_revision)?,
            serde_json::to_string(&payload)?,
            UtcTimestamp::now().to_string(),
        ],
    )?;
    Ok(CommittedProjection { outbox_id: transaction.last_insert_rowid(), source_revision, payload })
}

/// Removes one path's text row through its recorded rowid.
///
/// The FTS table declares `workspace_id` and `relative_path` as `UNINDEXED`, so
/// deleting by those columns scans the whole text index. The rowid map turns the same
/// removal into a primary-key lookup, which is what keeps one incremental batch linear
/// in the number of paths it actually changed. A path with no map entry has no text
/// row: text is only ever written together with its map, and the migration builds the
/// map for rows that already existed.
fn remove_workspace_text_row(
    transaction: &Transaction<'_>,
    workspace_id: WorkspaceId,
    path: &WorkspacePath,
) -> Result<(), PersistenceError> {
    // A mark row carries no text address, so the rowid is only present for a path
    // whose text the index actually holds.
    let rowid = transaction
        .query_row(
            "SELECT fts_rowid FROM workspace_content_index
             WHERE workspace_id=?1 AND relative_path=?2",
            params![workspace_id.to_string(), path.to_string()],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?;
    if let Some(Some(rowid)) = rowid {
        transaction.execute("DELETE FROM workspace_content_fts WHERE rowid=?1", [rowid])?;
    }
    transaction.execute(
        "DELETE FROM workspace_content_index WHERE workspace_id=?1 AND relative_path=?2",
        params![workspace_id.to_string(), path.to_string()],
    )?;
    Ok(())
}

fn query_blockers(
    connection: &rusqlite::Connection,
    workspace_id: WorkspaceId,
) -> Result<WorkspaceBlockers, PersistenceError> {
    let id = workspace_id.to_string();
    let running_objects = connection.query_row(
        "SELECT
            (SELECT COUNT(*) FROM workspace_runtime_objects
             WHERE workspace_id=?1 AND state='active')
          + (SELECT COUNT(*) FROM turn_tasks
             WHERE session_id IN (SELECT session_id FROM sessions WHERE workspace_id=?1)
               AND state NOT IN ('completed', 'cancelled', 'failed', 'interrupted'))",
        [&id],
        |row| row.get::<_, i64>(0),
    )?;
    let runtime_leases = connection.query_row(
        "SELECT COUNT(*) FROM runtime_leases WHERE owner_id=?1 AND state='active'",
        [&id],
        |row| row.get::<_, i64>(0),
    )?;
    let mutation_ownership =
        count_where(connection, "workspace_mutation_ownership", "workspace_id=?1", &id)?;
    let deferred_mutations = count_where(
        connection,
        "workspace_deferred_mutations",
        "workspace_id=?1 AND state='pending_reread'",
        &id,
    )?;
    let workspace_journals = count_where(
        connection,
        "workspace_operation_journals",
        "workspace_id=?1 AND state NOT IN ('completed', 'compensated')",
        &id,
    )?;
    let artifact_overwrite_journals = count_where(
        connection,
        "artifact_overwrite_journal",
        "workspace_id=?1 AND state IN ('prepared', 'applying', 'published', 'needs_reconcile')",
        &id,
    )?;
    let unfinished_journals =
        workspace_journals.checked_add(artifact_overwrite_journals).ok_or_else(|| {
            PersistenceError::InvalidRequest("journal blocker count overflow".to_owned())
        })?;
    Ok(WorkspaceBlockers {
        running_objects: parse_sql_u64(running_objects)?,
        runtime_leases: parse_sql_u64(runtime_leases)?,
        mutation_ownership: parse_sql_u64(mutation_ownership)?,
        deferred_mutations: parse_sql_u64(deferred_mutations)?,
        unfinished_journals: parse_sql_u64(unfinished_journals)?,
    })
}

fn count_where(
    connection: &rusqlite::Connection,
    table: &str,
    predicate: &str,
    value: &str,
) -> Result<i64, PersistenceError> {
    Ok(connection.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"),
        [value],
        |row| row.get::<_, i64>(0),
    )?)
}

fn entry_kind_name(kind: CatalogEntryKind) -> &'static str {
    match kind {
        CatalogEntryKind::File => "file",
        CatalogEntryKind::Directory => "directory",
        CatalogEntryKind::ReparsePoint => "reparse_point",
    }
}

fn parse_entry_kind(value: &str) -> Result<CatalogEntryKind, PersistenceError> {
    match value {
        "file" => Ok(CatalogEntryKind::File),
        "directory" => Ok(CatalogEntryKind::Directory),
        "reparse_point" => Ok(CatalogEntryKind::ReparsePoint),
        _ => Err(PersistenceError::InvalidRequest("invalid catalog entry kind".to_owned())),
    }
}

fn index_mode_name(mode: CatalogIndexMode) -> &'static str {
    match mode {
        CatalogIndexMode::FullText => "full_text",
        CatalogIndexMode::MetadataOnly => "metadata_only",
    }
}

fn parse_index_mode(value: &str) -> Result<CatalogIndexMode, PersistenceError> {
    match value {
        "full_text" => Ok(CatalogIndexMode::FullText),
        "metadata_only" => Ok(CatalogIndexMode::MetadataOnly),
        _ => Err(PersistenceError::InvalidRequest("invalid catalog index mode".to_owned())),
    }
}

fn encoding_name(encoding: TextEncoding) -> &'static str {
    match encoding {
        TextEncoding::Utf8 => "utf8",
        TextEncoding::Utf8Bom => "utf8_bom",
        TextEncoding::Utf16Le => "utf16_le",
        TextEncoding::Utf16Be => "utf16_be",
        TextEncoding::Windows1252 => "windows_1252",
    }
}

fn parse_encoding(value: &str) -> Result<TextEncoding, PersistenceError> {
    match value {
        "utf8" => Ok(TextEncoding::Utf8),
        "utf8_bom" => Ok(TextEncoding::Utf8Bom),
        "utf16_le" => Ok(TextEncoding::Utf16Le),
        "utf16_be" => Ok(TextEncoding::Utf16Be),
        "windows_1252" => Ok(TextEncoding::Windows1252),
        _ => Err(PersistenceError::InvalidRequest("invalid text encoding".to_owned())),
    }
}

fn parse_hash(value: String) -> Result<ContentHash, PersistenceError> {
    let (algorithm, hex) = value.split_once(':').ok_or_else(|| {
        PersistenceError::InvalidRequest("persisted content hash has no algorithm".to_owned())
    })?;
    let algorithm = match algorithm {
        "blake3" => ContentHashAlgorithm::Blake3,
        "sha256" => ContentHashAlgorithm::Sha256,
        _ => {
            return Err(PersistenceError::InvalidRequest(
                "persisted content hash algorithm is invalid".to_owned(),
            ));
        }
    };
    Ok(ContentHash::new(algorithm, hex.to_owned())?)
}

fn as_sql_u64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value)
        .map_err(|_| PersistenceError::InvalidRequest("value exceeds SQLite integer".to_owned()))
}

fn parse_sql_u64(value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value)
        .map_err(|_| PersistenceError::InvalidRequest("negative persisted integer".to_owned()))
}

fn invalid_path(error: aura_domain_common::WorkspacePathError) -> PersistenceError {
    PersistenceError::InvalidRequest(format!("invalid persisted workspace path: {error}"))
}
