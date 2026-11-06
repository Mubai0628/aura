//! A model whose name several catalog entries share is linked to the one the user chooses. Only
//! a chat entry the name matches, as discovery matches it, can be chosen, and only as the
//! settings listed it; discovering the model again keeps the choice while the name still matches it.
use aura_domain_common::{ContentHash, ModelProfileId, UtcTimestamp};
use aura_model_catalog::{match_catalog_model, official_catalog_provider};
use aura_model_domain::{
    ModelCatalogEntryView, ModelProfileCatalogEntriesView, ModelProviderConfig,
    ModelProviderSource, ModelRegistryConfig, ProviderCatalogMatch, ProviderCatalogMatchCandidate,
    ProviderCatalogSnapshot,
};

use crate::{
    ModelConfigError,
    discovery::{catalog_metadata, catalog_object_hash, relink, validate_catalog_object_hash},
};

/// The catalog Provider a connection's models are matched against: the one its Provider is bound
/// to, or the one whose own API the connection is. None matches across the whole catalog.
pub(crate) fn catalog_scope<'a>(
    provider: &'a ModelProviderConfig,
    base_url: &str,
    catalog: Option<&'a ProviderCatalogSnapshot>,
) -> Option<&'a str> {
    match &provider.source {
        ModelProviderSource::ModelsDev { catalog_provider_id } => {
            Some(catalog_provider_id.as_str())
        }
        // An unbound custom Provider whose Endpoint is a catalog Provider's own API matches
        // against that Provider only.
        ModelProviderSource::Custom { catalog_provider_id } => catalog_provider_id
            .as_deref()
            .or_else(|| catalog.and_then(|snapshot| official_catalog_provider(snapshot, base_url))),
    }
}

/// The entries a profile's model name matches, as discovery matches it, in the catalog's order.
fn candidates(
    registry: &ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    catalog: &ProviderCatalogSnapshot,
) -> Result<Vec<ProviderCatalogMatchCandidate>, ModelConfigError> {
    let profile = registry
        .models
        .get(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    let endpoint = registry
        .endpoints
        .get(&profile.endpoint_id)
        .ok_or(ModelConfigError::EndpointNotFound(model_profile_id))?;
    let provider = registry
        .providers
        .get(&endpoint.provider_id)
        .ok_or(ModelConfigError::ProviderForEndpointNotFound(endpoint.id))?;
    let scope = catalog_scope(provider, &endpoint.base_url, Some(catalog));
    let mut candidates = match match_catalog_model(catalog, scope, &profile.provider_model_name) {
        ProviderCatalogMatch::Matched { catalog_provider_id, catalog_model_id } => {
            vec![ProviderCatalogMatchCandidate { catalog_provider_id, catalog_model_id }]
        }
        ProviderCatalogMatch::Ambiguous { candidates } => candidates,
        ProviderCatalogMatch::Unmatched => Vec::new(),
    };
    let position = |id: &str| catalog.provider_order.iter().position(|each| each == id);
    candidates
        .sort_by_key(|candidate| position(&candidate.catalog_provider_id).unwrap_or(usize::MAX));
    Ok(candidates)
}

/// The chat entries of the catalog that a profile's model name matches.
pub fn catalog_entries(
    registry: &ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    catalog: &ProviderCatalogSnapshot,
) -> Result<ModelProfileCatalogEntriesView, ModelConfigError> {
    let mut entries = Vec::new();
    for candidate in candidates(registry, model_profile_id, catalog)? {
        let Some(provider) = catalog.providers.get(&candidate.catalog_provider_id) else {
            continue;
        };
        let Some(model) = provider.models.get(&candidate.catalog_model_id) else {
            continue;
        };
        if model.is_chat() {
            entries.push(ModelCatalogEntryView {
                provider_display_name: provider.display_name.clone(),
                catalog_object_hash: catalog_object_hash(model)?,
                catalog_provider_id: candidate.catalog_provider_id,
                catalog_model_id: candidate.catalog_model_id,
            });
        }
    }
    let chosen = registry.models.get(&model_profile_id).and_then(|profile| {
        let link = profile.catalog.as_ref()?;
        entries
            .iter()
            .any(|entry| {
                entry.catalog_provider_id == link.catalog_provider_id
                    && entry.catalog_model_id == link.catalog_model_id
            })
            .then(|| ProviderCatalogMatchCandidate {
                catalog_provider_id: link.catalog_provider_id.clone(),
                catalog_model_id: link.catalog_model_id.clone(),
            })
    });
    Ok(ModelProfileCatalogEntriesView {
        model_profile_id,
        catalog_revision: catalog.catalog_revision,
        entries,
        chosen,
    })
}

/// Links a profile to `entry`, one of its candidates, as the settings listed it.
pub fn choose_catalog_entry(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    catalog: &ProviderCatalogSnapshot,
    entry: &ProviderCatalogMatchCandidate,
    expected_catalog_object_hash: &ContentHash,
    updated_at: UtcTimestamp,
) -> Result<(), ModelConfigError> {
    if !candidates(registry, model_profile_id, catalog)?.contains(entry) {
        return Err(ModelConfigError::CatalogEntryNotForModel {
            catalog_provider_id: entry.catalog_provider_id.clone(),
            catalog_model_id: entry.catalog_model_id.clone(),
        });
    }
    let model = catalog
        .providers
        .get(&entry.catalog_provider_id)
        .and_then(|provider| provider.models.get(&entry.catalog_model_id))
        .ok_or_else(|| ModelConfigError::CatalogModelNotFound {
            catalog_provider_id: entry.catalog_provider_id.clone(),
            catalog_model_id: entry.catalog_model_id.clone(),
        })?;
    validate_catalog_object_hash(model, expected_catalog_object_hash)?;
    if !model.is_chat() {
        return Err(ModelConfigError::CatalogModelNotChat {
            catalog_provider_id: entry.catalog_provider_id.clone(),
            catalog_model_id: entry.catalog_model_id.clone(),
        });
    }
    let link = catalog_metadata(
        catalog,
        &entry.catalog_provider_id,
        model,
        expected_catalog_object_hash.clone(),
        updated_at,
    );
    let profile = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    relink(profile, link, updated_at);
    Ok(())
}
