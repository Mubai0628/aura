use std::collections::BTreeSet;

use aura_model_domain::{
    ProviderCatalogMatch, ProviderCatalogMatchCandidate, ProviderCatalogSnapshot,
};

pub fn match_catalog_model(
    snapshot: &ProviderCatalogSnapshot,
    catalog_provider_id: Option<&str>,
    provider_model_name: &str,
) -> ProviderCatalogMatch {
    let candidates = match catalog_provider_id {
        Some(provider_id) => snapshot
            .providers
            .get(provider_id)
            .into_iter()
            .flat_map(|provider| matching_model_ids(provider, provider_model_name))
            .map(|model_id| ProviderCatalogMatchCandidate {
                catalog_provider_id: provider_id.to_owned(),
                catalog_model_id: model_id,
            })
            .collect::<Vec<_>>(),
        None => snapshot
            .providers
            .iter()
            .flat_map(|(provider_id, provider)| {
                matching_model_ids(provider, provider_model_name).map(|model_id| {
                    ProviderCatalogMatchCandidate {
                        catalog_provider_id: provider_id.clone(),
                        catalog_model_id: model_id,
                    }
                })
            })
            .collect::<Vec<_>>(),
    };
    match candidates.as_slice() {
        [] => ProviderCatalogMatch::Unmatched,
        [candidate] => ProviderCatalogMatch::Matched {
            catalog_provider_id: candidate.catalog_provider_id.clone(),
            catalog_model_id: candidate.catalog_model_id.clone(),
        },
        _ => ProviderCatalogMatch::Ambiguous { candidates },
    }
}

/// The catalog Provider whose own API an Endpoint is: the one Provider whose `api_url` has the
/// Endpoint's HTTPS origin. A custom Provider without a catalog binding then matches models
/// against that Provider only, so the official DeepSeek Endpoint never takes another Provider's
/// model of the same name. Nothing is inferred when no Provider, or more than one, has that
/// origin; matching then stays across the whole catalog.
#[must_use]
pub fn official_catalog_provider<'a>(
    snapshot: &'a ProviderCatalogSnapshot,
    endpoint_base_url: &str,
) -> Option<&'a str> {
    let origin = https_origin(endpoint_base_url)?;
    let mut owners = snapshot.providers.iter().filter(|(_, provider)| {
        provider.api_url.as_deref().and_then(https_origin).is_some_and(|api| api == origin)
    });
    let (provider_id, _) = owners.next()?;
    owners.next().is_none().then_some(provider_id.as_str())
}

fn https_origin(value: &str) -> Option<(String, u16)> {
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    Some((url.host_str()?.to_ascii_lowercase(), url.port_or_known_default()?))
}

fn matching_model_ids<'a>(
    provider: &'a aura_model_domain::ProviderCatalogEntry,
    model_name: &'a str,
) -> impl Iterator<Item = String> + 'a {
    let mut matches = BTreeSet::new();
    if provider.models.contains_key(model_name) {
        matches.insert(model_name.to_owned());
    }
    matches.extend(
        provider
            .models
            .iter()
            .filter(|(_, model)| model.aliases.contains(model_name))
            .map(|(model_id, _)| model_id.clone()),
    );
    matches.into_iter()
}
