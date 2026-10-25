use aura_context_domain::PromptInspectorProjection;
use aura_domain_common::{Revision, RuntimeInstanceId};
use aura_model_domain::{
    ContextWindowCorrectionProjection, EndpointDetailProjection, ModelAvailabilityTestProjection,
    ModelDiscoveryProjection, ModelProfileProjection, ModelSettingsProjection,
    ProviderCatalogProjection, SessionModelSelectorProjection,
};
use aura_runtime_contract::CoreHealthProjection;
use aura_tool_projection::{
    SessionEffectiveToolRegistryProjection, SessionLoadedToolsProjection,
    SessionToolActivityProjection, SessionToolPinsProjectionV1, ToolBatchProjection,
    ToolCallProjection, ToolConformanceRunProjection, ToolDefinitionProjection,
    ToolDraftListProjection, ToolDraftProjection, ToolExecutionAttemptProjection,
    ToolLongOperationProjectionV1, ToolPackageListProjection, ToolPackageProjection,
    ToolSettingsProjection, WorkspaceToolPinsProjection, WorkspaceToolRegistrySummaryProjection,
};
use aura_workspace_domain::{WorkspaceFilesProjection, WorkspaceSessionProjection};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

use crate::{ProjectionKey, ProjectionKind, ProjectionRouteError, ProjectionScope};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "projection_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionChange {
    CoreHealth {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: CoreHealthProjection,
    },
    WorkspaceSession {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: WorkspaceSessionProjection,
    },
    WorkspaceFiles {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: WorkspaceFilesProjection,
    },
    ModelSettings {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ModelSettingsProjection>,
    },
    ProviderCatalog {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ProviderCatalogProjection>,
    },
    EndpointDetail {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_endpoint_scope")]
        #[schemars(schema_with = "crate::schema::endpoint_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<EndpointDetailProjection>,
    },
    ModelProfile {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_model_profile_scope")]
        #[schemars(schema_with = "crate::schema::model_profile_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ModelProfileProjection>,
    },
    SessionModelSelector {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_session_scope")]
        #[schemars(schema_with = "crate::schema::session_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<SessionModelSelectorProjection>,
    },
    ModelAvailabilityTest {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_model_availability_test_scope")]
        #[schemars(schema_with = "crate::schema::model_availability_test_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ModelAvailabilityTestProjection>,
    },
    ModelDiscovery {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_model_discovery_scope")]
        #[schemars(schema_with = "crate::schema::model_discovery_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ModelDiscoveryProjection>,
    },
    ContextWindowCorrection {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_session_scope")]
        #[schemars(schema_with = "crate::schema::session_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ContextWindowCorrectionProjection>,
    },
    PromptInspector {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_session_scope")]
        #[schemars(schema_with = "crate::schema::session_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<PromptInspectorProjection>,
    },
    ToolSettings {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolSettingsProjection>,
    },
    ToolPackageList {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolPackageListProjection>,
    },
    ToolDraftList {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_global_scope")]
        #[schemars(schema_with = "crate::schema::global_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolDraftListProjection>,
    },
    WorkspaceToolPins {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_workspace_scope")]
        #[schemars(schema_with = "crate::schema::workspace_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<WorkspaceToolPinsProjection>,
    },
    WorkspaceToolRegistrySummary {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_workspace_scope")]
        #[schemars(schema_with = "crate::schema::workspace_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<WorkspaceToolRegistrySummaryProjection>,
    },
    SessionEffectiveToolRegistry {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_session_scope")]
        #[schemars(schema_with = "crate::schema::session_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<SessionEffectiveToolRegistryProjection>,
    },
    SessionLoadedTools {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_session_scope")]
        #[schemars(schema_with = "crate::schema::session_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<SessionLoadedToolsProjection>,
    },
    SessionToolActivity {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_session_scope")]
        #[schemars(schema_with = "crate::schema::session_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<SessionToolActivityProjection>,
    },
    SessionToolPins {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_session_scope")]
        #[schemars(schema_with = "crate::schema::session_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<SessionToolPinsProjectionV1>,
    },
    ToolPackage {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_package_scope")]
        #[schemars(schema_with = "crate::schema::tool_package_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolPackageProjection>,
    },
    ToolDefinition {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_definition_scope")]
        #[schemars(schema_with = "crate::schema::tool_definition_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolDefinitionProjection>,
    },
    ToolDraft {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_draft_scope")]
        #[schemars(schema_with = "crate::schema::tool_draft_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolDraftProjection>,
    },
    ToolConformanceRun {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_conformance_run_scope")]
        #[schemars(schema_with = "crate::schema::tool_conformance_run_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolConformanceRunProjection>,
    },
    ToolBatch {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_batch_scope")]
        #[schemars(schema_with = "crate::schema::tool_batch_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolBatchProjection>,
    },
    ToolCall {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_call_scope")]
        #[schemars(schema_with = "crate::schema::tool_call_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolCallProjection>,
    },
    ToolExecutionAttempt {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_execution_attempt_scope")]
        #[schemars(schema_with = "crate::schema::tool_execution_attempt_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolExecutionAttemptProjection>,
    },
    ToolLongOperation {
        runtime_instance_id: RuntimeInstanceId,
        #[serde(deserialize_with = "deserialize_tool_long_operation_scope")]
        #[schemars(schema_with = "crate::schema::tool_long_operation_scope_schema")]
        projection_scope: ProjectionScope,
        revision: Revision,
        payload: Box<ToolLongOperationProjectionV1>,
    },
}

