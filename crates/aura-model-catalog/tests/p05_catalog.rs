use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

use aura_artifact_contract::{ArtifactAccess, ArtifactScope};
use aura_domain_common::{
    ArtifactId, CommandInvocationId, ContentHash, CorrelationId, EndpointId, IpcChannelId,
    ModelDiscoveryRunId, NetworkRequestId, NormalizedIssueCode, ProviderId, Revision,
    RuntimeInstanceId, TransportAttemptId, UtcTimestamp,
};
use aura_model_catalog::{
    CatalogCache, CatalogCacheRecord, CatalogConditionalRequest, CatalogError,
    CatalogFetchResponse, CatalogSource, CoreGatewayEndpointDiscoverySource,
    EndpointDiscoveryRequestAuthorizer, EndpointDiscoverySource, InMemoryCatalogCache,
    PersistenceCatalogCache, ProviderCatalogClient, ProviderCatalogService, match_catalog_model,
    parse_model_discovery_page, parse_models_dev_catalog, project_provider_cards,
    project_provider_model_page,
};
use aura_model_domain::{
    ModelApiFormat, ModelCatalogProviderGetRequest, ModelProviderConfig, ModelProviderSource,
    ModelRegistryConfig, ProviderCatalogConfigurationClass, ProviderCatalogEntry,
    ProviderCatalogMatch, ProviderCatalogSnapshot, ProviderCatalogState, ProviderCatalogStatus,
    TransportOperationContext,
};
use aura_model_fixtures::{FixtureServer, ModelProviderScenario};
use aura_network_control::{
    AuthenticatedConfigurationChannelIdentity, CONFIGURATION_MANIFEST_SCHEMA_VERSION,
    CONFIGURATION_REDIRECT_MAX_HOPS, ConfigurationOperationKind,
    ConfigurationOperationRequestManifestV1, ConfigurationRequestDataClass,
    ConfigurationRequestPurpose, ConfigurationRequestTargetV1, ConfigurationResponseMediaPolicyV1,
    ConfigurationTransportPolicyV1, CoreHttpClientGateway, InMemoryNetworkGrantStore,
    NetworkControlError, NetworkControlOptions, ProviderTransportGatewayRequest,
    ProviderTransportImmutableRequest, ProviderTransportRequestAuthority,
    UserConfigurationOperationIdentity, canonical_non_secret_headers_hash,
    issue_configuration_provider_transport_grant,
};
use aura_permission_domain::{NetworkByteLimits, NormalizedOrigin, RedirectPolicy};
use aura_persistence::{
    ArtifactMetadataRequest, PersistenceCoordinator, PersistenceOptions, PersistenceStartup,
};

const CATALOG: &[u8] = br#"{
  "provider-a": {
    "name": "Provider A",
    "npm": "@ai-sdk/openai-compatible",
    "api": "https://api.example.test/v1",
    "models": {
      "model-exact": {
        "name": "Model Exact",
        "aliases": ["model-alias"],
        "attachment": false,
        "reasoning": true,
        "tool_call": true,
        "limit": {"context": 128000, "output": 8192}
      }
    }
  }
}"#;

struct ScriptedSource {
    responses: Mutex<VecDeque<Result<CatalogFetchResponse, CatalogError>>>,
    conditionals: Mutex<Vec<CatalogConditionalRequest>>,
}

#[derive(Clone)]
struct LoopbackDiscoveryAuthorizer {
    origin: NormalizedOrigin,
    request_url: String,
}

