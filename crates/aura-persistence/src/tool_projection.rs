use aura_domain_common::{Revision, RuntimeInstanceId, UtcTimestamp};
use aura_projection_contract::{ProjectionChange, ProjectionKey, ProjectionKind, ProjectionScope};
use aura_tool_projection::{
    SessionEffectiveToolRegistryProjection, SessionLoadedToolsProjection,
    SessionToolActivityProjection, SessionToolPinsProjectionV1, ToolBatchProjection,
    ToolCallProjection, ToolConformanceRunProjection, ToolDefinitionProjection,
    ToolDraftListProjection, ToolDraftProjection, ToolExecutionAttemptProjection,
    ToolLongOperationProjectionV1, ToolPackageListProjection, ToolPackageProjection,
    ToolSettingsProjection, WorkspaceToolPinsProjection, WorkspaceToolRegistrySummaryProjection,
};
use rusqlite::{Transaction, params};
use serde::{Deserialize, Serialize};

use crate::{
    PersistenceError,
    coordinator::{next_projection_revision, to_sql_u64},
    model_projection::projection_kind_name,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "projection_kind", content = "payload", rename_all = "snake_case")]
pub enum ToolProjectionDraft {
    ToolSettings(ToolSettingsProjection),
    ToolPackageList(ToolPackageListProjection),
    ToolDraftList(ToolDraftListProjection),
    WorkspaceToolPins(WorkspaceToolPinsProjection),
    WorkspaceToolRegistrySummary(WorkspaceToolRegistrySummaryProjection),
    SessionEffectiveToolRegistry(SessionEffectiveToolRegistryProjection),
    SessionLoadedTools(SessionLoadedToolsProjection),
    SessionToolActivity(SessionToolActivityProjection),
    SessionToolPins(SessionToolPinsProjectionV1),
    ToolPackage(Box<ToolPackageProjection>),
    ToolDefinition(Box<ToolDefinitionProjection>),
    ToolDraft(Box<ToolDraftProjection>),
    ToolConformanceRun(Box<ToolConformanceRunProjection>),
    ToolBatch(Box<ToolBatchProjection>),
    ToolCall(Box<ToolCallProjection>),
    ToolExecutionAttempt(Box<ToolExecutionAttemptProjection>),
    ToolLongOperation(Box<ToolLongOperationProjectionV1>),
}

pub(crate) fn insert_tool_projections(
    transaction: &Transaction<'_>,
    runtime_instance_id: RuntimeInstanceId,
    drafts: Vec<ToolProjectionDraft>,
) -> Result<(Vec<ProjectionChange>, Vec<i64>), PersistenceError> {
    let mut changes = Vec::with_capacity(drafts.len());
    let mut outbox_ids = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let (kind, scope) = draft.route();
        let key =
            ProjectionKey { runtime_instance_id, projection_kind: kind, projection_scope: scope };
        key.validate().map_err(|error| {
            PersistenceError::InvalidRequest(format!("tool projection route is invalid: {error}"))
        })?;
        let kind_name = projection_kind_name(kind);
        let key_json = serde_json::to_string(&key)?;
        let revision = Revision::new(next_projection_revision(transaction, kind_name, &key_json)?);
        let change = draft.materialize(runtime_instance_id, scope, revision);
        change.validate().map_err(|error| {
            PersistenceError::InvalidRequest(format!("tool projection payload is invalid: {error}"))
        })?;
        transaction.execute(
            "INSERT INTO projection_outbox(
                projection_kind, projection_key, revision, payload_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                kind_name,
                key_json,
                to_sql_u64(revision.value())?,
                serde_json::to_string(&change)?,
                UtcTimestamp::now().to_string(),
            ],
        )?;
        outbox_ids.push(transaction.last_insert_rowid());
        changes.push(change);
    }
    Ok((changes, outbox_ids))
}

impl crate::PersistenceCoordinator {
    /// Rebuilds the global package list projection from SQLite authority. Core
    /// calls this once for each Runtime instance so an empty database still
    /// has an explicit committed empty snapshot rather than an indefinite
    /// loading state. The P07 Draft list is retired with the Draft commands.
    pub fn commit_current_tool_global_list_projections(
        &self,
    ) -> Result<Vec<ProjectionChange>, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let packages = crate::tool_package_reads::load_tool_package_summaries(&transaction)?;
            let (changes, _) = insert_tool_projections(
                &transaction,
                runtime_instance_id,
                vec![ToolProjectionDraft::ToolPackageList(ToolPackageListProjection { packages })],
            )?;
            transaction.commit()?;
            Ok(changes)
        })
    }

    pub fn commit_tool_projection_snapshots(
        &self,
        drafts: Vec<ToolProjectionDraft>,
    ) -> Result<Vec<ProjectionChange>, PersistenceError> {
        if drafts.is_empty() {
            return Ok(Vec::new());
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (changes, _) = insert_tool_projections(&transaction, runtime_instance_id, drafts)?;
            transaction.commit()?;
            Ok(changes)
        })
    }

    pub fn latest_tool_projection(
        &self,
        kind: ProjectionKind,
        scope: ProjectionScope,
    ) -> Result<Option<ProjectionChange>, PersistenceError> {
        let key = ProjectionKey {
            runtime_instance_id: self.runtime_instance_id(),
            projection_kind: kind,
            projection_scope: scope,
        };
        key.validate().map_err(|error| {
            PersistenceError::InvalidRequest(format!("tool projection route is invalid: {error}"))
        })?;
        let kind_name = projection_kind_name(kind);
        let key_json = serde_json::to_string(&key)?;
        self.with_reader(|connection| {
            crate::projection_latest::latest_payload(connection, kind_name, &key_json)?
                .map(|encoded| {
                    let change = serde_json::from_str::<ProjectionChange>(&encoded)?;
                    change.validate().map_err(|error| {
                        PersistenceError::InvalidRequest(format!(
                            "persisted tool projection is invalid: {error}"
                        ))
                    })?;
                    Ok(change)
                })
                .transpose()
        })
    }
}