impl ProjectionChange {
    #[must_use]
    pub const fn kind(&self) -> ProjectionKind {
        match self {
            Self::CoreHealth { .. } => ProjectionKind::CoreHealth,
            Self::WorkspaceSession { .. } => ProjectionKind::WorkspaceSession,
            Self::WorkspaceFiles { .. } => ProjectionKind::WorkspaceFiles,
            Self::ModelSettings { .. } => ProjectionKind::ModelSettings,
            Self::ProviderCatalog { .. } => ProjectionKind::ProviderCatalog,
            Self::EndpointDetail { .. } => ProjectionKind::EndpointDetail,
            Self::ModelProfile { .. } => ProjectionKind::ModelProfile,
            Self::SessionModelSelector { .. } => ProjectionKind::SessionModelSelector,
            Self::ModelAvailabilityTest { .. } => ProjectionKind::ModelAvailabilityTest,
            Self::ModelDiscovery { .. } => ProjectionKind::ModelDiscovery,
            Self::ContextWindowCorrection { .. } => ProjectionKind::ContextWindowCorrection,
            Self::PromptInspector { .. } => ProjectionKind::PromptInspector,
            Self::ToolSettings { .. } => ProjectionKind::ToolSettings,
            Self::ToolPackageList { .. } => ProjectionKind::ToolPackageList,
            Self::ToolDraftList { .. } => ProjectionKind::ToolDraftList,
            Self::WorkspaceToolPins { .. } => ProjectionKind::WorkspaceToolPins,
            Self::WorkspaceToolRegistrySummary { .. } => {
                ProjectionKind::WorkspaceToolRegistrySummary
            }
            Self::SessionEffectiveToolRegistry { .. } => {
                ProjectionKind::SessionEffectiveToolRegistry
            }
            Self::SessionLoadedTools { .. } => ProjectionKind::SessionLoadedTools,
            Self::SessionToolActivity { .. } => ProjectionKind::SessionToolActivity,
            Self::SessionToolPins { .. } => ProjectionKind::SessionToolPins,
            Self::ToolPackage { .. } => ProjectionKind::ToolPackage,
            Self::ToolDefinition { .. } => ProjectionKind::ToolDefinition,
            Self::ToolDraft { .. } => ProjectionKind::ToolDraft,
            Self::ToolConformanceRun { .. } => ProjectionKind::ToolConformanceRun,
            Self::ToolBatch { .. } => ProjectionKind::ToolBatch,
            Self::ToolCall { .. } => ProjectionKind::ToolCall,
            Self::ToolExecutionAttempt { .. } => ProjectionKind::ToolExecutionAttempt,
            Self::ToolLongOperation { .. } => ProjectionKind::ToolLongOperation,
        }
    }