impl EndpointDiscoveryRequestAuthorizer for LoopbackDiscoveryAuthorizer {
    async fn authorize_endpoint_discovery(
        &self,
    ) -> Result<ProviderTransportGatewayRequest, CatalogError> {
        let request_id = NetworkRequestId::new();
        let frozen_limits = ConfigurationTransportPolicyV1::MODEL_DISCOVERY;
        let byte_limits = NetworkByteLimits {
            max_upload_bytes: frozen_limits.max_request_body_bytes,
            max_download_bytes: frozen_limits.max_response_wire_bytes,
            max_decompressed_bytes: frozen_limits.max_response_decompressed_bytes,
        };
        let endpoint_id = EndpointId::new();
        let discovery_run_id = ModelDiscoveryRunId::new();
        let transport_attempt_id = TransportAttemptId::new();
        let operation_context = TransportOperationContext::ModelDiscovery { discovery_run_id };
        let correlation_id = CorrelationId::new();
        let command_invocation_id = CommandInvocationId::new();
        let identity = UserConfigurationOperationIdentity::from_authenticated_settings_command(
            command_invocation_id,
            correlation_id,
            AuthenticatedConfigurationChannelIdentity {
                core_runtime_instance_id: RuntimeInstanceId::new(),
                authenticated_main_runtime_instance_id: RuntimeInstanceId::new(),
                authenticated_channel_id: IpcChannelId::new(),
            },
            ConfigurationOperationKind::ModelDiscovery,
            endpoint_id,
            Revision::INITIAL,
            UtcTimestamp::now(),
        );
        let origin_prefix =
            format!("{}://{}:{}", self.origin.scheme, self.origin.host, self.origin.port);
        let path_and_query = self
            .request_url
            .strip_prefix(&origin_prefix)
            .filter(|path| path.starts_with('/'))
            .ok_or_else(|| {
                CatalogError::Source("fixture discovery URL is outside its origin".to_owned())
            })?
            .to_owned();
        let request_headers = Vec::new();
        let manifest = ConfigurationOperationRequestManifestV1 {
            schema_version: CONFIGURATION_MANIFEST_SCHEMA_VERSION,
            command_invocation_id,
            correlation_id,
            user_configuration_operation_identity_binding_hash: identity
                .binding_hash()
                .map_err(|error| CatalogError::Source(error.to_string()))?,
            purpose: ConfigurationRequestPurpose::ModelDiscovery {
                model_discovery_run_id: discovery_run_id,
            },
            transport_attempt_id,
            endpoint_id,
            models_config_revision: Revision::INITIAL,
            endpoint_security_fingerprint: ContentHash::sha256(origin_prefix.as_bytes()),
            target: ConfigurationRequestTargetV1 {
                method: "GET".to_owned(),
                origin: self.origin.clone(),
                path_and_query,
            },
            non_secret_headers_hash: canonical_non_secret_headers_hash(&request_headers)
                .map_err(|error| CatalogError::Source(error.to_string()))?,
            request_body_hash: ContentHash::sha256(b""),
            request_body_length: 0,
            credential_binding: None,
            max_request_bytes: byte_limits.max_upload_bytes,
            max_response_bytes: byte_limits.max_download_bytes,
            max_decompressed_response_bytes: byte_limits.max_decompressed_bytes,
            redirect_policy: RedirectPolicy::SameOrigin {
                max_hops: CONFIGURATION_REDIRECT_MAX_HOPS,
            },
            response_media_policy: ConfigurationResponseMediaPolicyV1 {
                accepted_media_types: BTreeSet::from(["application/json".to_owned()]),
                require_content_type: true,
            },
            data_class: ConfigurationRequestDataClass::ProtocolMetadata,
        };
        let grant = issue_configuration_provider_transport_grant(
            &identity,
            operation_context,
            &manifest,
            request_id,
            UtcTimestamp::now(),
        )
        .map_err(|error| CatalogError::Source(error.to_string()))?;
        let immutable_request = ProviderTransportImmutableRequest {
            request_id,
            method: "GET".to_owned(),
            requested_url: self.request_url.clone(),
            headers: request_headers,
            body: Vec::new(),
            redirect_policy: manifest.redirect_policy,
            byte_limits,
            connect_timeout_ms: 2_000,
            timeout_ms: Some(2_000),
        };
        Ok(ProviderTransportGatewayRequest {
            authority: ProviderTransportRequestAuthority::UserConfiguration(identity),
            grant,
            immutable_request,
        })
    }
}

impl ScriptedSource {
    fn new(responses: Vec<Result<CatalogFetchResponse, CatalogError>>) -> Self {
        Self { responses: Mutex::new(responses.into()), conditionals: Mutex::new(Vec::new()) }
    }
}

