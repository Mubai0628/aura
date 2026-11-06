//! What a model's models.dev entry says beyond its limits and capabilities: its type, its own
//! route, its reasoning field, its price tiers and its modes. A malformed one of these leaves
//! out only itself, never the catalog.
use aura_model_catalog::parse_models_dev_catalog;
use aura_model_domain::{
    CatalogDecimal, ProviderCatalogModelEntry, ProviderCatalogModelRoute, ProviderCatalogPrice,
};
use serde_json::{Value, json};

fn model(fields: Value) -> ProviderCatalogModelEntry {
    let mut model = json!({ "id": "m", "name": "M" });
    for (key, value) in fields.as_object().expect("fields are an object") {
        model[key] = value.clone();
    }
    let catalog = json!({ "p": { "id": "p", "name": "P", "models": { "m": model } } });
    let bytes = serde_json::to_vec(&catalog).expect("catalog JSON");
    let mut providers = parse_models_dev_catalog(&bytes).expect("the catalog is read");
    providers.remove("p").expect("provider").models.remove("m").expect("model")
}

fn decimal(value: &str) -> Option<CatalogDecimal> {
    Some(value.parse().expect("decimal"))
}

fn price(input: &str, output: &str, cache_read: Option<&str>) -> ProviderCatalogPrice {
    ProviderCatalogPrice {
        currency: aura_model_domain::CatalogPriceCurrency::Usd,
        unit: aura_model_domain::CatalogPriceUnit::PerMillionTokens,
        input: decimal(input),
        output: decimal(output),
        cache_read: cache_read.and_then(decimal),
        cache_write: None,
    }
}

#[test]
fn a_models_type_is_read_and_only_chat_models_are_chat() {
    let plain = model(json!({}));
    assert_eq!(plain.model_type, None);
    assert!(plain.is_chat());
    let embedding = model(json!({ "type": "embedding" }));
    assert_eq!(embedding.model_type.as_deref(), Some("embedding"));
    assert!(!embedding.is_chat());
    assert!(model(json!({ "type": "chat" })).is_chat());
    assert!(!model(json!({ "type": "realtime" })).is_chat());
}

#[test]
fn a_models_own_route_is_read() {
    let routed = model(json!({ "provider": {
        "api": "https://example.test/anthropic/v1",
        "npm": "@ai-sdk/anthropic",
        "shape": "responses"
    } }));
    assert_eq!(
        routed.route,
        Some(ProviderCatalogModelRoute {
            api_url: Some("https://example.test/anthropic/v1".into()),
            npm_package: Some("@ai-sdk/anthropic".into()),
            shape: Some("responses".into()),
        })
    );
    let npm_only = model(json!({ "provider": { "npm": "@ai-sdk/openai" } }));
    assert_eq!(
        npm_only.route.and_then(|route| route.npm_package).as_deref(),
        Some("@ai-sdk/openai")
    );
    assert_eq!(model(json!({})).route, None);
    assert_eq!(model(json!({ "provider": {} })).route, None);
}

#[test]
fn the_reasoning_field_comes_from_interleaved() {
    let details = model(json!({ "interleaved": { "field": "reasoning_details" } }));
    assert_eq!(details.reasoning_field.as_deref(), Some("reasoning_details"));
    assert_eq!(details.capabilities.interleaved, Some(true));
    let plain = model(json!({ "interleaved": true }));
    assert_eq!(plain.reasoning_field, None);
    assert_eq!(plain.capabilities.interleaved, Some(true));
}

#[test]
fn context_price_tiers_are_read_by_ascending_length() {
    let tiered = model(json!({ "cost": {
        "input": 1,
        "output": 2,
        "tiers": [
            { "input": 4, "output": 8, "tier": { "type": "context", "size": 272_000 } },
            { "input": 2, "output": 4, "cache_read": 0.5, "tier": { "type": "context", "size": 128_000 } },
            { "input": 9, "output": 9, "tier": { "type": "requests", "size": 10 } }
        ],
        "context_over_200k": { "input": 3, "output": 6 }
    } }));
    let tiers: Vec<_> = tiered
        .price_tiers
        .iter()
        .map(|tier| (tier.from_context_tokens, tier.price.clone()))
        .collect();
    assert_eq!(tiers, [(128_000, price("2", "4", Some("0.5"))), (272_000, price("4", "8", None))]);
    assert_eq!(tiered.price, Some(price("1", "2", None)));
}

#[test]
fn only_modes_that_change_listed_fields_are_kept() {
    let modes = model(json!({ "experimental": { "modes": {
        "fast": {
            "cost": { "input": 4, "output": 20 },
            "provider": { "body": { "service_tier": "priority" } }
        },
        "turbo": {
            "provider": {
                "body": { "speed": "fast" },
                "headers": { "anthropic-beta": "fast-mode-2026-02-01" }
            }
        },
        "pro": { "provider": { "body": { "reasoning": { "mode": "pro" } } } },
        "nested": {
            "provider": { "body": { "additionalModelRequestFields": { "service_tier": "flex" } } }
        },
        "other-header": { "provider": { "headers": { "x-other": "1" } } },
        "elsewhere": {
            "provider": { "body": { "service_tier": "flex" }, "api": "https://elsewhere.test/v1" }
        },
        "number": { "provider": { "body": { "service_tier": 1 } } },
        "nothing": { "cost": { "input": 1, "output": 1 } }
    } } }))
    .modes;
    assert_eq!(modes.keys().map(String::as_str).collect::<Vec<_>>(), ["fast", "turbo"]);
    assert_eq!(modes["fast"].price, Some(price("4", "20", None)));
    assert_eq!(modes["fast"].body["service_tier"], "priority");
    assert!(modes["fast"].headers.is_empty());
    assert_eq!(modes["turbo"].price, None);
    assert_eq!(modes["turbo"].body["speed"], "fast");
    assert_eq!(modes["turbo"].headers["anthropic-beta"], "fast-mode-2026-02-01");
}

#[test]
fn a_malformed_detail_leaves_out_only_itself() {
    let odd = model(json!({
        "type": 3,
        "provider": "elsewhere",
        "interleaved": { "field": 1 },
        "cost": {
            "input": 1,
            "output": 2,
            "tiers": [
                { "input": -1, "output": 2, "tier": { "type": "context", "size": 10 } },
                { "input": 1, "output": 2, "tier": { "type": "context" } },
                "cheap",
                { "input": 3, "output": 4, "tier": { "type": "context", "size": 64_000 } }
            ]
        },
        "experimental": { "modes": 5 }
    }));
    assert_eq!(odd.model_type, None);
    assert_eq!(odd.route, None);
    assert_eq!(odd.reasoning_field, None);
    assert_eq!(odd.capabilities.interleaved, Some(true));
    let tiers: Vec<_> = odd.price_tiers.iter().map(|tier| tier.from_context_tokens).collect();
    assert_eq!(tiers, [64_000]);
    assert!(odd.modes.is_empty());
    assert_eq!(odd.price, Some(price("1", "2", None)));
}
