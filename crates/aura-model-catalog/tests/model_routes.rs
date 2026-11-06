//! A Provider's model page says which of its routes each model goes by, as the configuration
//! policy decides; a policy that decides nothing about routes lets every model go by any.
use std::collections::BTreeMap;

use aura_domain_common::{ArtifactId, ContentHash, Revision, UtcTimestamp};
use aura_model_catalog::{
    ProviderConfigurationPolicy, parse_models_dev_catalog, project_provider_model_page,
};
use aura_model_domain::{
    ModelCatalogProviderGetRequest, ModelRegistryConfig, ProviderCatalogConfigurationClass,
    ProviderCatalogEntry, ProviderCatalogModelEntry, ProviderCatalogModelPage,
    ProviderCatalogModelRouteView, ProviderCatalogSnapshot, ProviderCatalogState,
    ProviderCatalogStatus,
};

const CATALOG: &[u8] = br#"{
  "provider-a": {
    "name": "Provider A",
    "npm": "@ai-sdk/openai-compatible",
    "api": "https://api.example.test/v1",
    "models": {
      "own-route": {
        "name": "Own Route",
        "provider": { "npm": "@ai-sdk/anthropic", "api": "https://api.example.test/anthropic/v1" }
      },
      "provider-route": { "name": "Provider Route" }
    }
  }
}"#;

fn status() -> ProviderCatalogStatus {
    let now = UtcTimestamp::now();
    let snapshot = ProviderCatalogSnapshot {
        catalog_revision: Revision::INITIAL,
        catalog_content_hash: ContentHash::blake3(CATALOG),
        checked_at: now,
        last_successful_refresh_at: now,
        etag: None,
        last_modified: None,
        raw_artifact_id: ArtifactId::new(),
        provider_order: vec!["provider-a".to_owned()],
        providers: parse_models_dev_catalog(CATALOG).expect("catalog"),
        parser_revision: aura_model_catalog::CATALOG_PARSER_REVISION,
    };
    ProviderCatalogStatus {
        state: ProviderCatalogState::Fresh,
        checked_at: Some(now),
        last_successful_refresh_at: Some(now),
        snapshot: Some(snapshot),
        last_error_code: None,
    }
}

/// Sends a model with its own route to the variant `anthropic`, and refuses the rest.
struct RoutingPolicy;

impl ProviderConfigurationPolicy for RoutingPolicy {
    fn class_for(&self, _entry: &ProviderCatalogEntry) -> ProviderCatalogConfigurationClass {
        ProviderCatalogConfigurationClass::TrustedPreset
    }

    fn model_route_for(
        &self,
        _entry: &ProviderCatalogEntry,
        model: &ProviderCatalogModelEntry,
    ) -> ProviderCatalogModelRouteView {
        if model.route.is_some() {
            ProviderCatalogModelRouteView::Variant { variant_id: "anthropic".to_owned() }
        } else {
            ProviderCatalogModelRouteView::Unsupported
        }
    }
}

fn page(policy: &impl ProviderConfigurationPolicy) -> ProviderCatalogModelPage {
    project_provider_model_page(
        &status(),
        &ModelRegistryConfig::default(),
        policy,
        &ModelCatalogProviderGetRequest {
            catalog_provider_id: "provider-a".to_owned(),
            query: None,
            expected_catalog_revision: None,
            after_model_id: None,
            limit: Some(50),
        },
        &BTreeMap::new(),
    )
    .expect("page")
}

fn routes(page: &ProviderCatalogModelPage) -> Vec<(&str, &ProviderCatalogModelRouteView)> {
    page.models.iter().map(|view| (view.model.catalog_model_id.as_str(), &view.route)).collect()
}

#[test]
fn the_page_says_which_route_each_model_goes_by() {
    let page = page(&RoutingPolicy);
    assert_eq!(
        routes(&page),
        [
            (
                "own-route",
                &ProviderCatalogModelRouteView::Variant { variant_id: "anthropic".to_owned() }
            ),
            ("provider-route", &ProviderCatalogModelRouteView::Unsupported),
        ]
    );
}

#[test]
fn a_policy_that_decides_nothing_about_routes_lets_every_model_go_by_any() {
    let policy =
        |_entry: &ProviderCatalogEntry| ProviderCatalogConfigurationClass::ManualConfiguration;
    let page = page(&policy);
    assert!(page.models.iter().all(|view| view.route == ProviderCatalogModelRouteView::Any));
    assert_eq!(page.models.len(), 2);
}

#[test]
fn a_model_view_without_a_route_reads_as_any() {
    let view = serde_json::json!({
        "catalog_object_hash": ContentHash::blake3(b"model"),
        "model": serde_json::to_value(&page(&RoutingPolicy).models[0].model).expect("model"),
    });
    let view: aura_model_domain::ProviderCatalogModelView =
        serde_json::from_value(view).expect("an earlier view");
    assert_eq!(view.route, ProviderCatalogModelRouteView::Any);
}