impl CatalogSource for ScriptedSource {
    async fn fetch(
        &self,
        conditional: CatalogConditionalRequest,
    ) -> Result<CatalogFetchResponse, CatalogError> {
        self.conditionals.lock().expect("conditionals").push(conditional);
        self.responses.lock().expect("responses").pop_front().expect("scripted response")
    }
}

#[tokio::test]
async fn successful_refresh_uses_conditionals_and_failure_keeps_stale_lkg() {
    let source = Arc::new(ScriptedSource::new(vec![
        Ok(CatalogFetchResponse {
            status: 200,
            etag: Some("\"catalog-v1\"".to_owned()),
            last_modified: Some("Wed, 01 Jan 2026 00:00:00 GMT".to_owned()),
            body: CATALOG.to_vec(),
        }),
        Ok(CatalogFetchResponse { status: 304, etag: None, last_modified: None, body: Vec::new() }),
        Err(CatalogError::Source("offline".to_owned())),
    ]));
    let cache = Arc::new(InMemoryCatalogCache::default());
    let service = ProviderCatalogService::open(Arc::clone(&source), Arc::clone(&cache))
        .expect("catalog service");

    let first = service.refresh().await.expect("initial refresh");
    assert_eq!(first.providers["provider-a"].models.len(), 1);
    let revalidated = service.refresh().await.expect("not modified");
    assert_eq!(revalidated.catalog_revision, first.catalog_revision);
    assert!(service.refresh().await.is_err());

    let status = service.status().expect("status");
    assert_eq!(status.state, ProviderCatalogState::RefreshFailedWithLkg);
    assert_eq!(
        status.snapshot.expect("last successful snapshot").catalog_revision,
        first.catalog_revision
    );
    let conditionals = source.conditionals.lock().expect("conditionals");
    assert_eq!(conditionals[0], CatalogConditionalRequest::default());
    assert_eq!(conditionals[1].etag.as_deref(), Some("\"catalog-v1\""));
}

#[tokio::test]
async fn invalid_catalog_does_not_replace_the_last_successful_snapshot() {
    let source = ScriptedSource::new(vec![
        Ok(CatalogFetchResponse {
            status: 200,
            etag: None,
            last_modified: None,
            body: CATALOG.to_vec(),
        }),
        Ok(CatalogFetchResponse {
            status: 200,
            etag: None,
            last_modified: None,
            body: br#"{}"#.to_vec(),
        }),
    ]);
    let service = ProviderCatalogService::open(source, InMemoryCatalogCache::default())
        .expect("catalog service");
    let first = service.refresh().await.expect("initial refresh");
    assert!(service.refresh().await.is_err());
    assert_eq!(
        service.current_snapshot().expect("last successful").catalog_revision,
        first.catalog_revision
    );
    assert_eq!(service.status().expect("status").state, ProviderCatalogState::RefreshFailedWithLkg);
}

/// models.dev passed the former 5 MiB ceiling in October 2026; Core now admits 32 MiB.
#[tokio::test]
async fn a_catalog_above_the_former_five_mib_ceiling_is_consumed_as_one_complete_response() {
    const TARGET_BYTES: usize = 8 * 1024 * 1024;
    let prefix = br#"{"provider-large":{"name":"Provider Large","padding":""#;
    let suffix = br#"","models":{}}}"#;
    let padding_length = TARGET_BYTES
        .checked_sub(prefix.len() + suffix.len())
        .expect("fixture overhead must fit the target size");
    let mut body = Vec::with_capacity(TARGET_BYTES);
    body.extend_from_slice(prefix);
    body.extend(std::iter::repeat_n(b'x', padding_length));
    body.extend_from_slice(suffix);
    assert_eq!(body.len(), TARGET_BYTES);

    let source = ScriptedSource::new(vec![Ok(CatalogFetchResponse {
        status: 200,
        etag: Some("\"large-v1\"".to_owned()),
        last_modified: None,
        body,
    })]);
    let service = ProviderCatalogService::open(source, InMemoryCatalogCache::default())
        .expect("catalog service");

    let snapshot = service.refresh().await.expect("complete large Catalog refresh");
    assert_eq!(snapshot.catalog_revision, Revision::INITIAL);
    assert_eq!(snapshot.provider_order, ["provider-large"]);
    assert!(snapshot.providers.contains_key("provider-large"));
}

