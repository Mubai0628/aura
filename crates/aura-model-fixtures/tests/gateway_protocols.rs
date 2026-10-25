use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, CorrelationId, EndpointId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, NetworkGrantId, NetworkRequestId,
    PromptBundleSnapshotId, ProviderId, Revision, SessionId, TransportAttemptId, UtcTimestamp,
    WorkspaceId,
};
use aura_domain_common::{ToolId, ToolRegistrySnapshotId};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    EndpointAuthScheme, ModelAdapterIdentity, ModelApiFormat, ModelEndpointTimeouts,
    ModelExecutionSnapshot, ModelFailureCategory, ModelRequestPriority, ModelStreamEventEnvelope,
    NormalizedModelOutput, NormalizedStopReason, PersistedModelFailure, ProviderContinuationState,
    ProviderRequestEvidence, ProviderRequestHeaderClass, ProviderRequestHeaderMetadata,
    ProviderRequestTargetMetadata, ProviderToolNameBinding, ResolvedModelRequestOptions,
    TransportAttemptKind, TransportAttemptRecord, TransportOperationContext,
};
use aura_model_fixtures::{
    CapturedFixtureRequest, FixtureResponse, FixtureResponseChunk, FixtureServer,
    ModelProviderScenario,
};
use aura_model_transport::{
    CORE_REQUEST_GRANT_TTL_SECONDS, CoreGatewayModelNetwork, EndpointGate, ModelAdapterError,
    ModelCancellationToken, ModelProtocolDecoder, ModelRequestAuthorizer, ModelTransport,
    ModelTransportError, ModelTransportExecution, ProviderContinuationArtifact,
    ProviderRequestFactory, ProviderRequestPlan, RawStreamWriter, SseLimits, TransportEventChannel,
    TransportRetryPolicy, TransportRunTerminal, TransportStore,
};
use aura_network_control::{
    ConfigurationRequestTargetV1, ConfigurationResponseMediaPolicyV1, CoreHttpClientGateway,
    InMemoryNetworkGrantStore, NetworkControlOptions, ProviderTransportAttemptGrant,
    ProviderTransportAuthorizationBinding, ProviderTransportGatewayRequest,
    ProviderTransportImmutableRequest, ProviderTransportRequestAuthority,
    canonical_non_secret_headers_hash, invocation_identity_binding_hash,
};
use aura_permission_domain::{
    CoreRequestGrant, NetworkByteLimits, NetworkDestinationPolicy, NetworkGrant,
    NetworkGrantIdentityScope, NetworkProtocolClass, NormalizedOrigin, RedirectPolicy,
};
use aura_security_contract::{
    InvocationIdentity, NetworkCallerKind, NetworkDataClass, NetworkDestinationClass,
    PermissionGrantLifetime,
};
use aura_tool_contract::{
    ToolDefinitionSnapshotRef, ToolInputSchemaV1, ToolRegistrySnapshotRef, ToolVersion,
};

#[derive(Default)]
struct StoreState {
    snapshots: BTreeMap<ModelExecutionSnapshotId, ModelExecutionSnapshot>,
    attempts: BTreeMap<TransportAttemptId, TransportAttemptRecord>,
    failures: Vec<PersistedModelFailure>,
    artifacts: BTreeMap<ArtifactId, Vec<u8>>,
    outputs: BTreeMap<ModelStepId, NormalizedModelOutput>,
}

#[derive(Clone, Default)]
struct FixtureStore {
    state: Arc<Mutex<StoreState>>,
}

struct FixtureRawWriter {
    artifact_id: ArtifactId,
    bytes: Vec<u8>,
    state: Arc<Mutex<StoreState>>,
}

impl RawStreamWriter for FixtureRawWriter {
    fn append(&mut self, bytes: &[u8]) -> Result<(), ModelTransportError> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    async fn finalize(self) -> Result<ArtifactId, ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("fixture store is poisoned".to_owned()))?
            .artifacts
            .insert(self.artifact_id, self.bytes);
        Ok(self.artifact_id)
    }
}

impl TransportStore for FixtureStore {
    type Writer = FixtureRawWriter;

