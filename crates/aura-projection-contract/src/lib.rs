#![forbid(unsafe_code)]

mod agent_display;
mod change;
mod compatibility;
mod compatibility_keys;
mod readiness;
mod routing;
mod schema;
mod subscription;
pub use compatibility::{PROJECTION_COMPATIBILITY_VERSION, ProjectionCompatibilitySnapshotV1};
pub use compatibility_keys::*;
pub use readiness::*;

pub use agent_display::{
    AgentAssistantView, AgentContextClearedView, AgentContextCompactedView,
    AgentContextCompactingView, AgentSessionProjection, AgentToolInputView, AgentTurnView,
};
pub use change::{ProjectionChange, ProjectionChangeValidationError};
pub use routing::{ProjectionKey, ProjectionKind, ProjectionRouteError, ProjectionScope};
pub use subscription::{
    ProjectionSnapshotRequest, ProjectionSubscription, ProjectionUnsubscribeRequest,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Schema-only aggregation root owned by the projection contract crate.
///
/// Runtime messages use the individual contracts below; this type prevents
/// the repository-wide schema generator from recreating their ownership.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionContractSchemaRoot {
    pub projection_key: ProjectionKey,
    pub projection_subscription: ProjectionSubscription,
    pub projection_snapshot_request: ProjectionSnapshotRequest,
    pub projection_unsubscribe_request: ProjectionUnsubscribeRequest,
    pub projection_change: ProjectionChange,
    pub compatibility_snapshot: ProjectionCompatibilitySnapshotV1,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{
        EndpointId, ModelAvailabilityTestId, ModelProfileId, MutationJournalId, Revision,
        RuntimeInstanceId, SessionId, WorkspaceId,
    };
    use aura_domain_common::{
        ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId, ToolExecutionAttemptId, ToolId,
        ToolPackageId,
    };
    use aura_execution_contract::SideEffectStateV1;
    use aura_model_domain::ContextWindowCorrectionProjection;
    use aura_runtime_contract::{CoreHealthProjection, CoreLifecycleState};
    use aura_tool_contract::ToolExecutionAttemptStatusV1;
    use aura_tool_projection::ToolExecutionAttemptProjection;
    use serde_json::json;

    use super::{ProjectionChange, ProjectionKind, ProjectionScope, ProjectionSubscription};

    #[test]
    fn frozen_kind_scope_matrix_accepts_only_registered_routes() {
        let endpoint_id = EndpointId::new();
        let model_profile_id = ModelProfileId::new();
        let session_id = SessionId::new();
        let model_availability_test_id = ModelAvailabilityTestId::new();
        let workspace_id = WorkspaceId::new();
        let tool_package_id = ToolPackageId::new();
        let tool_id = ToolId::new();
        let tool_draft_id = ToolDraftId::new();
        let tool_conformance_run_id = ToolConformanceRunId::new();
        let tool_batch_id = ToolBatchId::new();
        let tool_call_id = ToolCallId::new();
        let tool_execution_attempt_id = ToolExecutionAttemptId::new();
        let journal_id = MutationJournalId::new();
        let valid = [
            (ProjectionKind::CoreHealth, ProjectionScope::Global),
            (ProjectionKind::WorkspaceSession, ProjectionScope::Global),
            (ProjectionKind::WorkspaceFiles, ProjectionScope::Global),
            (ProjectionKind::ModelSettings, ProjectionScope::Global),
            (ProjectionKind::ProviderCatalog, ProjectionScope::Global),
            (ProjectionKind::EndpointDetail, ProjectionScope::Endpoint { endpoint_id }),
            (ProjectionKind::ModelProfile, ProjectionScope::ModelProfile { model_profile_id }),
            (ProjectionKind::SessionModelSelector, ProjectionScope::Session { session_id }),
            (
                ProjectionKind::ModelAvailabilityTest,
                ProjectionScope::ModelAvailabilityTest { model_availability_test_id },
            ),
            (ProjectionKind::ContextWindowCorrection, ProjectionScope::Session { session_id }),
            (ProjectionKind::PromptInspector, ProjectionScope::Session { session_id }),
            (ProjectionKind::ToolSettings, ProjectionScope::Global),
            (ProjectionKind::ToolPackageList, ProjectionScope::Global),
            (ProjectionKind::ToolDraftList, ProjectionScope::Global),
            (ProjectionKind::WorkspaceToolPins, ProjectionScope::Workspace { workspace_id }),
            (
                ProjectionKind::WorkspaceToolRegistrySummary,
                ProjectionScope::Workspace { workspace_id },
            ),
            (ProjectionKind::SessionEffectiveToolRegistry, ProjectionScope::Session { session_id }),
            (ProjectionKind::SessionLoadedTools, ProjectionScope::Session { session_id }),
            (ProjectionKind::SessionToolActivity, ProjectionScope::Session { session_id }),
            (ProjectionKind::SessionToolPins, ProjectionScope::Session { session_id }),
            (ProjectionKind::ToolPackage, ProjectionScope::ToolPackage { tool_package_id }),
            (ProjectionKind::ToolDefinition, ProjectionScope::ToolDefinition { tool_id }),
            (ProjectionKind::ToolDraft, ProjectionScope::ToolDraft { tool_draft_id }),
            (
                ProjectionKind::ToolConformanceRun,
                ProjectionScope::ToolConformanceRun { tool_conformance_run_id },
            ),
            (ProjectionKind::ToolBatch, ProjectionScope::ToolBatch { tool_batch_id }),
            (ProjectionKind::ToolCall, ProjectionScope::ToolCall { tool_call_id }),
            (
                ProjectionKind::ToolExecutionAttempt,
                ProjectionScope::ToolExecutionAttempt { tool_execution_attempt_id },
            ),
            (ProjectionKind::ToolLongOperation, ProjectionScope::ToolLongOperation { journal_id }),
        ];
        for (kind, scope) in valid {
            kind.validate_scope(scope).expect("registered route");
        }

        assert!(ProjectionKind::EndpointDetail.validate_scope(ProjectionScope::Global).is_err());
        assert!(
            ProjectionKind::ModelSettings
                .validate_scope(ProjectionScope::Workspace { workspace_id: WorkspaceId::new() })
                .is_err()
        );
    }

    #[test]
    fn subscription_decoder_rejects_invalid_kind_scope_pairs() {
        assert!(
            serde_json::from_value::<ProjectionSubscription>(json!({
                "projection_kind": "endpoint_detail",
                "projection_scope": {"type": "global"},
                "known_runtime_instance_id": null,
                "after_revision": null
            }))
            .is_err()
        );
    }

    #[test]
    fn projection_change_enforces_route_and_payload_identity() {
        let runtime_instance_id = RuntimeInstanceId::new();
        let global = ProjectionChange::CoreHealth {
            runtime_instance_id,
            projection_scope: ProjectionScope::Global,
            revision: Revision::INITIAL,
            payload: CoreHealthProjection {
                state: CoreLifecycleState::Ready,
                read_only: false,
                safe_mode: false,
                restart_attempt: 0,
                status_code: "core.ready".to_owned(),
                status_message: "ready".to_owned(),
                busy: None,
                waiting: None,
            },
        };
        let wire = serde_json::to_value(&global).expect("projection wire");
        assert_eq!(wire["projection_scope"], json!({"type": "global"}));
        serde_json::from_value::<ProjectionChange>(wire).expect("valid projection change");

        let mismatched = ProjectionChange::ContextWindowCorrection {
            runtime_instance_id,
            projection_scope: ProjectionScope::Session { session_id: SessionId::new() },
            revision: Revision::INITIAL,
            payload: Box::new(ContextWindowCorrectionProjection {
                session_id: SessionId::new(),
                active_corrections: Vec::new(),
            }),
        };
        assert!(mismatched.validate().is_err());

        let tool_call_id = ToolCallId::new();
        let tool_execution_attempt_id = ToolExecutionAttemptId::new();
        let tool_attempt = ProjectionChange::ToolExecutionAttempt {
            runtime_instance_id,
            projection_scope: ProjectionScope::ToolExecutionAttempt { tool_execution_attempt_id },
            revision: Revision::INITIAL,
            payload: Box::new(ToolExecutionAttemptProjection {
                tool_execution_attempt_id,
                tool_call_id,
                attempt_number: 1,
                status: ToolExecutionAttemptStatusV1::Started,
                side_effect_state: SideEffectStateV1::NotStarted,
                progress: None,
                started_at: aura_domain_common::UtcTimestamp::now(),
                dispatch_prepared_at: None,
                completed_at: None,
                safe_error: None,
                user_readable_artifacts: Vec::new(),
                can_cancel: true,
            }),
        };
        tool_attempt.validate().expect("tool attempt projection route");
        let wire = serde_json::to_value(tool_attempt).expect("tool attempt wire");
        serde_json::from_value::<ProjectionChange>(wire).expect("tool attempt roundtrip");
    }
}
