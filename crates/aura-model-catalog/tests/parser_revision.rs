//! A cached snapshot read by an older parser is read again: it counts as stale, its refresh asks
//! for the whole catalog without the cached validators, and the same content is parsed anew
//! instead of revalidated, so what the newer parser reads is there at once.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use aura_domain_common::{ArtifactId, ContentHash, Revision, UtcTimestamp};
use aura_model_catalog::{
    CATALOG_PARSER_REVISION, CatalogCache, CatalogConditionalRequest, CatalogError,
    CatalogFetchResponse, CatalogSource, InMemoryCatalogCache, ProviderCatalogClient,
    ProviderCatalogService, parse_models_dev_catalog,
};
use aura_model_domain::{ProviderCatalogSnapshot, ProviderCatalogState};

const CATALOG: &[u8] = br#"{
  "provider-a": {
    "name": "Provider A",
    "models": {
      "chat-model": { "name": "Chat Model" },
      "embedder": { "name": "Embedder", "type": "embedding" }
    }
  }
}"#;

#[derive(Default)]
struct Source {
    responses: Mutex<VecDeque<CatalogFetchResponse>>,
    conditionals: Mutex<Vec<CatalogConditionalRequest>>,
}

impl CatalogSource for Source {
    async fn fetch(
        &self,
        conditional: CatalogConditionalRequest,
    ) -> Result<CatalogFetchResponse, CatalogError> {
        self.conditionals.lock().expect("conditionals").push(conditional);
        Ok(self.responses.lock().expect("responses").pop_front().expect("scripted response"))
    }
}

/// The snapshot an older parser left: the same content, without the model types it did not read.
fn older_snapshot() -> ProviderCatalogSnapshot {
    let mut providers = parse_models_dev_catalog(CATALOG).expect("catalog");
    for model in providers.values_mut().flat_map(|provider| provider.models.values_mut()) {
        model.model_type = None;
    }
    let now = UtcTimestamp::now();
    ProviderCatalogSnapshot {
        catalog_revision: Revision::new(4),
        catalog_content_hash: ContentHash::blake3(CATALOG),
        checked_at: now,
        last_successful_refresh_at: now,
        etag: Some("\"catalog-v4\"".to_owned()),
        last_modified: Some("Mon, 02 Nov 2026 00:00:00 GMT".to_owned()),
        raw_artifact_id: ArtifactId::new(),
        provider_order: vec!["provider-a".to_owned()],
        providers,
        parser_revision: 0,
    }
}

#[tokio::test]
async fn a_snapshot_from_an_older_parser_is_fetched_whole_and_read_again() {
    let cache = Arc::new(InMemoryCatalogCache::default());
    cache.persist_revalidated(older_snapshot()).expect("cached snapshot");
    let source = Arc::new(Source::default());
    source.responses.lock().expect("responses").push_back(CatalogFetchResponse {
        status: 200,
        etag: Some("\"catalog-v4\"".to_owned()),
        last_modified: None,
        body: CATALOG.to_vec(),
    });
    let service =
        ProviderCatalogService::open(Arc::clone(&source), Arc::clone(&cache)).expect("service");
    assert_eq!(service.status().expect("status").state, ProviderCatalogState::Stale);
    assert!(service.needs_refresh(UtcTimestamp::now()));

    let fresh = service.refresh().await.expect("refresh");
    assert_eq!(
        source.conditionals.lock().expect("conditionals")[0],
        CatalogConditionalRequest::default()
    );
    assert_eq!(fresh.parser_revision, CATALOG_PARSER_REVISION);
    assert_eq!(fresh.catalog_revision, Revision::new(5));
    let embedder = &fresh.providers["provider-a"].models["embedder"];
    assert_eq!(embedder.model_type.as_deref(), Some("embedding"));
    assert!(!service.needs_refresh(UtcTimestamp::now()));
}

#[tokio::test]
async fn a_snapshot_from_this_parser_is_revalidated_as_before() {
    let mut current = older_snapshot();
    current.parser_revision = CATALOG_PARSER_REVISION;
    let cache = Arc::new(InMemoryCatalogCache::default());
    cache.persist_revalidated(current).expect("cached snapshot");
    let source = Arc::new(Source::default());
    source.responses.lock().expect("responses").push_back(CatalogFetchResponse {
        status: 304,
        etag: None,
        last_modified: None,
        body: Vec::new(),
    });
    let service =
        ProviderCatalogService::open(Arc::clone(&source), Arc::clone(&cache)).expect("service");
    assert_eq!(service.status().expect("status").state, ProviderCatalogState::Fresh);

    let revalidated = service.refresh().await.expect("refresh");
    let conditionals = source.conditionals.lock().expect("conditionals");
    assert_eq!(conditionals[0].etag.as_deref(), Some("\"catalog-v4\""));
    assert_eq!(revalidated.catalog_revision, Revision::new(4));
}
