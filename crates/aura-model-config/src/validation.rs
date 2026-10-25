use std::collections::{BTreeMap, BTreeSet};

use aura_config::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, ModuleConfigValidator,
    validate_base_document,
};
use aura_domain_common::{EndpointId, ModelProfileId, ProviderId};
use aura_model_domain::{
    EndpointAuthScheme, ModelApiFormat, ModelDiscoveryMode, ModelEndpointConfig,
    ModelProviderSource, ModelRegistryConfig,
};
use thiserror::Error;

use crate::{endpoint_fingerprint, model_name_hash, resolve_models_url};

#[derive(Clone, Copy, Debug, Default)]
pub struct ModelConfigValidator;

impl ModuleConfigValidator for ModelConfigValidator {
    fn validate(
        &self,
        module: ConfigModule,
        document: &ModuleConfigDocument,
    ) -> Result<(), ConfigValidationError> {
        validate_base_document(document)?;
        if module != ConfigModule::Models {
            return Err(ConfigValidationError::Semantic(
                ModelConfigValidationError::WrongModule.to_string(),
            ));
        }
        let registry = registry_from_document(document).map_err(|error| {
            ConfigValidationError::Semantic(ModelConfigValidationError::Decode(error).to_string())
        })?;
        validate_model_registry(&registry)
            .map_err(|error| ConfigValidationError::Semantic(error.to_string()))
    }
}

