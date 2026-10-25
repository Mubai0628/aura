//! A custom Provider whose Endpoint is a catalog Provider's own API matches discovered models
//! against that Provider only: the official DeepSeek Endpoint uses
//! the official DeepSeek catalog entries, never another Provider's model of the same name.
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::{
    ArtifactId, ContentHash, EndpointId, ModelProfileId, ProviderId, Revision, UtcTimestamp,
};
use aura_model_config::merge_discovery_results;
use aura_model_domain::{
    CatalogMatchState, DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS, DEFAULT_MAX_PARALLEL_MODEL_REQUESTS,
    DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS, EndpointAuthScheme, ModelApiFormat,
    ModelContextWindowConfig, ModelDiscoveryMode, ModelEndpointConfig, ModelEndpointTimeouts,
    ModelProfileConfig, ModelProfileOrigin, ModelProviderConfig, ModelProviderSource,
    ModelRegistryConfig, ModelRequestDefaults, ProviderCatalogEntry,
    ProviderCatalogModelCapabilities, ProviderCatalogModelEntry, ProviderCatalogSnapshot,
};

fn model(id: &str, context: u64, output: u64) -> (String, ProviderCatalogModelEntry) {
    let entry = ProviderCatalogModelEntry {
        catalog_model_id: id.to_owned(),
        display_name: id.to_owned(),
        family: None,
        description: None,
        aliases: BTreeSet::new(),
        context_window_tokens: Some(context),
        input_limit_tokens: None,
        output_limit_tokens: Some(output),
        input_modalities: BTreeSet::from(["text".to_owned()]),
        output_modalities: BTreeSet::from(["text".to_owned()]),
        capabilities: ProviderCatalogModelCapabilities {
            attachment: false,
            reasoning: true,
            tool_call: true,
            structured_output: false,
            temperature: true,
            interleaved: None,
        },
        reasoning_options: BTreeSet::new(),
        knowledge_cutoff: None,
        release_date: None,
        last_updated: None,
        open_weights: None,
        status: None,
        price: None,
    };
    (id.to_owned(), entry)
}

fn provider(
    id: &str,
    api_url: Option<&str>,
    models: Vec<(String, ProviderCatalogModelEntry)>,
) -> (String, ProviderCatalogEntry) {
    let entry = ProviderCatalogEntry {
        catalog_provider_id: id.to_owned(),
        display_name: id.to_owned(),
        npm_package: None,
        api_url: api_url.map(str::to_owned),
        documentation_url: None,
        model_order: models.iter().map(|(id, _)| id.clone()).collect(),
        models: models.into_iter().collect(),
    };
    (id.to_owned(), entry)
}

fn catalog(providers: Vec<(String, ProviderCatalogEntry)>) -> ProviderCatalogSnapshot {
    ProviderCatalogSnapshot {
        catalog_revision: Revision::INITIAL,
        catalog_content_hash: ContentHash::blake3(b"catalog"),
        checked_at: UtcTimestamp::now(),
        last_successful_refresh_at: UtcTimestamp::now(),
        etag: None,
        last_modified: None,
        raw_artifact_id: ArtifactId::new(),
        provider_order: providers.iter().map(|(id, _)| id.clone()).collect(),
        providers: providers.into_iter().collect(),
    }
}

/// The models.dev shape: the official DeepSeek API, plus another Provider that
/// serves a model under the same name.
fn deepseek_and_a_namesake() -> ProviderCatalogSnapshot {
    catalog(vec![
        provider(
            "deepseek",
            Some("https://api.deepseek.com"),
            vec![
                model("deepseek-flash", 1_000_000, 393_216),
                model("deepseek-v4-pro", 1_000_000, 393_216),
            ],
        ),
        provider(
            "sensenova",
            Some("https://token.sensenova.cn/v1"),
            vec![model("deepseek-v4-pro", 1_048_576, 65_536)],
        ),
    ])
}

