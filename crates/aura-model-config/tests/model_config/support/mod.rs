//! Catalog snapshots for the tests: one Provider and its models, as the catalog parser leaves
//! them; and a registry with one Provider bound to a catalog Provider and its connections.
#![allow(dead_code)]
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::{ArtifactId, ContentHash, EndpointId, ProviderId, Revision, UtcTimestamp};
use aura_model_domain::{
    DEFAULT_MAX_PARALLEL_MODEL_REQUESTS, EndpointAuthScheme, ModelApiFormat, ModelDiscoveryMode,
    ModelEndpointConfig, ModelEndpointTimeouts, ModelProviderConfig, ModelProviderSource,
    ModelRegistryConfig, ProviderCatalogEntry, ProviderCatalogModelCapabilities,
    ProviderCatalogModelEntry, ProviderCatalogModelRoute, ProviderCatalogSnapshot,
};

/// A text model with tool calls, a 128k window and an 8k output limit, of the catalog type
/// `model_type`.
pub fn catalog_model(id: &str, model_type: Option<&str>) -> ProviderCatalogModelEntry {
    ProviderCatalogModelEntry {
        catalog_model_id: id.to_owned(),
        display_name: id.to_owned(),
        family: None,
        description: None,
        aliases: BTreeSet::new(),
        context_window_tokens: Some(128_000),
        input_limit_tokens: None,
        output_limit_tokens: Some(8_192),
        input_modalities: BTreeSet::from(["text".to_owned()]),
        output_modalities: BTreeSet::from(["text".to_owned()]),
        capabilities: ProviderCatalogModelCapabilities {
            attachment: false,
            reasoning: false,
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
        model_type: model_type.map(str::to_owned),
        route: None,
        reasoning_field: None,
        price_tiers: Vec::new(),
        modes: BTreeMap::new(),
    }
}

/// A chat model served where the catalog's `npm` package and `api` address say, not where its
/// Provider's other models are.
pub fn catalog_model_at(id: &str, npm: &str, api: &str) -> ProviderCatalogModelEntry {
    ProviderCatalogModelEntry {
        route: Some(ProviderCatalogModelRoute {
            api_url: Some(api.to_owned()),
            npm_package: Some(npm.to_owned()),
            shape: None,
        }),
        ..catalog_model(id, None)
    }
}

/// A snapshot whose one Provider, `provider_id`, lists `models` in this order.
pub fn catalog(
    provider_id: &str,
    api_url: Option<&str>,
    models: Vec<ProviderCatalogModelEntry>,
) -> ProviderCatalogSnapshot {
    provider_catalog(provider_id, None, api_url, models)
}

/// A snapshot whose one Provider, `provider_id`, serves its models with the catalog's `npm`
/// package at `api_url`, unless a model names its own route.
pub fn provider_catalog(
    provider_id: &str,
    npm: Option<&str>,
    api_url: Option<&str>,
    models: Vec<ProviderCatalogModelEntry>,
) -> ProviderCatalogSnapshot {
    let provider = ProviderCatalogEntry {
        catalog_provider_id: provider_id.to_owned(),
        display_name: provider_id.to_owned(),
        npm_package: npm.map(str::to_owned),
        api_url: api_url.map(str::to_owned),
        documentation_url: None,
        model_order: models.iter().map(|model| model.catalog_model_id.clone()).collect(),
        models: models.into_iter().map(|model| (model.catalog_model_id.clone(), model)).collect(),
    };
    ProviderCatalogSnapshot {
        catalog_revision: Revision::INITIAL,
        catalog_content_hash: ContentHash::blake3(b"catalog"),
        checked_at: UtcTimestamp::now(),
        last_successful_refresh_at: UtcTimestamp::now(),
        etag: None,
        last_modified: None,
        raw_artifact_id: ArtifactId::new(),
        provider_order: vec![provider_id.to_owned()],
        providers: BTreeMap::from([(provider_id.to_owned(), provider)]),
        parser_revision: 0,
    }
}

/// A registry with one Provider bound to the catalog's `catalog_provider_id` and one connection
/// for each `(base_url, api_format)`, without models; the connections' ids in the same order.
pub fn registry(
    catalog_provider_id: &str,
    connections: &[(&str, ModelApiFormat)],
) -> (ModelRegistryConfig, Vec<EndpointId>) {
    let provider_id = ProviderId::new();
    let now = UtcTimestamp::now();
    let endpoints = connections
        .iter()
        .enumerate()
        .map(|(index, (base_url, api_format))| {
            let id = EndpointId::new();
            let endpoint = ModelEndpointConfig {
                id,
                revision: Revision::INITIAL,
                provider_id,
                display_name: format!("Connection {index}"),
                slug: format!("connection-{index}").parse().expect("valid Endpoint slug"),
                api_format: *api_format,
                base_url: (*base_url).to_owned(),
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
                model_order: Vec::new(),
                compatibility: None,
                created_at: now,
                updated_at: now,
            };
            (id, endpoint)
        })
        .collect::<Vec<_>>();
    let ids = endpoints.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let provider = ModelProviderConfig {
        id: provider_id,
        display_name: catalog_provider_id.to_owned(),
        slug: "provider".parse().expect("valid Provider slug"),
        source: ModelProviderSource::ModelsDev {
            catalog_provider_id: catalog_provider_id.to_owned(),
        },
        endpoint_order: ids.clone(),
        created_at: now,
        updated_at: now,
        default_endpoint_id: None,
        backup_endpoint_ids: vec![],
    };
    let registry = ModelRegistryConfig {
        provider_order: vec![provider_id],
        providers: BTreeMap::from([(provider_id, provider)]),
        endpoints: endpoints.into_iter().collect(),
        models: BTreeMap::new(),
    };
    (registry, ids)
}