    #[must_use]
    pub const fn runtime_instance_id(&self) -> RuntimeInstanceId {
        match self {
            Self::CoreHealth { runtime_instance_id, .. }
            | Self::WorkspaceSession { runtime_instance_id, .. }
            | Self::WorkspaceFiles { runtime_instance_id, .. }
            | Self::ModelSettings { runtime_instance_id, .. }
            | Self::ProviderCatalog { runtime_instance_id, .. }
            | Self::EndpointDetail { runtime_instance_id, .. }
            | Self::ModelProfile { runtime_instance_id, .. }
            | Self::SessionModelSelector { runtime_instance_id, .. }
            | Self::ModelAvailabilityTest { runtime_instance_id, .. }
            | Self::ModelDiscovery { runtime_instance_id, .. }
            | Self::ContextWindowCorrection { runtime_instance_id, .. }
            | Self::PromptInspector { runtime_instance_id, .. }
            | Self::ToolSettings { runtime_instance_id, .. }
            | Self::ToolPackageList { runtime_instance_id, .. }
            | Self::ToolDraftList { runtime_instance_id, .. }
            | Self::WorkspaceToolPins { runtime_instance_id, .. }
            | Self::WorkspaceToolRegistrySummary { runtime_instance_id, .. }
            | Self::SessionEffectiveToolRegistry { runtime_instance_id, .. }
            | Self::SessionLoadedTools { runtime_instance_id, .. }
            | Self::SessionToolActivity { runtime_instance_id, .. }
            | Self::SessionToolPins { runtime_instance_id, .. }
            | Self::ToolPackage { runtime_instance_id, .. }
            | Self::ToolDefinition { runtime_instance_id, .. }
            | Self::ToolDraft { runtime_instance_id, .. }
            | Self::ToolConformanceRun { runtime_instance_id, .. }
            | Self::ToolBatch { runtime_instance_id, .. }
            | Self::ToolCall { runtime_instance_id, .. }
            | Self::ToolExecutionAttempt { runtime_instance_id, .. }
            | Self::ToolLongOperation { runtime_instance_id, .. } => *runtime_instance_id,
        }
    }

