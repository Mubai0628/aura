//! A model's speed modes come from its catalog entry. A turn is sent the mode its conversation
//! chose only when the model's settings allow speed modes and the entry offers that mode. A mode
//! sets only the body fields and headers on Aura's list, each to a plain word; a mode asking for
//! anything else is not offered, and neither is any mode of an entry the model was not matched
//! to. The settings list each model's modes with the price each is charged at, allowed or not; a
//! conversation can choose among them once they are allowed. A profile that does not allow them,
//! and a request with no mode, are written as before, so an older Aura reads them.
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::{
    ContentHash, EndpointId, ModelProfileId, ProviderId, Revision, UtcTimestamp,
};
use aura_model_domain::{
    CatalogDecimal, CatalogMatchState, CatalogPriceCurrency, CatalogPriceUnit,
    DEFAULT_MAX_PARALLEL_MODEL_REQUESTS, EndpointAuthScheme, ModelApiFormat, ModelCatalogMetadata,
    ModelContextWindowConfig, ModelDiscoveryMode, ModelEndpointConfig, ModelEndpointTimeouts,
    ModelProfileConfig, ModelProfileOrigin, ModelProviderConfig, ModelProviderSource,
    ModelRegistryConfig, ModelRequestDefaults, ModelSpeedMode, ModelSpeedModeView,
    ModelSpeedModesView, ProviderCatalogModelCapabilities, ProviderCatalogModelEntry,
    ProviderCatalogModelMode, ProviderCatalogPrice, ResolvedModelRequestOptions,
    allowed_speed_modes, is_speed_mode_word, offered_speed_modes, registry_speed_modes,
    speed_mode_to_send,
};

fn price(input: &str, output: &str) -> ProviderCatalogPrice {
    ProviderCatalogPrice {
        currency: CatalogPriceCurrency::Usd,
        unit: CatalogPriceUnit::PerMillionTokens,
        input: Some(CatalogDecimal::new(input).expect("decimal")),
        output: Some(CatalogDecimal::new(output).expect("decimal")),
        cache_read: None,
        cache_write: None,
    }
}

fn mode(
    body: &[(&str, &str)],
    headers: &[(&str, &str)],
    cost: Option<(&str, &str)>,
) -> ProviderCatalogModelMode {
    let pairs = |items: &[(&str, &str)]| {
        items.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
    };
    ProviderCatalogModelMode {
        price: cost.map(|(input, output)| price(input, output)),
        body: pairs(body),
        headers: pairs(headers),
    }
}

/// OpenAI's `fast` and `flex`, as models.dev lists them for GPT models, and Anthropic's `fast`.
fn listed_modes() -> BTreeMap<String, ProviderCatalogModelMode> {
    BTreeMap::from([
        ("fast".to_owned(), mode(&[("service_tier", "priority")], &[], Some(("4", "20")))),
        ("flex".to_owned(), mode(&[("service_tier", "flex")], &[], Some(("1", "5")))),
        (
            "anthropic-fast".to_owned(),
            mode(&[("speed", "fast")], &[("anthropic-beta", "fast-mode-2026-02-01")], None),
        ),
    ])
}

fn entry(modes: BTreeMap<String, ProviderCatalogModelMode>) -> ProviderCatalogModelEntry {
    ProviderCatalogModelEntry {
        catalog_model_id: "gpt-fixture".to_owned(),
        display_name: "GPT Fixture".to_owned(),
        family: None,
        description: None,
        aliases: BTreeSet::new(),
        context_window_tokens: Some(400_000),
        input_limit_tokens: None,
        output_limit_tokens: Some(128_000),
        input_modalities: BTreeSet::from(["text".to_owned()]),
        output_modalities: BTreeSet::from(["text".to_owned()]),
        capabilities: ProviderCatalogModelCapabilities {
            attachment: false,
            reasoning: true,
            tool_call: true,
            structured_output: false,
            temperature: false,
            interleaved: None,
        },
        reasoning_options: BTreeSet::new(),
        knowledge_cutoff: None,
        release_date: None,
        last_updated: None,
        open_weights: None,
        status: None,
        price: Some(price("2", "10")),
        model_type: None,
        route: None,
        reasoning_field: None,
        price_tiers: Vec::new(),
        modes,
    }
}

