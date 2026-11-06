//! What can go wrong reading or changing the model configuration.
use aura_config::SettingsFacadeError;
use aura_domain_common::{EndpointId, ModelProfileId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelConfigError {
    #[error(transparent)]
    Settings(#[from] SettingsFacadeError),
    #[error("failed to decode models.json: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("model configuration is invalid: {0}")]
    Validation(#[from] crate::ModelConfigValidationError),
    #[error("model profile was not found: {0}")]
    ProfileNotFound(ModelProfileId),
    #[error("model profile is disabled: {0}")]
    ProfileDisabled(ModelProfileId),
    #[error("model endpoint was not found for profile: {0}")]
    EndpointNotFound(ModelProfileId),
    #[error("model provider was not found for profile: {0}")]
    ProviderNotFound(ModelProfileId),
    #[error("model provider was not found: {0}")]
    ProviderNotFoundById(aura_domain_common::ProviderId),
    /// A default or backup connection that is not this provider's, or named twice.
    #[error("provider connections must be the provider's own, each named once: {0}")]
    InvalidProviderConnections(aura_domain_common::ProviderId),
    #[error("model endpoint URL is invalid: {0}")]
    Url(#[from] crate::ModelUrlError),
    #[error("model endpoint was not found: {0}")]
    EndpointIdNotFound(aura_domain_common::EndpointId),
    #[error("model provider was not found for endpoint: {0}")]
    ProviderForEndpointNotFound(aura_domain_common::EndpointId),
    #[error("provider model name must not be blank")]
    BlankProviderModelName,
    #[error("endpoint {endpoint_id} already has provider model name {provider_model_name:?}")]
    DuplicateProviderModelName {
        endpoint_id: aura_domain_common::EndpointId,
        provider_model_name: String,
    },
    #[error(
        "discovery response for endpoint {endpoint_id} duplicated provider model name {provider_model_name:?}"
    )]
    DuplicateDiscoveredModelName {
        endpoint_id: aura_domain_common::EndpointId,
        provider_model_name: String,
    },
    #[error("manual model context window must be greater than zero")]
    InvalidManualContextWindow,
    #[error("aura.toml default_model_profile_id must be a ModelProfileId string")]
    InvalidDefaultModelProfileId,
    #[error("default model profile was not found: {0}")]
    DefaultProfileNotFound(ModelProfileId),
    #[error("model configuration invariant failed after a committed transaction: {0}")]
    InternalInvariant(&'static str),
    #[error("context window must be greater than zero")]
    InvalidContextWindow,
    #[error(
        "model execution credential binding must be absent for auth=none and complete after secret-bearing preflight"
    )]
    InvalidExecutionCredentialBinding,
    #[error("historical snapshot maximum must be greater than zero")]
    InvalidHistoricalSnapshotLimit,
    #[error("model profile requires an explicit or catalog-provided max output token limit: {0}")]
    OutputLimitConfigurationRequired(ModelProfileId),
    #[error("user context-window override revision is exhausted")]
    UserOverrideRevisionExhausted,
    #[error("no smaller automatic context-window candidate remains")]
    NoSmallerContextWindowCandidate,
    #[error("runtime context-window candidate must be {expected}, found {actual}")]
    InvalidRuntimeContextWindowCandidate { expected: u64, actual: u64 },
    #[error("model endpoint revision is exhausted: {0}")]
    EndpointRevisionExhausted(EndpointId),
    #[error("model endpoint revision conflict: expected {expected}, actual {actual}")]
    EndpointObjectRevisionConflict { expected: u64, actual: u64 },
    #[error("model profile revision is exhausted: {0}")]
    ProfileRevisionExhausted(ModelProfileId),
    #[error("model profile revision conflict: expected {expected}, actual {actual}")]
    ProfileObjectRevisionConflict { expected: u64, actual: u64 },
    #[error("model notes exceed 4096 UTF-8 bytes")]
    ModelNotesTooLarge,
    #[error("model notes contain a prohibited control character")]
    ModelNotesInvalidCharacter,
    #[error("provider catalog entry was not found: {0}")]
    CatalogProviderNotFound(String),
    #[error("provider catalog model was not found: {catalog_provider_id}/{catalog_model_id}")]
    CatalogModelNotFound { catalog_provider_id: String, catalog_model_id: String },
    #[error("provider catalog model is not a chat model: {catalog_provider_id}/{catalog_model_id}")]
    CatalogModelNotChat { catalog_provider_id: String, catalog_model_id: String },
    #[error(
        "provider catalog model {catalog_provider_id}/{catalog_model_id} goes by the route {route_id}, not this connection's"
    )]
    CatalogModelOnAnotherRoute {
        catalog_provider_id: String,
        catalog_model_id: String,
        route_id: String,
    },
    #[error(
        "no route of the provider serves catalog model {catalog_provider_id}/{catalog_model_id}"
    )]
    CatalogModelRouteUnsupported { catalog_provider_id: String, catalog_model_id: String },
    #[error(
        "catalog model {catalog_provider_id}/{catalog_model_id} is not one the model's name matches"
    )]
    CatalogEntryNotForModel { catalog_provider_id: String, catalog_model_id: String },
    #[error("endpoint provider is not bound to catalog provider {catalog_provider_id}")]
    CatalogProviderBindingMismatch { catalog_provider_id: String },
    #[error("catalog model object changed before the mutation was committed")]
    CatalogObjectHashMismatch,
    #[error("model profile has no catalog snapshot: {0}")]
    CatalogSnapshotMissing(ModelProfileId),
    #[error("output-token field alias must switch to the other supported field")]
    InvalidOutputTokenFieldAlias,
    #[error(
        "output-token alias model {provider_model_name:?} was not found on endpoint {endpoint_id}"
    )]
    OutputTokenAliasProfileNotFound { endpoint_id: EndpointId, provider_model_name: String },
    #[error("output-token aliases require an OpenAI Chat endpoint: {0}")]
    OutputTokenAliasFormatMismatch(EndpointId),
    #[error("output-token alias endpoint has no configured field: {0}")]
    OutputTokenAliasConfiguredFieldMissing(EndpointId),
    #[error("output-token alias configured field changed before persistence: {0}")]
    OutputTokenAliasConfiguredFieldChanged(EndpointId),
    #[error(
        "compatibility.output_token_field_conflict: endpoint {endpoint_id}, model {provider_model_name:?}"
    )]
    OutputTokenFieldAliasConflict { endpoint_id: EndpointId, provider_model_name: String },
    #[error("output-token alias endpoint fingerprint changed before persistence: {0}")]
    OutputTokenAliasEndpointFingerprintChanged(EndpointId),
}