    #[must_use]
    pub const fn projection_scope(&self) -> ProjectionScope {
        match self {
            Self::CoreHealth { projection_scope, .. }
            | Self::WorkspaceSession { projection_scope, .. }
            | Self::WorkspaceFiles { projection_scope, .. }
            | Self::ModelSettings { projection_scope, .. }
            | Self::ProviderCatalog { projection_scope, .. }
            | Self::EndpointDetail { projection_scope, .. }
            | Self::ModelProfile { projection_scope, .. }
            | Self::SessionModelSelector { projection_scope, .. }
            | Self::ModelAvailabilityTest { projection_scope, .. }
            | Self::ModelDiscovery { projection_scope, .. }
            | Self::ContextWindowCorrection { projection_scope, .. }
            | Self::PromptInspector { projection_scope, .. }
            | Self::ToolSettings { projection_scope, .. }
            | Self::ToolPackageList { projection_scope, .. }
            | Self::ToolDraftList { projection_scope, .. }
            | Self::WorkspaceToolPins { projection_scope, .. }
            | Self::WorkspaceToolRegistrySummary { projection_scope, .. }
            | Self::SessionEffectiveToolRegistry { projection_scope, .. }
            | Self::SessionLoadedTools { projection_scope, .. }
            | Self::SessionToolActivity { projection_scope, .. }
            | Self::SessionToolPins { projection_scope, .. }
            | Self::ToolPackage { projection_scope, .. }
            | Self::ToolDefinition { projection_scope, .. }
            | Self::ToolDraft { projection_scope, .. }
            | Self::ToolConformanceRun { projection_scope, .. }
            | Self::ToolBatch { projection_scope, .. }
            | Self::ToolCall { projection_scope, .. }
            | Self::ToolExecutionAttempt { projection_scope, .. }
            | Self::ToolLongOperation { projection_scope, .. } => *projection_scope,
        }
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        match self {
            Self::CoreHealth { revision, .. }
            | Self::WorkspaceSession { revision, .. }
            | Self::WorkspaceFiles { revision, .. }
            | Self::ModelSettings { revision, .. }
            | Self::ProviderCatalog { revision, .. }
            | Self::EndpointDetail { revision, .. }
            | Self::ModelProfile { revision, .. }
            | Self::SessionModelSelector { revision, .. }
            | Self::ModelAvailabilityTest { revision, .. }
            | Self::ModelDiscovery { revision, .. }
            | Self::ContextWindowCorrection { revision, .. }
            | Self::PromptInspector { revision, .. }
            | Self::ToolSettings { revision, .. }
            | Self::ToolPackageList { revision, .. }
            | Self::ToolDraftList { revision, .. }
            | Self::WorkspaceToolPins { revision, .. }
            | Self::WorkspaceToolRegistrySummary { revision, .. }
            | Self::SessionEffectiveToolRegistry { revision, .. }
            | Self::SessionLoadedTools { revision, .. }
            | Self::SessionToolActivity { revision, .. }
            | Self::SessionToolPins { revision, .. }
            | Self::ToolPackage { revision, .. }
            | Self::ToolDefinition { revision, .. }
            | Self::ToolDraft { revision, .. }
            | Self::ToolConformanceRun { revision, .. }
            | Self::ToolBatch { revision, .. }
            | Self::ToolCall { revision, .. }
            | Self::ToolExecutionAttempt { revision, .. }
            | Self::ToolLongOperation { revision, .. } => *revision,
        }
    }

    #[must_use]
    pub const fn key(&self) -> ProjectionKey {
        ProjectionKey {
            runtime_instance_id: self.runtime_instance_id(),
            projection_kind: self.kind(),
            projection_scope: self.projection_scope(),
        }
    }