#[derive(Debug, Error)]
pub enum ModelConfigValidationError {
    #[error("model validator can only validate models.json")]
    WrongModule,
    #[error("models.json payload is invalid: {0}")]
    Decode(serde_json::Error),
    #[error("{collection} key {key} does not match embedded ID {embedded}")]
    MapKeyMismatch { collection: &'static str, key: String, embedded: String },
    #[error("{path} contains duplicate ID {id}")]
    DuplicateOrderEntry { path: String, id: String },
    #[error("{path} is missing configured ID {id}")]
    MissingOrderEntry { path: String, id: String },
    #[error("{path} references unknown ID {id}")]
    UnknownOrderEntry { path: String, id: String },
    #[error("endpoint {endpoint_id} references missing provider {provider_id}")]
    MissingProvider { endpoint_id: EndpointId, provider_id: ProviderId },
    #[error("model {model_profile_id} references missing endpoint {endpoint_id}")]
    MissingEndpoint { model_profile_id: ModelProfileId, endpoint_id: EndpointId },
    #[error(
        "provider {provider_id} endpoint order contains endpoint owned by another provider: {endpoint_id}"
    )]
    EndpointOwnershipMismatch { provider_id: ProviderId, endpoint_id: EndpointId },
    #[error(
        "endpoint {endpoint_id} model order contains model owned by another endpoint: {model_profile_id}"
    )]
    ModelOwnershipMismatch { endpoint_id: EndpointId, model_profile_id: ModelProfileId },
    #[error("endpoint {endpoint_id} has duplicate provider model name {model_name:?}")]
    DuplicateProviderModelName { endpoint_id: EndpointId, model_name: String },
    #[error("{path} must not be blank")]
    BlankValue { path: String },
    #[error("endpoint {endpoint_id} base URL is invalid: {message}")]
    InvalidBaseUrl { endpoint_id: EndpointId, message: String },
    #[error("endpoint {endpoint_id} Models Path is invalid: {message}")]
    InvalidModelsPath { endpoint_id: EndpointId, message: String },
    #[error(
        "endpoint {endpoint_id} auth scheme {auth_scheme:?} is invalid for API format {api_format:?}"
    )]
    InvalidAuthScheme {
        endpoint_id: EndpointId,
        api_format: ModelApiFormat,
        auth_scheme: EndpointAuthScheme,
    },
    #[error("endpoint {endpoint_id} routing field {field} is not valid for its API format")]
    InvalidRoutingField { endpoint_id: EndpointId, field: &'static str },
    #[error("endpoint {endpoint_id} requires a Chat output-token field")]
    MissingChatOutputTokenField { endpoint_id: EndpointId },
    #[error("endpoint {endpoint_id} requires a Chat instruction role")]
    MissingChatInstructionRole { endpoint_id: EndpointId },
    #[error("endpoint {endpoint_id} max_parallel_model_requests must be at least 1")]
    InvalidParallelLimit { endpoint_id: EndpointId },
    #[error("endpoint {endpoint_id} timeout {field} must be greater than zero when configured")]
    InvalidTimeout { endpoint_id: EndpointId, field: &'static str },
    #[error(
        "model {model_profile_id} context field {field} must be greater than zero when configured"
    )]
    InvalidContextLimit { model_profile_id: ModelProfileId, field: &'static str },
    #[error("model {model_profile_id} runtime correction is incomplete or stale")]
    InvalidRuntimeCorrection { model_profile_id: ModelProfileId },
    #[error(
        "model {model_profile_id} stored effective context window is stale; expected {expected}, found {actual}"
    )]
    StaleEffectiveWindow { model_profile_id: ModelProfileId, expected: u64, actual: u64 },
    #[error("endpoint {endpoint_id} compatibility record is bound to a stale fingerprint")]
    StaleCompatibilityFingerprint { endpoint_id: EndpointId },
    #[error(
        "endpoint {endpoint_id} contains compatibility parameter removals, but the v1 removal whitelist is empty"
    )]
    CompatibilityRemovalNotAllowed { endpoint_id: EndpointId },
    #[error(
        "model {model_profile_id} output token limit must be greater than zero when configured"
    )]
    InvalidOutputLimit { model_profile_id: ModelProfileId },
    #[error("model {model_profile_id} has Chat-only overrides on a non-Chat endpoint")]
    InvalidModelChatOverride { model_profile_id: ModelProfileId },
    #[error("endpoint {endpoint_id} revision must be at least 1")]
    InvalidEndpointRevision { endpoint_id: EndpointId },
    #[error("model {model_profile_id} revision must be at least 1")]
    InvalidModelProfileRevision { model_profile_id: ModelProfileId },
    #[error("provider slug is duplicated: {slug}")]
    DuplicateProviderSlug { slug: String },
    #[error("provider {provider_id} has duplicate endpoint slug: {slug}")]
    DuplicateEndpointSlug { provider_id: ProviderId, slug: String },
    #[error("catalog provider is bound to multiple local providers: {catalog_provider_id}")]
    DuplicateCatalogProviderBinding { catalog_provider_id: String },
    #[error("model {model_profile_id} notes are not canonical or contain prohibited characters")]
    InvalidModelNotes { model_profile_id: ModelProfileId },
}

pub fn registry_from_document(
    document: &ModuleConfigDocument,
) -> Result<ModelRegistryConfig, serde_json::Error> {
    if document.values.is_empty() {
        return Ok(ModelRegistryConfig::default());
    }
    serde_json::from_value(serde_json::to_value(&document.values)?)
}

pub fn registry_values(
    registry: &ModelRegistryConfig,
) -> Result<BTreeMap<String, serde_json::Value>, serde_json::Error> {
    serde_json::from_value(serde_json::to_value(registry)?)
}