#[tokio::test]
async fn oversized_refresh_keeps_lkg_and_uses_the_stable_catalog_error() {
    let source = ScriptedSource::new(vec![
        Ok(CatalogFetchResponse {
            status: 200,
            etag: Some("\"catalog-v1\"".to_owned()),
            last_modified: None,
            body: CATALOG.to_vec(),
        }),
        Err(CatalogError::Network(NetworkControlError::ResponseTooLarge)),
    ]);
    let service = ProviderCatalogService::open(source, InMemoryCatalogCache::default())
        .expect("catalog service");
    let first = service.refresh().await.expect("seed LKG");

    assert!(matches!(
        service.refresh().await,
        Err(CatalogError::Network(NetworkControlError::ResponseTooLarge))
    ));
    let status = service.status().expect("status after oversized refresh");
    assert_eq!(status.state, ProviderCatalogState::RefreshFailedWithLkg);
    assert_eq!(
        status.last_error_code.as_ref().map(|code| code.as_str()),
        Some("provider.catalog_response_too_large")
    );
    assert_eq!(status.snapshot.expect("retained LKG").catalog_revision, first.catalog_revision);
}

struct UnreadableCatalogCache;

impl CatalogCache for UnreadableCatalogCache {
    fn load(&self) -> Result<CatalogCacheRecord, CatalogError> {
        Err(CatalogError::Cache("unreadable catalog cache".to_owned()))
    }

    fn persist_success(
        &self,
        _raw: &[u8],
        snapshot: ProviderCatalogSnapshot,
    ) -> Result<ProviderCatalogSnapshot, CatalogError> {
        Ok(snapshot)
    }

    fn persist_revalidated(&self, _snapshot: ProviderCatalogSnapshot) -> Result<(), CatalogError> {
        Ok(())
    }

    fn persist_failure(&self, _issue_code: &NormalizedIssueCode) -> Result<(), CatalogError> {
        Ok(())
    }
}

struct FailurePersistenceUnavailable;

impl CatalogCache for FailurePersistenceUnavailable {
    fn load(&self) -> Result<CatalogCacheRecord, CatalogError> {
        Ok(CatalogCacheRecord { snapshot: None, last_error_code: None })
    }

    fn persist_success(
        &self,
        _raw: &[u8],
        snapshot: ProviderCatalogSnapshot,
    ) -> Result<ProviderCatalogSnapshot, CatalogError> {
        Ok(snapshot)
    }

    fn persist_revalidated(&self, _snapshot: ProviderCatalogSnapshot) -> Result<(), CatalogError> {
        Ok(())
    }

    fn persist_failure(&self, _issue_code: &NormalizedIssueCode) -> Result<(), CatalogError> {
        Err(CatalogError::Cache("failure state is not writable".to_owned()))
    }
}

#[test]
fn unreadable_cache_does_not_block_startup_or_publish_unverified_data() {
    let service =
        ProviderCatalogService::open(ScriptedSource::new(Vec::new()), UnreadableCatalogCache)
            .expect("catalog cache failure must not block startup");

    let status = service.status().expect("catalog status");
    assert_eq!(status.state, ProviderCatalogState::Unavailable);
    assert!(status.snapshot.is_none());
    assert!(
        status
            .last_error_code
            .as_ref()
            .is_some_and(|code| code.as_str() == "persistence.catalog_cache_failed")
    );
}

#[tokio::test]
async fn cache_write_failure_cannot_leave_catalog_stuck_in_loading_state() {
    let service = ProviderCatalogService::open(
        ScriptedSource::new(vec![Err(CatalogError::Source("offline".to_owned()))]),
        FailurePersistenceUnavailable,
    )
    .expect("catalog service");

    assert!(service.refresh().await.is_err());
    let status = service.status().expect("terminal catalog status");
    assert_eq!(status.state, ProviderCatalogState::Unavailable);
    assert!(
        status
            .last_error_code
            .as_ref()
            .is_some_and(|code| code.as_str() == "provider.catalog_refresh_failed")
    );
}

