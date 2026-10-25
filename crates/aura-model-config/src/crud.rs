use aura_domain_common::{EndpointId, ModelProfileId, ProviderId, Revision, UtcTimestamp};
use aura_model_domain::{
    ChatOutputTokenField, ChatOutputTokenFieldAlias, CreateModelEndpointInput,
    CreateModelProviderInput, DeleteModelConfigResult, EndpointCompatibilityOverrides,
    EndpointSlug, ModelApiFormat, ModelEndpointConfig, ModelProviderSource, ModelRegistryConfig,
    ModelRequestDefaults, ProviderSlug, UpdateModelEndpointInput, normalize_slug_stem,
    suffix_slug_for_collision,
};

use crate::{ModelConfigError, endpoint_fingerprint};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LearnChatOutputTokenFieldAliasResult {
    Learned(ChatOutputTokenFieldAlias),
    AlreadyPresent(ChatOutputTokenFieldAlias),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateModelProfileInput {
    pub model_profile_id: ModelProfileId,
    pub display_name: String,
    pub notes: Option<String>,
    pub enabled: bool,
    pub request_defaults: ModelRequestDefaults,
    pub historical_snapshot_max_tokens: u64,
}

pub fn learn_chat_output_token_field_alias(
    registry: &mut ModelRegistryConfig,
    endpoint_id: EndpointId,
    provider_model_name: &str,
    configured_field: ChatOutputTokenField,
    effective_field: ChatOutputTokenField,
    learned_from_transport_attempt_id: aura_domain_common::TransportAttemptId,
    learned_at: UtcTimestamp,
) -> Result<LearnChatOutputTokenFieldAliasResult, ModelConfigError> {
    if configured_field == effective_field {
        return Err(ModelConfigError::InvalidOutputTokenFieldAlias);
    }
    let profile = registry
        .models
        .values()
        .find(|profile| {
            profile.endpoint_id == endpoint_id && profile.provider_model_name == provider_model_name
        })
        .ok_or_else(|| ModelConfigError::OutputTokenAliasProfileNotFound {
            endpoint_id,
            provider_model_name: provider_model_name.to_owned(),
        })?;
    let endpoint = registry
        .endpoints
        .get_mut(&endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
    if endpoint.api_format != ModelApiFormat::OpenAiChatCompletions {
        return Err(ModelConfigError::OutputTokenAliasFormatMismatch(endpoint_id));
    }
    let actual_configured_field = profile
        .request_defaults
        .chat_output_token_field
        .or(endpoint.chat_output_token_field)
        .ok_or(ModelConfigError::OutputTokenAliasConfiguredFieldMissing(endpoint_id))?;
    if actual_configured_field != configured_field {
        return Err(ModelConfigError::OutputTokenAliasConfiguredFieldChanged(endpoint_id));
    }

    if let Some(existing) = endpoint
        .compatibility
        .as_ref()
        .and_then(|compatibility| {
            compatibility
                .output_token_field_aliases
                .iter()
                .find(|alias| alias.provider_model_name == provider_model_name)
        })
        .cloned()
    {
        if existing.configured_field == configured_field
            && existing.effective_field == effective_field
        {
            return Ok(LearnChatOutputTokenFieldAliasResult::AlreadyPresent(existing));
        }
        return Err(ModelConfigError::OutputTokenFieldAliasConflict {
            endpoint_id,
            provider_model_name: provider_model_name.to_owned(),
        });
    }

    let fingerprint = endpoint_fingerprint(endpoint)?;
    let alias = ChatOutputTokenFieldAlias {
        provider_model_name: provider_model_name.to_owned(),
        configured_field,
        effective_field,
        learned_at,
        learned_from_transport_attempt_id,
    };
    let compatibility =
        endpoint.compatibility.get_or_insert_with(|| EndpointCompatibilityOverrides {
            endpoint_fingerprint: fingerprint.clone(),
            unsupported_parameters: Default::default(),
            unsupported_features: Default::default(),
            output_token_field_aliases: Vec::new(),
            learned_at: Some(learned_at),
        });
    if compatibility.endpoint_fingerprint != fingerprint {
        return Err(ModelConfigError::OutputTokenAliasEndpointFingerprintChanged(endpoint_id));
    }
    compatibility.output_token_field_aliases.push(alias.clone());
    compatibility.output_token_field_aliases.sort_by(|left, right| {
        left.provider_model_name.as_bytes().cmp(right.provider_model_name.as_bytes())
    });
    compatibility.learned_at = Some(learned_at);
    endpoint.updated_at = learned_at;
    Ok(LearnChatOutputTokenFieldAliasResult::Learned(alias))
}

pub fn create_provider(
    registry: &mut ModelRegistryConfig,
    input: CreateModelProviderInput,
    created_at: UtcTimestamp,
) -> Result<ProviderId, ModelConfigError> {
    let provider_id = ProviderId::new();
    let preferred = match &input.source {
        ModelProviderSource::ModelsDev { catalog_provider_id } => catalog_provider_id,
        ModelProviderSource::Custom { catalog_provider_id: Some(catalog_provider_id) } => {
            catalog_provider_id
        }
        ModelProviderSource::Custom { catalog_provider_id: None } => &input.display_name,
    };
    let stem = normalize_slug_stem(preferred, "provider");
    let slug = allocate_provider_slug(registry, &stem, provider_id)?;
    registry.providers.insert(
        provider_id,
        aura_model_domain::ModelProviderConfig {
            id: provider_id,
            display_name: input.display_name,
            slug,
            source: input.source,
            endpoint_order: Vec::new(),
            created_at,
            updated_at: created_at,
            default_endpoint_id: None,
            backup_endpoint_ids: vec![],
        },
    );
    registry.provider_order.push(provider_id);
    Ok(provider_id)
}

pub fn rename_provider(
    registry: &mut ModelRegistryConfig,
    provider_id: ProviderId,
    display_name: String,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    let provider = registry
        .providers
        .get_mut(&provider_id)
        .ok_or(ModelConfigError::ProviderNotFoundById(provider_id))?;
    provider.display_name = display_name;
    provider.updated_at = updated_at;
    Ok(())
}

/// Which
/// of the provider's own connections its models use, and which stand in, in order, when the one used
/// fails before answering. Each connection is named once, and only this provider's.
pub fn set_provider_connections(
    registry: &mut ModelRegistryConfig,
    provider_id: ProviderId,
    default_endpoint_id: Option<EndpointId>,
    backup_endpoint_ids: Vec<EndpointId>,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    let own = |id: &EndpointId| {
        registry.endpoints.get(id).is_some_and(|endpoint| endpoint.provider_id == provider_id)
    };
    let mut named = default_endpoint_id.into_iter().chain(backup_endpoint_ids.iter().copied());
    let mut seen = std::collections::BTreeSet::new();
    if !named.all(|id| own(&id) && seen.insert(id)) {
        return Err(ModelConfigError::InvalidProviderConnections(provider_id));
    }
    let provider = registry
        .providers
        .get_mut(&provider_id)
        .ok_or(ModelConfigError::ProviderNotFoundById(provider_id))?;
    provider.default_endpoint_id = default_endpoint_id;
    provider.backup_endpoint_ids = backup_endpoint_ids;
    provider.updated_at = updated_at;
    Ok(())
}

pub fn create_endpoint(
    registry: &mut ModelRegistryConfig,
    input: CreateModelEndpointInput,
    created_at: UtcTimestamp,
) -> Result<EndpointId, ModelConfigError> {
    if !registry.providers.contains_key(&input.provider_id) {
        return Err(ModelConfigError::ProviderNotFoundById(input.provider_id));
    }
    let endpoint_id = EndpointId::new();
    let stem = normalize_slug_stem(&input.display_name, "endpoint");
    let slug = allocate_endpoint_slug(registry, input.provider_id, &stem, endpoint_id)?;
    registry.endpoints.insert(
        endpoint_id,
        ModelEndpointConfig {
            id: endpoint_id,
            revision: Revision::INITIAL,
            provider_id: input.provider_id,
            display_name: input.display_name,
            slug,
            api_format: input.api_format,
            base_url: input.base_url,
            discovery_mode: input.discovery_mode,
            chat_output_token_field: input.chat_output_token_field,
            chat_instruction_role: input.chat_instruction_role,
            auth_scheme: input.auth_scheme,
            openai_organization_id: input.openai_organization_id,
            openai_project_id: input.openai_project_id,
            anthropic_workspace_id: input.anthropic_workspace_id,
            timeouts: input.timeouts,
            max_parallel_model_requests: input.max_parallel_model_requests,
            model_order: Vec::new(),
            compatibility: None,
            created_at,
            updated_at: created_at,
        },
    );
    registry
        .providers
        .get_mut(&input.provider_id)
        .ok_or(ModelConfigError::ProviderNotFoundById(input.provider_id))?
        .endpoint_order
        .push(endpoint_id);
    Ok(endpoint_id)
}

pub fn update_endpoint(
    registry: &mut ModelRegistryConfig,
    input: UpdateModelEndpointInput,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    let endpoint = registry
        .endpoints
        .get_mut(&input.endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(input.endpoint_id))?;
    let old_fingerprint = endpoint_fingerprint(endpoint)?;
    endpoint.display_name = input.display_name;
    endpoint.api_format = input.api_format;
    endpoint.base_url = input.base_url;
    endpoint.discovery_mode = input.discovery_mode;
    endpoint.chat_output_token_field = input.chat_output_token_field;
    endpoint.chat_instruction_role = input.chat_instruction_role;
    endpoint.auth_scheme = input.auth_scheme;
    endpoint.openai_organization_id = input.openai_organization_id;
    endpoint.openai_project_id = input.openai_project_id;
    endpoint.anthropic_workspace_id = input.anthropic_workspace_id;
    endpoint.timeouts = input.timeouts;
    endpoint.max_parallel_model_requests = input.max_parallel_model_requests;
    endpoint.updated_at = updated_at;
    if endpoint_fingerprint(endpoint)? != old_fingerprint {
        endpoint.compatibility = None;
        for model in
            registry.models.values_mut().filter(|model| model.endpoint_id == input.endpoint_id)
        {
            model.context.runtime_corrected_window_tokens = None;
            model.context.runtime_correction_basis = None;
            model.context.runtime_effective_window_tokens =
                model.context.effective_window_tokens(false);
            model.updated_at = updated_at;
        }
    }
    Ok(())
}

pub fn rename_model_profile(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    display_name: String,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    let model = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    model.display_name = display_name;
    model.updated_at = updated_at;
    Ok(())
}

pub fn update_model_profile(
    registry: &mut ModelRegistryConfig,
    input: UpdateModelProfileInput,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    if input.historical_snapshot_max_tokens == 0 {
        return Err(ModelConfigError::InvalidHistoricalSnapshotLimit);
    }
    let (endpoint_id, provider_model_name, previous_output_token_field) = registry
        .models
        .get(&input.model_profile_id)
        .map(|model| {
            (
                model.endpoint_id,
                model.provider_model_name.clone(),
                model.request_defaults.chat_output_token_field,
            )
        })
        .ok_or(ModelConfigError::ProfileNotFound(input.model_profile_id))?;
    let configured_output_token_field_changed =
        previous_output_token_field != input.request_defaults.chat_output_token_field;
    let model = registry
        .models
        .get_mut(&input.model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(input.model_profile_id))?;
    model.display_name = input.display_name;
    model.notes = normalize_model_notes(input.notes)?;
    model.enabled = input.enabled;
    model.request_defaults = input.request_defaults;
    model.context.historical_snapshot_max_tokens = input.historical_snapshot_max_tokens;
    model.updated_at = updated_at;
    if configured_output_token_field_changed {
        let endpoint = registry
            .endpoints
            .get_mut(&endpoint_id)
            .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
        if let Some(compatibility) = endpoint.compatibility.as_mut() {
            let previous_len = compatibility.output_token_field_aliases.len();
            compatibility
                .output_token_field_aliases
                .retain(|alias| alias.provider_model_name != provider_model_name);
            if compatibility.output_token_field_aliases.len() != previous_len {
                endpoint.updated_at = updated_at;
                if compatibility.unsupported_parameters.is_empty()
                    && compatibility.unsupported_features.is_empty()
                    && compatibility.output_token_field_aliases.is_empty()
                {
                    endpoint.compatibility = None;
                }
            }
        }
    }
    Ok(())
}

pub fn normalize_model_notes(notes: Option<String>) -> Result<Option<String>, ModelConfigError> {
    let Some(notes) = notes else {
        return Ok(None);
    };
    let normalized = notes.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.len() > 4_096 {
        return Err(ModelConfigError::ModelNotesTooLarge);
    }
    if normalized.chars().any(|character| {
        matches!(character, '\u{0000}'..='\u{0008}' | '\u{000b}'..='\u{001f}' | '\u{007f}')
    }) {
        return Err(ModelConfigError::ModelNotesInvalidCharacter);
    }
    if normalized.chars().all(char::is_whitespace) { Ok(None) } else { Ok(Some(normalized)) }
}

fn allocate_provider_slug(
    registry: &ModelRegistryConfig,
    stem: &str,
    provider_id: ProviderId,
) -> Result<ProviderSlug, ModelConfigError> {
    if !registry.providers.values().any(|provider| provider.slug.as_str() == stem) {
        return stem.parse().map_err(|_| {
            ModelConfigError::InternalInvariant("generated provider slug is invalid")
        });
    }
    suffix_slug_for_collision(stem, &provider_id.ulid().to_string())
        .parse()
        .map_err(|_| ModelConfigError::InternalInvariant("generated provider slug is invalid"))
}

fn allocate_endpoint_slug(
    registry: &ModelRegistryConfig,
    provider_id: ProviderId,
    stem: &str,
    endpoint_id: EndpointId,
) -> Result<EndpointSlug, ModelConfigError> {
    let collision = registry
        .providers
        .get(&provider_id)
        .ok_or(ModelConfigError::ProviderNotFoundById(provider_id))?
        .endpoint_order
        .iter()
        .filter_map(|id| registry.endpoints.get(id))
        .any(|endpoint| endpoint.slug.as_str() == stem);
    let slug = if collision {
        suffix_slug_for_collision(stem, &endpoint_id.ulid().to_string())
    } else {
        stem.to_owned()
    };
    slug.parse()
        .map_err(|_| ModelConfigError::InternalInvariant("generated endpoint slug is invalid"))
}

pub fn reset_endpoint_compatibility(
    registry: &mut ModelRegistryConfig,
    endpoint_id: EndpointId,
    updated_at: UtcTimestamp,
) -> Result<Option<EndpointCompatibilityOverrides>, ModelConfigError> {
    let endpoint = registry
        .endpoints
        .get_mut(&endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
    let previous = endpoint.compatibility.take();
    if previous.is_some() {
        endpoint.updated_at = updated_at;
    }
    Ok(previous)
}

pub fn set_model_subagent_availability(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    available: bool,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    let model = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    if available && model.effective_max_output_tokens().is_none_or(|value| value == 0) {
        return Err(ModelConfigError::OutputLimitConfigurationRequired(model_profile_id));
    }
    model.available_for_subagents = available;
    model.updated_at = updated_at;
    Ok(())
}

pub fn set_provider_current_models_subagent_availability(
    registry: &mut ModelRegistryConfig,
    provider_id: ProviderId,
    available: bool,
    updated_at: UtcTimestamp,
) -> Result<Vec<ModelProfileId>, ModelConfigError> {
    let endpoint_ids = registry
        .providers
        .get(&provider_id)
        .ok_or(ModelConfigError::ProviderNotFoundById(provider_id))?
        .endpoint_order
        .clone();
    let model_ids = endpoint_ids
        .iter()
        .map(|endpoint_id| {
            registry
                .endpoints
                .get(endpoint_id)
                .ok_or(ModelConfigError::EndpointIdNotFound(*endpoint_id))
                .map(|endpoint| endpoint.model_order.clone())
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if available {
        for model_id in &model_ids {
            let model = registry
                .models
                .get(model_id)
                .ok_or(ModelConfigError::ProfileNotFound(*model_id))?;
            if model.effective_max_output_tokens().is_none_or(|value| value == 0) {
                return Err(ModelConfigError::OutputLimitConfigurationRequired(*model_id));
            }
        }
    }

    let mut changed = Vec::with_capacity(model_ids.len());
    for model_id in model_ids {
        let model = registry
            .models
            .get_mut(&model_id)
            .ok_or(ModelConfigError::ProfileNotFound(model_id))?;
        if model.available_for_subagents == available {
            continue;
        }
        model.available_for_subagents = available;
        model.updated_at = updated_at;
        changed.push(model_id);
    }
    Ok(changed)
}

pub fn delete_model_profile(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
) -> Result<DeleteModelConfigResult, ModelConfigError> {
    let model = registry
        .models
        .remove(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    registry
        .endpoints
        .get_mut(&model.endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(model.endpoint_id))?
        .model_order
        .retain(|id| *id != model_profile_id);
    Ok(DeleteModelConfigResult {
        deleted_provider_ids: Vec::new(),
        deleted_endpoint_ids: Vec::new(),
        deleted_model_profile_ids: vec![model_profile_id],
    })
}

pub fn delete_endpoint(
    registry: &mut ModelRegistryConfig,
    endpoint_id: EndpointId,
) -> Result<DeleteModelConfigResult, ModelConfigError> {
    let endpoint = registry
        .endpoints
        .remove(&endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
    let provider = registry
        .providers
        .get_mut(&endpoint.provider_id)
        .ok_or(ModelConfigError::ProviderNotFoundById(endpoint.provider_id))?;
    provider.endpoint_order.retain(|id| *id != endpoint_id);
    // A deleted connection is no one's default or backup.
    if provider.default_endpoint_id == Some(endpoint_id) {
        provider.default_endpoint_id = None;
    }
    provider.backup_endpoint_ids.retain(|id| *id != endpoint_id);
    for model_id in &endpoint.model_order {
        registry.models.remove(model_id);
    }
    Ok(DeleteModelConfigResult {
        deleted_provider_ids: Vec::new(),
        deleted_endpoint_ids: vec![endpoint_id],
        deleted_model_profile_ids: endpoint.model_order,
    })
}

pub fn delete_provider(
    registry: &mut ModelRegistryConfig,
    provider_id: ProviderId,
) -> Result<DeleteModelConfigResult, ModelConfigError> {
    let provider = registry
        .providers
        .remove(&provider_id)
        .ok_or(ModelConfigError::ProviderNotFoundById(provider_id))?;
    registry.provider_order.retain(|id| *id != provider_id);
    let mut deleted_model_profile_ids = Vec::new();
    for endpoint_id in &provider.endpoint_order {
        let endpoint = registry
            .endpoints
            .remove(endpoint_id)
            .ok_or(ModelConfigError::EndpointIdNotFound(*endpoint_id))?;
        for model_id in &endpoint.model_order {
            registry.models.remove(model_id);
        }
        deleted_model_profile_ids.extend(endpoint.model_order);
    }
    Ok(DeleteModelConfigResult {
        deleted_provider_ids: vec![provider_id],
        deleted_endpoint_ids: provider.endpoint_order,
        deleted_model_profile_ids,
    })
}
