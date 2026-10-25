use aura_domain_common::{
    ContentHash, CredentialId, EndpointId, ExpectedRevision, ModelAvailabilityTestId,
    ModelDiscoveryRunId, ModelProfileId, ProviderId, Revision, SessionId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AddManualModelInput, ChatInstructionRole, ChatOutputTokenField, ContextWindowUpdateResult,
    CreateModelEndpointInput, CreateModelProviderInput, DefaultModelSelection,
    DeleteModelConfigCommitResult, EndpointAuthScheme, EndpointCompatibilityOverrides,
    EndpointCredentialStatus, EndpointDetailProjection, ModelApiFormat, ModelAvailabilityTestState,
    ModelCredentialSlot, ModelDiscoveryMode, ModelDiscoveryState, ModelEndpointConfig,
    ModelEndpointTimeouts, ModelProfileConfig, ModelProviderConfig, ModelRequestDefaults,
    ProviderCatalogModelPage, SessionModelBinding,
};

pub mod model_ipc_method {
    pub const PROVIDER_ICON_READ: &str = "provider_icon.read";
    pub const SETTINGS_GET_SNAPSHOT: &str = "models.settings.get_snapshot";
    pub const CATALOG_GET: &str = "models.catalog.get";
    pub const CATALOG_ENSURE_FRESH: &str = "models.catalog.ensure_fresh";
    pub const CATALOG_REFRESH: &str = "models.catalog.refresh";
    pub const CATALOG_PROVIDER_GET: &str = "models.catalog.provider.get";
    pub const PROVIDER_CREATE: &str = "models.provider.create";
    pub const PROVIDER_RENAME: &str = "models.provider.rename";
    /// A provider's default connection and its backups.
    pub const PROVIDER_SET_CONNECTIONS: &str = "models.provider.set_connections";
    pub const PROVIDER_DELETE: &str = "models.provider.delete";
    pub const ENDPOINT_CREATE: &str = "models.endpoint.create";
    pub const ENDPOINT_CREATE_WITH_CREDENTIAL: &str = "models.endpoint.create_with_credential";
    pub const ENDPOINT_UPDATE: &str = "models.endpoint.update";
    pub const ENDPOINT_DELETE: &str = "models.endpoint.delete";
    pub const ENDPOINT_CONFIGURE_CREDENTIAL: &str = "models.endpoint.configure_credential";
    pub const ENDPOINT_REPLACE_CREDENTIAL: &str = "models.endpoint.replace_credential";
    pub const ENDPOINT_CLEAR_CREDENTIAL: &str = "models.endpoint.clear_credential";
    pub const DISCOVERY_START: &str = "models.discovery.start";
    pub const DISCOVERY_CANCEL: &str = "models.discovery.cancel";
    pub const PROFILE_CREATE_MANUAL: &str = "models.profile.create_manual";
    pub const PROFILE_CREATE_FROM_CATALOG: &str = "models.profile.create_from_catalog";
    pub const PROFILE_APPLY_CATALOG_UPDATE: &str = "models.profile.apply_catalog_update";
    pub const PROFILE_TEST_AVAILABILITY: &str = "models.profile.test_availability";
    pub const AVAILABILITY_TEST_CANCEL: &str = "models.availability_test.cancel";
    pub const PROFILE_UPDATE: &str = "models.profile.update";
    pub const PROFILE_DELETE: &str = "models.profile.delete";
    pub const PROFILE_SET_SUBAGENT_AVAILABILITY: &str = "models.profile.set_subagent_availability";
    pub const PROVIDER_SET_ALL_CURRENT_MODELS_SUBAGENT_AVAILABILITY: &str =
        "models.provider.set_all_current_models_subagent_availability";
    pub const DEFAULT_SET: &str = "models.default.set";
    pub const SESSION_SET_BINDING: &str = "models.session.set_binding";
    pub const CONTEXT_WINDOW_CORRECT: &str = "models.context_window.correct";
    pub const CONTEXT_WINDOW_CLEAR_USER_OVERRIDE: &str =
        "models.context_window.clear_user_override";
    pub const CONTEXT_WINDOW_RESET_RUNTIME_CORRECTION: &str =
        "models.context_window.reset_runtime_correction";
    pub const COMPATIBILITY_GET: &str = "models.compatibility.get";
    pub const COMPATIBILITY_RESET: &str = "models.compatibility.reset";
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIconReadRequest {
    pub artifact_id: aura_domain_common::ArtifactId,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIconReadResult {
    pub content_hash: ContentHash,
    pub byte_length: u64,
    pub png_base64: String,
}

macro_rules! define_model_config_mutation_result {
    ($name:ident, $value:ty) => {
        #[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub changed: bool,
            pub revision: Revision,
            pub value: $value,
        }
    };
}

define_model_config_mutation_result!(ModelProviderMutationResult, ModelProviderConfig);
define_model_config_mutation_result!(ModelEndpointMutationResult, ModelEndpointConfig);
define_model_config_mutation_result!(ModelProfileMutationResult, ModelProfileConfig);
define_model_config_mutation_result!(ModelDeletionMutationResult, DeleteModelConfigCommitResult);
define_model_config_mutation_result!(DefaultModelMutationResult, DefaultModelSelection);
define_model_config_mutation_result!(
    SessionModelBindingMutationResult,
    Option<SessionModelBinding>
);
define_model_config_mutation_result!(ContextWindowMutationResult, ContextWindowUpdateResult);

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointCredentialMutationResult {
    pub endpoint_id: EndpointId,
    pub changed: bool,
    pub models_revision: Revision,
    pub credential_status: EndpointCredentialStatus,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointCreatedWithCredentialResult {
    pub models_revision: Revision,
    pub endpoint_projection: EndpointDetailProjection,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBulkAvailabilityResult {
    pub provider_id: ProviderId,
    pub changed_model_profile_ids: Vec<ModelProfileId>,
    pub revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointCompatibilitySnapshot {
    pub endpoint_id: EndpointId,
    pub revision: Revision,
    pub compatibility: Option<EndpointCompatibilityOverrides>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCompatibilityResetResult {
    pub endpoint_id: EndpointId,
    pub changed: bool,
    pub revision: Revision,
    pub compatibility: Option<EndpointCompatibilityOverrides>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSettingsGetSnapshotRequest {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogGetRequest {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogEnsureFreshRequest {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogRefreshRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogProviderGetRequest {
    pub catalog_provider_id: String,
    pub query: Option<String>,
    pub expected_catalog_revision: Option<Revision>,
    pub after_model_id: Option<String>,
    pub limit: Option<u32>,
}

pub type ModelCatalogProviderGetResult = ProviderCatalogModelPage;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderCreateRequest {
    pub expected_revision: ExpectedRevision,
    pub input: CreateModelProviderInput,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderRenameRequest {
    pub expected_revision: ExpectedRevision,
    pub provider_id: ProviderId,
    pub display_name: String,
}

/// Which of a provider's connections its models use, and which stand in when it fails before
/// answering. An empty choice returns each model to its own connection with no backup.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderConnectionsRequest {
    pub expected_revision: ExpectedRevision,
    pub provider_id: ProviderId,
    pub default_endpoint_id: Option<EndpointId>,
    pub backup_endpoint_ids: Vec<EndpointId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderDeleteRequest {
    pub expected_revision: ExpectedRevision,
    pub provider_id: ProviderId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointCreateRequest {
    pub expected_revision: ExpectedRevision,
    pub provider_id: ProviderId,
    pub display_name: String,
    pub api_format: ModelApiFormat,
    pub base_url: String,
    pub discovery_mode: ModelDiscoveryMode,
    pub chat_output_token_field: Option<ChatOutputTokenField>,
    pub chat_instruction_role: Option<ChatInstructionRole>,
    pub auth_scheme: EndpointAuthScheme,
    pub openai_organization_id: Option<String>,
    pub openai_project_id: Option<String>,
    pub anthropic_workspace_id: Option<String>,
    pub timeouts: ModelEndpointTimeouts,
    pub max_parallel_model_requests: u32,
}

impl ModelEndpointCreateRequest {
    #[must_use]
    pub fn into_input(self) -> CreateModelEndpointInput {
        CreateModelEndpointInput {
            provider_id: self.provider_id,
            display_name: self.display_name,
            api_format: self.api_format,
            base_url: self.base_url,
            discovery_mode: self.discovery_mode,
            chat_output_token_field: self.chat_output_token_field,
            chat_instruction_role: self.chat_instruction_role,
            auth_scheme: self.auth_scheme,
            openai_organization_id: self.openai_organization_id,
            openai_project_id: self.openai_project_id,
            anthropic_workspace_id: self.anthropic_workspace_id,
            timeouts: self.timeouts,
            max_parallel_model_requests: self.max_parallel_model_requests,
        }
    }
}

/// Non-secret Control Command half of `models.endpoint.create_with_credential`.
///
/// The matching API key is carried only by the negotiated SecretInput frame.
/// Endpoint, credential, Journal, Event, and reserved Revision identities are
/// generated by Core and therefore intentionally absent here.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointCreateWithCredentialRequest {
    pub expected_revision: ExpectedRevision,
    pub provider_id: ProviderId,
    pub display_name: String,
    pub api_format: ModelApiFormat,
    pub base_url: String,
    pub discovery_mode: ModelDiscoveryMode,
    pub chat_output_token_field: Option<ChatOutputTokenField>,
    pub chat_instruction_role: Option<ChatInstructionRole>,
    pub auth_scheme: EndpointAuthScheme,
    pub openai_organization_id: Option<String>,
    pub openai_project_id: Option<String>,
    pub anthropic_workspace_id: Option<String>,
    pub timeouts: ModelEndpointTimeouts,
    pub max_parallel_model_requests: u32,
}

impl ModelEndpointCreateWithCredentialRequest {
    #[must_use]
    pub fn into_input(self) -> CreateModelEndpointInput {
        CreateModelEndpointInput {
            provider_id: self.provider_id,
            display_name: self.display_name,
            api_format: self.api_format,
            base_url: self.base_url,
            discovery_mode: self.discovery_mode,
            chat_output_token_field: self.chat_output_token_field,
            chat_instruction_role: self.chat_instruction_role,
            auth_scheme: self.auth_scheme,
            openai_organization_id: self.openai_organization_id,
            openai_project_id: self.openai_project_id,
            anthropic_workspace_id: self.anthropic_workspace_id,
            timeouts: self.timeouts,
            max_parallel_model_requests: self.max_parallel_model_requests,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointUpdateRequest {
    pub expected_models_revision: ExpectedRevision,
    pub expected_endpoint_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
    pub display_name: String,
    pub api_format: ModelApiFormat,
    pub base_url: String,
    pub discovery_mode: ModelDiscoveryMode,
    pub chat_output_token_field: Option<ChatOutputTokenField>,
    pub chat_instruction_role: Option<ChatInstructionRole>,
    pub auth_scheme: EndpointAuthScheme,
    pub openai_organization_id: Option<String>,
    pub openai_project_id: Option<String>,
    pub anthropic_workspace_id: Option<String>,
    pub timeouts: ModelEndpointTimeouts,
    pub max_parallel_model_requests: u32,
}

impl ModelEndpointUpdateRequest {
    #[must_use]
    pub fn into_input(self) -> crate::UpdateModelEndpointInput {
        crate::UpdateModelEndpointInput {
            endpoint_id: self.endpoint_id,
            display_name: self.display_name,
            api_format: self.api_format,
            base_url: self.base_url,
            discovery_mode: self.discovery_mode,
            chat_output_token_field: self.chat_output_token_field,
            chat_instruction_role: self.chat_instruction_role,
            auth_scheme: self.auth_scheme,
            openai_organization_id: self.openai_organization_id,
            openai_project_id: self.openai_project_id,
            anthropic_workspace_id: self.anthropic_workspace_id,
            timeouts: self.timeouts,
            max_parallel_model_requests: self.max_parallel_model_requests,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointDeleteRequest {
    pub expected_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointConfigureCredentialRequest {
    pub expected_models_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
    pub credential_slot: ModelCredentialSlot,
    pub credential_id: CredentialId,
    pub expected_row_revision: ExpectedRevision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointReplaceCredentialRequest {
    pub expected_models_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
    pub credential_slot: ModelCredentialSlot,
    pub credential_id: CredentialId,
    pub expected_row_revision: ExpectedRevision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointClearCredentialRequest {
    pub expected_models_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
    pub credential_slot: ModelCredentialSlot,
    pub credential_id: CredentialId,
    pub expected_row_revision: ExpectedRevision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestStartRequest {
    pub model_profile_id: ModelProfileId,
    pub expected_model_profile_revision: ExpectedRevision,
    pub expected_endpoint_revision: ExpectedRevision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestStartAccepted {
    pub model_availability_test_id: ModelAvailabilityTestId,
    pub operational_revision: Revision,
    pub state: ModelAvailabilityTestState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestCancelRequest {
    pub model_availability_test_id: ModelAvailabilityTestId,
    pub expected_operational_revision: ExpectedRevision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAvailabilityTestCancelResult {
    pub model_availability_test_id: ModelAvailabilityTestId,
    pub operational_revision: Revision,
    pub state: ModelAvailabilityTestState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailabilityPrerequisiteFailure {
    ModelProfileNotFound,
    ModelProfileNotPersisted,
    EndpointNotFound,
    EndpointConfigurationInvalid,
    BaseUrlMissing,
    ApiFormatMissing,
    ModelNameMissing,
    CredentialNotConfigured,
    RequestContractUnresolved,
    ConfigurationRevisionChanged,
    ModelConfigMutationInProgress,
    TestAlreadyRunning,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryStartRequest {
    pub expected_models_revision: ExpectedRevision,
    pub expected_endpoint_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryStartAccepted {
    pub model_discovery_run_id: ModelDiscoveryRunId,
    pub operational_revision: Revision,
    pub state: ModelDiscoveryState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryCancelRequest {
    pub model_discovery_run_id: ModelDiscoveryRunId,
    pub expected_operational_revision: ExpectedRevision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryCancelResult {
    pub model_discovery_run_id: ModelDiscoveryRunId,
    pub operational_revision: Revision,
    pub state: ModelDiscoveryState,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileCreateManualRequest {
    pub expected_revision: ExpectedRevision,
    pub input: AddManualModelInput,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileCreateFromCatalogRequest {
    pub expected_models_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
    pub expected_endpoint_revision: ExpectedRevision,
    pub catalog_provider_id: String,
    pub catalog_model_id: String,
    pub expected_catalog_revision: Revision,
    pub expected_catalog_object_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileApplyCatalogUpdateRequest {
    pub expected_models_revision: ExpectedRevision,
    pub model_profile_id: ModelProfileId,
    pub expected_model_profile_revision: ExpectedRevision,
    pub expected_catalog_revision: Revision,
    pub expected_catalog_object_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileUpdateRequest {
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: ModelProfileId,
    pub display_name: String,
    pub notes: Option<String>,
    pub enabled: bool,
    pub request_defaults: ModelRequestDefaults,
    pub historical_snapshot_max_tokens: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileDeleteRequest {
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: ModelProfileId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileSetSubagentAvailabilityRequest {
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: ModelProfileId,
    pub available_for_subagents: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest {
    pub expected_revision: ExpectedRevision,
    pub provider_id: ProviderId,
    pub available_for_subagents: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefaultSetRequest {
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: Option<ModelProfileId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelBindingSetRequest {
    pub expected_revision: ExpectedRevision,
    pub session_id: SessionId,
    pub model_profile_id: Option<ModelProfileId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowCorrectRequest {
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: ModelProfileId,
    pub window_tokens: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowClearUserOverrideRequest {
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: ModelProfileId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowResetRuntimeCorrectionRequest {
    pub expected_revision: ExpectedRevision,
    pub model_profile_id: ModelProfileId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCompatibilityGetRequest {
    pub endpoint_id: EndpointId,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCompatibilityResetRequest {
    pub expected_revision: ExpectedRevision,
    pub endpoint_id: EndpointId,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CredentialId, EndpointId, ExpectedRevision};
    use serde_json::json;

    use crate::{
        ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
        ModelCredentialSlot, ModelDiscoveryMode, ModelEndpointTimeouts,
    };

    use super::{ModelEndpointConfigureCredentialRequest, ModelEndpointUpdateRequest};

    #[test]
    fn renderer_credential_request_contains_no_secret_or_custom_target() {
        let request = ModelEndpointConfigureCredentialRequest {
            expected_models_revision: ExpectedRevision::new(6),
            endpoint_id: EndpointId::new(),
            credential_slot: ModelCredentialSlot::ApiKey,
            credential_id: CredentialId::new(),
            expected_row_revision: ExpectedRevision::new(3),
        };
        let value = serde_json::to_value(request).expect("credential request");
        assert_eq!(value.as_object().expect("object").len(), 5);
        assert_eq!(value["credential_slot"], "api-key");
        for forbidden in ["api_key", "secret", "credential_ref", "target", "header"] {
            assert!(!value.to_string().to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn endpoint_update_requires_global_and_object_revisions() {
        let request = ModelEndpointUpdateRequest {
            expected_models_revision: ExpectedRevision::new(11),
            expected_endpoint_revision: ExpectedRevision::new(7),
            endpoint_id: EndpointId::new(),
            display_name: "Primary".to_owned(),
            api_format: ModelApiFormat::OpenAiChatCompletions,
            base_url: "https://api.example.com/v1".to_owned(),
            discovery_mode: ModelDiscoveryMode::Automatic,
            chat_output_token_field: Some(ChatOutputTokenField::MaxCompletionTokens),
            chat_instruction_role: Some(ChatInstructionRole::Developer),
            auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
            openai_organization_id: None,
            openai_project_id: None,
            anthropic_workspace_id: None,
            timeouts: ModelEndpointTimeouts {
                connect_ms: 15_000,
                first_event_ms: 120_000,
                stream_idle_ms: 120_000,
                total_ms: None,
            },
            max_parallel_model_requests: 2,
        };
        let value = serde_json::to_value(request).expect("endpoint update request");
        assert_eq!(value["expected_models_revision"], "11");
        assert_eq!(value["expected_endpoint_revision"], "7");
        assert!(value.get("expected_revision").is_none());

        let mut legacy = value;
        legacy
            .as_object_mut()
            .expect("request object")
            .insert("expected_revision".to_owned(), json!("11"));
        assert!(serde_json::from_value::<ModelEndpointUpdateRequest>(legacy).is_err());
    }
}