    pub fn validate(&self) -> Result<(), ProjectionChangeValidationError> {
        let kind = self.kind();
        let scope = self.projection_scope();
        if let Err(source) = kind.validate_scope(scope) {
            return Err(ProjectionChangeValidationError::InvalidRoute(source));
        }
        let payload_matches_scope = match self {
            Self::EndpointDetail { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Endpoint { endpoint_id } if *endpoint_id == payload.endpoint_id
            ),
            Self::ModelProfile { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ModelProfile { model_profile_id }
                    if *model_profile_id == payload.model_profile_id
            ),
            Self::SessionModelSelector { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Session { session_id } if *session_id == payload.session_id
            ),
            Self::ModelAvailabilityTest { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ModelAvailabilityTest { model_availability_test_id }
                    if *model_availability_test_id == payload.model_availability_test_id
            ),
            Self::ModelDiscovery { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ModelDiscovery { model_discovery_run_id }
                    if *model_discovery_run_id == payload.model_discovery_run_id
            ),
            Self::ContextWindowCorrection { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Session { session_id } if *session_id == payload.session_id
            ),
            Self::PromptInspector { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Session { session_id } if *session_id == payload.session_id
            ),
            Self::WorkspaceToolPins { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Workspace { workspace_id } if *workspace_id == payload.workspace_id
            ),
            Self::WorkspaceToolRegistrySummary { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Workspace { workspace_id } if *workspace_id == payload.workspace_id
            ),
            Self::SessionEffectiveToolRegistry { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Session { session_id } if *session_id == payload.session_id
            ),
            Self::SessionLoadedTools { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Session { session_id } if *session_id == payload.session_id
            ),
            Self::SessionToolActivity { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Session { session_id } if *session_id == payload.session_id
            ),
            Self::SessionToolPins { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::Session { session_id } if *session_id == payload.session_id
            ),
            Self::ToolPackage { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolPackage { tool_package_id }
                    if *tool_package_id == payload.tool_package_id
            ),
            Self::ToolDefinition { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolDefinition { tool_id }
                    if *tool_id == payload.tool_id
            ),
            Self::ToolDraft { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolDraft { tool_draft_id }
                    if *tool_draft_id == payload.tool_draft_id
            ),
            Self::ToolConformanceRun { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolConformanceRun { tool_conformance_run_id }
                    if *tool_conformance_run_id == payload.tool_conformance_run_id
            ),
            Self::ToolBatch { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolBatch { tool_batch_id }
                    if *tool_batch_id == payload.tool_batch_id
            ),
            Self::ToolCall { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolCall { tool_call_id }
                    if *tool_call_id == payload.tool_call_id
            ),
            Self::ToolExecutionAttempt { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolExecutionAttempt { tool_execution_attempt_id }
                    if *tool_execution_attempt_id == payload.tool_execution_attempt_id
            ),
            Self::ToolLongOperation { projection_scope, payload, .. } => matches!(
                projection_scope,
                ProjectionScope::ToolLongOperation { journal_id }
                    if *journal_id == payload.operation.journal_id()
            ),
            Self::CoreHealth { .. }
            | Self::WorkspaceSession { .. }
            | Self::WorkspaceFiles { .. }
            | Self::ModelSettings { .. }
            | Self::ProviderCatalog { .. }
            | Self::ToolSettings { .. }
            | Self::ToolPackageList { .. }
            | Self::ToolDraftList { .. } => true,
        };
        if payload_matches_scope {
            Ok(())
        } else {
            Err(ProjectionChangeValidationError::PayloadScopeMismatch { kind, scope })
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProjectionChangeValidationError {
    #[error(transparent)]
    InvalidRoute(#[from] ProjectionRouteError),
    #[error("projection payload identity does not match {kind:?} scope {scope:?}")]
    PayloadScopeMismatch { kind: ProjectionKind, scope: ProjectionScope },
}

fn deserialize_global_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| matches!(scope, ProjectionScope::Global))
}

fn deserialize_endpoint_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| matches!(scope, ProjectionScope::Endpoint { .. }))
}

fn deserialize_model_profile_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ModelProfile { .. })
    })
}

fn deserialize_session_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| matches!(scope, ProjectionScope::Session { .. }))
}

fn deserialize_workspace_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| matches!(scope, ProjectionScope::Workspace { .. }))
}

fn deserialize_tool_package_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ToolPackage { .. })
    })
}

fn deserialize_tool_definition_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ToolDefinition { .. })
    })
}

fn deserialize_tool_draft_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| matches!(scope, ProjectionScope::ToolDraft { .. }))
}

fn deserialize_tool_conformance_run_scope<'de, D>(
    deserializer: D,
) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ToolConformanceRun { .. })
    })
}

fn deserialize_tool_batch_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| matches!(scope, ProjectionScope::ToolBatch { .. }))
}

fn deserialize_tool_call_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| matches!(scope, ProjectionScope::ToolCall { .. }))
}

fn deserialize_tool_execution_attempt_scope<'de, D>(
    deserializer: D,
) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ToolExecutionAttempt { .. })
    })
}

fn deserialize_tool_long_operation_scope<'de, D>(
    deserializer: D,
) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ToolLongOperation { .. })
    })
}

fn deserialize_model_availability_test_scope<'de, D>(
    deserializer: D,
) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ModelAvailabilityTest { .. })
    })
}

fn deserialize_model_discovery_scope<'de, D>(deserializer: D) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_scope_for(deserializer, |scope| {
        matches!(scope, ProjectionScope::ModelDiscovery { .. })
    })
}

fn deserialize_scope_for<'de, D>(
    deserializer: D,
    accepts: impl FnOnce(ProjectionScope) -> bool,
) -> Result<ProjectionScope, D::Error>
where
    D: Deserializer<'de>,
{
    let scope = ProjectionScope::deserialize(deserializer)?;
    if accepts(scope) {
        Ok(scope)
    } else {
        Err(D::Error::custom("projection_scope is not valid for projection_kind"))
    }
}