pub fn validate_model_registry(
    registry: &ModelRegistryConfig,
) -> Result<(), ModelConfigValidationError> {
    validate_map_ids(registry)?;
    validate_order("provider_order", &registry.provider_order, registry.providers.keys().copied())?;

    let mut provider_slugs = BTreeSet::new();
    let mut catalog_provider_ids = BTreeSet::new();

    for provider in registry.providers.values() {
        require_nonblank("provider.display_name", &provider.display_name)?;
        if !provider_slugs.insert(provider.slug.as_str()) {
            return Err(ModelConfigValidationError::DuplicateProviderSlug {
                slug: provider.slug.to_string(),
            });
        }
        let catalog_provider_id = match &provider.source {
            ModelProviderSource::ModelsDev { catalog_provider_id } => Some(catalog_provider_id),
            ModelProviderSource::Custom { catalog_provider_id } => catalog_provider_id.as_ref(),
        };
        if let Some(catalog_provider_id) = catalog_provider_id
            && !catalog_provider_ids.insert(catalog_provider_id)
        {
            return Err(ModelConfigValidationError::DuplicateCatalogProviderBinding {
                catalog_provider_id: catalog_provider_id.clone(),
            });
        }
        let owned = registry
            .endpoints
            .values()
            .filter(|endpoint| endpoint.provider_id == provider.id)
            .map(|endpoint| endpoint.id);
        validate_order(
            &format!("providers.{}.endpoint_order", provider.id),
            &provider.endpoint_order,
            owned,
        )?;
        for endpoint_id in &provider.endpoint_order {
            let endpoint = registry.endpoints.get(endpoint_id).ok_or_else(|| {
                ModelConfigValidationError::UnknownOrderEntry {
                    path: format!("providers.{}.endpoint_order", provider.id),
                    id: endpoint_id.to_string(),
                }
            })?;
            if endpoint.provider_id != provider.id {
                return Err(ModelConfigValidationError::EndpointOwnershipMismatch {
                    provider_id: provider.id,
                    endpoint_id: *endpoint_id,
                });
            }
        }
        // A default or backup connection is one of this provider's own.
        for endpoint_id in provider.default_endpoint_id.iter().chain(&provider.backup_endpoint_ids)
        {
            if registry.endpoints.get(endpoint_id).is_none_or(|e| e.provider_id != provider.id) {
                return Err(ModelConfigValidationError::EndpointOwnershipMismatch {
                    provider_id: provider.id,
                    endpoint_id: *endpoint_id,
                });
            }
        }
        let mut endpoint_slugs = BTreeSet::new();
        for endpoint_id in &provider.endpoint_order {
            let endpoint = &registry.endpoints[endpoint_id];
            if !endpoint_slugs.insert(endpoint.slug.as_str()) {
                return Err(ModelConfigValidationError::DuplicateEndpointSlug {
                    provider_id: provider.id,
                    slug: endpoint.slug.to_string(),
                });
            }
        }
    }

    for endpoint in registry.endpoints.values() {
        validate_endpoint(registry, endpoint)?;
    }
    for model in registry.models.values() {
        validate_model(registry, model)?;
    }
    Ok(())
}