impl ToolProjectionDraft {
    fn route(&self) -> (ProjectionKind, ProjectionScope) {
        match self {
            Self::ToolSettings(_) => (ProjectionKind::ToolSettings, ProjectionScope::Global),
            Self::ToolPackageList(_) => (ProjectionKind::ToolPackageList, ProjectionScope::Global),
            Self::ToolDraftList(_) => (ProjectionKind::ToolDraftList, ProjectionScope::Global),
            Self::WorkspaceToolPins(payload) => (
                ProjectionKind::WorkspaceToolPins,
                ProjectionScope::Workspace { workspace_id: payload.workspace_id },
            ),
            Self::WorkspaceToolRegistrySummary(payload) => (
                ProjectionKind::WorkspaceToolRegistrySummary,
                ProjectionScope::Workspace { workspace_id: payload.workspace_id },
            ),
            Self::SessionEffectiveToolRegistry(payload) => (
                ProjectionKind::SessionEffectiveToolRegistry,
                ProjectionScope::Session { session_id: payload.session_id },
            ),
            Self::SessionLoadedTools(payload) => (
                ProjectionKind::SessionLoadedTools,
                ProjectionScope::Session { session_id: payload.session_id },
            ),
            Self::SessionToolActivity(payload) => (
                ProjectionKind::SessionToolActivity,
                ProjectionScope::Session { session_id: payload.session_id },
            ),
            Self::SessionToolPins(payload) => (
                ProjectionKind::SessionToolPins,
                ProjectionScope::Session { session_id: payload.session_id },
            ),
            Self::ToolPackage(payload) => (
                ProjectionKind::ToolPackage,
                ProjectionScope::ToolPackage { tool_package_id: payload.tool_package_id },
            ),
            Self::ToolDefinition(payload) => (
                ProjectionKind::ToolDefinition,
                ProjectionScope::ToolDefinition { tool_id: payload.tool_id },
            ),
            Self::ToolDraft(payload) => (
                ProjectionKind::ToolDraft,
                ProjectionScope::ToolDraft { tool_draft_id: payload.tool_draft_id },
            ),
            Self::ToolConformanceRun(payload) => (
                ProjectionKind::ToolConformanceRun,
                ProjectionScope::ToolConformanceRun {
                    tool_conformance_run_id: payload.tool_conformance_run_id,
                },
            ),
            Self::ToolBatch(payload) => (
                ProjectionKind::ToolBatch,
                ProjectionScope::ToolBatch { tool_batch_id: payload.tool_batch_id },
            ),
            Self::ToolCall(payload) => (
                ProjectionKind::ToolCall,
                ProjectionScope::ToolCall { tool_call_id: payload.tool_call_id },
            ),
            Self::ToolExecutionAttempt(payload) => (
                ProjectionKind::ToolExecutionAttempt,
                ProjectionScope::ToolExecutionAttempt {
                    tool_execution_attempt_id: payload.tool_execution_attempt_id,
                },
            ),
            Self::ToolLongOperation(payload) => (
                ProjectionKind::ToolLongOperation,
                ProjectionScope::ToolLongOperation { journal_id: payload.operation.journal_id() },
            ),
        }
    }

    fn materialize(
        self,
        runtime_instance_id: RuntimeInstanceId,
        projection_scope: ProjectionScope,
        revision: Revision,
    ) -> ProjectionChange {
        macro_rules! change {
            ($variant:ident, $payload:expr) => {
                ProjectionChange::$variant {
                    runtime_instance_id,
                    projection_scope,
                    revision,
                    payload: $payload,
                }
            };
        }
        match self {
            Self::ToolSettings(payload) => change!(ToolSettings, Box::new(payload)),
            Self::ToolPackageList(payload) => change!(ToolPackageList, Box::new(payload)),
            Self::ToolDraftList(payload) => change!(ToolDraftList, Box::new(payload)),
            Self::WorkspaceToolPins(mut payload) => {
                payload.pin_set_revision = revision;
                change!(WorkspaceToolPins, Box::new(payload))
            }
            Self::WorkspaceToolRegistrySummary(payload) => {
                change!(WorkspaceToolRegistrySummary, Box::new(payload))
            }
            Self::SessionEffectiveToolRegistry(payload) => {
                change!(SessionEffectiveToolRegistry, Box::new(payload))
            }
            Self::SessionLoadedTools(payload) => change!(SessionLoadedTools, Box::new(payload)),
            Self::SessionToolActivity(payload) => change!(SessionToolActivity, Box::new(payload)),
            Self::SessionToolPins(mut payload) => {
                payload.pin_set_revision = revision;
                change!(SessionToolPins, Box::new(payload))
            }
            Self::ToolPackage(payload) => change!(ToolPackage, payload),
            Self::ToolDefinition(payload) => change!(ToolDefinition, payload),
            Self::ToolDraft(payload) => change!(ToolDraft, payload),
            Self::ToolConformanceRun(payload) => change!(ToolConformanceRun, payload),
            Self::ToolBatch(payload) => change!(ToolBatch, payload),
            Self::ToolCall(payload) => change!(ToolCall, payload),
            Self::ToolExecutionAttempt(payload) => change!(ToolExecutionAttempt, payload),
            Self::ToolLongOperation(payload) => change!(ToolLongOperation, payload),
        }
    }
}