fn catalog(
    match_state: CatalogMatchState,
    modes: BTreeMap<String, ProviderCatalogModelMode>,
) -> ModelCatalogMetadata {
    ModelCatalogMetadata {
        catalog_provider_id: "openai".to_owned(),
        catalog_model_id: "gpt-fixture".to_owned(),
        catalog_revision: Revision::INITIAL,
        catalog_content_hash: ContentHash::blake3(b"catalog"),
        catalog_object_hash: ContentHash::blake3(b"model"),
        snapshot: entry(modes),
        matched_at: Some(UtcTimestamp::now()),
        match_state,
    }
}

fn allowing(allowed: bool) -> ModelRequestDefaults {
    ModelRequestDefaults { allow_speed_modes: allowed, ..ModelRequestDefaults::default() }
}

/// The mode a turn that chose `name` is sent with, its profile allowing speed modes.
fn sent(name: Option<&str>, catalog: Option<&ModelCatalogMetadata>) -> Option<ModelSpeedMode> {
    speed_mode_to_send(name, &allowing(true), catalog)
}

#[test]
fn a_turn_is_sent_only_a_mode_its_matched_catalog_entry_offers() {
    let matched = catalog(CatalogMatchState::Matched, listed_modes());
    assert_eq!(
        sent(Some("fast"), Some(&matched)),
        Some(ModelSpeedMode {
            name: "fast".to_owned(),
            body: BTreeMap::from([("service_tier".to_owned(), "priority".to_owned())]),
            headers: BTreeMap::new(),
        })
    );
    assert_eq!(
        sent(Some("anthropic-fast"), Some(&matched)),
        Some(ModelSpeedMode {
            name: "anthropic-fast".to_owned(),
            body: BTreeMap::from([("speed".to_owned(), "fast".to_owned())]),
            headers: BTreeMap::from([(
                "anthropic-beta".to_owned(),
                "fast-mode-2026-02-01".to_owned()
            )]),
        })
    );
    // The standard mode sends nothing; a mode the entry does not list is not sent either.
    assert_eq!(sent(None, Some(&matched)), None);
    assert_eq!(sent(Some("ultrafast"), Some(&matched)), None);
    assert_eq!(sent(Some("fast"), None), None);
    // An entry the model was not matched to offers nothing.
    for state in [CatalogMatchState::Unmatched, CatalogMatchState::Ambiguous] {
        let other = catalog(state, listed_modes());
        assert!(offered_speed_modes(Some(&other)).is_empty(), "{state:?}");
        assert_eq!(sent(Some("fast"), Some(&other)), None, "{state:?}");
    }
    assert_eq!(
        offered_speed_modes(Some(&matched)).into_keys().collect::<Vec<_>>(),
        ["anthropic-fast", "fast", "flex"]
    );
}

#[test]
fn a_mode_sets_only_listed_fields_and_headers_to_plain_words() {
    for word in ["priority", "fast-mode-2026-02-01", "flex", "v1.2_beta", &"a".repeat(64)] {
        assert!(is_speed_mode_word(word), "{word}");
    }
    for word in [
        "",
        " fast",
        "fast mode",
        "fast\r\nx-evil: 1",
        "fast,slow",
        "\"fast\"",
        &"a".repeat(65),
        "速",
    ] {
        assert!(!is_speed_mode_word(word), "{word:?}");
    }
    // Written by another build or by hand: each is refused as a whole, never in part.
    let refused = BTreeMap::from([
        ("unlisted-field".to_owned(), mode(&[("reasoning", "pro")], &[], None)),
        (
            "partly-unlisted".to_owned(),
            mode(&[("service_tier", "priority"), ("store", "true")], &[], None),
        ),
        ("unlisted-header".to_owned(), mode(&[], &[("x-api-key", "anything")], None)),
        ("odd-value".to_owned(), mode(&[("service_tier", "priority\r\nx-evil: 1")], &[], None)),
        ("odd-header-value".to_owned(), mode(&[], &[("anthropic-beta", "a b")], None)),
        ("empty".to_owned(), mode(&[], &[], Some(("1", "1")))),
        ("odd name".to_owned(), mode(&[("service_tier", "priority")], &[], None)),
    ]);
    let mut modes = listed_modes();
    modes.extend(refused.clone());
    let matched = catalog(CatalogMatchState::Matched, modes);
    assert_eq!(
        offered_speed_modes(Some(&matched)).into_keys().collect::<Vec<_>>(),
        ["anthropic-fast", "fast", "flex"]
    );
    for name in refused.keys() {
        assert_eq!(sent(Some(name), Some(&matched)), None, "{name}");
    }
}

