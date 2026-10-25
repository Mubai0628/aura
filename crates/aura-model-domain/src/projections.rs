use aura_domain_common::{
    AgentRunId, ContentHash, EndpointId, ModelAvailabilityTestId, ModelDiscoveryRunId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, NormalizedIssueCode, ProviderId,
    Revision, SessionId, UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{
    ContextWindowResolution, DormantEndpointCredentialSummary, EndpointAuthenticationKind,
    EndpointCredentialStatus, ModelApiFormat, ModelAvailabilityTestResult,
    ModelAvailabilityTestStage, ModelAvailabilityTestState, ModelDiscoveryOutcome,
    ModelDiscoveryStage, ModelDiscoveryState, ModelEndpointConfig, ModelProfileConfig,
    ModelProviderConfig, ModelRegistryConfig, ProviderCatalogCardView, ProviderCatalogState,
    ProviderErrorSummaryView,
};

impl ModelDiscoveryRunView {
    #[must_use]
    pub fn from_record(record: &crate::ModelDiscoveryRunRecord) -> Self {
        let (returned_count, created_count, not_returned_count) =
            record.result.as_ref().map_or((None, None, None), |result| {
                (
                    u32::try_from(result.returned_profiles.len()).ok(),
                    u32::try_from(
                        result.returned_profiles.iter().filter(|profile| profile.created).count(),
                    )
                    .ok(),
                    u32::try_from(result.not_returned_profile_ids.len()).ok(),
                )
            });
        Self {
            id: record.id,
            revision: record.revision,
            state: record.state,
            stage: record.stage,
            outcome: record.outcome,
            cancellation_reason: record.cancellation_reason,
            returned_count,
            created_count,
            not_returned_count,
            provider_error: record.provider_error.clone(),
            started_at: record.started_at,
            finished_at: record.finished_at,
        }
    }
}

impl ModelAvailabilityTestView {
    #[must_use]
    pub fn from_record(record: &crate::ModelAvailabilityTestRecord) -> Self {
        Self {
            id: record.id,
            revision: record.revision,
            state: record.state,
            stage: record.stage,
            cancellation_reason: record.cancellation_reason,
            endpoint_id: record.endpoint_id,
            model_profile_id: record.model_profile_id,
            endpoint_revision_at_start: record.endpoint_revision_at_start,
            model_profile_revision_at_start: record.model_profile_revision_at_start,
            models_revision_at_start: record.models_revision_at_start,
            endpoint_security_fingerprint: record.endpoint_security_fingerprint.clone(),
            result: record.result.clone(),
            started_at: record.started_at,
            completed_at: record.completed_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryRunView {
    pub id: ModelDiscoveryRunId,
    pub revision: Revision,
    pub state: ModelDiscoveryState,
    pub stage: Option<ModelDiscoveryStage>,
    pub outcome: Option<ModelDiscoveryOutcome>,
    pub cancellation_reason: Option<crate::ModelOperationCancellationReason>,
    pub returned_count: Option<u32>,
    pub created_count: Option<u32>,
    pub not_returned_count: Option<u32>,
    pub provider_error: Option<ProviderErrorSummaryView>,
    pub started_at: UtcTimestamp,
    pub finished_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestView {
    pub id: ModelAvailabilityTestId,
    pub revision: Revision,
    pub state: ModelAvailabilityTestState,
    pub stage: Option<ModelAvailabilityTestStage>,
    pub cancellation_reason: Option<crate::ModelOperationCancellationReason>,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub endpoint_revision_at_start: Revision,
    pub model_profile_revision_at_start: Revision,
    pub models_revision_at_start: Revision,
    pub endpoint_security_fingerprint: ContentHash,
    pub result: Option<ModelAvailabilityTestResult>,
    pub started_at: UtcTimestamp,
    pub completed_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSettingsProjection {
    pub models_revision: Revision,
    pub aura_revision: Revision,
    pub registry: ModelRegistryConfig,
    pub default_model_profile_id: Option<ModelProfileId>,
    pub active_credentials: Vec<EndpointCredentialStatus>,
    pub dormant_credentials: Vec<DormantEndpointCredentialSummary>,
    /// The thinking levels each model offers; a model offering none is absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_efforts: Vec<ModelReasoningEffortsView>,
}

/// The thinking levels one model offers, lowest first.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelReasoningEffortsView {
    pub model_profile_id: ModelProfileId,
    pub reasoning_efforts: Vec<crate::ReasoningEffort>,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointAuthenticationProjection {
    pub auth_kind: EndpointAuthenticationKind,
    pub active_credential: Option<EndpointCredentialStatus>,
    pub dormant_credential: Option<DormantEndpointCredentialSummary>,
}

impl<'de> Deserialize<'de> for EndpointAuthenticationProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireProjection {
            auth_kind: EndpointAuthenticationKind,
            active_credential: Option<EndpointCredentialStatus>,
            dormant_credential: Option<DormantEndpointCredentialSummary>,
        }

        let wire = WireProjection::deserialize(deserializer)?;
        let projection = Self {
            auth_kind: wire.auth_kind,
            active_credential: wire.active_credential,
            dormant_credential: wire.dormant_credential,
        };
        if !projection.is_valid() {
            return Err(D::Error::custom(
                "endpoint authentication projection violates active/dormant credential invariants",
            ));
        }
        Ok(projection)
    }
}

impl EndpointAuthenticationProjection {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        match self.auth_kind {
            EndpointAuthenticationKind::Secret => {
                self.active_credential.is_some() && self.dormant_credential.is_none()
            }
            EndpointAuthenticationKind::None => self.active_credential.is_none(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelExecutionReadinessView {
    pub is_executable: bool,
    pub issue_code: Option<NormalizedIssueCode>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogProjection {
    pub state: ProviderCatalogState,
    pub catalog_revision: Option<Revision>,
    pub catalog_content_hash: Option<ContentHash>,
    pub checked_at: Option<UtcTimestamp>,
    pub last_successful_refresh_at: Option<UtcTimestamp>,
    pub cards: Vec<ProviderCatalogCardView>,
    pub last_error_code: Option<NormalizedIssueCode>,
    pub projection_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointDetailProjection {
    pub endpoint_id: EndpointId,
    pub models_revision: Revision,
    pub provider: ModelProviderConfig,
    pub endpoint: ModelEndpointConfig,
    pub authentication: EndpointAuthenticationProjection,
    pub models: Vec<ModelProfileConfig>,
    pub latest_discovery_run: Option<ModelDiscoveryRunView>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileProjection {
    pub model_profile_id: ModelProfileId,
    pub models_revision: Revision,
    pub provider: ModelProviderConfig,
    pub endpoint: ModelEndpointConfig,
    pub execution_readiness: ModelExecutionReadinessView,
    pub profile: ModelProfileConfig,
    pub active_availability_test: Option<ModelAvailabilityTestView>,
    pub latest_availability_test: Option<ModelAvailabilityTestView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelProviderView {
    pub provider_id: ProviderId,
    pub display_name: String,
    pub order: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelEndpointView {
    pub endpoint_id: EndpointId,
    pub provider_id: ProviderId,
    pub display_name: String,
    pub api_format: ModelApiFormat,
    pub order: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelOptionView {
    pub model_profile_id: ModelProfileId,
    pub endpoint_id: EndpointId,
    pub display_name: String,
    pub provider_model_name: String,
    pub effective_context_window_tokens: u64,
    pub effective_max_output_tokens: Option<u64>,
    pub execution_readiness: ModelExecutionReadinessView,
    pub order: u32,
    /// The thinking levels the model offers, lowest first; empty when it offers none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_efforts: Vec<crate::ReasoningEffort>,
    /// The level its profile asks for; none is the provider's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::ReasoningEffort>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelSelectorProjection {
    pub session_id: SessionId,
    pub binding_revision: Revision,
    pub selected_model_profile_id: Option<ModelProfileId>,
    pub default_model_profile_id: Option<ModelProfileId>,
    pub providers: Vec<SessionModelProviderView>,
    pub endpoints: Vec<SessionModelEndpointView>,
    pub models: Vec<SessionModelOptionView>,
    pub selected_model_execution_readiness: ModelExecutionReadinessView,
    pub projection_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryProjection {
    pub model_discovery_run_id: ModelDiscoveryRunId,
    pub run: ModelDiscoveryRunView,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestProjection {
    pub model_availability_test_id: ModelAvailabilityTestId,
    pub test: ModelAvailabilityTestView,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowCorrectionView {
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub failed_execution_snapshot_id: ModelExecutionSnapshotId,
    pub model_profile_id: ModelProfileId,
    pub resolution: ContextWindowResolution,
    pub provider_error: Option<ProviderErrorSummaryView>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowCorrectionProjection {
    pub session_id: SessionId,
    pub active_corrections: Vec<ContextWindowCorrectionView>,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CredentialId, CredentialRevision, EndpointId, Revision};
    use serde_json::json;

    use super::EndpointAuthenticationProjection;
    use crate::{
        DormantEndpointCredentialSummary, EndpointCredentialState, EndpointCredentialStatus,
        ModelCredentialSlot,
    };

    fn active() -> EndpointCredentialStatus {
        let revision = CredentialRevision::new(Revision::new(2)).expect("credential revision");
        EndpointCredentialStatus {
            endpoint_id: EndpointId::new(),
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id: CredentialId::new(),
            state: EndpointCredentialState::Configured,
            active_credential_revision: Some(revision),
            last_committed_credential_revision: Some(revision),
            row_revision: Revision::new(3),
            error_code: None,
        }
    }

    fn dormant() -> DormantEndpointCredentialSummary {
        DormantEndpointCredentialSummary {
            endpoint_id: EndpointId::new(),
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id: CredentialId::new(),
            retained_state: EndpointCredentialState::Cleared,
            last_committed_credential_revision: Some(
                CredentialRevision::new(Revision::new(2)).expect("credential revision"),
            ),
            row_revision: Revision::new(4),
            error_code: None,
        }
    }

    #[test]
    fn authentication_projection_enforces_active_dormant_exclusivity() {
        let active = active();
        let dormant = dormant();
        serde_json::from_value::<EndpointAuthenticationProjection>(json!({
            "auth_kind": "secret",
            "active_credential": active.clone(),
            "dormant_credential": null
        }))
        .expect("secret authentication requires one active credential");
        serde_json::from_value::<EndpointAuthenticationProjection>(json!({
            "auth_kind": "none",
            "active_credential": null,
            "dormant_credential": dormant.clone()
        }))
        .expect("auth none may expose retained dormant credential");

        for invalid in [
            json!({"auth_kind":"secret","active_credential":null,"dormant_credential":null}),
            json!({"auth_kind":"none","active_credential":active.clone(),"dormant_credential":null}),
            json!({"auth_kind":"secret","active_credential":active.clone(),"dormant_credential":dormant.clone()}),
        ] {
            assert!(serde_json::from_value::<EndpointAuthenticationProjection>(invalid).is_err());
        }
    }
}
