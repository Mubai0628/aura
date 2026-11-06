//! A provider's catalog card and model page list its chat models only: image, embedding, speech
//! and the other kinds are not models a chat can use.
use std::collections::BTreeMap;

use aura_domain_common::{ArtifactId, ContentHash, Revision, UtcTimestamp};
use aura_model_catalog::{
    parse_models_dev_catalog, project_provider_cards, project_provider_model_page,
};
use aura_model_domain::{
    ModelCatalogProviderGetRequest, ModelRegistryConfig, ProviderCatalogConfigurationClass,
    ProviderCatalogEntry, ProviderCatalogSnapshot, ProviderCatalogState, ProviderCatalogStatus,
};

const CATALOG: &[u8] = br#"{
  "provider-a": {
    "name": "Provider A",
    "models": {
      "chat-model": { "name": "Chat Model" },
      "chat-typed": { "name": "Chat Typed", "type": "chat" },
      "embedder": { "name": "Embedder", "type": "embedding" },
      "painter": { "name": "Painter", "type": "image" },
      "talker": { "name": "Talker", "type": "realtime" }
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

fn policy(_entry: &ProviderCatalogEntry) -> ProviderCatalogConfigurationClass {
    ProviderCatalogConfigurationClass::ManualConfiguration
}

#[test]
fn the_card_counts_only_chat_models() {
    let cards = project_provider_cards(
        &status(),
        &ModelRegistryConfig::default(),
        &policy,
        &BTreeMap::new(),
    )
    .expect("cards");
    assert_eq!(cards[0].catalog_model_count, 2);
}

#[test]
fn the_model_page_lists_only_chat_models() {
    let page = project_provider_model_page(
        &status(),
        &ModelRegistryConfig::default(),
        &policy,
        &ModelCatalogProviderGetRequest {
            catalog_provider_id: "provider-a".to_owned(),
            query: None,
            expected_catalog_revision: None,
            after_model_id: None,
            limit: Some(50),
        },
        &BTreeMap::new(),
    )
    .expect("page");
    let ids: Vec<_> = page.models.iter().map(|view| view.model.catalog_model_id.as_str()).collect();
    assert_eq!(ids, ["chat-model", "chat-typed"]);
    assert_eq!(page.provider.catalog_model_count, 2);
    assert_eq!(page.next_model_id, None);
}