/// A registry with one model on a Responses connection, with `defaults` and `catalog`.
fn registry_of(
    defaults: ModelRequestDefaults,
    catalog: Option<ModelCatalogMetadata>,
) -> (ModelRegistryConfig, ModelProfileId) {
    let provider_id = ProviderId::new();
    let endpoint_id = EndpointId::new();
    let model_id = ModelProfileId::new();
    let now = UtcTimestamp::now();
    let endpoint = ModelEndpointConfig {
        id: endpoint_id,
        revision: Revision::INITIAL,
        provider_id,
        display_name: "Responses".to_owned(),
        slug: "responses".parse().expect("valid Endpoint slug"),
        api_format: ModelApiFormat::OpenAiResponses,
        base_url: "https://api.openai.com/v1".to_owned(),
        discovery_mode: ModelDiscoveryMode::Disabled,
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
        model_order: vec![model_id],
        compatibility: None,
        created_at: now,
        updated_at: now,
    };
    let model = ModelProfileConfig {
        id: model_id,
        revision: Revision::INITIAL,
        endpoint_id,
        provider_model_name: "gpt-fixture".to_owned(),
        display_name: "GPT Fixture".to_owned(),
        notes: None,
        origin: ModelProfileOrigin::Discovered,
        enabled: true,
        available_for_subagents: false,
        catalog,
        context: ModelContextWindowConfig {
            catalog_window_tokens: Some(400_000),
            user_override_window_tokens: None,
            user_override_revision: 0,
            runtime_corrected_window_tokens: None,
            runtime_correction_basis: None,
            runtime_effective_window_tokens: 400_000,
            historical_snapshot_max_tokens: 1_024,
        },
        request_defaults: defaults,
        created_at: now,
        updated_at: now,
    };
    let provider = ModelProviderConfig {
        id: provider_id,
        display_name: "OpenAI".to_owned(),
        slug: "openai".parse().expect("valid Provider slug"),
        source: ModelProviderSource::Custom { catalog_provider_id: Some("openai".to_owned()) },
        endpoint_order: vec![endpoint_id],
        created_at: now,
        updated_at: now,
        default_endpoint_id: None,
        backup_endpoint_ids: Vec::new(),
    };
    let registry = ModelRegistryConfig {
        provider_order: vec![provider_id],
        providers: BTreeMap::from([(provider_id, provider)]),
        endpoints: BTreeMap::from([(endpoint_id, endpoint)]),
        models: BTreeMap::from([(model_id, model)]),
    };
    (registry, model_id)
}

/// Until the model's settings allow speed modes, every turn goes in the standard mode, whatever
/// its conversation chose; a mode the profile named before turns chose their own is not sent.
#[test]
fn a_profile_that_does_not_allow_speed_modes_sends_none() {
    let matched = catalog(CatalogMatchState::Matched, listed_modes());
    let named = ModelRequestDefaults {
        speed_mode: Some("fast".to_owned()),
        ..ModelRequestDefaults::default()
    };
    for defaults in [allowing(false), named] {
        for name in [Some("fast"), Some("flex"), None] {
            assert_eq!(speed_mode_to_send(name, &defaults, Some(&matched)), None, "{name:?}");
        }
    }
}