    async fn record_execution_snapshot(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Result<(), ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("fixture store is poisoned".to_owned()))?
            .snapshots
            .insert(snapshot.id, snapshot.clone());
        Ok(())
    }

    async fn canonical_request_body_artifact(
        &self,
        _attempt_id: TransportAttemptId,
        attempt_kind: TransportAttemptKind,
        request_plan: &ProviderRequestPlan,
    ) -> Result<Option<ArtifactId>, ModelTransportError> {
        if attempt_kind != TransportAttemptKind::CompatibilityAdjustedRetry {
            return Ok(None);
        }
        let artifact_id = ArtifactId::new();
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("fixture store is poisoned".to_owned()))?
            .artifacts
            .insert(artifact_id, request_plan.body.clone());
        Ok(Some(artifact_id))
    }

    async fn begin_raw_stream(
        &self,
        _attempt_id: TransportAttemptId,
    ) -> Result<Self::Writer, ModelTransportError> {
        Ok(FixtureRawWriter {
            artifact_id: ArtifactId::new(),
            bytes: Vec::new(),
            state: Arc::clone(&self.state),
        })
    }

    async fn provider_continuation_artifact(
        &self,
        _attempt_id: TransportAttemptId,
        continuation: &ProviderContinuationArtifact,
    ) -> Result<ArtifactId, ModelTransportError> {
        let artifact_id = ArtifactId::new();
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("fixture store is poisoned".to_owned()))?
            .artifacts
            .insert(artifact_id, continuation.bytes.clone());
        Ok(artifact_id)
    }

    async fn record_attempt(
        &self,
        attempt: &TransportAttemptRecord,
    ) -> Result<(), ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("fixture store is poisoned".to_owned()))?
            .attempts
            .insert(attempt.id, attempt.clone());
        Ok(())
    }

    async fn record_failure(
        &self,
        failure: &PersistedModelFailure,
    ) -> Result<(), ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("fixture store is poisoned".to_owned()))?
            .failures
            .push(failure.clone());
        Ok(())
    }

    async fn record_output(
        &self,
        _snapshot: &ModelExecutionSnapshot,
        _attempt_id: TransportAttemptId,
        output: &NormalizedModelOutput,
    ) -> Result<(), ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("fixture store is poisoned".to_owned()))?
            .outputs
            .insert(output.model_step_id, output.clone());
        Ok(())
    }
}

#[derive(Clone)]
struct LoopbackAuthorizer {
    grants: Arc<InMemoryNetworkGrantStore>,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    origin: NormalizedOrigin,
}

