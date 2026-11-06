//! Only chat models become model profiles from the catalog: adding an embedding or image model
//! from the catalog is refused, and discovering an Endpoint's models leaves out the ones the
//! catalog says are not chat models. A profile the user already has stays.
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::{
    ContentHash, EndpointId, ModelProfileId, ProviderId, Revision, UtcTimestamp,
};
use aura_model_config::{ModelConfigError, create_model_from_catalog, merge_discovery_results};
use aura_model_domain::{
    DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS, DEFAULT_MAX_PARALLEL_MODEL_REQUESTS,
    DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS, EndpointAuthScheme, ModelApiFormat,
    ModelContextWindowConfig, ModelDiscoveryMode, ModelEndpointConfig, ModelEndpointTimeouts,
    ModelProfileConfig, ModelProfileOrigin, ModelProviderConfig, ModelProviderSource,
    ModelRegistryConfig, ModelRequestDefaults, ProviderCatalogSnapshot,
};

mod support;

fn catalog() -> ProviderCatalogSnapshot {
    support::catalog(
        "provider-a",
        Some("https://api.example.test/v1"),
        vec![
            support::catalog_model("chat-model", None),
            support::catalog_model("embedder", Some("embedding")),
        ],
    )
}

/// A Provider bound to the catalog's `provider-a`, one Endpoint, and profiles named `existing`.
fn registry(existing: &[&str]) -> (ModelRegistryConfig, EndpointId) {
    let provider_id = ProviderId::new();
    let endpoint_id = EndpointId::new();
    let now = UtcTimestamp::now();
    let models = existing
        .iter()
        .map(|name| {
            let id = ModelProfileId::new();
            let profile = ModelProfileConfig {
                id,
                revision: Revision::INITIAL,
                endpoint_id,
                provider_model_name: (*name).to_owned(),
                display_name: (*name).to_owned(),
                notes: None,
                origin: ModelProfileOrigin::Manual,
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
                request_defaults: ModelRequestDefaults::default(),
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
        display_name: "Provider A".to_owned(),
        slug: "provider-a".parse().expect("valid Endpoint slug"),
        api_format: ModelApiFormat::OpenAiChatCompletions,
        base_url: "https://api.example.test/v1".to_owned(),
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
        display_name: "Provider A".to_owned(),
        slug: "provider-a".parse().expect("valid Provider slug"),
        source: ModelProviderSource::ModelsDev { catalog_provider_id: "provider-a".to_owned() },
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

fn object_hash(catalog: &ProviderCatalogSnapshot, id: &str) -> ContentHash {
    let model = &catalog.providers["provider-a"].models[id];
    ContentHash::blake3(&serde_json::to_vec(model).expect("model JSON"))
}

#[test]
fn a_model_that_is_not_a_chat_model_is_not_added_from_the_catalog() {
    let catalog = catalog();
    let (mut registry, endpoint) = registry(&[]);
    let refused = create_model_from_catalog(
        &mut registry,
        endpoint,
        &catalog,
        "provider-a",
        "embedder",
        &object_hash(&catalog, "embedder"),
        UtcTimestamp::now(),
    );
    assert!(
        matches!(&refused, Err(ModelConfigError::CatalogModelNotChat { catalog_model_id, .. }) if catalog_model_id == "embedder"),
        "{refused:?}"
    );
    assert!(registry.models.is_empty());
    create_model_from_catalog(
        &mut registry,
        endpoint,
        &catalog,
        "provider-a",
        "chat-model",
        &object_hash(&catalog, "chat-model"),
        UtcTimestamp::now(),
    )
    .expect("a chat model is added");
    assert_eq!(registry.models.len(), 1);
}

#[test]
fn discovery_leaves_out_new_models_that_are_not_chat_models() {
    let catalog = catalog();
    let (mut registry, endpoint) = registry(&[]);
    let names = ["chat-model".to_owned(), "embedder".to_owned(), "unknown".to_owned()];
    let merged = merge_discovery_results(
        &mut registry,
        endpoint,
        &names,
        Some(&catalog),
        UtcTimestamp::now(),
    )
    .expect("merge");
    let created: Vec<_> = merged
        .returned_profiles
        .iter()
        .map(|profile| profile.provider_model_name.as_str())
        .collect();
    assert_eq!(created, ["chat-model", "unknown"]);
    let profiles: BTreeSet<_> =
        registry.models.values().map(|profile| profile.provider_model_name.as_str()).collect();
    assert_eq!(profiles, BTreeSet::from(["chat-model", "unknown"]));
}

#[test]
fn a_profile_the_user_already_has_stays_when_discovery_returns_it() {
    let catalog = catalog();
    let (mut registry, endpoint) = registry(&["embedder"]);
    let names = ["embedder".to_owned()];
    let merged = merge_discovery_results(
        &mut registry,
        endpoint,
        &names,
        Some(&catalog),
        UtcTimestamp::now(),
    )
    .expect("merge");
    assert_eq!(merged.returned_profiles.len(), 1);
    assert!(!merged.returned_profiles[0].created);
    assert_eq!(registry.models.len(), 1);
}
