use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::{
    ArtifactId, ContentHash, EndpointId, ModelProfileId, ProviderId, Revision, TransportAttemptId,
    UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::EndpointAuthScheme;
use crate::{EndpointSlug, ProviderCatalogModelEntry, ProviderSlug};

pub const DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS: u64 = 256_000;
pub const DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS: u64 = 1_024;
pub const DEFAULT_MAX_PARALLEL_MODEL_REQUESTS: u32 = 2;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelApiFormat {
    OpenAiResponses,
    OpenAiChatCompletions,
    AnthropicMessages,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelDiscoveryMode {
    Automatic,
    Custom { path_and_query: String },
    Disabled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatOutputTokenField {
    MaxCompletionTokens,
    MaxTokens,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatInstructionRole {
    System,
    Developer,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelProviderSource {
    ModelsDev { catalog_provider_id: String },
    Custom { catalog_provider_id: Option<String> },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderConfig {
    pub id: ProviderId,
    pub display_name: String,
    pub slug: ProviderSlug,
    pub source: ModelProviderSource,
    pub endpoint_order: Vec<EndpointId>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    /// The
    /// connection this provider's models are sent through. None makes the first created the
    /// default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_endpoint_id: Option<EndpointId>,
    /// Connections tried in this order when the one used fails before it answers. Every other connection is a backup in the order they were created; a list saved before is
    /// still read and decides nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backup_endpoint_ids: Vec<EndpointId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointTimeouts {
    pub connect_ms: u64,
    pub first_event_ms: u64,
    pub stream_idle_ms: u64,
    pub total_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChatOutputTokenFieldAlias {
    pub provider_model_name: String,
    pub configured_field: ChatOutputTokenField,
    pub effective_field: ChatOutputTokenField,
    pub learned_at: UtcTimestamp,
    pub learned_from_transport_attempt_id: TransportAttemptId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointCompatibilityOverrides {
    pub endpoint_fingerprint: ContentHash,
    pub unsupported_parameters: BTreeSet<String>,
    pub unsupported_features: BTreeSet<String>,
    pub output_token_field_aliases: Vec<ChatOutputTokenFieldAlias>,
    pub learned_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEndpointConfig {
    pub id: EndpointId,
    pub revision: Revision,
    pub provider_id: ProviderId,
    pub display_name: String,
    pub slug: EndpointSlug,
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
    pub model_order: Vec<ModelProfileId>,
    pub compatibility: Option<EndpointCompatibilityOverrides>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProfileOrigin {
    Discovered,
    Manual,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogMatchState {
    Matched,
    Unmatched,
    Ambiguous,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogMetadata {
    pub catalog_provider_id: String,
    pub catalog_model_id: String,
    pub catalog_revision: Revision,
    pub catalog_content_hash: ContentHash,
    pub catalog_object_hash: ContentHash,
    pub snapshot: ProviderCatalogModelEntry,
    pub matched_at: Option<UtcTimestamp>,
    pub match_state: CatalogMatchState,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeWindowCorrectionBasis {
    pub endpoint_fingerprint: ContentHash,
    pub model_name_hash: ContentHash,
    pub user_override_revision: u64,
    pub provider_error_ref: ArtifactId,
    pub corrected_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelContextWindowConfig {
    pub catalog_window_tokens: Option<u64>,
    pub user_override_window_tokens: Option<u64>,
    pub user_override_revision: u64,
    pub runtime_corrected_window_tokens: Option<u64>,
    pub runtime_correction_basis: Option<RuntimeWindowCorrectionBasis>,
    pub runtime_effective_window_tokens: u64,
    pub historical_snapshot_max_tokens: u64,
}

impl ModelContextWindowConfig {
    #[must_use]
    pub fn effective_window_tokens(&self, basis_is_current: bool) -> u64 {
        if basis_is_current && let Some(corrected) = self.runtime_corrected_window_tokens {
            return corrected;
        }
        self.user_override_window_tokens
            .or(self.catalog_window_tokens)
            .unwrap_or(DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS)
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequestDefaults {
    pub max_output_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub chat_output_token_field: Option<ChatOutputTokenField>,
    pub chat_instruction_role: Option<ChatInstructionRole>,
    /// The thinking level the profile asks for; none is the provider's own default and
    /// sends nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::ReasoningEffort>,
    /// The levels the model offers, when its user listed them (a model the catalog does
    /// not know, or knows wrongly); none takes them from the catalog and the API format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_efforts: Option<Vec<crate::ReasoningEffort>>,
    /// Whether the model is sent images (the browser's screenshots), when its user said so;
    /// none takes it from the matched catalog entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProfileOutputLimitStatus {
    Configured,
    NeedsOutputLimitConfiguration,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileConfig {
    pub id: ModelProfileId,
    pub revision: Revision,
    pub endpoint_id: EndpointId,
    pub provider_model_name: String,
    pub display_name: String,
    pub notes: Option<String>,
    pub origin: ModelProfileOrigin,
    pub enabled: bool,
    pub available_for_subagents: bool,
    pub catalog: Option<ModelCatalogMetadata>,
    pub context: ModelContextWindowConfig,
    pub request_defaults: ModelRequestDefaults,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRegistryConfig {
    pub provider_order: Vec<ProviderId>,
    pub providers: BTreeMap<ProviderId, ModelProviderConfig>,
    pub endpoints: BTreeMap<EndpointId, ModelEndpointConfig>,
    pub models: BTreeMap<ModelProfileId, ModelProfileConfig>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateModelProviderInput {
    pub display_name: String,
    pub source: ModelProviderSource,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateModelEndpointInput {
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

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateModelEndpointInput {
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

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteModelConfigResult {
    pub deleted_provider_ids: Vec<ProviderId>,
    pub deleted_endpoint_ids: Vec<EndpointId>,
    pub deleted_model_profile_ids: Vec<ModelProfileId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultModelSelection {
    pub model_profile_id: Option<ModelProfileId>,
    pub aura_config_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteModelConfigCommitResult {
    pub deleted: DeleteModelConfigResult,
    pub default_model_cleared: bool,
    /// The configuration delete committed successfully, but one or more
    /// internal CredentialDelete children still require secure cleanup.
    pub credential_cleanup_pending: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowUpdateResult {
    pub model_profile_id: ModelProfileId,
    pub runtime_effective_window_tokens: u64,
    pub user_override_revision: u64,
    pub experience_warning: bool,
}

impl ModelProfileConfig {
    #[must_use]
    pub fn effective_max_output_tokens(&self) -> Option<u64> {
        self.request_defaults.max_output_tokens.or_else(|| {
            self.catalog.as_ref().and_then(|catalog| catalog.snapshot.output_limit_tokens)
        })
    }

    #[must_use]
    pub fn output_limit_status(&self) -> ModelProfileOutputLimitStatus {
        if self.effective_max_output_tokens().is_some_and(|value| value > 0) {
            ModelProfileOutputLimitStatus::Configured
        } else {
            ModelProfileOutputLimitStatus::NeedsOutputLimitConfiguration
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS, ModelContextWindowConfig};

    #[test]
    fn effective_window_uses_the_frozen_precedence() {
        let config = ModelContextWindowConfig {
            catalog_window_tokens: Some(128_000),
            user_override_window_tokens: Some(64_000),
            user_override_revision: 3,
            runtime_corrected_window_tokens: Some(32_000),
            runtime_correction_basis: None,
            runtime_effective_window_tokens: 32_000,
            historical_snapshot_max_tokens: 1_024,
        };

        assert_eq!(config.effective_window_tokens(true), 32_000);
        assert_eq!(config.effective_window_tokens(false), 64_000);

        let fallback = ModelContextWindowConfig {
            catalog_window_tokens: None,
            user_override_window_tokens: None,
            runtime_corrected_window_tokens: None,
            runtime_effective_window_tokens: DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS,
            ..config
        };
        assert_eq!(fallback.effective_window_tokens(false), DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS);
    }

    #[test]
    fn context_window_contract_rejects_unknown_fields() {
        let value = json!({
            "catalog_window_tokens": 128000,
            "user_override_window_tokens": null,
            "user_override_revision": 0,
            "runtime_corrected_window_tokens": null,
            "runtime_correction_basis": null,
            "runtime_effective_window_tokens": 128000,
            "historical_snapshot_max_tokens": 1024,
            "unknown": true
        });
        assert!(serde_json::from_value::<ModelContextWindowConfig>(value).is_err());
    }
}