impl ModelRequestAuthorizer for LoopbackAuthorizer {
    async fn authorize_model_request(
        &self,
        snapshot: &ModelExecutionSnapshot,
        request_plan: &ProviderRequestPlan,
        attempt_id: TransportAttemptId,
        client_request_id: NetworkRequestId,
    ) -> Result<ProviderTransportGatewayRequest, ModelTransportError> {
        let grant_id = NetworkGrantId::new();
        let byte_limits = NetworkByteLimits {
            max_upload_bytes: 64 * 1024,
            max_download_bytes: 1024 * 1024,
            max_decompressed_bytes: 1024 * 1024,
        };
        let network_grant_expires_at =
            UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::minutes(1));
        self.grants.insert(NetworkGrant {
            id: grant_id,
            identity_scope: NetworkGrantIdentityScope::Session,
            identity_scope_id: self.session_id.to_string(),
            lifetime: PermissionGrantLifetime::Session,
            origins: vec![self.origin.clone()],
            destination_classes: vec![NetworkDestinationClass::Loopback],
            methods: vec!["POST".to_owned()],
            allowed_data_classes: vec![NetworkDataClass::PublicQuery],
            max_upload_bytes: Some(byte_limits.max_upload_bytes),
            max_download_bytes: Some(byte_limits.max_download_bytes),
            source_fingerprint: None,
            expires_at: Some(network_grant_expires_at),
            revoked_at: None,
        })?;
        let identity = InvocationIdentity::runtime_user(
            self.workspace_id,
            self.session_id,
            CorrelationId::new(),
        );
        let request_url = url::Url::parse(&request_plan.request_url).map_err(|_| {
            ModelTransportError::InvalidProviderRequest("fixture request URL is invalid".to_owned())
        })?;
        let mut path_and_query = request_url.path().to_owned();
        if let Some(query) = request_url.query() {
            path_and_query.push('?');
            path_and_query.push_str(query);
        }
        let non_secret_headers_hash = canonical_non_secret_headers_hash(&request_plan.headers)
            .map_err(|error| ModelTransportError::InvalidProviderRequest(error.to_string()))?;
        let operation_context = TransportOperationContext::AgentModelAttempt {
            model_step_id: snapshot.model_step_id,
            model_execution_snapshot_id: snapshot.id,
            context_snapshot_id: snapshot.context_snapshot_id,
            prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
        };
        let purpose = ProviderTransportAttemptGrant {
            transport_attempt_id: attempt_id,
            operation_context,
            endpoint_id: snapshot.endpoint_id,
            endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
            request_manifest_hash: ContentHash::sha256(b"fixture agent request manifest"),
            non_secret_headers_hash,
            request_body_hash: ContentHash::sha256(&request_plan.body),
            request_body_length: u64::try_from(request_plan.body.len()).unwrap_or(u64::MAX),
            target: ConfigurationRequestTargetV1 {
                method: request_plan.method.clone(),
                origin: self.origin.clone(),
                path_and_query,
            },
            credential_binding: None,
            byte_limits: byte_limits.clone(),
            redirect_policy: RedirectPolicy::Deny,
            response_media_policy: ConfigurationResponseMediaPolicyV1 {
                accepted_media_types: BTreeSet::from([
                    "application/json".to_owned(),
                    "text/event-stream".to_owned(),
                ]),
                require_content_type: false,
            },
            authorization: ProviderTransportAuthorizationBinding::AgentModel {
                invocation_identity_binding_hash: invocation_identity_binding_hash(&identity)
                    .map_err(|error| {
                        ModelTransportError::InvalidProviderRequest(error.to_string())
                    })?,
                model_egress_manifest_hash: ContentHash::sha256(b"fixture model egress manifest"),
            },
        };
        let core_grant = CoreRequestGrant {
            logical_request_id: client_request_id,
            caller: NetworkCallerKind::ModelTransport,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: self.origin.host.clone(),
                port: self.origin.port,
                allowed_destination_classes: vec![NetworkDestinationClass::Loopback],
            },
            methods: vec!["POST".to_owned()],
            redirect_policy: RedirectPolicy::Deny,
            data_classes: vec![NetworkDataClass::PublicQuery],
            byte_limits: byte_limits.clone(),
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value()
                    + time::Duration::seconds(CORE_REQUEST_GRANT_TTL_SECONDS),
            ),
            purpose,
        };
        Ok(ProviderTransportGatewayRequest {
            authority: ProviderTransportRequestAuthority::AgentModel(identity),
            grant: core_grant,
            immutable_request: ProviderTransportImmutableRequest {
                request_id: client_request_id,
                method: request_plan.method.clone(),
                requested_url: request_plan.request_url.clone(),
                headers: request_plan.headers.clone(),
                body: request_plan.body.clone(),
                redirect_policy: RedirectPolicy::Deny,
                byte_limits,
                connect_timeout_ms: 2_000,
                timeout_ms: Some(2_000),
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureRequestShape {
    Static,
    ToolCalls,
    OptionalStreamOptions,
}

struct FixtureRequestFactory {
    request_url: String,
    endpoint_fingerprint: ContentHash,
    api_format: ModelApiFormat,
    shape: FixtureRequestShape,
}

impl ProviderRequestFactory for FixtureRequestFactory {
    fn build_request(
        &self,
        operation_context: &TransportOperationContext,
        applied_compatibility_overrides: &BTreeSet<String>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        if !applied_compatibility_overrides.is_empty() {
            return Err(ModelAdapterError::invalid_request(
                "model_request.compatibility_removal_not_allowed",
                "Adapter Compilation Contract v1 has an empty compatibility-removal whitelist",
            ));
        }
        let mut body = serde_json::json!({"model": "fixture-model", "stream": true});
        if self.shape == FixtureRequestShape::OptionalStreamOptions {
            body.as_object_mut()
                .expect("fixture request body is an object")
                .insert("stream_options".to_owned(), serde_json::json!({"include_usage": true}));
        }
        let tool_name_bindings = if self.shape == FixtureRequestShape::ToolCalls {
            let bindings = fixture_tool_name_bindings();
            let tools = bindings
                .iter()
                .map(|binding| match self.api_format {
                    ModelApiFormat::OpenAiResponses => serde_json::json!({
                        "type": "function",
                        "name": binding.provider_wire_name,
                        "parameters": {"type": "object"}
                    }),
                    ModelApiFormat::OpenAiChatCompletions => serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": binding.provider_wire_name,
                            "parameters": {"type": "object"}
                        }
                    }),
                    ModelApiFormat::AnthropicMessages => serde_json::json!({
                        "name": binding.provider_wire_name,
                        "input_schema": {"type": "object"}
                    }),
                })
                .collect();
            body.as_object_mut()
                .expect("fixture request body is an object")
                .insert("tools".to_owned(), serde_json::Value::Array(tools));
            bindings
        } else {
            Vec::new()
        };
        let body = serde_json::to_vec(&body).map_err(|error| {
            ModelAdapterError::protocol(
                "fixture_request_serialization_failed",
                format!("fixture request serialization failed: {error}"),
            )
        })?;
        Ok(ProviderRequestPlan {
            method: "POST".to_owned(),
            request_url: self.request_url.clone(),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            request_evidence: fixture_request_evidence(
                operation_context,
                &self.request_url,
                &body,
                self.api_format,
                applied_compatibility_overrides,
                tool_name_bindings,
            ),
            body,
            safe_optional_parameters: BTreeSet::new(),
        })
    }

    fn current_endpoint_fingerprint(&self) -> Result<ContentHash, ModelTransportError> {
        Ok(self.endpoint_fingerprint.clone())
    }
}

