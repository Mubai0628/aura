//! A model that several catalog Providers list under one name is linked to the entry the user
//! chooses: only a chat entry the name matches, as the settings listed it. Discovering the model
//! again keeps the choice while the name still matches that entry.
use std::collections::BTreeMap;

use aura_domain_common::{ContentHash, EndpointId, ModelProfileId, UtcTimestamp};
use aura_model_config::{
    ModelConfigError, catalog_entries, choose_catalog_entry, merge_discovery_results,
};
use aura_model_domain::{
    CatalogMatchState, ModelApiFormat, ModelProviderSource, ModelRegistryConfig,
    ProviderCatalogMatchCandidate, ProviderCatalogModelEntry, ProviderCatalogModelMode,
    ProviderCatalogSnapshot, offered_speed_modes,
};

mod support;

/// A gateway on this computer, which Aura cannot tell apart from any catalog Provider's API.
const GATEWAY: &str = "http://127.0.0.1:8317/v1";
const NAME: &str = "gpt-gateway";

/// OpenAI's entry with its speed modes; then, in the catalog's order, a relay and an embedding
/// model under the same name. The relay's id sorts before OpenAI's.
fn catalog() -> ProviderCatalogSnapshot {
    let tier = |value: &str| ProviderCatalogModelMode {
        price: None,
        body: BTreeMap::from([("service_tier".to_owned(), value.to_owned())]),
        headers: BTreeMap::new(),
    };
    let openai = ProviderCatalogModelEntry {
        context_window_tokens: Some(400_000),
        modes: BTreeMap::from([
            ("fast".to_owned(), tier("priority")),
            ("ultrafast".to_owned(), tier("ultrafast")),
        ]),
        ..support::catalog_model(NAME, None)
    };
    let mut snapshot = support::catalog("openai", Some("https://api.openai.com/v1"), vec![openai]);
    snapshot.providers.get_mut("openai").expect("OpenAI").display_name = "OpenAI".to_owned();
    for (id, model) in [
        ("azure", support::catalog_model(NAME, None)),
        ("embedder", support::catalog_model(NAME, Some("embedding"))),
    ] {
        let other =
            support::catalog(id, Some(&format!("https://{id}.example.test/v1")), vec![model]);
        snapshot.provider_order.extend(other.provider_order);
        snapshot.providers.extend(other.providers);
    }
    snapshot
}

/// A Provider of the user's own, bound to no catalog Provider, with one connection.
fn gateway() -> (ModelRegistryConfig, EndpointId) {
    let (mut registry, endpoints) =
        support::registry("openai", &[(GATEWAY, ModelApiFormat::OpenAiResponses)]);
    for provider in registry.providers.values_mut() {
        provider.source = ModelProviderSource::Custom { catalog_provider_id: None };
    }
    (registry, endpoints[0])
}

/// The model's profile, its match and whether it asks to be confirmed.
fn discover(
    registry: &mut ModelRegistryConfig,
    endpoint: EndpointId,
    catalog: &ProviderCatalogSnapshot,
) -> (ModelProfileId, CatalogMatchState, bool) {
    let result = merge_discovery_results(
        registry,
        endpoint,
        &[NAME.to_owned()],
        Some(catalog),
        UtcTimestamp::now(),
    )
    .expect("merge discovery");
    let profile = &result.returned_profiles[0];
    (profile.model_profile_id, profile.catalog_match_state, profile.needs_manual_confirmation)
}

fn entry(provider: &str, model: &str) -> ProviderCatalogMatchCandidate {
    ProviderCatalogMatchCandidate {
        catalog_provider_id: provider.to_owned(),
        catalog_model_id: model.to_owned(),
    }
}

fn hash(catalog: &ProviderCatalogSnapshot, provider: &str, model: &str) -> ContentHash {
    let listed = &catalog.providers[provider].models[model];
    ContentHash::blake3(&serde_json::to_vec(listed).expect("entry JSON"))
}

