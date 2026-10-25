use aura_context_domain::PromptInspectorProjection;
use aura_domain_common::{Revision, RuntimeInstanceId, UtcTimestamp};
use aura_model_domain::{
    ContextWindowCorrectionProjection, EndpointDetailProjection, ModelAvailabilityTestProjection,
    ModelDiscoveryProjection, ModelProfileProjection, ModelSettingsProjection,
    ProviderCatalogProjection, SessionModelSelectorProjection,
};
use aura_projection_contract::{ProjectionChange, ProjectionKey, ProjectionKind, ProjectionScope};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::{
    PersistenceError,
    coordinator::{next_projection_revision, to_sql_u64},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "projection_kind", content = "payload", rename_all = "snake_case")]
pub enum ModelProjectionDraft {
    ModelSettings(ModelSettingsProjection),
    ProviderCatalog(ProviderCatalogProjection),
    EndpointDetail(Box<EndpointDetailProjection>),
    ModelProfile(Box<ModelProfileProjection>),
    SessionModelSelector(SessionModelSelectorProjection),
    ModelAvailabilityTest(Box<ModelAvailabilityTestProjection>),
    ModelDiscovery(Box<ModelDiscoveryProjection>),
    ContextWindowCorrection(ContextWindowCorrectionProjection),
    PromptInspector(Box<PromptInspectorProjection>),
}

pub(crate) fn insert_model_projections(
    transaction: &Transaction<'_>,
    runtime_instance_id: RuntimeInstanceId,
    drafts: Vec<ModelProjectionDraft>,
) -> Result<(Vec<ProjectionChange>, Vec<i64>), PersistenceError> {
    let mut changes = Vec::with_capacity(drafts.len());
    let mut outbox_ids = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let (kind, scope) = draft.route();
        let key =
            ProjectionKey { runtime_instance_id, projection_kind: kind, projection_scope: scope };
        key.validate().map_err(|error| {
            PersistenceError::InvalidRequest(format!("model projection route is invalid: {error}"))
        })?;
        let kind_name = projection_kind_name(kind);
        let key_json = serde_json::to_string(&key)?;
        let revision = Revision::new(next_projection_revision(transaction, kind_name, &key_json)?);
        let change = draft.materialize(runtime_instance_id, scope, revision);
        change.validate().map_err(|error| {
            PersistenceError::InvalidRequest(format!(
                "model projection payload is invalid: {error}"
            ))
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
    pub fn commit_model_projection_snapshots(
        &self,
        drafts: Vec<ModelProjectionDraft>,
    ) -> Result<Vec<ProjectionChange>, PersistenceError> {
        if drafts.is_empty() {
            return Ok(Vec::new());
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (changes, _) = insert_model_projections(&transaction, runtime_instance_id, drafts)?;
            transaction.commit()?;
            Ok(changes)
        })
    }

    pub fn latest_model_projection(
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
            PersistenceError::InvalidRequest(format!("model projection route is invalid: {error}"))
        })?;
        let kind_name = projection_kind_name(kind);
        let key_json = serde_json::to_string(&key)?;
        self.with_reader(|connection| {
            let encoded = connection
                .query_row(
                    "SELECT payload_json FROM projection_outbox
                     WHERE projection_kind=?1 AND projection_key=?2
                     ORDER BY revision DESC, outbox_id DESC LIMIT 1",
                    params![kind_name, key_json],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            encoded
                .map(|encoded| {
                    let change = serde_json::from_str::<ProjectionChange>(&encoded)?;
                    change.validate().map_err(|error| {
                        PersistenceError::InvalidRequest(format!(
                            "persisted model projection is invalid: {error}"
                        ))
                    })?;
                    Ok(change)
                })
                .transpose()
        })
    }
}

impl ModelProjectionDraft {
    fn route(&self) -> (ProjectionKind, ProjectionScope) {
        match self {
            Self::ModelSettings(_) => (ProjectionKind::ModelSettings, ProjectionScope::Global),
            Self::ProviderCatalog(_) => (ProjectionKind::ProviderCatalog, ProjectionScope::Global),
            Self::EndpointDetail(payload) => (
                ProjectionKind::EndpointDetail,
                ProjectionScope::Endpoint { endpoint_id: payload.endpoint_id },
            ),
            Self::ModelProfile(payload) => (
                ProjectionKind::ModelProfile,
                ProjectionScope::ModelProfile { model_profile_id: payload.model_profile_id },
            ),
            Self::SessionModelSelector(payload) => (
                ProjectionKind::SessionModelSelector,
                ProjectionScope::Session { session_id: payload.session_id },
            ),
            Self::ModelAvailabilityTest(payload) => (
                ProjectionKind::ModelAvailabilityTest,
                ProjectionScope::ModelAvailabilityTest {
                    model_availability_test_id: payload.model_availability_test_id,
                },
            ),
            Self::ModelDiscovery(payload) => (
                ProjectionKind::ModelDiscovery,
                ProjectionScope::ModelDiscovery {
                    model_discovery_run_id: payload.model_discovery_run_id,
                },
            ),
            Self::ContextWindowCorrection(payload) => (
                ProjectionKind::ContextWindowCorrection,
                ProjectionScope::Session { session_id: payload.session_id },
            ),
            Self::PromptInspector(payload) => (
                ProjectionKind::PromptInspector,
                ProjectionScope::Session { session_id: payload.session_id },
            ),
        }
    }