fn fixture_request_evidence(
    operation_context: &TransportOperationContext,
    request_url: &str,
    body: &[u8],
    api_format: ModelApiFormat,
    compatibility_overrides: &BTreeSet<String>,
    tool_name_bindings: Vec<ProviderToolNameBinding>,
) -> ProviderRequestEvidence {
    let url = url::Url::parse(request_url).expect("fixture request URL");
    ProviderRequestEvidence {
        body_sha256: ContentHash::sha256(body),
        body_byte_length: u64::try_from(body.len()).expect("fixture body length"),
        target: ProviderRequestTargetMetadata {
            method: "POST".to_owned(),
            origin: NormalizedOrigin {
                scheme: url.scheme().to_owned(),
                host: url.host_str().expect("fixture host").to_owned(),
                port: url.port_or_known_default().expect("fixture port"),
            },
            path: url.path().to_owned(),
            query_parameter_names: url.query_pairs().map(|(name, _)| name.into_owned()).collect(),
        },
        headers: vec![ProviderRequestHeaderMetadata {
            name: "content-type".to_owned(),
            class: ProviderRequestHeaderClass::ContentMetadata,
            non_secret_value: Some("application/json".to_owned()),
        }],
        compatibility_removals: compatibility_overrides.clone(),
        applied_output_token_field_alias: None,
        adapter: ModelAdapterIdentity::v1(api_format),
        operation_context: operation_context.clone(),
        tool_registry_snapshot_ref: matches!(
            operation_context,
            TransportOperationContext::AgentModelAttempt { .. }
        )
        .then(fixture_registry_ref),
        tool_name_bindings,
        historical_tool_call_wire_bindings: Vec::new(),
        canonical_body_artifact_id: None,
    }
}

fn fixture_tool_name_bindings() -> Vec<ProviderToolNameBinding> {
    [
        ("tol_01ARZ3NDEKTSV4RRFFQ69G5FAV", "workspace.read"),
        ("tol_01ARZ3NDEKTSV4RRFFQ69G5FAW", "workspace.search"),
    ]
    .into_iter()
    .map(|(tool_id, canonical_name)| {
        let tool_id = tool_id.parse::<ToolId>().expect("fixed fixture ToolId");
        let schema = ToolInputSchemaV1::new(serde_json::json!({"type": "object"}))
            .expect("fixture schema")
            .into_document();
        let (_, wrapped_hash) =
            aura_model_domain::provider_arguments_envelope_schema_v1(&schema.document)
                .expect("wrapped schema");
        ProviderToolNameBinding {
            tool_definition: ToolDefinitionSnapshotRef {
                tool_id,
                tool_version: ToolVersion::new(None, Revision::INITIAL)
                    .expect("fixture ToolVersion"),
                registry_snapshot_ref: fixture_registry_ref(),
            },
            canonical_name: canonical_name.to_owned(),
            provider_wire_name: aura_model_domain::provider_wire_tool_name(canonical_name, tool_id),
            package_schema_hash: schema.schema_hash,
            provider_wrapped_schema_hash: wrapped_hash,
            provider_arguments_envelope_version: 1,
        }
    })
    .collect()
}

fn fixture_registry_ref() -> ToolRegistrySnapshotRef {
    ToolRegistrySnapshotRef {
        tool_registry_snapshot_id: ToolRegistrySnapshotId::from_bytes([42; 16]),
        registry_revision: Revision::INITIAL,
        content_hash: CanonicalHashV1::sha256(b"gateway-protocol-fixture-registry"),
    }
}

struct FixtureRun {
    terminal: TransportRunTerminal,
    requests: Vec<CapturedFixtureRequest>,
    store: FixtureStore,
    events: Vec<ModelStreamEventEnvelope>,
}