/// A custom Provider with no catalog binding and one Endpoint, as the owner configured it.
fn custom_provider(
    base_url: &str,
    existing: &[(&str, Option<u64>)],
) -> (ModelRegistryConfig, EndpointId) {
    let provider_id = ProviderId::new();
    let endpoint_id = EndpointId::new();
    let now = UtcTimestamp::now();
    let models = existing
        .iter()
        .map(|(name, max_output)| {
            let id = ModelProfileId::new();
            let profile = ModelProfileConfig {
                id,
                revision: Revision::INITIAL,
                endpoint_id,
                provider_model_name: (*name).to_owned(),
                display_name: (*name).to_owned(),
                notes: None,
                origin: ModelProfileOrigin::Discovered,
                enabled: true,
                available_for_subagents: false,
                catalog: None,
                context: ModelContextWindowConfig {
                    catalog_window_tokens: None,
                    user_override_window_tokens: None,
                    user_override_revision: 0,
                    runtime_corrected_window_tokens: None,
                    runtime_correction_basis: None,
                    runtime_effective_window_tokens: DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS,
                    historical_snapshot_max_tokens: DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS,
                },
                request_defaults: ModelRequestDefaults {
                    max_output_tokens: *max_output,
                    ..ModelRequestDefaults::default()
                },
                created_at: now,
                updated_at: now,
            };
            (id, profile)
        })
        .collect::<BTreeMap<_, _>>();
    let endpoint = ModelEndpointConfig {
        id: endpoint_id,
        revision: Revision::INITIAL,
        provider_id,
        display_name: "DeepSeek".to_owned(),
        slug: "deepseek".parse().expect("valid Endpoint slug"),
        api_format: ModelApiFormat::OpenAiResponses,
        base_url: base_url.to_owned(),
        discovery_mode: ModelDiscoveryMode::Automatic,
        chat_output_token_field: None,
        chat_instruction_role: None,
        auth_scheme: EndpointAuthScheme::None,
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        timeouts: ModelEndpointTimeouts {
            connect_ms: 15_000,
            first_event_ms: 120_000,
            stream_idle_ms: 120_000,
            total_ms: None,
        },
        max_parallel_model_requests: DEFAULT_MAX_PARALLEL_MODEL_REQUESTS,
        model_order: models.keys().copied().collect(),
        compatibility: None,
        created_at: now,
        updated_at: now,
    };
    let provider = ModelProviderConfig {
        id: provider_id,
        display_name: "deepseek".to_owned(),
        slug: "deepseek".parse().expect("valid Provider slug"),
        source: ModelProviderSource::Custom { catalog_provider_id: None },
        endpoint_order: vec![endpoint_id],
        created_at: now,
        updated_at: now,
        default_endpoint_id: None,
        backup_endpoint_ids: vec![],
    };
    let registry = ModelRegistryConfig {
        provider_order: vec![provider_id],
        providers: BTreeMap::from([(provider_id, provider)]),
        endpoints: BTreeMap::from([(endpoint_id, endpoint)]),
        models,
    };
    (registry, endpoint_id)
}

fn discover(
    registry: &mut ModelRegistryConfig,
    endpoint: EndpointId,
    names: &[&str],
    catalog: &ProviderCatalogSnapshot,
) -> BTreeMap<String, (ModelProfileId, CatalogMatchState)> {
    let names = names.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
    merge_discovery_results(registry, endpoint, &names, Some(catalog), UtcTimestamp::now())
        .expect("merge discovery")
        .returned_profiles
        .into_iter()
        .map(|profile| {
            (profile.provider_model_name, (profile.model_profile_id, profile.catalog_match_state))
        })
        .collect()
}

#[test]
fn the_official_endpoint_takes_the_official_entry_and_its_window_and_output_limit() {
    let catalog = deepseek_and_a_namesake();
    let (mut registry, endpoint) = custom_provider("https://api.deepseek.com", &[]);
    let found = discover(&mut registry, endpoint, &["deepseek-flash", "deepseek-v4-pro"], &catalog);
    for name in ["deepseek-flash", "deepseek-v4-pro"] {
        let (id, state) = found[name];
        assert_eq!(state, CatalogMatchState::Matched, "{name}");
        let profile = &registry.models[&id];
        let link = profile.catalog.as_ref().expect("linked to the catalog");
        assert_eq!(
            (link.catalog_provider_id.as_str(), link.catalog_model_id.as_str()),
            ("deepseek", name)
        );
        assert_eq!(profile.context.runtime_effective_window_tokens, 1_000_000, "{name}");
        assert_eq!(
            profile.effective_max_output_tokens(),
            Some(393_216),
            "{name}: the catalog maximum"
        );
    }
}