    fn materialize(
        self,
        runtime_instance_id: RuntimeInstanceId,
        projection_scope: ProjectionScope,
        revision: Revision,
    ) -> ProjectionChange {
        match self {
            Self::ModelSettings(payload) => ProjectionChange::ModelSettings {
                runtime_instance_id,
                projection_scope,
                revision,
                payload: Box::new(payload),
            },
            Self::ProviderCatalog(mut payload) => {
                payload.projection_revision = revision;
                ProjectionChange::ProviderCatalog {
                    runtime_instance_id,
                    projection_scope,
                    revision,
                    payload: Box::new(payload),
                }
            }
            Self::EndpointDetail(payload) => ProjectionChange::EndpointDetail {
                runtime_instance_id,
                projection_scope,
                revision,
                payload,
            },
            Self::ModelProfile(payload) => ProjectionChange::ModelProfile {
                runtime_instance_id,
                projection_scope,
                revision,
                payload,
            },
            Self::SessionModelSelector(mut payload) => {
                payload.projection_revision = revision;
                ProjectionChange::SessionModelSelector {
                    runtime_instance_id,
                    projection_scope,
                    revision,
                    payload: Box::new(payload),
                }
            }
            Self::ModelAvailabilityTest(payload) => ProjectionChange::ModelAvailabilityTest {
                runtime_instance_id,
                projection_scope,
                revision,
                payload,
            },
            Self::ModelDiscovery(payload) => ProjectionChange::ModelDiscovery {
                runtime_instance_id,
                projection_scope,
                revision,
                payload,
            },
            Self::ContextWindowCorrection(payload) => ProjectionChange::ContextWindowCorrection {
                runtime_instance_id,
                projection_scope,
                revision,
                payload: Box::new(payload),
            },
            Self::PromptInspector(mut payload) => {
                payload.revision = revision;
                ProjectionChange::PromptInspector {
                    runtime_instance_id,
                    projection_scope,
                    revision,
                    payload,
                }
            }
        }
    }
}

pub(crate) const fn projection_kind_name(kind: ProjectionKind) -> &'static str {
    match kind {
        ProjectionKind::CoreHealth => "core_health",
        ProjectionKind::WorkspaceSession => "workspace_session",
        ProjectionKind::WorkspaceFiles => "workspace_files",
        ProjectionKind::ModelSettings => "model_settings",
        ProjectionKind::ProviderCatalog => "provider_catalog",
        ProjectionKind::EndpointDetail => "endpoint_detail",
        ProjectionKind::ModelProfile => "model_profile",
        ProjectionKind::SessionModelSelector => "session_model_selector",
        ProjectionKind::ModelAvailabilityTest => "model_availability_test",
        ProjectionKind::ModelDiscovery => "model_discovery",
        ProjectionKind::ContextWindowCorrection => "context_window_correction",
        ProjectionKind::PromptInspector => "prompt_inspector",
        ProjectionKind::ToolSettings => "tool_settings",
        ProjectionKind::ToolPackageList => "tool_package_list",
        ProjectionKind::ToolDraftList => "tool_draft_list",
        ProjectionKind::WorkspaceToolPins => "workspace_tool_pins",
        ProjectionKind::WorkspaceToolRegistrySummary => "workspace_tool_registry_summary",
        ProjectionKind::SessionEffectiveToolRegistry => "session_effective_tool_registry",
        ProjectionKind::SessionLoadedTools => "session_loaded_tools",
        ProjectionKind::SessionToolActivity => "session_tool_activity",
        ProjectionKind::SessionToolPins => "session_tool_pins",
        ProjectionKind::ToolPackage => "tool_package",
        ProjectionKind::ToolDefinition => "tool_definition",
        ProjectionKind::ToolDraft => "tool_draft",
        ProjectionKind::ToolConformanceRun => "tool_conformance_run",
        ProjectionKind::ToolBatch => "tool_batch",
        ProjectionKind::ToolCall => "tool_call",
        ProjectionKind::ToolExecutionAttempt => "tool_execution_attempt",
        ProjectionKind::ToolLongOperation => "tool_long_operation",
    }
}
