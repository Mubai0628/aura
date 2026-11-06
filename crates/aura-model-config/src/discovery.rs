use std::collections::BTreeSet;

use aura_domain_common::{ContentHash, EndpointId, ModelProfileId, Revision, UtcTimestamp};
use aura_model_catalog::match_catalog_model;
use aura_model_domain::{
    AddManualModelInput, CatalogMatchState, DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS,
    DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS, ManualModelCreateResult, ModelCatalogMetadata,
    ModelContextWindowConfig, ModelDiscoveryMergeResult, ModelDiscoveryProfileResult,
    ModelProfileConfig, ModelProfileDiscoveryState, ModelProfileOrigin, ModelProviderSource,
    ModelRegistryConfig, ModelRequestDefaults, ProviderCatalogMatch, ProviderCatalogMatchCandidate,
    ProviderCatalogSnapshot,
};
use aura_provider_profile::{ModelRoute, connection_takes_model, route_for_catalog_model};

use crate::{ModelConfigError, catalog_choice::catalog_scope, crud::normalize_model_notes};

pub fn merge_discovery_results(
    registry: &mut ModelRegistryConfig,
    endpoint_id: EndpointId,
    provider_model_names: &[String],
    catalog: Option<&ProviderCatalogSnapshot>,
    discovered_at: UtcTimestamp,
) -> Result<ModelDiscoveryMergeResult, ModelConfigError> {
    let endpoint = registry
        .endpoints
        .get(&endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
    let provider = registry
        .providers
        .get(&endpoint.provider_id)
        .ok_or(ModelConfigError::ProviderForEndpointNotFound(endpoint_id))?;
    let catalog_provider_id = catalog_scope(provider, &endpoint.base_url, catalog);
    // A Provider whose models go by their own routes: a new model joins this connection only
    // when it goes by its route, a model the catalog does not list going by the Provider's own.
    let routed_provider = catalog
        .zip(catalog_provider_id)
        .and_then(|(snapshot, catalog_provider_id)| snapshot.providers.get(catalog_provider_id));
    let (base_url, api_format) = (endpoint.base_url.clone(), endpoint.api_format);
    let existing_ids = endpoint.model_order.clone();
    let mut existing_by_name = existing_ids
        .iter()
        .filter_map(|id| {
            registry.models.get(id).map(|model| (model.provider_model_name.clone(), *id))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut returned_names = BTreeSet::new();
    let mut returned_profiles = Vec::with_capacity(provider_model_names.len());
    let mut created_ids = Vec::new();

    for provider_model_name in provider_model_names {
        if provider_model_name.trim().is_empty() {
            return Err(ModelConfigError::BlankProviderModelName);
        }
        if !returned_names.insert(provider_model_name.clone()) {
            return Err(ModelConfigError::DuplicateDiscoveredModelName {
                endpoint_id,
                provider_model_name: provider_model_name.clone(),
            });
        }
        let enrichment = catalog
            .map(|snapshot| {
                resolve_catalog_enrichment(
                    snapshot,
                    catalog_provider_id,
                    provider_model_name,
                    discovered_at,
                )
            })
            .transpose()?;
        if let Some(model_profile_id) = existing_by_name.remove(provider_model_name) {
            let model = registry
                .models
                .get_mut(&model_profile_id)
                .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
            let enrichment = match (enrichment, catalog) {
                (Some(found), Some(snapshot)) => {
                    Some(keep_choice(found, model, snapshot, discovered_at)?)
                }
                (found, _) => found,
            };
            let (match_state, needs_manual_confirmation) = if let Some(enrichment) = enrichment {
                apply_enrichment(model, &enrichment);
                (enrichment.match_state, enrichment.needs_manual_confirmation)
            } else {
                let state = model
                    .catalog
                    .as_ref()
                    .map_or(CatalogMatchState::Unmatched, |catalog| catalog.match_state);
                (state, state != CatalogMatchState::Matched)
            };
            model.updated_at = discovered_at;
            returned_profiles.push(ModelDiscoveryProfileResult {
                model_profile_id,
                provider_model_name: provider_model_name.clone(),
                created: false,
                discovery_state: ModelProfileDiscoveryState::ReturnedInLatestDiscovery,
                catalog_match_state: match_state,
                needs_manual_confirmation,
            });
            continue;
        }

        // The catalog says it is an image, embedding or other model that is not for chats.
        if enrichment
            .as_ref()
            .and_then(|found| found.metadata.as_ref())
            .is_some_and(|metadata| !metadata.snapshot.is_chat())
        {
            continue;
        }
        let model_route = enrichment
            .as_ref()
            .and_then(|found| found.metadata.as_ref())
            .and_then(|metadata| metadata.snapshot.route.as_ref());
        if routed_provider.is_some_and(|provider| {
            !connection_takes_model(provider, model_route, &base_url, api_format)
        }) {
            continue;
        }
        let model_profile_id = ModelProfileId::new();
        let enrichment = enrichment.unwrap_or_else(|| CatalogEnrichment::unmatched(None));
        let context_window =
            enrichment.catalog_window_tokens.unwrap_or(DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS);
        registry.models.insert(
            model_profile_id,
            ModelProfileConfig {
                id: model_profile_id,
                revision: Revision::INITIAL,
                endpoint_id,
                provider_model_name: provider_model_name.clone(),
                display_name: enrichment
                    .display_name
                    .clone()
                    .unwrap_or_else(|| provider_model_name.clone()),
                notes: None,
                origin: ModelProfileOrigin::Discovered,
                enabled: true,
                available_for_subagents: false,
                catalog: enrichment.metadata.clone(),
                context: ModelContextWindowConfig {
                    catalog_window_tokens: enrichment.catalog_window_tokens,
                    user_override_window_tokens: None,
                    user_override_revision: 0,
                    runtime_corrected_window_tokens: None,
                    runtime_correction_basis: None,
                    runtime_effective_window_tokens: context_window,
                    historical_snapshot_max_tokens: DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS,
                },
                request_defaults: ModelRequestDefaults::default(),
                created_at: discovered_at,
                updated_at: discovered_at,
            },
        );
        created_ids.push(model_profile_id);
        returned_profiles.push(ModelDiscoveryProfileResult {
            model_profile_id,
            provider_model_name: provider_model_name.clone(),
            created: true,
            discovery_state: ModelProfileDiscoveryState::ReturnedInLatestDiscovery,
            catalog_match_state: enrichment.match_state,
            needs_manual_confirmation: enrichment.needs_manual_confirmation,
        });
    }

    registry
        .endpoints
        .get_mut(&endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?
        .model_order
        .extend(created_ids);
    let not_returned_profile_ids = existing_ids
        .into_iter()
        .filter(|model_id| {
            registry
                .models
                .get(model_id)
                .is_some_and(|model| !returned_names.contains(&model.provider_model_name))
        })
        .collect();
    Ok(ModelDiscoveryMergeResult { endpoint_id, returned_profiles, not_returned_profile_ids })
}

pub fn add_manual_model(
    registry: &mut ModelRegistryConfig,
    input: AddManualModelInput,
    created_at: UtcTimestamp,
) -> Result<ManualModelCreateResult, ModelConfigError> {
    if input.provider_model_name.trim().is_empty() {
        return Err(ModelConfigError::BlankProviderModelName);
    }
    let endpoint = registry
        .endpoints
        .get(&input.endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(input.endpoint_id))?;
    if endpoint.model_order.iter().any(|model_id| {
        registry
            .models
            .get(model_id)
            .is_some_and(|model| model.provider_model_name == input.provider_model_name)
    }) {
        return Err(ModelConfigError::DuplicateProviderModelName {
            endpoint_id: input.endpoint_id,
            provider_model_name: input.provider_model_name,
        });
    }
    if input.context_window_tokens == Some(0) {
        return Err(ModelConfigError::InvalidManualContextWindow);
    }
    let model_profile_id = ModelProfileId::new();
    let needs_manual_confirmation = input.context_window_tokens.is_none();
    let context_window_tokens =
        input.context_window_tokens.unwrap_or(DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS);
    let user_override_revision = u64::from(input.context_window_tokens.is_some());
    let display_name = input
        .display_name
        .filter(|display_name| !display_name.trim().is_empty())
        .unwrap_or_else(|| input.provider_model_name.clone());
    registry.models.insert(
        model_profile_id,
        ModelProfileConfig {
            id: model_profile_id,
            revision: Revision::INITIAL,
            endpoint_id: input.endpoint_id,
            provider_model_name: input.provider_model_name,
            display_name,
            notes: normalize_model_notes(input.notes)?,
            origin: ModelProfileOrigin::Manual,
            enabled: true,
            available_for_subagents: false,
            catalog: None,
            context: ModelContextWindowConfig {
                catalog_window_tokens: None,
                user_override_window_tokens: input.context_window_tokens,
                user_override_revision,
                runtime_corrected_window_tokens: None,
                runtime_correction_basis: None,
                runtime_effective_window_tokens: context_window_tokens,
                historical_snapshot_max_tokens: DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS,
            },
            request_defaults: ModelRequestDefaults::default(),
            created_at,
            updated_at: created_at,
        },
    );
    registry
        .endpoints
        .get_mut(&input.endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(input.endpoint_id))?
        .model_order
        .push(model_profile_id);
    Ok(ManualModelCreateResult { model_profile_id, needs_manual_confirmation })
}

pub fn create_model_from_catalog(
    registry: &mut ModelRegistryConfig,
    endpoint_id: EndpointId,
    catalog: &ProviderCatalogSnapshot,
    catalog_provider_id: &str,
    catalog_model_id: &str,
    expected_catalog_object_hash: &ContentHash,
    created_at: UtcTimestamp,
) -> Result<ModelProfileId, ModelConfigError> {
    let endpoint = registry
        .endpoints
        .get(&endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
    let provider = registry
        .providers
        .get(&endpoint.provider_id)
        .ok_or(ModelConfigError::ProviderForEndpointNotFound(endpoint_id))?;
    let bound_catalog_provider_id = match &provider.source {
        ModelProviderSource::ModelsDev { catalog_provider_id } => Some(catalog_provider_id),
        ModelProviderSource::Custom { catalog_provider_id } => catalog_provider_id.as_ref(),
    };
    if bound_catalog_provider_id.map(String::as_str) != Some(catalog_provider_id) {
        return Err(ModelConfigError::CatalogProviderBindingMismatch {
            catalog_provider_id: catalog_provider_id.to_owned(),
        });
    }
    let catalog_provider = catalog
        .providers
        .get(catalog_provider_id)
        .ok_or_else(|| ModelConfigError::CatalogProviderNotFound(catalog_provider_id.to_owned()))?;
    let catalog_model = catalog_provider.models.get(catalog_model_id).ok_or_else(|| {
        ModelConfigError::CatalogModelNotFound {
            catalog_provider_id: catalog_provider_id.to_owned(),
            catalog_model_id: catalog_model_id.to_owned(),
        }
    })?;
    validate_catalog_object_hash(catalog_model, expected_catalog_object_hash)?;
    if !catalog_model.is_chat() {
        return Err(ModelConfigError::CatalogModelNotChat {
            catalog_provider_id: catalog_provider_id.to_owned(),
            catalog_model_id: catalog_model_id.to_owned(),
        });
    }
    let model_route = catalog_model.route.as_ref();
    if !connection_takes_model(
        catalog_provider,
        model_route,
        &endpoint.base_url,
        endpoint.api_format,
    ) {
        return Err(match route_for_catalog_model(catalog_provider, model_route) {
            ModelRoute::Route(route) => ModelConfigError::CatalogModelOnAnotherRoute {
                catalog_provider_id: catalog_provider_id.to_owned(),
                catalog_model_id: catalog_model_id.to_owned(),
                route_id: route.route_id.clone(),
            },
            ModelRoute::Any | ModelRoute::Unsupported => {
                ModelConfigError::CatalogModelRouteUnsupported {
                    catalog_provider_id: catalog_provider_id.to_owned(),
                    catalog_model_id: catalog_model_id.to_owned(),
                }
            }
        });
    }
    if endpoint.model_order.iter().any(|model_id| {
        registry
            .models
            .get(model_id)
            .is_some_and(|model| model.provider_model_name == catalog_model.catalog_model_id)
    }) {
        return Err(ModelConfigError::DuplicateProviderModelName {
            endpoint_id,
            provider_model_name: catalog_model.catalog_model_id.clone(),
        });
    }

    let model_profile_id = ModelProfileId::new();
    let catalog_metadata = catalog_metadata(
        catalog,
        catalog_provider_id,
        catalog_model,
        expected_catalog_object_hash.clone(),
        created_at,
    );
    let effective_window =
        catalog_model.context_window_tokens.unwrap_or(DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS);
    registry.models.insert(
        model_profile_id,
        ModelProfileConfig {
            id: model_profile_id,
            revision: Revision::INITIAL,
            endpoint_id,
            provider_model_name: catalog_model.catalog_model_id.clone(),
            display_name: catalog_model.display_name.clone(),
            notes: None,
            origin: ModelProfileOrigin::Manual,
            enabled: true,
            available_for_subagents: false,
            catalog: Some(catalog_metadata),
            context: ModelContextWindowConfig {
                catalog_window_tokens: catalog_model.context_window_tokens,
                user_override_window_tokens: None,
                user_override_revision: 0,
                runtime_corrected_window_tokens: None,
                runtime_correction_basis: None,
                runtime_effective_window_tokens: effective_window,
                historical_snapshot_max_tokens: DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS,
            },
            request_defaults: ModelRequestDefaults::default(),
            created_at,
            updated_at: created_at,
        },
    );
    registry
        .endpoints
        .get_mut(&endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?
        .model_order
        .push(model_profile_id);
    Ok(model_profile_id)
}

pub fn apply_catalog_update(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    catalog: &ProviderCatalogSnapshot,
    expected_catalog_object_hash: &ContentHash,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    let profile = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    let current_catalog = profile
        .catalog
        .as_ref()
        .ok_or(ModelConfigError::CatalogSnapshotMissing(model_profile_id))?;
    let catalog_provider_id = current_catalog.catalog_provider_id.clone();
    let catalog_model_id = current_catalog.catalog_model_id.clone();
    let catalog_provider = catalog
        .providers
        .get(&catalog_provider_id)
        .ok_or_else(|| ModelConfigError::CatalogProviderNotFound(catalog_provider_id.clone()))?;
    let catalog_model = catalog_provider.models.get(&catalog_model_id).ok_or_else(|| {
        ModelConfigError::CatalogModelNotFound {
            catalog_provider_id: catalog_provider_id.clone(),
            catalog_model_id: catalog_model_id.clone(),
        }
    })?;
    validate_catalog_object_hash(catalog_model, expected_catalog_object_hash)?;

    let next_catalog = catalog_metadata(
        catalog,
        &catalog_provider_id,
        catalog_model,
        expected_catalog_object_hash.clone(),
        updated_at,
    );
    relink(profile, next_catalog, updated_at);
    Ok(())
}

/// Links `profile` to the catalog entry `next` describes, with the entry's context window.
pub(crate) fn relink(
    profile: &mut ModelProfileConfig,
    next: ModelCatalogMetadata,
    updated_at: UtcTimestamp,
) {
    let window = next.snapshot.context_window_tokens;
    if profile.catalog.as_ref() != Some(&next) || profile.context.catalog_window_tokens != window {
        profile.catalog = Some(next);
        profile.context.catalog_window_tokens = window;
        profile.context.runtime_effective_window_tokens = profile
            .context
            .effective_window_tokens(profile.context.runtime_correction_basis.is_some());
        profile.updated_at = updated_at;
    }
}

/// The hash a catalog entry is listed and checked with.
pub(crate) fn catalog_object_hash(
    model: &aura_model_domain::ProviderCatalogModelEntry,
) -> Result<ContentHash, ModelConfigError> {
    Ok(ContentHash::blake3(&serde_json::to_vec(model)?))
}

pub(crate) fn validate_catalog_object_hash(
    model: &aura_model_domain::ProviderCatalogModelEntry,
    expected: &ContentHash,
) -> Result<(), ModelConfigError> {
    if &catalog_object_hash(model)? != expected {
        return Err(ModelConfigError::CatalogObjectHashMismatch);
    }
    Ok(())
}

pub(crate) fn catalog_metadata(
    catalog: &ProviderCatalogSnapshot,
    catalog_provider_id: &str,
    model: &aura_model_domain::ProviderCatalogModelEntry,
    catalog_object_hash: ContentHash,
    matched_at: UtcTimestamp,
) -> ModelCatalogMetadata {
    ModelCatalogMetadata {
        catalog_provider_id: catalog_provider_id.to_owned(),
        catalog_model_id: model.catalog_model_id.clone(),
        catalog_revision: catalog.catalog_revision,
        catalog_content_hash: catalog.catalog_content_hash.clone(),
        catalog_object_hash,
        snapshot: model.clone(),
        matched_at: Some(matched_at),
        match_state: CatalogMatchState::Matched,
    }
}

#[derive(Clone)]
struct CatalogEnrichment {
    display_name: Option<String>,
    metadata: Option<ModelCatalogMetadata>,
    catalog_window_tokens: Option<u64>,
    match_state: CatalogMatchState,
    needs_manual_confirmation: bool,
    /// The entries an ambiguous name matches.
    candidates: Vec<ProviderCatalogMatchCandidate>,
}

impl CatalogEnrichment {
    fn unmatched(catalog_provider_id: Option<&str>) -> Self {
        let _ = catalog_provider_id;
        Self {
            display_name: None,
            metadata: None,
            catalog_window_tokens: None,
            match_state: CatalogMatchState::Unmatched,
            needs_manual_confirmation: true,
            candidates: Vec::new(),
        }
    }
}

/// An ambiguous name stays linked to the entry the user chose, while it is one of the name's
/// entries; the link then follows the entry as the catalog now lists it.
fn keep_choice(
    found: CatalogEnrichment,
    model: &ModelProfileConfig,
    snapshot: &ProviderCatalogSnapshot,
    matched_at: UtcTimestamp,
) -> Result<CatalogEnrichment, ModelConfigError> {
    match model.catalog.as_ref() {
        Some(link)
            if found.candidates.iter().any(|candidate| {
                candidate.catalog_provider_id == link.catalog_provider_id
                    && candidate.catalog_model_id == link.catalog_model_id
            }) =>
        {
            matched(
                snapshot,
                link.catalog_provider_id.clone(),
                link.catalog_model_id.clone(),
                matched_at,
            )
        }
        _ => Ok(found),
    }
}

fn matched(
    snapshot: &ProviderCatalogSnapshot,
    catalog_provider_id: String,
    catalog_model_id: String,
    matched_at: UtcTimestamp,
) -> Result<CatalogEnrichment, ModelConfigError> {
    let model = &snapshot.providers[&catalog_provider_id].models[&catalog_model_id];
    Ok(CatalogEnrichment {
        display_name: Some(model.display_name.clone()),
        metadata: Some(ModelCatalogMetadata {
            catalog_provider_id,
            catalog_model_id,
            catalog_revision: snapshot.catalog_revision,
            catalog_content_hash: snapshot.catalog_content_hash.clone(),
            catalog_object_hash: catalog_object_hash(model)?,
            snapshot: model.clone(),
            matched_at: Some(matched_at),
            match_state: CatalogMatchState::Matched,
        }),
        catalog_window_tokens: model.context_window_tokens,
        match_state: CatalogMatchState::Matched,
        needs_manual_confirmation: false,
        candidates: Vec::new(),
    })
}

fn resolve_catalog_enrichment(
    snapshot: &ProviderCatalogSnapshot,
    catalog_provider_id: Option<&str>,
    provider_model_name: &str,
    matched_at: UtcTimestamp,
) -> Result<CatalogEnrichment, ModelConfigError> {
    match match_catalog_model(snapshot, catalog_provider_id, provider_model_name) {
        ProviderCatalogMatch::Matched { catalog_provider_id, catalog_model_id } => {
            matched(snapshot, catalog_provider_id, catalog_model_id, matched_at)
        }
        ProviderCatalogMatch::Unmatched => {
            let mut enrichment = CatalogEnrichment::unmatched(catalog_provider_id);
            if let Some(metadata) = enrichment.metadata.as_mut() {
                metadata.matched_at = Some(matched_at);
            }
            Ok(enrichment)
        }
        ProviderCatalogMatch::Ambiguous { candidates } => Ok(CatalogEnrichment {
            display_name: None,
            metadata: None,
            catalog_window_tokens: None,
            match_state: CatalogMatchState::Ambiguous,
            needs_manual_confirmation: true,
            candidates,
        }),
    }
}

fn apply_enrichment(model: &mut ModelProfileConfig, enrichment: &CatalogEnrichment) {
    model.catalog = enrichment.metadata.clone();
    model.context.catalog_window_tokens = enrichment.catalog_window_tokens;
    model.context.runtime_effective_window_tokens =
        model.context.effective_window_tokens(model.context.runtime_correction_basis.is_some());
}