async fn execute_fixture(
    responses: Vec<FixtureResponse>,
    expected_requests: usize,
    api_format: ModelApiFormat,
    path: &str,
    adapter: &dyn ModelProtocolDecoder,
) -> FixtureRun {
    let run = execute_fixture_with_shape(
        responses,
        expected_requests,
        api_format,
        path,
        adapter,
        FixtureRequestShape::Static,
    )
    .await;
    for request in &run.requests {
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&request.body).expect("request JSON"),
            serde_json::json!({"model": "fixture-model", "stream": true})
        );
    }
    run
}

async fn execute_fixture_with_shape(
    responses: Vec<FixtureResponse>,
    expected_requests: usize,
    api_format: ModelApiFormat,
    path: &str,
    adapter: &dyn ModelProtocolDecoder,
    request_shape: FixtureRequestShape,
) -> FixtureRun {
    let server = FixtureServer::start(responses).await.expect("fixture server");
    let origin = NormalizedOrigin {
        scheme: "http".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: server.address().port(),
    };
    let grants = Arc::new(InMemoryNetworkGrantStore::default());
    let authorizer = LoopbackAuthorizer {
        grants: Arc::clone(&grants),
        workspace_id: WorkspaceId::new(),
        session_id: SessionId::new(),
        origin,
    };
    let gateway =
        Arc::new(CoreHttpClientGateway::new(grants, None, NetworkControlOptions::default()));
    let network = CoreGatewayModelNetwork::new(gateway, authorizer);
    let store = FixtureStore::default();
    let transport = ModelTransport::new(
        network,
        store.clone(),
        TransportRetryPolicy::new(Duration::ZERO, Duration::ZERO, Duration::ZERO),
        SseLimits::default(),
    );
    let resolved_request_url = format!("{}{path}", server.origin());
    let endpoint_fingerprint = ContentHash::blake3(resolved_request_url.as_bytes());
    let snapshot = ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Fixture Provider".to_owned(),
        endpoint_display_name: "Fixture Endpoint".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Fixture Provider / Fixture Endpoint / fixture-model".to_owned(),
        api_format,
        chat_output_token_field: matches!(api_format, ModelApiFormat::OpenAiChatCompletions)
            .then_some(aura_model_domain::ChatOutputTokenField::MaxTokens),
        chat_instruction_role: matches!(api_format, ModelApiFormat::OpenAiChatCompletions)
            .then_some(aura_model_domain::ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::None,
        base_url: server.origin(),
        resolved_request_url: resolved_request_url.clone(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 256_000,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions::output_only(512),
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: endpoint_fingerprint.clone(),
        created_at: UtcTimestamp::now(),
    };
    let request_factory = FixtureRequestFactory {
        request_url: resolved_request_url,
        endpoint_fingerprint,
        api_format,
        shape: request_shape,
    };
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("endpoint gate");
    let (events, mut receiver) = TransportEventChannel::bounded(128);
    let terminal = transport
        .execute(ModelTransportExecution {
            snapshot: &snapshot,
            endpoint_timeouts: &ModelEndpointTimeouts {
                connect_ms: 2_000,
                first_event_ms: 2_000,
                stream_idle_ms: 2_000,
                total_ms: Some(10_000),
            },
            gate: &gate,
            priority: ModelRequestPriority::InteractiveParent,
            request_factory: &request_factory,
            adapter,
            initial_compatibility_overrides: BTreeSet::new(),
            cancellation: &ModelCancellationToken::default(),
            events: &events,
        })
        .await
        .expect("fixture transport execution");
    drop(events);
    let mut emitted_events = Vec::new();
    while let Some(event) = receiver.recv().await {
        emitted_events.push(event);
    }
    let requests = server
        .wait_for_requests(expected_requests, Duration::from_secs(5))
        .await
        .unwrap_or_else(|error| {
            panic!("captured fixture requests: {error:?}; transport terminal: {terminal:?}")
        });
    assert_eq!(requests.len(), expected_requests);
    for request in &requests {
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, path);
        assert!(!request.headers.iter().any(|(name, _)| {
            matches!(
                name.as_str(),
                "authorization" | "proxy-authorization" | "x-api-key" | "cookie"
            )
        }));
    }
    assert_eq!(gate.metrics().expect("gate metrics").active_requests, 0);
    FixtureRun { terminal, requests, store, events: emitted_events }
}

async fn execute_tool_fixture(
    responses: Vec<FixtureResponse>,
    api_format: ModelApiFormat,
    path: &str,
    adapter: &dyn ModelProtocolDecoder,
) -> FixtureRun {
    execute_fixture_with_shape(
        responses,
        1,
        api_format,
        path,
        adapter,
        FixtureRequestShape::ToolCalls,
    )
    .await
}

