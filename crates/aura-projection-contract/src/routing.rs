use aura_domain_common::{
    EndpointId, ModelAvailabilityTestId, ModelDiscoveryRunId, ModelProfileId, MutationJournalId,
    RuntimeInstanceId, SessionId, WorkspaceId,
};
use aura_domain_common::{
    ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId, ToolExecutionAttemptId, ToolId,
    ToolPackageId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionScope {
    Global,
    Workspace { workspace_id: WorkspaceId },
    Session { session_id: SessionId },
    Endpoint { endpoint_id: EndpointId },
    ModelProfile { model_profile_id: ModelProfileId },
    ModelAvailabilityTest { model_availability_test_id: ModelAvailabilityTestId },
    ModelDiscovery { model_discovery_run_id: ModelDiscoveryRunId },
    ToolPackage { tool_package_id: ToolPackageId },
    ToolDefinition { tool_id: ToolId },
    ToolDraft { tool_draft_id: ToolDraftId },
    ToolConformanceRun { tool_conformance_run_id: ToolConformanceRunId },
    ToolBatch { tool_batch_id: ToolBatchId },
    ToolCall { tool_call_id: ToolCallId },
    ToolExecutionAttempt { tool_execution_attempt_id: ToolExecutionAttemptId },
    ToolLongOperation { journal_id: MutationJournalId },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionKind {
    CoreHealth,
    WorkspaceSession,
    WorkspaceFiles,
    ModelSettings,
    ProviderCatalog,
    EndpointDetail,
    ModelProfile,
    SessionModelSelector,
    ModelAvailabilityTest,
    ModelDiscovery,
    ContextWindowCorrection,
    PromptInspector,
    ToolSettings,
    ToolPackageList,
    ToolDraftList,
    WorkspaceToolPins,
    WorkspaceToolRegistrySummary,
    SessionEffectiveToolRegistry,
    SessionLoadedTools,
    SessionToolActivity,
    SessionToolPins,
    ToolPackage,
    ToolDefinition,
    ToolDraft,
    ToolConformanceRun,
    ToolBatch,
    ToolCall,
    ToolExecutionAttempt,
    ToolLongOperation,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("projection kind {projection_kind:?} does not accept scope {projection_scope:?}")]
pub struct ProjectionRouteError {
    pub projection_kind: ProjectionKind,
    pub projection_scope: ProjectionScope,
}

impl ProjectionKind {
    #[must_use]
    pub const fn accepts_scope(self, projection_scope: ProjectionScope) -> bool {
        matches!(
            (self, projection_scope),
            (
                Self::CoreHealth
                    | Self::WorkspaceSession
                    | Self::WorkspaceFiles
                    | Self::ModelSettings
                    | Self::ProviderCatalog
                    | Self::ToolSettings
                    | Self::ToolPackageList
                    | Self::ToolDraftList,
                ProjectionScope::Global,
            ) | (Self::EndpointDetail, ProjectionScope::Endpoint { .. })
                | (Self::ModelProfile, ProjectionScope::ModelProfile { .. })
                | (Self::SessionModelSelector, ProjectionScope::Session { .. })
                | (Self::ModelAvailabilityTest, ProjectionScope::ModelAvailabilityTest { .. },)
                | (Self::ModelDiscovery, ProjectionScope::ModelDiscovery { .. })
                | (Self::ContextWindowCorrection, ProjectionScope::Session { .. })
                | (Self::PromptInspector, ProjectionScope::Session { .. })
                | (
                    Self::WorkspaceToolPins | Self::WorkspaceToolRegistrySummary,
                    ProjectionScope::Workspace { .. },
                )
                | (
                    Self::SessionEffectiveToolRegistry
                        | Self::SessionLoadedTools
                        | Self::SessionToolActivity
                        | Self::SessionToolPins,
                    ProjectionScope::Session { .. },
                )
                | (Self::ToolPackage, ProjectionScope::ToolPackage { .. })
                | (Self::ToolDefinition, ProjectionScope::ToolDefinition { .. })
                | (Self::ToolDraft, ProjectionScope::ToolDraft { .. })
                | (Self::ToolConformanceRun, ProjectionScope::ToolConformanceRun { .. })
                | (Self::ToolBatch, ProjectionScope::ToolBatch { .. })
                | (Self::ToolCall, ProjectionScope::ToolCall { .. })
                | (Self::ToolExecutionAttempt, ProjectionScope::ToolExecutionAttempt { .. })
                | (Self::ToolLongOperation, ProjectionScope::ToolLongOperation { .. })
        )
    }

    pub const fn validate_scope(
        self,
        projection_scope: ProjectionScope,
    ) -> Result<(), ProjectionRouteError> {
        if self.accepts_scope(projection_scope) {
            Ok(())
        } else {
            Err(ProjectionRouteError { projection_kind: self, projection_scope })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[schemars(schema_with = "crate::schema::projection_key_schema")]
#[serde(deny_unknown_fields)]
pub struct ProjectionKey {
    pub runtime_instance_id: RuntimeInstanceId,
    pub projection_kind: ProjectionKind,
    pub projection_scope: ProjectionScope,
}

impl ProjectionKey {
    pub const fn validate(self) -> Result<(), ProjectionRouteError> {
        self.projection_kind.validate_scope(self.projection_scope)
    }
}

impl<'de> Deserialize<'de> for ProjectionKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireKey {
            runtime_instance_id: RuntimeInstanceId,
            projection_kind: ProjectionKind,
            projection_scope: ProjectionScope,
        }

        let wire = WireKey::deserialize(deserializer)?;
        let key = Self {
            runtime_instance_id: wire.runtime_instance_id,
            projection_kind: wire.projection_kind,
            projection_scope: wire.projection_scope,
        };
        key.validate().map_err(D::Error::custom)?;
        Ok(key)
    }
}
