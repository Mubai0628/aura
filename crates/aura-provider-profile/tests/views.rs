//! What the catalog page shows of a built-in profile: a Provider's routes as preset variants,
//! marked as alternatives or as one per model's route, with the values of the user's own each
//! one's address takes, and which variant each model goes by.
use std::collections::BTreeMap;

use aura_model_domain::{
    ProviderCatalogConnectionDefaultsView, ProviderCatalogEntry, ProviderCatalogModelRoute,
    ProviderCatalogModelRouteView, ProviderPresetParameterKind, ProviderPresetParameterView,
    ProviderPresetVariantView,
};
use aura_provider_profile::{model_route_view, preset_defaults};

fn variants(catalog_provider_id: &str) -> (Vec<ProviderPresetVariantView>, bool) {
    match preset_defaults(catalog_provider_id) {
        Some(ProviderCatalogConnectionDefaultsView::AuraPreset { variants, by_model }) => {
            (variants, by_model)
        }
        other => panic!("{catalog_provider_id}: {other:?}"),
    }
}

#[test]
fn opencode_go_offers_its_routes_together_and_none_alone() {
    let (variants, by_model) = variants("opencode-go");
    assert!(by_model);
    let ids: Vec<_> = variants.iter().map(|variant| variant.variant_id.as_str()).collect();
    assert_eq!(ids, ["chat-completions", "responses", "anthropic-messages"]);
    assert!(variants.iter().all(|variant| !variant.recommended));
}

#[test]
fn a_provider_with_alternative_routes_recommends_one() {
    let (variants, by_model) = variants("deepseek");
    assert!(!by_model);
    assert_eq!(variants.len(), 3);
    assert_eq!(variants.iter().filter(|variant| variant.recommended).count(), 1);
    assert_eq!(preset_defaults("ollama-cloud"), None);
}

#[test]
fn each_model_goes_by_the_variant_of_its_route() {
    let go = ProviderCatalogEntry {
        catalog_provider_id: "opencode-go".to_owned(),
        display_name: "OpenCode Go".to_owned(),
        npm_package: Some("@ai-sdk/openai-compatible".to_owned()),
        api_url: Some("https://opencode.ai/inference/go/openai/v1".to_owned()),
        documentation_url: None,
        model_order: Vec::new(),
        models: BTreeMap::new(),
    };
    let own = |npm: &str, api: &str| ProviderCatalogModelRoute {
        api_url: Some(api.to_owned()),
        npm_package: Some(npm.to_owned()),
        shape: None,
    };
    assert_eq!(
        model_route_view(&go, None),
        ProviderCatalogModelRouteView::Variant { variant_id: "chat-completions".to_owned() }
    );
    assert_eq!(
        model_route_view(
            &go,
            Some(&own("@ai-sdk/anthropic", "https://opencode.ai/inference/go/anthropic/v1"))
        ),
        ProviderCatalogModelRouteView::Variant { variant_id: "anthropic-messages".to_owned() }
    );
    assert_eq!(
        model_route_view(
            &go,
            Some(&own("@ai-sdk/google", "https://opencode.ai/inference/go/google/v1beta"))
        ),
        ProviderCatalogModelRouteView::Unsupported
    );
    let deepseek =
        ProviderCatalogEntry { catalog_provider_id: "deepseek".to_owned(), ..go.clone() };
    assert_eq!(model_route_view(&deepseek, None), ProviderCatalogModelRouteView::Any);
}

#[test]
fn a_variant_names_the_values_of_the_users_own_its_address_takes() {
    let (variants_, by_model) = variants("amazon-bedrock");
    assert!(by_model);
    let region = vec![ProviderPresetParameterView {
        name: "region".to_owned(),
        display_name: "区域".to_owned(),
        kind: ProviderPresetParameterKind::DomainLabel,
        example: "us-east-1".to_owned(),
    }];
    let summary = variants_
        .iter()
        .map(|variant| {
            (
                variant.variant_id.as_str(),
                variant.base_url.as_str(),
                variant.base_url_parameters.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        vec![
            ("mantle-v1", "https://bedrock-mantle.{region}.api.aws/v1", region.clone()),
            ("mantle-openai-v1", "https://bedrock-mantle.{region}.api.aws/openai/v1", region),
        ]
    );
    let (deepseek, _) = variants("deepseek");
    assert!(deepseek.iter().all(|variant| variant.base_url_parameters.is_empty()));
}