fn completed(run: FixtureRun) -> NormalizedModelOutput {
    assert!(!run.requests.is_empty());
    assert!(!run.events.is_empty());
    assert!(
        run.events.windows(2).all(|pair| pair[0].sequence < pair[1].sequence),
        "ModelStreamEvent sequences must be strictly increasing"
    );
    match run.terminal {
        TransportRunTerminal::Completed { output, attempts } => {
            assert_eq!(attempts.len(), 1);
            assert_eq!(run.store.state.lock().expect("fixture store").outputs.len(), 1);
            output
        }
        TransportRunTerminal::Failed { failure, .. } => {
            panic!("fixture unexpectedly failed: {}", failure.failure.code)
        }
        TransportRunTerminal::Cancelled { .. } => panic!("fixture unexpectedly cancelled"),
    }
}

#[test]
fn runtime_capability_facts_come_from_each_current_provider_request() {
    let cases = [
        (
            &OpenAiResponsesAdapter as &dyn ModelProtocolDecoder,
            ModelApiFormat::OpenAiResponses,
            serde_json::json!({
                "tools": [{"type": "function"}],
                "reasoning": {"effort": "medium"},
                "input": [{"type": "input_image", "image_url": "data:image/png;base64,AA=="}]
            }),
        ),
        (
            &OpenAiChatCompletionsAdapter as &dyn ModelProtocolDecoder,
            ModelApiFormat::OpenAiChatCompletions,
            serde_json::json!({
                "tools": [{"type": "function"}],
                "reasoning_effort": "medium",
                "messages": [{"content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,AA=="}}]}]
            }),
        ),
        (
            &AnthropicMessagesAdapter as &dyn ModelProtocolDecoder,
            ModelApiFormat::AnthropicMessages,
            serde_json::json!({
                "tools": [{"name": "fixture"}],
                "thinking": {"type": "enabled", "budget_tokens": 1024},
                "messages": [{"content": [{"type": "image", "source": {"type": "base64"}}]}]
            }),
        ),
    ];
    for (adapter, expected_format, body) in cases {
        let body = serde_json::to_vec(&body).expect("request body");
        let operation_context = TransportOperationContext::AgentModelAttempt {
            model_step_id: ModelStepId::new(),
            model_execution_snapshot_id: ModelExecutionSnapshotId::new(),
            context_snapshot_id: ContextProjectionSnapshotId::new(),
            prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        };
        let plan = ProviderRequestPlan {
            method: "POST".to_owned(),
            request_url: "https://api.example.test/v1/model".to_owned(),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            request_evidence: fixture_request_evidence(
                &operation_context,
                "https://api.example.test/v1/model",
                &body,
                expected_format,
                &BTreeSet::new(),
                Vec::new(),
            ),
            body,
            safe_optional_parameters: BTreeSet::new(),
        };
        let facts = adapter.runtime_capability_facts(&plan);
        assert_eq!(facts.api_format, expected_format);
        assert!(facts.native_tool_channel_exposed);
        assert!(facts.native_attachment_parts_present);
        assert!(facts.reasoning_channel_observed);
        assert!(facts.native_capability_names.is_empty());
    }
}

#[tokio::test]
async fn three_protocols_complete_through_the_p04_gateway() {
    let responses = completed(
        execute_tool_fixture(
            vec![ModelProviderScenario::OpenAiResponsesReasoningTool.response()],
            ModelApiFormat::OpenAiResponses,
            "/v1/responses",
            &OpenAiResponsesAdapter,
        )
        .await,
    );
    assert_eq!(responses.reasoning_blocks[0].text, "fixture reasoning");
    assert_eq!(responses.tool_calls[0].provider_call_id.as_str(), "Call_Response_ExactCase");
    assert_eq!(responses.usage.total_tokens, 25);
    assert!(matches!(
        responses.provider_continuation,
        Some(ProviderContinuationState::OpenAiResponses { .. })
    ));

    let chat = completed(
        execute_tool_fixture(
            vec![ModelProviderScenario::OpenAiChatParallelTools.response()],
            ModelApiFormat::OpenAiChatCompletions,
            "/v1/chat/completions",
            &OpenAiChatCompletionsAdapter,
        )
        .await,
    );
    assert_eq!(chat.reasoning_blocks[0].text, "fixture reasoning");
    assert_eq!(chat.tool_calls.len(), 2);
    assert_eq!(chat.tool_calls[0].provider_call_id.as_str(), "Call_Chat_A");
    assert_eq!(chat.tool_calls[1].provider_call_id.as_str(), "Call_Chat_B");
    assert_eq!(chat.usage.total_tokens, 28);

    let anthropic = completed(
        execute_tool_fixture(
            vec![ModelProviderScenario::AnthropicThinkingTool.response()],
            ModelApiFormat::AnthropicMessages,
            "/v1/messages",
            &AnthropicMessagesAdapter,
        )
        .await,
    );
    assert_eq!(anthropic.reasoning_blocks[0].text, "fixture reasoning");
    assert_eq!(anthropic.tool_calls[0].provider_call_id.as_str(), "Call_Anthropic_ExactCase");
    assert_eq!(anthropic.usage.total_tokens, 25);
    assert!(matches!(
        anthropic.provider_continuation,
        Some(ProviderContinuationState::Anthropic { .. })
    ));
}