fn choose(
    registry: &mut ModelRegistryConfig,
    id: ModelProfileId,
    catalog: &ProviderCatalogSnapshot,
    provider: &str,
) -> Result<(), ModelConfigError> {
    let hash = hash(catalog, provider, NAME);
    choose_catalog_entry(registry, id, catalog, &entry(provider, NAME), &hash, UtcTimestamp::now())
}

#[test]
fn a_name_several_providers_list_offers_each_of_their_chat_entries() {
    let catalog = catalog();
    let (mut registry, endpoint) = gateway();
    let (id, state, confirm) = discover(&mut registry, endpoint, &catalog);
    assert_eq!((state, confirm), (CatalogMatchState::Ambiguous, true));
    let view = catalog_entries(&registry, id, &catalog).expect("entries");
    let listed = view
        .entries
        .iter()
        .map(|each| {
            (
                each.catalog_provider_id.as_str(),
                each.catalog_model_id.as_str(),
                each.provider_display_name.as_str(),
                each.catalog_object_hash.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        listed,
        [
            ("openai", NAME, "OpenAI", hash(&catalog, "openai", NAME)),
            ("azure", NAME, "azure", hash(&catalog, "azure", NAME)),
        ],
        "the chat entries, in the catalog's order"
    );
    assert_eq!((view.model_profile_id, view.chosen), (id, None));
    assert_eq!(view.catalog_revision, catalog.catalog_revision);
}

#[test]
fn choosing_an_entry_links_the_model_to_it() {
    let catalog = catalog();
    let (mut registry, endpoint) = gateway();
    let (id, ..) = discover(&mut registry, endpoint, &catalog);
    choose(&mut registry, id, &catalog, "openai").expect("chosen");
    let profile = &registry.models[&id];
    let link = profile.catalog.as_ref().expect("linked to the catalog");
    assert_eq!(
        (link.catalog_provider_id.as_str(), link.catalog_model_id.as_str(), link.match_state),
        ("openai", NAME, CatalogMatchState::Matched)
    );
    assert_eq!(link.catalog_object_hash, hash(&catalog, "openai", NAME));
    assert_eq!(profile.context.catalog_window_tokens, Some(400_000));
    assert_eq!(profile.context.runtime_effective_window_tokens, 400_000);
    assert_eq!(
        offered_speed_modes(profile.catalog.as_ref()).into_keys().collect::<Vec<_>>(),
        ["fast", "ultrafast"]
    );
    let view = catalog_entries(&registry, id, &catalog).expect("entries");
    assert_eq!(view.chosen, Some(entry("openai", NAME)));
    assert_eq!(view.entries.len(), 2, "another entry can still be chosen");
    choose(&mut registry, id, &catalog, "azure").expect("chosen again");
    let link = registry.models[&id].catalog.as_ref().expect("linked to the catalog");
    assert_eq!(link.catalog_provider_id, "azure");
    assert_eq!(registry.models[&id].context.runtime_effective_window_tokens, 128_000);
    assert!(offered_speed_modes(registry.models[&id].catalog.as_ref()).is_empty());
}

#[test]
fn only_an_unchanged_chat_entry_the_name_matches_can_be_chosen() {
    let mut catalog = catalog();
    let other = support::catalog_model("gpt-other", None);
    let openai = catalog.providers.get_mut("openai").expect("OpenAI");
    openai.model_order.push("gpt-other".to_owned());
    openai.models.insert("gpt-other".to_owned(), other);
    let (mut registry, endpoint) = gateway();
    let (id, ..) = discover(&mut registry, endpoint, &catalog);
    let now = UtcTimestamp::now();
    let other = choose_catalog_entry(
        &mut registry,
        id,
        &catalog,
        &entry("openai", "gpt-other"),
        &hash(&catalog, "openai", "gpt-other"),
        now,
    );
    assert!(
        matches!(other, Err(ModelConfigError::CatalogEntryNotForModel { .. })),
        "another model's entry: {other:?}"
    );
    let embedding = choose(&mut registry, id, &catalog, "embedder");
    assert!(
        matches!(embedding, Err(ModelConfigError::CatalogModelNotChat { .. })),
        "an embedding model: {embedding:?}"
    );
    let changed = choose_catalog_entry(
        &mut registry,
        id,
        &catalog,
        &entry("openai", NAME),
        &hash(&catalog, "azure", NAME),
        now,
    );
    assert!(
        matches!(changed, Err(ModelConfigError::CatalogObjectHashMismatch)),
        "an entry other than the one listed: {changed:?}"
    );
    assert!(registry.models[&id].catalog.is_none(), "nothing was linked");
}

#[test]
fn discovering_again_keeps_the_chosen_entry_as_the_catalog_now_lists_it() {
    let catalog = catalog();
    let (mut registry, endpoint) = gateway();
    let (id, ..) = discover(&mut registry, endpoint, &catalog);
    choose(&mut registry, id, &catalog, "openai").expect("chosen");
    let (again, state, confirm) = discover(&mut registry, endpoint, &catalog);
    assert_eq!(again, id);
    assert_eq!((state, confirm), (CatalogMatchState::Matched, false));
    let mut changed = catalog.clone();
    changed
        .providers
        .get_mut("openai")
        .and_then(|openai| openai.models.get_mut(NAME))
        .expect("OpenAI's entry")
        .context_window_tokens = Some(1_000_000);
    let (_, state, _) = discover(&mut registry, endpoint, &changed);
    assert_eq!(state, CatalogMatchState::Matched);
    let profile = &registry.models[&id];
    let link = profile.catalog.as_ref().expect("still linked");
    assert_eq!(link.catalog_provider_id, "openai");
    assert_eq!(link.catalog_object_hash, hash(&changed, "openai", NAME));
    assert_eq!(profile.context.runtime_effective_window_tokens, 1_000_000);
}

#[test]
fn a_choice_the_catalog_no_longer_lists_under_the_name_is_dropped() {
    let catalog = catalog();
    let (mut registry, endpoint) = gateway();
    let (id, ..) = discover(&mut registry, endpoint, &catalog);
    choose(&mut registry, id, &catalog, "openai").expect("chosen");
    let mut renamed = catalog.clone();
    let openai = renamed.providers.get_mut("openai").expect("OpenAI");
    let mut listed = openai.models.remove(NAME).expect("OpenAI's entry");
    listed.catalog_model_id = "gpt-renamed".to_owned();
    openai.model_order = vec!["gpt-renamed".to_owned()];
    openai.models.insert("gpt-renamed".to_owned(), listed);
    let (_, state, confirm) = discover(&mut registry, endpoint, &renamed);
    assert_eq!((state, confirm), (CatalogMatchState::Ambiguous, true));
    assert!(registry.models[&id].catalog.is_none());
}

#[test]
fn a_provider_bound_to_the_catalog_offers_its_own_entries_only() {
    let catalog = catalog();
    let (mut registry, endpoints) = support::registry(
        "openai",
        &[("https://api.openai.com/v1", ModelApiFormat::OpenAiResponses)],
    );
    let (id, state, _) = discover(&mut registry, endpoints[0], &catalog);
    assert_eq!(state, CatalogMatchState::Matched);
    let view = catalog_entries(&registry, id, &catalog).expect("entries");
    assert_eq!(
        view.entries.iter().map(|each| each.catalog_provider_id.as_str()).collect::<Vec<_>>(),
        ["openai"]
    );
    assert_eq!(view.chosen, Some(entry("openai", NAME)));
    let other = choose(&mut registry, id, &catalog, "azure");
    assert!(
        matches!(other, Err(ModelConfigError::CatalogEntryNotForModel { .. })),
        "another Provider's entry: {other:?}"
    );
}