#[tokio::test]
async fn persistent_cache_survives_restart_and_preserves_stale_lkg() {
    let directory = tempfile::tempdir().expect("temp root");
    let database_path = directory.path().join("data/aura.db");
    let artifact_root = directory.path().join("data/artifact-objects");
    let persistence = Arc::new(start_persistence(database_path.clone(), artifact_root.clone()));
    let source = ScriptedSource::new(vec![Ok(CatalogFetchResponse {
        status: 200,
        etag: Some("\"catalog-v1\"".to_owned()),
        last_modified: Some("Wed, 01 Jan 2026 00:00:00 GMT".to_owned()),
        body: CATALOG.to_vec(),
    })]);
    let service = ProviderCatalogService::open(
        source,
        PersistenceCatalogCache::new(Arc::clone(&persistence)),
    )
    .expect("catalog service");
    let first = service.refresh().await.expect("persisted catalog refresh");
    let artifact = persistence
        .artifact_record(ArtifactMetadataRequest {
            artifact_id: first.raw_artifact_id,
            session_id: None,
            required_access: ArtifactAccess::USER_READ,
            allow_quarantine: false,
        })
        .expect("raw catalog artifact");
    assert_eq!(artifact.scope, ArtifactScope::SystemCache);
    assert_eq!(artifact.content_hash, first.catalog_content_hash);
    drop(service);
    drop(persistence);

    let reopened = Arc::new(start_persistence(database_path, artifact_root));
    let service = ProviderCatalogService::open(
        ScriptedSource::new(vec![Err(CatalogError::Source("offline".to_owned()))]),
        PersistenceCatalogCache::new(Arc::clone(&reopened)),
    )
    .expect("reopened catalog service");
    let restored = service.current_snapshot().expect("restored LKG");
    assert_eq!(restored.catalog_revision, first.catalog_revision);
    assert_eq!(service.status().expect("restored status").state, ProviderCatalogState::Fresh);
    assert!(service.refresh().await.is_err());
    drop(service);
    drop(reopened);

    let final_open = Arc::new(start_persistence(
        directory.path().join("data/aura.db"),
        directory.path().join("data/artifact-objects"),
    ));
    let cached = aura_model_catalog::CatalogCache::load(&PersistenceCatalogCache::new(final_open))
        .expect("load cache after failure");
    assert_eq!(cached.snapshot.expect("retained LKG").catalog_revision, first.catalog_revision);
    assert!(
        cached
            .last_error_code
            .as_ref()
            .is_some_and(|code| code.as_str() == "provider.catalog_refresh_failed")
    );
}

#[tokio::test]
async fn endpoint_discovery_uses_the_p04_gateway_and_parses_a_reviewed_fixture() {
    let server = FixtureServer::start(vec![ModelProviderScenario::DiscoveryModels.response()])
        .await
        .expect("fixture server");
    let origin = NormalizedOrigin {
        scheme: "http".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: server.address().port(),
    };
    let grants = Arc::new(InMemoryNetworkGrantStore::default());
    let request_url = format!("{}/models", server.origin());
    let authorizer = LoopbackDiscoveryAuthorizer { origin, request_url };
    let source = CoreGatewayEndpointDiscoverySource::new(
        Arc::new(CoreHttpClientGateway::new(grants, None, NetworkControlOptions::default())),
        authorizer,
    );

    let response = source.fetch_models().await.expect("Gateway discovery");
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type.as_deref(), Some("application/json"));
    let page = parse_model_discovery_page(ModelApiFormat::OpenAiResponses, &response.body)
        .expect("discovery response");
    assert_eq!(page.model_ids, vec!["fixture-model-a", "fixture-model-b"]);
    let requests = server
        .wait_for_requests(1, std::time::Duration::from_secs(2))
        .await
        .expect("captured discovery request");
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].target, "/models");
    assert!(requests[0].body.is_empty());
}