#[tokio::test]
async fn three_protocol_refusals_normalize_through_the_p04_gateway() {
    let responses = completed(
        execute_fixture(
            vec![ModelProviderScenario::OpenAiResponsesRefusal.response()],
            1,
            ModelApiFormat::OpenAiResponses,
            "/v1/responses",
            &OpenAiResponsesAdapter,
        )
        .await,
    );
    assert_eq!(responses.stop_reason, NormalizedStopReason::Refusal);
    assert_eq!(responses.refusal.expect("Responses refusal").message, "I cannot help with that.");

    let chat = completed(
        execute_fixture(
            vec![ModelProviderScenario::OpenAiChatRefusal.response()],
            1,
            ModelApiFormat::OpenAiChatCompletions,
            "/v1/chat/completions",
            &OpenAiChatCompletionsAdapter,
        )
        .await,
    );
    assert_eq!(chat.stop_reason, NormalizedStopReason::Refusal);
    assert_eq!(chat.refusal.expect("Chat refusal").message, "I cannot help with that.");

    let anthropic = completed(
        execute_fixture(
            vec![ModelProviderScenario::AnthropicRefusal.response()],
            1,
            ModelApiFormat::AnthropicMessages,
            "/v1/messages",
            &AnthropicMessagesAdapter,
        )
        .await,
    );
    assert_eq!(anthropic.stop_reason, NormalizedStopReason::Refusal);
    let refusal = anthropic.refusal.expect("Anthropic refusal");
    assert_eq!(refusal.message, "This request was declined.");
    assert_eq!(refusal.provider_code.as_deref(), Some("cyber"));
}

#[tokio::test]
async fn provider_error_scenarios_keep_retry_and_failure_classes_distinct() {
    let authentication = execute_fixture(
        vec![ModelProviderScenario::AuthenticationFailed.response()],
        1,
        ModelApiFormat::OpenAiResponses,
        "/v1/responses",
        &OpenAiResponsesAdapter,
    )
    .await;
    match authentication.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::Authentication);
            assert_eq!(attempts.len(), 1);
        }
        TransportRunTerminal::Completed { .. } => {
            panic!("authentication failure must not complete")
        }
        TransportRunTerminal::Cancelled { .. } => {
            panic!("authentication failure must not be cancelled")
        }
    }

    let rate_response = ModelProviderScenario::RateLimited.response();
    let rate = execute_fixture(
        vec![rate_response.clone(), rate_response.clone(), rate_response],
        3,
        ModelApiFormat::OpenAiResponses,
        "/v1/responses",
        &OpenAiResponsesAdapter,
    )
    .await;
    let persisted_rate_failures = rate.store.state.lock().expect("fixture store").failures.clone();
    assert_eq!(persisted_rate_failures.len(), 3);
    assert_eq!(
        persisted_rate_failures.iter().map(|failure| failure.id).collect::<BTreeSet<_>>().len(),
        3,
        "each persisted failure receives a fresh ModelFailureId"
    );
    assert!(
        persisted_rate_failures
            .iter()
            .all(|failure| failure.fingerprint == persisted_rate_failures[0].fingerprint)
    );
    match rate.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::RateLimit);
            assert_eq!(attempts.len(), 3);
            assert!(attempts.iter().all(|attempt| {
                attempt.request_evidence.operation_context
                    == attempts[0].request_evidence.operation_context
            }));
            assert!(attempts.windows(2).all(|pair| pair[0].id != pair[1].id));
        }
        TransportRunTerminal::Completed { .. } => panic!("rate limit must not complete"),
        TransportRunTerminal::Cancelled { .. } => panic!("rate limit must not be cancelled"),
    }

    let quota = execute_fixture(
        vec![ModelProviderScenario::QuotaExhausted.response()],
        1,
        ModelApiFormat::OpenAiResponses,
        "/v1/responses",
        &OpenAiResponsesAdapter,
    )
    .await;
    match quota.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::QuotaOrBalance);
            assert_eq!(attempts.len(), 1);
        }
        TransportRunTerminal::Completed { .. } => panic!("quota failure must not complete"),
        TransportRunTerminal::Cancelled { .. } => panic!("quota failure must not be cancelled"),
    }

    let context = execute_fixture(
        vec![ModelProviderScenario::ContextWindowExceeded.response()],
        1,
        ModelApiFormat::OpenAiResponses,
        "/v1/responses",
        &OpenAiResponsesAdapter,
    )
    .await;
    match context.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::ContextWindowExceeded);
            assert_eq!(attempts.len(), 1);
        }
        TransportRunTerminal::Completed { .. } => {
            panic!("context-window failure must not complete")
        }
        TransportRunTerminal::Cancelled { .. } => {
            panic!("context-window failure must not be cancelled")
        }
    }

    let unavailable_response = ModelProviderScenario::ServiceUnavailable.response();
    let unavailable = execute_fixture(
        vec![unavailable_response.clone(), unavailable_response.clone(), unavailable_response],
        3,
        ModelApiFormat::OpenAiResponses,
        "/v1/responses",
        &OpenAiResponsesAdapter,
    )
    .await;
    match unavailable.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::Transport);
            assert_eq!(attempts.len(), 3);
        }
        TransportRunTerminal::Completed { .. } => {
            panic!("service-unavailable failure must not complete")
        }
        TransportRunTerminal::Cancelled { .. } => {
            panic!("service-unavailable failure must not be cancelled")
        }
    }
}

