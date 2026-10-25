use std::collections::BTreeMap;

use aura_domain_common::ProviderId;
use aura_model_domain::{
    ModelCatalogProviderGetRequest, ModelProviderSource, ModelRegistryConfig,
    ProviderCatalogCardView, ProviderCatalogConfigurationClass,
    ProviderCatalogConnectionDefaultsView, ProviderCatalogEntry, ProviderCatalogModelPage,
    ProviderCatalogModelView, ProviderCatalogStatus, ProviderCatalogSummaryView, ProviderIconRef,
};

use crate::CatalogError;

pub trait ProviderConfigurationPolicy {
    fn class_for(&self, entry: &ProviderCatalogEntry) -> ProviderCatalogConfigurationClass;

    fn connection_defaults_for(
        &self,
        _entry: &ProviderCatalogEntry,
    ) -> Option<ProviderCatalogConnectionDefaultsView> {
        None
    }
}

impl<F> ProviderConfigurationPolicy for F
where
    F: Fn(&ProviderCatalogEntry) -> ProviderCatalogConfigurationClass,
{
    fn class_for(&self, entry: &ProviderCatalogEntry) -> ProviderCatalogConfigurationClass {
        self(entry)
    }
}

pub fn project_provider_cards(
    status: &ProviderCatalogStatus,
    registry: &ModelRegistryConfig,
    policy: &impl ProviderConfigurationPolicy,
    icons: &BTreeMap<String, ProviderIconRef>,
) -> Result<Vec<ProviderCatalogCardView>, CatalogError> {
    let Some(snapshot) = status.snapshot.as_ref() else {
        return Ok(Vec::new());
    };
    let mut local_by_catalog = BTreeMap::<String, Vec<ProviderId>>::new();
    for provider in registry.providers.values() {
        let catalog_provider_id = match &provider.source {
            ModelProviderSource::ModelsDev { catalog_provider_id } => Some(catalog_provider_id),
            ModelProviderSource::Custom { catalog_provider_id } => catalog_provider_id.as_ref(),
        };
        if let Some(catalog_provider_id) = catalog_provider_id {
            local_by_catalog.entry(catalog_provider_id.clone()).or_default().push(provider.id);
        }
    }
    snapshot
        .provider_order
        .iter()
        .filter_map(|provider_id| snapshot.providers.get(provider_id))
        .map(|entry| {
            let local_ids = local_by_catalog
                .get(&entry.catalog_provider_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let local_provider_id = match local_ids {
                [] => None,
                [provider_id] => Some(*provider_id),
                _ => {
                    return Err(CatalogError::AmbiguousLocalProvider(
                        entry.catalog_provider_id.clone(),
                    ));
                }
            };
            Ok(ProviderCatalogCardView {
                catalog_provider_id: entry.catalog_provider_id.clone(),
                display_name: entry.display_name.clone(),
                icon_ref: icons.get(&entry.catalog_provider_id).cloned(),
                fallback_initial: fallback_initial(&entry.display_name, &entry.catalog_provider_id),
                catalog_model_count: u32::try_from(entry.models.len()).unwrap_or(u32::MAX),
                configuration_class: policy.class_for(entry),
                connection_defaults: policy.connection_defaults_for(entry),
                local_provider_id,
            })
        })
        .collect()
}

pub fn project_provider_model_page(
    status: &ProviderCatalogStatus,
    registry: &ModelRegistryConfig,
    policy: &impl ProviderConfigurationPolicy,
    request: &ModelCatalogProviderGetRequest,
    icons: &BTreeMap<String, ProviderIconRef>,
) -> Result<ProviderCatalogModelPage, CatalogError> {
    let snapshot = status.snapshot.as_ref().ok_or(CatalogError::Unavailable)?;
    if request.after_model_id.is_some() && request.expected_catalog_revision.is_none() {
        return Err(CatalogError::MissingContinuationRevision);
    }
    if let Some(expected) = request.expected_catalog_revision
        && expected != snapshot.catalog_revision
    {
        return Err(CatalogError::RevisionConflict {
            expected: expected.value(),
            actual: snapshot.catalog_revision.value(),
        });
    }
    let limit = request.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(CatalogError::InvalidPageLimit);
    }
    let entry = snapshot
        .providers
        .get(&request.catalog_provider_id)
        .ok_or_else(|| CatalogError::ProviderNotFound(request.catalog_provider_id.clone()))?;
    let local_provider_id = local_provider_id(registry, &entry.catalog_provider_id)?;
    let query = request.query.as_ref().map(|query| query.to_lowercase());
    let mut models = entry
        .models
        .values()
        .filter(|model| {
            query.as_ref().is_none_or(|query| {
                model.catalog_model_id.to_lowercase().contains(query)
                    || model.display_name.to_lowercase().contains(query)
            })
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| {
        left.catalog_model_id.as_bytes().cmp(right.catalog_model_id.as_bytes())
    });
    let start = if let Some(after_model_id) = request.after_model_id.as_ref() {
        models
            .iter()
            .position(|model| model.catalog_model_id == *after_model_id)
            .map(|index| index + 1)
            .ok_or_else(|| CatalogError::InvalidContinuation(after_model_id.clone()))?
    } else {
        0
    };
    let limit = usize::try_from(limit).map_err(|_| CatalogError::InvalidPageLimit)?;
    let end = start.saturating_add(limit).min(models.len());
    let page = models[start..end]
        .iter()
        .map(|model| {
            let model = (*model).clone();
            let bytes = serde_json::to_vec(&model)?;
            Ok(ProviderCatalogModelView {
                catalog_object_hash: aura_domain_common::ContentHash::blake3(&bytes),
                model,
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    let next_model_id = (end < models.len())
        .then(|| page.last().map(|view| view.model.catalog_model_id.clone()))
        .flatten();
    Ok(ProviderCatalogModelPage {
        catalog_revision: snapshot.catalog_revision,
        catalog_content_hash: snapshot.catalog_content_hash.clone(),
        provider: ProviderCatalogSummaryView {
            catalog_provider_id: entry.catalog_provider_id.clone(),
            display_name: entry.display_name.clone(),
            icon_ref: icons.get(&entry.catalog_provider_id).cloned(),
            fallback_initial: fallback_initial(&entry.display_name, &entry.catalog_provider_id),
            catalog_model_count: u32::try_from(entry.models.len()).unwrap_or(u32::MAX),
            configuration_class: policy.class_for(entry),
            connection_defaults: policy.connection_defaults_for(entry),
            local_provider_id,
        },
        models: page,
        next_model_id,
    })
}

fn local_provider_id(
    registry: &ModelRegistryConfig,
    catalog_provider_id: &str,
) -> Result<Option<ProviderId>, CatalogError> {
    let local_ids = registry
        .providers
        .values()
        .filter_map(|provider| {
            let bound = match &provider.source {
                ModelProviderSource::ModelsDev { catalog_provider_id } => Some(catalog_provider_id),
                ModelProviderSource::Custom { catalog_provider_id } => catalog_provider_id.as_ref(),
            };
            (bound.map(String::as_str) == Some(catalog_provider_id)).then_some(provider.id)
        })
        .collect::<Vec<_>>();
    match local_ids.as_slice() {
        [] => Ok(None),
        [provider_id] => Ok(Some(*provider_id)),
        _ => Err(CatalogError::AmbiguousLocalProvider(catalog_provider_id.to_owned())),
    }
}

fn fallback_initial(display_name: &str, catalog_provider_id: &str) -> String {
    display_name
        .chars()
        .find(char::is_ascii_alphanumeric)
        .or_else(|| catalog_provider_id.chars().find(char::is_ascii_alphanumeric))
        .map(|character| character.to_ascii_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned())
}

#[cfg(test)]
mod tests {
    use super::fallback_initial;

    #[test]
    fn provider_fallback_initial_is_stable() {
        assert_eq!(fallback_initial(" OpenAI", "openai"), "O");
        assert_eq!(fallback_initial("你好", "deepseek"), "D");
        assert_eq!(fallback_initial("你好", "无"), "?");
    }
}