#[test]
fn matching_is_exact_and_reports_unscoped_ambiguity_without_guessing() {
    let mut snapshot = snapshot();
    let exact = match_catalog_model(&snapshot, Some("provider-a"), "model-exact");
    assert!(matches!(exact, ProviderCatalogMatch::Matched { .. }));
    let alias = match_catalog_model(&snapshot, Some("provider-a"), "model-alias");
    assert!(matches!(alias, ProviderCatalogMatch::Matched { .. }));
    assert!(matches!(
        match_catalog_model(&snapshot, Some("provider-a"), "MODEL-EXACT"),
        ProviderCatalogMatch::Unmatched
    ));

    let mut second = snapshot.providers["provider-a"].clone();
    second.catalog_provider_id = "provider-b".to_owned();
    snapshot.providers.insert("provider-b".to_owned(), second);
    snapshot.provider_order.push("provider-b".to_owned());
    assert!(matches!(
        match_catalog_model(&snapshot, None, "model-exact"),
        ProviderCatalogMatch::Ambiguous { candidates } if candidates.len() == 2
    ));
}

#[test]
fn discovery_parser_accepts_only_deterministic_list_shapes() {
    assert_eq!(
        parse_model_discovery_page(
            ModelApiFormat::OpenAiResponses,
            br#"{"object":"list","data":[{"id":"model-c","object":"model"}]}"#,
        )
        .expect("OpenAI list wrapper")
        .model_ids,
        vec!["model-c"]
    );
    for invalid in [
        br#"["Model-A", {"id":"model-b"}]"#.as_slice(),
        br#"{"data":[{"id":"same"},{"id":"same"}]}"#,
        br#"{"unknown":[]}"#,
    ] {
        assert!(parse_model_discovery_page(ModelApiFormat::OpenAiResponses, invalid).is_err());
    }
    assert!(
        parse_model_discovery_page(ModelApiFormat::OpenAiResponses, br#"{"data":[],"models":[]}"#,)
            .is_ok(),
        "unknown additional fields are ignored by the frozen OpenAI schema"
    );
}

#[test]
fn discovery_parser_enforces_the_shared_operation_model_limit() {
    let maximum = ConfigurationTransportPolicyV1::MODEL_DISCOVERY_OPERATION.max_models;
    let entries = (0..=maximum)
        .map(|index| serde_json::json!({"id": format!("model-{index}")}))
        .collect::<Vec<_>>();
    let body = serde_json::to_vec(&serde_json::json!({"data": entries}))
        .expect("discovery response fixture");

    assert!(matches!(
        parse_model_discovery_page(ModelApiFormat::OpenAiResponses, &body),
        Err(CatalogError::DiscoveryLimitExceeded)
    ));
}

#[test]
fn provider_card_projection_preserves_catalog_state_and_exact_local_mapping() {
    let snapshot = snapshot();
    let local_provider_id = ProviderId::new();
    let now = UtcTimestamp::now();
    let local_provider = ModelProviderConfig {
        id: local_provider_id,
        display_name: "Locally Configured Provider".to_owned(),
        slug: "local-provider".parse().expect("valid Provider slug"),
        source: ModelProviderSource::ModelsDev { catalog_provider_id: "provider-a".to_owned() },
        endpoint_order: Vec::new(),
        created_at: now,
        updated_at: now,
        default_endpoint_id: None,
        backup_endpoint_ids: vec![],
    };
    let mut registry = ModelRegistryConfig {
        provider_order: vec![local_provider_id],
        providers: BTreeMap::from([(local_provider_id, local_provider)]),
        ..ModelRegistryConfig::default()
    };
    let status = ProviderCatalogStatus {
        state: ProviderCatalogState::RefreshFailedWithLkg,
        snapshot: Some(snapshot),
        checked_at: Some(now),
        last_successful_refresh_at: Some(now),
        last_error_code: Some(
            NormalizedIssueCode::new("provider.catalog_refresh_failed")
                .expect("registered issue code"),
        ),
    };
    let policy =
        |_entry: &ProviderCatalogEntry| ProviderCatalogConfigurationClass::ManualConfiguration;

    let cards = project_provider_cards(&status, &registry, &policy, &BTreeMap::new())
        .expect("provider cards");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].catalog_provider_id, "provider-a");
    assert_eq!(cards[0].catalog_model_count, 1);
    assert_eq!(cards[0].local_provider_id, Some(local_provider_id));
    assert_eq!(
        cards[0].configuration_class,
        ProviderCatalogConfigurationClass::ManualConfiguration
    );

    let duplicate_id = ProviderId::new();
    let mut duplicate = registry.providers[&local_provider_id].clone();
    duplicate.id = duplicate_id;
    registry.provider_order.push(duplicate_id);
    registry.providers.insert(duplicate_id, duplicate);
    assert!(matches!(
        project_provider_cards(&status, &registry, &policy, &BTreeMap::new()),
        Err(CatalogError::AmbiguousLocalProvider(provider)) if provider == "provider-a"
    ));
}