#[tokio::test]
async fn disconnect_retry_depends_on_whether_a_model_event_was_emitted() {
    let before_event = FixtureResponse::sse(std::iter::empty()).disconnect_after_chunks();
    let before = execute_fixture(
        vec![before_event.clone(), before_event.clone(), before_event],
        3,
        ModelApiFormat::OpenAiResponses,
        "/v1/responses",
        &OpenAiResponsesAdapter,
    )
    .await;
    match before.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::Transport);
            assert_eq!(attempts.len(), 3);
        }
        TransportRunTerminal::Completed { .. } => {
            panic!("disconnect before the first event must not complete")
        }
        TransportRunTerminal::Cancelled { .. } => {
            panic!("disconnect before the first event must not be cancelled")
        }
    }

    let after_event = FixtureResponse::sse([FixtureResponseChunk::immediate(
        br#"event: response.created
data: {"type":"response.created","sequence_number":0,"response":{"id":"resp_partial","status":"in_progress"}}

"#
        .to_vec(),
    )])
    .disconnect_after_chunks();
    let after = execute_fixture(
        vec![after_event],
        1,
        ModelApiFormat::OpenAiResponses,
        "/v1/responses",
        &OpenAiResponsesAdapter,
    )
    .await;
    match after.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::StreamInterrupted);
            assert_eq!(attempts.len(), 1);
            assert!(failure.failure.partial_output_ref.is_none());
            assert!(attempts[0].raw_stream_artifact_id.is_none());
        }
        TransportRunTerminal::Completed { .. } => {
            panic!("disconnect after a valid event must not complete")
        }
        TransportRunTerminal::Cancelled { .. } => {
            panic!("disconnect after a valid event must not be cancelled")
        }
    }
}

#[tokio::test]
async fn unsupported_optional_parameter_is_not_removed_retried_or_learned() {
    let run = execute_fixture_with_shape(
        vec![ModelProviderScenario::UnsupportedOptionalParameter.response()],
        1,
        ModelApiFormat::OpenAiChatCompletions,
        "/v1/chat/completions",
        &OpenAiChatCompletionsAdapter,
        FixtureRequestShape::OptionalStreamOptions,
    )
    .await;

    let attempts = match &run.terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.category, ModelFailureCategory::UnsupportedParameter);
            assert_eq!(failure.failure.code, "unsupported_parameter");
            attempts
        }
        TransportRunTerminal::Completed { .. } => {
            panic!("unsupported optional parameter must not be silently removed")
        }
        TransportRunTerminal::Cancelled { .. } => {
            panic!("unsupported optional parameter unexpectedly cancelled")
        }
    };
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt_kind, TransportAttemptKind::Initial);
    assert!(attempts[0].request_evidence.compatibility_removals.is_empty());

    let first_body =
        serde_json::from_slice::<serde_json::Value>(&run.requests[0].body).expect("first body");
    assert_eq!(
        first_body,
        serde_json::json!({
            "model": "fixture-model",
            "stream": true,
            "stream_options": {"include_usage": true}
        })
    );
    assert!(attempts[0].request_evidence.canonical_body_artifact_id.is_none());
}