/// The owner's two existing profiles: discovering again links them, and an output limit the
/// user chose stays the user's.
#[test]
fn rediscovery_links_existing_profiles_and_keeps_a_chosen_output_limit() {
    let catalog = deepseek_and_a_namesake();
    let (mut registry, endpoint) = custom_provider(
        "https://api.deepseek.com/",
        &[("deepseek-flash", Some(32_768)), ("deepseek-v4-pro", None)],
    );
    let found = discover(&mut registry, endpoint, &["deepseek-flash", "deepseek-v4-pro"], &catalog);
    let flash = &registry.models[&found["deepseek-flash"].0];
    assert_eq!(
        flash.catalog.as_ref().map(|link| link.catalog_provider_id.as_str()),
        Some("deepseek")
    );
    assert_eq!(flash.context.runtime_effective_window_tokens, 1_000_000);
    assert_eq!(flash.effective_max_output_tokens(), Some(32_768));
    let pro = &registry.models[&found["deepseek-v4-pro"].0];
    assert_eq!(
        pro.catalog.as_ref().map(|link| link.catalog_provider_id.as_str()),
        Some("deepseek")
    );
    assert_eq!(pro.effective_max_output_tokens(), Some(393_216));
}

#[test]
fn an_endpoint_that_is_no_providers_own_api_still_asks_about_a_shared_name() {
    let catalog = deepseek_and_a_namesake();
    let (mut registry, endpoint) = custom_provider("https://gateway.example.test/v1", &[]);
    let found = discover(&mut registry, endpoint, &["deepseek-flash", "deepseek-v4-pro"], &catalog);
    assert_eq!(
        found["deepseek-flash"].1,
        CatalogMatchState::Matched,
        "a unique name still matches"
    );
    assert_eq!(found["deepseek-v4-pro"].1, CatalogMatchState::Ambiguous);
    assert!(registry.models[&found["deepseek-v4-pro"].0].catalog.is_none());
}

#[test]
fn an_api_shared_by_two_providers_names_neither() {
    let mut catalog = deepseek_and_a_namesake();
    catalog.providers.get_mut("sensenova").expect("namesake").api_url =
        Some("https://api.deepseek.com/v1".to_owned());
    let (mut registry, endpoint) = custom_provider("https://api.deepseek.com", &[]);
    let found = discover(&mut registry, endpoint, &["deepseek-v4-pro"], &catalog);
    assert_eq!(found["deepseek-v4-pro"].1, CatalogMatchState::Ambiguous);
}

/// Stored a catalog's `max` as `xhigh`. A DeepSeek profile kept that
/// level; a later version then rejected the whole models document at the next start, and the recovery
/// could only reset it to its default. A level an older Aura stored always loads.
#[test]
fn a_level_an_older_aura_stored_still_loads() {
    let mut catalog = deepseek_and_a_namesake();
    for entry in catalog.providers.get_mut("deepseek").expect("deepseek").models.values_mut() {
        entry.reasoning_options =
            BTreeSet::from([r#"{"type":"effort","values":["low","high","max"]}"#.to_owned()]);
    }
    let (mut registry, endpoint) = custom_provider("https://api.deepseek.com/", &[]);
    let found = discover(&mut registry, endpoint, &["deepseek-flash"], &catalog);
    let (id, state) = found["deepseek-flash"];
    assert_eq!(state, CatalogMatchState::Matched);
    let model = registry.models.get_mut(&id).expect("discovered");
    model.request_defaults.reasoning_effort = Some(aura_model_domain::ReasoningEffort::Xhigh);
    aura_model_config::validate_model_registry(&registry).expect("an older level still loads");
}