fn validate_map_ids(registry: &ModelRegistryConfig) -> Result<(), ModelConfigValidationError> {
    for (key, provider) in &registry.providers {
        if *key != provider.id {
            return Err(ModelConfigValidationError::MapKeyMismatch {
                collection: "providers",
                key: key.to_string(),
                embedded: provider.id.to_string(),
            });
        }
    }
    for (key, endpoint) in &registry.endpoints {
        if *key != endpoint.id {
            return Err(ModelConfigValidationError::MapKeyMismatch {
                collection: "endpoints",
                key: key.to_string(),
                embedded: endpoint.id.to_string(),
            });
        }
    }
    for (key, model) in &registry.models {
        if *key != model.id {
            return Err(ModelConfigValidationError::MapKeyMismatch {
                collection: "models",
                key: key.to_string(),
                embedded: model.id.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_endpoint(
    registry: &ModelRegistryConfig,
    endpoint: &ModelEndpointConfig,
) -> Result<(), ModelConfigValidationError> {
    if endpoint.revision.value() == 0 {
        return Err(ModelConfigValidationError::InvalidEndpointRevision {
            endpoint_id: endpoint.id,
        });
    }
    if !registry.providers.contains_key(&endpoint.provider_id) {
        return Err(ModelConfigValidationError::MissingProvider {
            endpoint_id: endpoint.id,
            provider_id: endpoint.provider_id,
        });
    }
    require_nonblank("endpoint.display_name", &endpoint.display_name)?;
    require_nonblank("endpoint.base_url", &endpoint.base_url)?;
    validate_endpoint_protocol(endpoint)?;
    let fingerprint = endpoint_fingerprint(endpoint).map_err(|error| {
        ModelConfigValidationError::InvalidBaseUrl {
            endpoint_id: endpoint.id,
            message: error.to_string(),
        }
    })?;
    if !matches!(endpoint.discovery_mode, ModelDiscoveryMode::Disabled) {
        resolve_models_url(&endpoint.base_url, &endpoint.discovery_mode).map_err(|error| {
            ModelConfigValidationError::InvalidModelsPath {
                endpoint_id: endpoint.id,
                message: error.to_string(),
            }
        })?;
    }
    if endpoint.max_parallel_model_requests == 0 {
        return Err(ModelConfigValidationError::InvalidParallelLimit { endpoint_id: endpoint.id });
    }
    for (field, value) in [
        ("connect_ms", endpoint.timeouts.connect_ms),
        ("first_event_ms", endpoint.timeouts.first_event_ms),
        ("stream_idle_ms", endpoint.timeouts.stream_idle_ms),
    ] {
        if value == 0 {
            return Err(ModelConfigValidationError::InvalidTimeout {
                endpoint_id: endpoint.id,
                field,
            });
        }
    }
    if endpoint.timeouts.total_ms == Some(0) {
        return Err(ModelConfigValidationError::InvalidTimeout {
            endpoint_id: endpoint.id,
            field: "total_ms",
        });
    }
    if endpoint
        .compatibility
        .as_ref()
        .is_some_and(|compatibility| compatibility.endpoint_fingerprint != fingerprint)
    {
        return Err(ModelConfigValidationError::StaleCompatibilityFingerprint {
            endpoint_id: endpoint.id,
        });
    }
    if endpoint
        .compatibility
        .as_ref()
        .is_some_and(|compatibility| !compatibility.unsupported_parameters.is_empty())
    {
        return Err(ModelConfigValidationError::CompatibilityRemovalNotAllowed {
            endpoint_id: endpoint.id,
        });
    }
    let owned = registry
        .models
        .values()
        .filter(|model| model.endpoint_id == endpoint.id)
        .map(|model| model.id);
    validate_order(
        &format!("endpoints.{}.model_order", endpoint.id),
        &endpoint.model_order,
        owned,
    )?;
    let mut names = BTreeSet::new();
    for model_id in &endpoint.model_order {
        let model = registry.models.get(model_id).ok_or_else(|| {
            ModelConfigValidationError::UnknownOrderEntry {
                path: format!("endpoints.{}.model_order", endpoint.id),
                id: model_id.to_string(),
            }
        })?;
        if model.endpoint_id != endpoint.id {
            return Err(ModelConfigValidationError::ModelOwnershipMismatch {
                endpoint_id: endpoint.id,
                model_profile_id: *model_id,
            });
        }
        if !names.insert(model.provider_model_name.clone()) {
            return Err(ModelConfigValidationError::DuplicateProviderModelName {
                endpoint_id: endpoint.id,
                model_name: model.provider_model_name.clone(),
            });
        }
    }
    Ok(())
}

fn validate_endpoint_protocol(
    endpoint: &ModelEndpointConfig,
) -> Result<(), ModelConfigValidationError> {
    match endpoint.api_format {
        ModelApiFormat::OpenAiChatCompletions => {
            if endpoint.chat_output_token_field.is_none() {
                return Err(ModelConfigValidationError::MissingChatOutputTokenField {
                    endpoint_id: endpoint.id,
                });
            }
            if endpoint.chat_instruction_role.is_none() {
                return Err(ModelConfigValidationError::MissingChatInstructionRole {
                    endpoint_id: endpoint.id,
                });
            }
        }
        ModelApiFormat::OpenAiResponses | ModelApiFormat::AnthropicMessages => {
            if endpoint.chat_output_token_field.is_some() {
                return Err(ModelConfigValidationError::InvalidRoutingField {
                    endpoint_id: endpoint.id,
                    field: "chat_output_token_field",
                });
            }
            if endpoint.chat_instruction_role.is_some() {
                return Err(ModelConfigValidationError::InvalidRoutingField {
                    endpoint_id: endpoint.id,
                    field: "chat_instruction_role",
                });
            }
        }
    }
    for (field, value) in [
        ("openai_organization_id", endpoint.openai_organization_id.as_deref()),
        ("openai_project_id", endpoint.openai_project_id.as_deref()),
        ("anthropic_workspace_id", endpoint.anthropic_workspace_id.as_deref()),
    ] {
        if value.is_some_and(|value| value.trim().is_empty()) {
            return Err(ModelConfigValidationError::BlankValue {
                path: format!("endpoint.{field}"),
            });
        }
    }

    match endpoint.api_format {
        ModelApiFormat::OpenAiResponses | ModelApiFormat::OpenAiChatCompletions => {
            if endpoint.auth_scheme == EndpointAuthScheme::AnthropicApiKey {
                return Err(ModelConfigValidationError::InvalidAuthScheme {
                    endpoint_id: endpoint.id,
                    api_format: endpoint.api_format,
                    auth_scheme: endpoint.auth_scheme,
                });
            }
            if endpoint.anthropic_workspace_id.is_some() {
                return Err(ModelConfigValidationError::InvalidRoutingField {
                    endpoint_id: endpoint.id,
                    field: "anthropic_workspace_id",
                });
            }
        }
        ModelApiFormat::AnthropicMessages => {
            if endpoint.auth_scheme == EndpointAuthScheme::OpenAiBearerApiKey {
                return Err(ModelConfigValidationError::InvalidAuthScheme {
                    endpoint_id: endpoint.id,
                    api_format: endpoint.api_format,
                    auth_scheme: endpoint.auth_scheme,
                });
            }
            for (field, value) in [
                ("openai_organization_id", endpoint.openai_organization_id.as_ref()),
                ("openai_project_id", endpoint.openai_project_id.as_ref()),
            ] {
                if value.is_some() {
                    return Err(ModelConfigValidationError::InvalidRoutingField {
                        endpoint_id: endpoint.id,
                        field,
                    });
                }
            }
        }
    }

    Ok(())
}

fn validate_model(
    registry: &ModelRegistryConfig,
    model: &aura_model_domain::ModelProfileConfig,
) -> Result<(), ModelConfigValidationError> {
    if model.revision.value() == 0 {
        return Err(ModelConfigValidationError::InvalidModelProfileRevision {
            model_profile_id: model.id,
        });
    }
    let endpoint = registry.endpoints.get(&model.endpoint_id).ok_or(
        ModelConfigValidationError::MissingEndpoint {
            model_profile_id: model.id,
            endpoint_id: model.endpoint_id,
        },
    )?;
    require_nonblank("model.provider_model_name", &model.provider_model_name)?;
    require_nonblank("model.display_name", &model.display_name)?;
    if model.notes.as_ref().is_some_and(|notes| {
        notes.len() > 4_096
            || notes.contains('\r')
            || notes.chars().all(char::is_whitespace)
            || notes.chars().any(|character| {
                matches!(
                    character,
                    '\u{0000}'..='\u{0008}' | '\u{000b}'..='\u{001f}' | '\u{007f}'
                )
            })
    }) {
        return Err(ModelConfigValidationError::InvalidModelNotes { model_profile_id: model.id });
    }
    if model.request_defaults.max_output_tokens == Some(0)
        || model
            .catalog
            .as_ref()
            .is_some_and(|catalog| catalog.snapshot.output_limit_tokens == Some(0))
    {
        return Err(ModelConfigValidationError::InvalidOutputLimit { model_profile_id: model.id });
    }
    if endpoint.api_format != ModelApiFormat::OpenAiChatCompletions
        && (model.request_defaults.chat_output_token_field.is_some()
            || model.request_defaults.chat_instruction_role.is_some())
    {
        return Err(ModelConfigValidationError::InvalidModelChatOverride {
            model_profile_id: model.id,
        });
    }
    // A stored thinking level is never refused; a request sends only what the model takes
    // (`aura_model_domain::level_to_send`).
    for (field, value) in [
        ("catalog_window_tokens", model.context.catalog_window_tokens),
        ("user_override_window_tokens", model.context.user_override_window_tokens),
        ("runtime_corrected_window_tokens", model.context.runtime_corrected_window_tokens),
        ("historical_snapshot_max_tokens", Some(model.context.historical_snapshot_max_tokens)),
        ("runtime_effective_window_tokens", Some(model.context.runtime_effective_window_tokens)),
    ] {
        if value == Some(0) {
            return Err(ModelConfigValidationError::InvalidContextLimit {
                model_profile_id: model.id,
                field,
            });
        }
    }
    let current_endpoint_fingerprint = endpoint_fingerprint(endpoint).map_err(|error| {
        ModelConfigValidationError::InvalidBaseUrl {
            endpoint_id: endpoint.id,
            message: error.to_string(),
        }
    })?;
    let correction_is_current = match (
        model.context.runtime_corrected_window_tokens,
        model.context.runtime_correction_basis.as_ref(),
    ) {
        (None, None) => false,
        (Some(_), Some(basis)) => {
            if basis.endpoint_fingerprint != current_endpoint_fingerprint
                || basis.model_name_hash != model_name_hash(&model.provider_model_name)
                || basis.user_override_revision != model.context.user_override_revision
            {
                return Err(ModelConfigValidationError::InvalidRuntimeCorrection {
                    model_profile_id: model.id,
                });
            }
            true
        }
        _ => {
            return Err(ModelConfigValidationError::InvalidRuntimeCorrection {
                model_profile_id: model.id,
            });
        }
    };
    let expected = model.context.effective_window_tokens(correction_is_current);
    if expected == 0 {
        return Err(ModelConfigValidationError::InvalidContextLimit {
            model_profile_id: model.id,
            field: "runtime_effective_window_tokens",
        });
    }
    if model.context.runtime_effective_window_tokens != expected {
        return Err(ModelConfigValidationError::StaleEffectiveWindow {
            model_profile_id: model.id,
            expected,
            actual: model.context.runtime_effective_window_tokens,
        });
    }
    Ok(())
}

fn validate_order<T: Copy + Ord + ToString>(
    path: &str,
    order: &[T],
    configured: impl IntoIterator<Item = T>,
) -> Result<(), ModelConfigValidationError> {
    let configured = configured.into_iter().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for id in order {
        if !seen.insert(*id) {
            return Err(ModelConfigValidationError::DuplicateOrderEntry {
                path: path.to_owned(),
                id: id.to_string(),
            });
        }
        if !configured.contains(id) {
            return Err(ModelConfigValidationError::UnknownOrderEntry {
                path: path.to_owned(),
                id: id.to_string(),
            });
        }
    }
    if let Some(missing) = configured.difference(&seen).next() {
        return Err(ModelConfigValidationError::MissingOrderEntry {
            path: path.to_owned(),
            id: missing.to_string(),
        });
    }
    Ok(())
}

fn require_nonblank(path: &str, value: &str) -> Result<(), ModelConfigValidationError> {
    if value.trim().is_empty() {
        return Err(ModelConfigValidationError::BlankValue { path: path.to_owned() });
    }
    Ok(())
}
