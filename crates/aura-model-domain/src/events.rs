use std::collections::BTreeSet;

use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, CredentialId, CredentialRevision, EndpointId,
    ModelAvailabilityTestId, ModelDiscoveryRunId, ModelExecutionSnapshotId, ModelFailureId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, Revision, SessionId,
    TransportAttemptId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ChatOutputTokenField, CredentialBindingFingerprint, EndpointCompatibilityOverrides,
    EndpointCredentialStatus, ModelAvailabilityTestResult, ModelAvailabilityTestStage,
    ModelDiscoveryOutcome, ModelDiscoveryUnsupportedReason, ModelEndpointConfig,
    ModelOperationCancellationReason, ModelProfileConfig, ModelProviderConfig, ModelUsage,
    RuntimeWindowCorrectionBasis, SessionModelBinding, TransportAttemptKind,
    TransportOperationContext,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationSource {
    UserTurnTaskStopped,
    RecoveryDecision,
    SessionLifecycle,
    CoreShutdown,
    EndpointConfigurationRevoked,
    UpstreamCancellationToken,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderCreatedPayload {
    pub provider: ModelProviderConfig,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderRenamedPayload {
    pub provider_id: ProviderId,
    pub previous_display_name: String,
    pub display_name: String,
    pub models_revision: Revision,
}

/// The provider's default connection or its backups changed.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderConnectionsChangedPayload {
    pub provider_id: ProviderId,
    pub previous_default_endpoint_id: Option<EndpointId>,
    pub default_endpoint_id: Option<EndpointId>,
    pub previous_backup_endpoint_ids: Vec<EndpointId>,
    pub backup_endpoint_ids: Vec<EndpointId>,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderDeletedPayload {
    pub provider_id: ProviderId,
    pub deleted_endpoint_ids: Vec<EndpointId>,
    pub deleted_model_profile_ids: Vec<ModelProfileId>,
    pub default_model_cleared: bool,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointCreatedPayload {
    pub endpoint: ModelEndpointConfig,
    pub credential_status: Option<EndpointCredentialStatus>,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointUpdatedPayload {
    pub endpoint: ModelEndpointConfig,
    pub credential_status: Option<EndpointCredentialStatus>,
    pub previous_endpoint_fingerprint: ContentHash,
    pub endpoint_fingerprint: ContentHash,
    pub credential_binding_mismatch: bool,
    pub changed_fields: BTreeSet<String>,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointCredentialConfiguredPayload {
    pub endpoint_id: EndpointId,
    pub credential_id: CredentialId,
    pub active_credential_revision: CredentialRevision,
    pub row_revision: Revision,
    pub credential_binding_fingerprint: CredentialBindingFingerprint,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointCredentialClearedPayload {
    pub endpoint_id: EndpointId,
    pub credential_id: CredentialId,
    pub last_committed_credential_revision: CredentialRevision,
    pub row_revision: Revision,
    pub previous_credential_binding_fingerprint: CredentialBindingFingerprint,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointDeletedPayload {
    pub endpoint_id: EndpointId,
    pub provider_id: ProviderId,
    pub deleted_model_profile_ids: Vec<ModelProfileId>,
    pub credential_was_configured: bool,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestCompletedPayload {
    pub model_availability_test_id: ModelAvailabilityTestId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub endpoint_revision_at_start: Revision,
    pub model_profile_revision_at_start: Revision,
    pub models_revision_at_start: Revision,
    pub endpoint_security_fingerprint: ContentHash,
    pub result: ModelAvailabilityTestResult,
    pub test_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestCancelledPayload {
    pub model_availability_test_id: ModelAvailabilityTestId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub stage: Option<ModelAvailabilityTestStage>,
    pub cancelled_by: ModelOperationCancellationReason,
    pub test_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryStartedPayload {
    pub discovery_run_id: ModelDiscoveryRunId,
    pub endpoint_id: EndpointId,
    pub run_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryCompletedPayload {
    pub discovery_run_id: ModelDiscoveryRunId,
    pub endpoint_id: EndpointId,
    pub outcome: ModelDiscoveryOutcome,
    pub returned_count: u32,
    pub created_count: u32,
    pub not_returned_count: u32,
    pub run_revision: Revision,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryFailedPayload {
    pub discovery_run_id: ModelDiscoveryRunId,
    pub endpoint_id: EndpointId,
    pub failure_id: ModelFailureId,
    pub run_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryCancelledPayload {
    pub discovery_run_id: ModelDiscoveryRunId,
    pub endpoint_id: EndpointId,
    pub cancelled_by: ModelOperationCancellationReason,
    pub run_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryUnsupportedPayload {
    pub discovery_run_id: ModelDiscoveryRunId,
    pub endpoint_id: EndpointId,
    pub reason_code: ModelDiscoveryUnsupportedReason,
    pub run_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileCreatedPayload {
    pub provider_id: ProviderId,
    pub endpoint_id: EndpointId,
    pub profile: ModelProfileConfig,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileUpdatedPayload {
    pub provider_id: ProviderId,
    pub endpoint_id: EndpointId,
    pub profile: ModelProfileConfig,
    pub changed_fields: BTreeSet<String>,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileDeletedPayload {
    pub provider_id: ProviderId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub default_model_cleared: bool,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSubagentAvailabilityChangedPayload {
    pub provider_id: ProviderId,
    pub model_profile_id: ModelProfileId,
    pub previous_value: bool,
    pub available_for_subagents: bool,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultModelChangedPayload {
    pub previous_model_profile_id: Option<ModelProfileId>,
    pub model_profile_id: Option<ModelProfileId>,
    pub aura_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelBindingChangedPayload {
    pub session_id: SessionId,
    pub previous_binding: Option<SessionModelBinding>,
    pub binding: Option<SessionModelBinding>,
    pub binding_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelExecutionSnapshotCreatedPayload {
    pub model_execution_snapshot_id: ModelExecutionSnapshotId,
    pub model_step_id: ModelStepId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub context_snapshot_id: ContextProjectionSnapshotId,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTransportAttemptStartedPayload {
    pub transport_attempt_id: TransportAttemptId,
    pub operation_context: TransportOperationContext,
    pub endpoint_id: EndpointId,
    pub attempt_number: u32,
    pub attempt_kind: TransportAttemptKind,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTransportAttemptRetriedPayload {
    pub previous_transport_attempt_id: TransportAttemptId,
    pub transport_attempt_id: TransportAttemptId,
    pub operation_context: TransportOperationContext,
    pub retry_kind: TransportAttemptKind,
    pub previous_failure_id: Option<ModelFailureId>,
    pub compatibility_overrides: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTransportAttemptCompletedPayload {
    pub transport_attempt_id: TransportAttemptId,
    pub operation_context: TransportOperationContext,
    pub provider_request_id: Option<String>,
    pub usage: Option<ModelUsage>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTransportAttemptFailedPayload {
    pub transport_attempt_id: TransportAttemptId,
    pub operation_context: TransportOperationContext,
    pub failure_id: ModelFailureId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTransportAttemptCancelledPayload {
    pub transport_attempt_id: TransportAttemptId,
    pub operation_context: TransportOperationContext,
    pub cancelled_by: CancellationSource,
    pub provider_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelUsageRecordedPayload {
    pub transport_attempt_id: TransportAttemptId,
    pub model_execution_snapshot_id: ModelExecutionSnapshotId,
    pub usage: ModelUsage,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCompatibilityLearnedPayload {
    pub endpoint_id: EndpointId,
    pub endpoint_fingerprint: ContentHash,
    pub learned_parameters: BTreeSet<String>,
    pub learned_features: BTreeSet<String>,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelChatOutputTokenFieldAliasLearnedPayload {
    pub endpoint_id: EndpointId,
    pub provider_model_name: String,
    pub configured_field: ChatOutputTokenField,
    pub effective_field: ChatOutputTokenField,
    pub learned_from_transport_attempt_id: TransportAttemptId,
    pub previous_endpoint_revision: Revision,
    pub endpoint_revision: Revision,
    pub previous_models_revision: Revision,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCompatibilityResetPayload {
    pub endpoint_id: EndpointId,
    pub previous_compatibility: EndpointCompatibilityOverrides,
    pub models_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelContextWindowCorrectedPayload {
    pub model_profile_id: ModelProfileId,
    pub failed_execution_snapshot_id: ModelExecutionSnapshotId,
    pub previous_effective_window_tokens: u64,
    pub corrected_window_tokens: u64,
    pub correction_basis: RuntimeWindowCorrectionBasis,
    pub models_revision: Revision,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{EndpointId, ModelDiscoveryRunId, Revision};
    use serde_json::json;

    use super::{
        ModelDiscoveryCancelledPayload, ModelDiscoveryUnsupportedPayload,
        ModelDiscoveryUnsupportedReason, ModelOperationCancellationReason,
    };

    #[test]
    fn cancellation_and_unsupported_are_distinct_terminal_facts() {
        let endpoint_id = EndpointId::new();
        let discovery_run_id = ModelDiscoveryRunId::new();
        let cancelled = ModelDiscoveryCancelledPayload {
            discovery_run_id,
            endpoint_id,
            cancelled_by: ModelOperationCancellationReason::ApplicationRestarted,
            run_revision: Revision::new(2),
        };
        assert_eq!(
            serde_json::to_value(cancelled).expect("cancelled payload")["cancelled_by"],
            "application_restarted"
        );
        let unsupported = ModelDiscoveryUnsupportedPayload {
            discovery_run_id,
            endpoint_id,
            reason_code: ModelDiscoveryUnsupportedReason::ModelsEndpointDisabledByConfiguration,
            run_revision: Revision::new(2),
        };
        assert_eq!(
            serde_json::to_value(unsupported).expect("unsupported payload")["reason_code"],
            "models_endpoint_disabled_by_configuration"
        );
        assert!(
            serde_json::from_value::<ModelDiscoveryCancelledPayload>(json!({
                "discovery_run_id": discovery_run_id,
                "endpoint_id": endpoint_id,
                "cancelled_by": "failed",
                "run_revision": "2"
            }))
            .is_err()
        );
    }
}