#[test]
fn provider_model_page_uses_exact_object_hash_and_revision_bound_continuation() {
    let snapshot = snapshot();
    let status = ProviderCatalogStatus {
        state: ProviderCatalogState::Fresh,
        checked_at: Some(snapshot.checked_at),
        last_successful_refresh_at: Some(snapshot.last_successful_refresh_at),
        snapshot: Some(snapshot.clone()),
        last_error_code: None,
    };
    let policy =
        |_entry: &ProviderCatalogEntry| ProviderCatalogConfigurationClass::ManualConfiguration;
    let first = project_provider_model_page(
        &status,
        &ModelRegistryConfig::default(),
        &policy,
        &ModelCatalogProviderGetRequest {
            catalog_provider_id: "provider-a".to_owned(),
            query: None,
            expected_catalog_revision: None,
            after_model_id: None,
            limit: Some(1),
        },
        &BTreeMap::new(),
    )
    .expect("first catalog model page");
    let view = first.models.first().expect("catalog model view");
    let canonical_object = serde_json::to_vec(&view.model).expect("catalog model JSON");
    assert_eq!(view.catalog_object_hash, ContentHash::blake3(&canonical_object));
    assert_eq!(first.catalog_revision, snapshot.catalog_revision);

    assert!(matches!(
        project_provider_model_page(
            &status,
            &ModelRegistryConfig::default(),
            &policy,
            &ModelCatalogProviderGetRequest {
                catalog_provider_id: "provider-a".to_owned(),
                query: None,
                expected_catalog_revision: None,
                after_model_id: Some(view.model.catalog_model_id.clone()),
                limit: Some(1),
            },
            &BTreeMap::new(),
        ),
        Err(CatalogError::MissingContinuationRevision)
    ));

    assert!(matches!(
        project_provider_model_page(
            &status,
            &ModelRegistryConfig::default(),
            &policy,
            &ModelCatalogProviderGetRequest {
                catalog_provider_id: "provider-a".to_owned(),
                query: None,
                expected_catalog_revision: Some(Revision::new(2)),
                after_model_id: Some(view.model.catalog_model_id.clone()),
                limit: Some(1),
            },
            &BTreeMap::new(),
        ),
        Err(CatalogError::RevisionConflict { expected: 2, actual: 1 })
    ));
}

fn snapshot() -> ProviderCatalogSnapshot {
    let providers = parse_models_dev_catalog(CATALOG).expect("catalog fixture");
    let now = UtcTimestamp::now();
    ProviderCatalogSnapshot {
        catalog_revision: Revision::INITIAL,
        catalog_content_hash: ContentHash::blake3(CATALOG),
        checked_at: now,
        last_successful_refresh_at: now,
        etag: None,
        last_modified: None,
        raw_artifact_id: ArtifactId::new(),
        provider_order: vec!["provider-a".to_owned()],
        providers,
    }
}

fn start_persistence(
    database_path: std::path::PathBuf,
    artifact_root: std::path::PathBuf,
) -> PersistenceCoordinator {
    let options = PersistenceOptions::new(database_path, artifact_root, RuntimeInstanceId::new());
    match PersistenceCoordinator::start(options).expect("start persistence") {
        PersistenceStartup::Ready(coordinator) => coordinator,
        PersistenceStartup::RecoveryRequired(recovery) => {
            panic!("unexpected recovery requirement: {}", recovery.reason)
        }
    }
}