/// A conversation chooses among the modes of a model whose settings allow them and whose
/// matched entry offers them.
#[test]
fn a_conversation_can_choose_the_modes_an_allowing_profile_offers() {
    let matched = || Some(catalog(CatalogMatchState::Matched, listed_modes()));
    let (allowed, model_id) = registry_of(allowing(true), matched());
    assert_eq!(allowed_speed_modes(&allowed.models[&model_id]), ["anthropic-fast", "fast", "flex"]);
    for (defaults, catalog) in [
        (allowing(false), matched()),
        (allowing(true), Some(catalog(CatalogMatchState::Ambiguous, listed_modes()))),
        (allowing(true), Some(catalog(CatalogMatchState::Matched, BTreeMap::new()))),
        (allowing(true), None),
    ] {
        let (registry, model_id) = registry_of(defaults, catalog);
        assert!(allowed_speed_modes(&registry.models[&model_id]).is_empty());
    }
}

#[test]
fn the_settings_list_each_models_modes_with_their_prices() {
    let (registry, model_id) = registry_of(
        allowing(false),
        Some(catalog(CatalogMatchState::Matched, {
            let mut modes = listed_modes();
            modes.insert("unlisted-field".to_owned(), mode(&[("reasoning", "pro")], &[], None));
            modes
        })),
    );
    assert_eq!(
        registry_speed_modes(&registry),
        [ModelSpeedModesView {
            model_profile_id: model_id,
            standard_price: Some(price("2", "10")),
            modes: vec![
                ModelSpeedModeView { name: "anthropic-fast".to_owned(), price: None },
                ModelSpeedModeView { name: "fast".to_owned(), price: Some(price("4", "20")) },
                ModelSpeedModeView { name: "flex".to_owned(), price: Some(price("1", "5")) },
            ],
        }]
    );
    // A model offering none is absent.
    let (plain, _) =
        registry_of(allowing(true), Some(catalog(CatalogMatchState::Matched, BTreeMap::new())));
    assert!(registry_speed_modes(&plain).is_empty());
    let (unmatched, _) =
        registry_of(allowing(true), Some(catalog(CatalogMatchState::Unmatched, listed_modes())));
    assert!(registry_speed_modes(&unmatched).is_empty());
    let (manual, _) = registry_of(allowing(true), None);
    assert!(registry_speed_modes(&manual).is_empty());
}

#[test]
fn a_profile_or_request_naming_no_mode_is_written_as_before() {
    let defaults = serde_json::to_value(allowing(false)).expect("defaults JSON");
    assert!(defaults.get("speed_mode").is_none(), "{defaults}");
    assert!(defaults.get("allow_speed_modes").is_none(), "{defaults}");
    let options =
        serde_json::to_value(ResolvedModelRequestOptions::output_only(512)).expect("options JSON");
    assert!(options.get("speed_mode").is_none(), "{options}");
    // Allowing is kept, and read back as written; so is a mode a profile named before.
    let allowed = allowing(true);
    let written = serde_json::to_value(&allowed).expect("defaults JSON");
    assert_eq!(written["allow_speed_modes"], true);
    assert_eq!(
        serde_json::from_value::<ModelRequestDefaults>(written).expect("read back"),
        allowed
    );
    let named = serde_json::from_value::<ModelRequestDefaults>(serde_json::json!({
        "max_output_tokens": null, "temperature": null, "top_p": null,
        "chat_output_token_field": null, "chat_instruction_role": null, "speed_mode": "fast"
    }))
    .expect("a mode a profile named before is read");
    assert_eq!(named.speed_mode.as_deref(), Some("fast"));
    let sent = ResolvedModelRequestOptions {
        speed_mode: sent(Some("fast"), Some(&catalog(CatalogMatchState::Matched, listed_modes()))),
        ..ResolvedModelRequestOptions::output_only(512)
    };
    let written = serde_json::to_value(&sent).expect("options JSON");
    assert_eq!(written["speed_mode"]["body"]["service_tier"], "priority");
    assert_eq!(
        serde_json::from_value::<ResolvedModelRequestOptions>(written).expect("read back"),
        sent
    );
}
