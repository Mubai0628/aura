use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::pending,
    sync::{Arc, Mutex},
    time::Duration,
};

use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, EndpointId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, NetworkRequestId, PromptBundleSnapshotId, ProviderId,
    TransportAttemptId, UtcTimestamp,
};
use aura_model_domain::{
    EndpointAuthScheme, ModelAdapterIdentity, ModelApiFormat, ModelEndpointTimeouts,
    ModelExecutionSnapshot, ModelFailureCategory, ModelRequestPriority, ModelRetryClass,
    ModelRuntimeCapabilityFacts, ModelStreamEvent, NormalizedModelFailure, NormalizedModelOutput,
    NormalizedStopReason, PartialModelUsage, PersistedModelFailure, ProviderProtocolReference,
    ProviderRequestEvidence, ProviderRequestHeaderClass, ProviderRequestHeaderMetadata,
    ProviderRequestTargetMetadata, ResolvedModelRequestOptions, TransportAttemptKind,
    TransportAttemptRecord, TransportAttemptState, TransportOperationContext,
};
use aura_model_transport::{
    EndpointGate, ModelAdapterError, ModelCancellationToken, ModelNetworkExecutor,
    ModelProtocolDecoder, ModelStreamDecoder, ModelTransport, ModelTransportError,
    ModelTransportExecution, NormalizedOutputBuilder, ProviderContinuationArtifact,
    ProviderErrorResponse, ProviderRequestFactory, ProviderRequestPlan, ProviderStreamFrame,
    RawStreamWriter, SseLimits, TransportEventChannel, TransportHttpBody, TransportHttpResponse,
    TransportRetryPolicy, TransportRunTerminal, TransportStore,
};
use aura_permission_domain::NormalizedOrigin;
use serde_json::Value;

#[derive(Default)]
struct StoreState {
    snapshots: BTreeMap<ModelExecutionSnapshotId, ModelExecutionSnapshot>,
    attempts: BTreeMap<TransportAttemptId, TransportAttemptRecord>,
    failures: Vec<PersistedModelFailure>,
    raw_artifacts: BTreeMap<ArtifactId, Vec<u8>>,
    outputs: BTreeMap<ModelStepId, NormalizedModelOutput>,
}

#[derive(Clone, Default)]
struct MemoryStore {
    state: Arc<Mutex<StoreState>>,
}

struct MemoryWriter {
    artifact_id: ArtifactId,
    bytes: Vec<u8>,
    state: Arc<Mutex<StoreState>>,
}

impl RawStreamWriter for MemoryWriter {
    fn append(&mut self, bytes: &[u8]) -> Result<(), ModelTransportError> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    async fn finalize(self) -> Result<ArtifactId, ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("test store poisoned".to_owned()))?
            .raw_artifacts
            .insert(self.artifact_id, self.bytes);
        Ok(self.artifact_id)
    }
}

impl TransportStore for MemoryStore {
    type Writer = MemoryWriter;

    async fn record_execution_snapshot(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Result<(), ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("test store poisoned".to_owned()))?
            .snapshots
            .insert(snapshot.id, snapshot.clone());
        Ok(())
    }

    async fn canonical_request_body_artifact(
        &self,
        _attempt_id: TransportAttemptId,
        attempt_kind: TransportAttemptKind,
        _request_plan: &ProviderRequestPlan,
    ) -> Result<Option<ArtifactId>, ModelTransportError> {
        Ok((attempt_kind == TransportAttemptKind::CompatibilityAdjustedRetry).then(ArtifactId::new))
    }

    async fn begin_raw_stream(
        &self,
        _attempt_id: TransportAttemptId,
    ) -> Result<Self::Writer, ModelTransportError> {
        Ok(MemoryWriter {
            artifact_id: ArtifactId::new(),
            bytes: Vec::new(),
            state: self.state.clone(),
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
            .map_err(|_| ModelTransportError::Store("test store poisoned".to_owned()))?
            .raw_artifacts
            .insert(artifact_id, continuation.bytes.clone());
        Ok(artifact_id)
    }

    async fn record_attempt(
        &self,
        attempt: &TransportAttemptRecord,
    ) -> Result<(), ModelTransportError> {
        self.state
            .lock()
            .map_err(|_| ModelTransportError::Store("test store poisoned".to_owned()))?
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
            .map_err(|_| ModelTransportError::Store("test store poisoned".to_owned()))?
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
            .map_err(|_| ModelTransportError::Store("test store poisoned".to_owned()))?
            .outputs
            .insert(output.model_step_id, output.clone());
        Ok(())
    }
}

struct ResponseSpec {
    status: u16,
    headers: Vec<(String, String)>,
    chunks: Vec<Vec<u8>>,
}

struct FakeBody {
    chunks: VecDeque<Vec<u8>>,
}

impl TransportHttpBody for FakeBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, ModelTransportError> {
        Ok(self.chunks.pop_front())
    }
}

#[derive(Clone)]
struct FakeNetwork {
    responses: Arc<Mutex<VecDeque<ResponseSpec>>>,
    requests: Arc<Mutex<Vec<(TransportAttemptId, NetworkRequestId)>>>,
}

impl FakeNetwork {
    fn new(responses: Vec<ResponseSpec>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ModelNetworkExecutor for FakeNetwork {
    type Body = FakeBody;

    async fn open(
        &self,
        _snapshot: &ModelExecutionSnapshot,
        _request_plan: &ProviderRequestPlan,
        attempt_id: TransportAttemptId,
        client_request_id: NetworkRequestId,
    ) -> Result<TransportHttpResponse<Self::Body>, ModelTransportError> {
        self.requests
            .lock()
            .map_err(|_| ModelTransportError::Store("test network poisoned".to_owned()))?
            .push((attempt_id, client_request_id));
        let response = self
            .responses
            .lock()
            .map_err(|_| ModelTransportError::Store("test network poisoned".to_owned()))?
            .pop_front()
            .ok_or_else(|| {
                ModelTransportError::InvalidProviderRequest(
                    "test network has no scripted response".to_owned(),
                )
            })?;
        Ok(TransportHttpResponse {
            status: response.status,
            headers: response.headers,
            body: FakeBody { chunks: response.chunks.into() },
        })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum StallingMode {
    Connect,
    /// The provider answers once its model starts, later than the connect limit.
    SlowHeaders,
    AwaitingFirstEvent,
    StreamIdle,
    StreamIdleWithEmptyChunks,
    Total,
}

struct StallingBody {
    first_chunk: Option<Vec<u8>>,
    empty_chunk_delay: Option<Duration>,
    empty_chunks_remaining: usize,
    ends: bool,
}

impl TransportHttpBody for StallingBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, ModelTransportError> {
        if let Some(chunk) = self.first_chunk.take() {
            return Ok(Some(chunk));
        }
        if self.empty_chunks_remaining > 0 {
            self.empty_chunks_remaining -= 1;
            tokio::time::sleep(self.empty_chunk_delay.expect("empty chunk delay")).await;
            return Ok(Some(Vec::new()));
        }
        if self.ends { Ok(None) } else { pending().await }
    }
}

#[derive(Clone)]
struct StallingNetwork {
    mode: StallingMode,
    requests: Arc<Mutex<Vec<(TransportAttemptId, NetworkRequestId)>>>,
}

impl StallingNetwork {
    fn new(mode: StallingMode) -> Self {
        Self { mode, requests: Arc::new(Mutex::new(Vec::new())) }
    }
}

impl ModelNetworkExecutor for StallingNetwork {
    type Body = StallingBody;

    async fn open(
        &self,
        _snapshot: &ModelExecutionSnapshot,
        _request_plan: &ProviderRequestPlan,
        attempt_id: TransportAttemptId,
        client_request_id: NetworkRequestId,
    ) -> Result<TransportHttpResponse<Self::Body>, ModelTransportError> {
        self.requests
            .lock()
            .map_err(|_| ModelTransportError::Store("test network poisoned".to_owned()))?
            .push((attempt_id, client_request_id));
        if self.mode == StallingMode::Connect {
            return pending().await;
        }
        if self.mode == StallingMode::SlowHeaders {
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
        let first_chunk = match self.mode {
            StallingMode::StreamIdle | StallingMode::StreamIdleWithEmptyChunks => {
                Some(b"data: {\"type\":\"delta\",\"text\":\"partial\"}\n\n".to_vec())
            }
            StallingMode::SlowHeaders => Some(
                b"data: {\"type\":\"delta\",\"text\":\"late\"}\n\ndata: {\"type\":\"complete\"}\n\n"
                    .to_vec(),
            ),
            _ => None,
        };
        let empty_chunks_remaining =
            usize::from(self.mode == StallingMode::StreamIdleWithEmptyChunks) * 16;
        Ok(TransportHttpResponse {
            status: 200,
            headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
            body: StallingBody {
                first_chunk,
                empty_chunk_delay: (empty_chunks_remaining > 0)
                    .then_some(Duration::from_millis(10)),
                empty_chunks_remaining,
                ends: self.mode == StallingMode::SlowHeaders,
            },
        })
    }
}

struct StaticRequestFactory {
    endpoint_fingerprint: ContentHash,
    request_url: String,
}

impl ProviderRequestFactory for StaticRequestFactory {
    fn build_request(
        &self,
        operation_context: &TransportOperationContext,
        applied_compatibility_overrides: &BTreeSet<String>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        let body = br#"{"stream":true}"#.to_vec();
        Ok(ProviderRequestPlan {
            method: "POST".to_owned(),
            request_url: self.request_url.clone(),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            request_evidence: fixture_request_evidence(
                operation_context,
                &self.request_url,
                &body,
                applied_compatibility_overrides,
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
    compatibility_overrides: &BTreeSet<String>,
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
        adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiChatCompletions),
        operation_context: operation_context.clone(),
        tool_registry_snapshot_ref: matches!(
            operation_context,
            TransportOperationContext::AgentModelAttempt { .. }
        )
        .then(|| aura_tool_contract::ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: aura_domain_common::ToolRegistrySnapshotId::new(),
            registry_revision: aura_domain_common::Revision::INITIAL,
            content_hash: aura_domain_common::jcs::CanonicalHashV1::sha256(
                b"transport-test-registry",
            ),
        }),
        tool_name_bindings: Vec::new(),
        historical_tool_call_wire_bindings: Vec::new(),
        canonical_body_artifact_id: None,
    }
}

struct FixtureAdapter;

struct FixtureDecoder {
    output: NormalizedOutputBuilder,
    assistant_block: Option<aura_domain_common::ModelContentBlockId>,
}

impl ModelProtocolDecoder for FixtureAdapter {
    fn api_format(&self) -> ModelApiFormat {
        ModelApiFormat::OpenAiChatCompletions
    }

    fn create_stream_decoder(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Box<dyn ModelStreamDecoder> {
        Box::new(FixtureDecoder {
            output: NormalizedOutputBuilder::new(snapshot.model_step_id),
            assistant_block: None,
        })
    }

    fn classify_http_error(&self, response: &ProviderErrorResponse) -> NormalizedModelFailure {
        NormalizedModelFailure {
            category: ModelFailureCategory::Transport,
            code: format!("fixture_http_{}", response.status),
            message: "fixture HTTP failure".to_owned(),
            retry_class: if response.status == 503 {
                ModelRetryClass::TransparentRetry { retry_after_ms: Some(0) }
            } else {
                ModelRetryClass::NotRetryable
            },
            provider_status: Some(response.status),
            provider_request_id: response.provider_request_id.clone(),
            provider_error_ref: response.raw_error_artifact_id,
            partial_output_ref: None,
            transport_attempt_id: None,
        }
    }

    fn runtime_capability_facts(
        &self,
        _request_plan: &ProviderRequestPlan,
    ) -> ModelRuntimeCapabilityFacts {
        ModelRuntimeCapabilityFacts {
            api_format: ModelApiFormat::OpenAiChatCompletions,
            native_tool_channel_exposed: false,
            native_attachment_parts_present: false,
            reasoning_channel_observed: false,
            native_capability_names: Vec::new(),
        }
    }
}

impl ModelStreamDecoder for FixtureDecoder {
    fn push_frame(
        &mut self,
        frame: ProviderStreamFrame,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let value: Value = serde_json::from_str(&frame.data).map_err(|_| {
            ModelAdapterError::protocol("fixture_json_invalid", "fixture event is invalid JSON")
        })?;
        match value.get("type").and_then(Value::as_str) {
            Some("delta") => {
                let text = value.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ModelAdapterError::protocol("fixture_text_missing", "fixture delta has no text")
                })?;
                let mut events = Vec::new();
                let block_id = if let Some(block_id) = self.assistant_block {
                    block_id
                } else {
                    let block_id =
                        self.output.start_assistant_block(Some(ProviderProtocolReference {
                            item_id: Some("fixture-item".to_owned()),
                            block_id: None,
                            index: Some(0),
                        }));
                    self.assistant_block = Some(block_id);
                    events.push(ModelStreamEvent::AssistantTextStarted { block_id });
                    block_id
                };
                self.output.append_assistant(block_id, text)?;
                events
                    .push(ModelStreamEvent::AssistantTextDelta { block_id, text: text.to_owned() });
                Ok(events)
            }
            Some("complete") => {
                let usage = PartialModelUsage {
                    input_tokens: Some(1),
                    output_tokens: Some(1),
                    total_tokens: Some(2),
                    reasoning_tokens: None,
                    cached_input_tokens: None,
                    cache_write_tokens: None,
                };
                self.output.update_usage(usage.clone());
                self.output.complete_response(NormalizedStopReason::EndTurn)?;
                Ok(vec![
                    ModelStreamEvent::UsageUpdated { usage },
                    ModelStreamEvent::ResponseCompleted {
                        stop_reason: NormalizedStopReason::EndTurn,
                    },
                ])
            }
            _ => Err(ModelAdapterError::protocol(
                "fixture_event_unknown",
                "fixture event type is unknown",
            )),
        }
    }

    fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError> {
        self.output.finish()
    }
}

fn snapshot() -> ModelExecutionSnapshot {
    let endpoint_fingerprint = ContentHash::blake3(b"transport-endpoint");
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Fixture Provider".to_owned(),
        endpoint_display_name: "Fixture Endpoint".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Fixture Provider / Fixture Endpoint / fixture-model".to_owned(),
        api_format: ModelApiFormat::OpenAiChatCompletions,
        chat_output_token_field: Some(aura_model_domain::ChatOutputTokenField::MaxTokens),
        chat_instruction_role: Some(aura_model_domain::ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: "https://api.example.test/v1/chat/completions".to_owned(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 256_000,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions::output_only(128),
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint,
        created_at: UtcTimestamp::now(),
    }
}

fn limits(connect: u64, first_event: u64, stream_idle: u64, total: u64) -> ModelEndpointTimeouts {
    ModelEndpointTimeouts {
        connect_ms: connect,
        first_event_ms: first_event,
        stream_idle_ms: stream_idle,
        total_ms: Some(total),
    }
}

fn timeouts() -> ModelEndpointTimeouts {
    limits(1_000, 1_000, 1_000, 5_000)
}

fn success_response(include_terminal: bool) -> ResponseSpec {
    let mut body = b"data: {\"type\":\"delta\",\"text\":\"hello\"}\n\n".to_vec();
    if include_terminal {
        body.extend_from_slice(b"data: {\"type\":\"complete\"}\n\n");
    }
    ResponseSpec {
        status: 200,
        headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
        chunks: vec![body],
    }
}

fn transport(network: FakeNetwork, store: MemoryStore) -> ModelTransport<FakeNetwork, MemoryStore> {
    ModelTransport::new(
        network,
        store,
        TransportRetryPolicy::new(Duration::ZERO, Duration::ZERO, Duration::ZERO),
        SseLimits::default(),
    )
}

async fn execute_stalling_scenario(
    mode: StallingMode,
    timeout_settings: ModelEndpointTimeouts,
) -> (TransportRunTerminal, usize, usize, Vec<ModelStreamEvent>) {
    let snapshot = snapshot();
    let network = StallingNetwork::new(mode);
    let observed_network = network.clone();
    let store = MemoryStore::default();
    let observed_store = store.clone();
    let model_transport = ModelTransport::new(
        network,
        store,
        TransportRetryPolicy::new(Duration::ZERO, Duration::ZERO, Duration::ZERO),
        SseLimits::default(),
    );
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("gate");
    let factory = StaticRequestFactory {
        endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_url: snapshot.resolved_request_url.clone(),
    };
    let (events, mut receiver) = TransportEventChannel::bounded(32);
    let terminal = model_transport
        .execute(ModelTransportExecution {
            snapshot: &snapshot,
            endpoint_timeouts: &timeout_settings,
            gate: &gate,
            priority: ModelRequestPriority::InteractiveParent,
            request_factory: &factory,
            adapter: &FixtureAdapter,
            initial_compatibility_overrides: BTreeSet::new(),
            cancellation: &ModelCancellationToken::default(),
            events: &events,
        })
        .await
        .expect("transport execution");
    assert_eq!(gate.metrics().expect("gate metrics").active_requests, 0);
    let request_count = observed_network.requests.lock().expect("requests").len();
    let raw_artifact_count = observed_store.state.lock().expect("store state").raw_artifacts.len();
    let mut observed_events = Vec::new();
    while let Ok(envelope) = receiver.try_recv() {
        observed_events.push(envelope.event);
    }
    (terminal, request_count, raw_artifact_count, observed_events)
}

fn assert_timeout_failure(
    terminal: TransportRunTerminal,
    expected_code: &str,
    expected_category: ModelFailureCategory,
    expected_attempts: usize,
) {
    match terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(failure.failure.code, expected_code);
            assert_eq!(failure.failure.category, expected_category);
            assert_eq!(attempts.len(), expected_attempts);
            assert!(attempts.iter().all(|attempt| {
                attempt.state == TransportAttemptState::Failed
                    && attempt.request_evidence.operation_context
                        == attempts[0].request_evidence.operation_context
            }));
            let distinct_attempts =
                attempts.iter().map(|attempt| attempt.id).collect::<BTreeSet<_>>();
            assert_eq!(distinct_attempts.len(), expected_attempts);
        }
        TransportRunTerminal::Completed { .. } => panic!("timed-out request must not complete"),
        TransportRunTerminal::Cancelled { .. } => panic!("timed-out request was not cancelled"),
    }
}

fn assert_terminal_failure_event(events: &[ModelStreamEvent], expected_code: &str) {
    let terminal_failures = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                ModelStreamEvent::StreamError { failure } if failure.code == expected_code
            )
        })
        .count();
    assert_eq!(terminal_failures, 1);
    assert!(
        !events.iter().any(|event| { matches!(event, ModelStreamEvent::ResponseCompleted { .. }) })
    );
}

#[tokio::test]
async fn transparent_retry_reuses_mex_and_creates_new_tat_and_client_request_id() {
    let snapshot = snapshot();
    let network = FakeNetwork::new(vec![
        ResponseSpec {
            status: 503,
            headers: vec![("x-request-id".to_owned(), "provider-first".to_owned())],
            chunks: vec![br#"{"error":{"message":"temporary"}}"#.to_vec()],
        },
        success_response(true),
    ]);
    let observed_network = network.clone();
    let store = MemoryStore::default();
    let observed_store = store.clone();
    let model_transport = transport(network, store);
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("gate");
    let factory = StaticRequestFactory {
        endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_url: snapshot.resolved_request_url.clone(),
    };
    let (events, mut receiver) = TransportEventChannel::bounded(32);

    let terminal = model_transport
        .execute(ModelTransportExecution {
            snapshot: &snapshot,
            endpoint_timeouts: &timeouts(),
            gate: &gate,
            priority: ModelRequestPriority::InteractiveParent,
            request_factory: &factory,
            adapter: &FixtureAdapter,
            initial_compatibility_overrides: BTreeSet::new(),
            cancellation: &ModelCancellationToken::default(),
            events: &events,
        })
        .await
        .expect("transport execution");
    let attempts = match terminal {
        TransportRunTerminal::Completed { output, attempts } => {
            assert_eq!(output.assistant_blocks[0].text, "hello");
            attempts
        }
        TransportRunTerminal::Failed { failure, .. } => {
            panic!("transport unexpectedly failed: {}", failure.failure.code)
        }
        TransportRunTerminal::Cancelled { .. } => {
            panic!("transport unexpectedly cancelled")
        }
    };

    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].attempt_kind, TransportAttemptKind::Initial);
    assert_eq!(attempts[1].attempt_kind, TransportAttemptKind::TransparentRetry);
    assert_eq!(attempts[0].state, TransportAttemptState::Failed);
    assert_eq!(attempts[1].state, TransportAttemptState::Completed);
    let expected_operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    assert_eq!(
        attempts[0].request_evidence.operation_context, expected_operation_context,
        "transparent retry must reuse the logical model-execution context"
    );
    assert_eq!(
        attempts[1].request_evidence.operation_context,
        attempts[0].request_evidence.operation_context
    );
    assert_ne!(attempts[0].id, attempts[1].id);
    assert_ne!(attempts[0].client_request_id, attempts[1].client_request_id);
    assert_eq!(gate.metrics().expect("gate metrics").active_requests, 0);

    let requests = observed_network.requests.lock().expect("requests");
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].0, requests[1].0);
    assert_ne!(requests[0].1, requests[1].1);
    drop(requests);
    let state = observed_store.state.lock().expect("store state");
    assert_eq!(state.failures.len(), 1);
    assert_eq!(state.raw_artifacts.len(), 2);
    drop(state);

    let mut completed = 0;
    let mut failed = 0;
    while let Ok(envelope) = receiver.try_recv() {
        match envelope.event {
            ModelStreamEvent::ResponseCompleted { .. } => completed += 1,
            ModelStreamEvent::StreamError { .. } => failed += 1,
            _ => {}
        }
    }
    assert_eq!(completed, 1);
    assert_eq!(failed, 0);
}

#[tokio::test]
async fn a_valid_event_then_missing_terminal_is_not_retried_and_emits_one_error() {
    let snapshot = snapshot();
    let network = FakeNetwork::new(vec![success_response(false), success_response(true)]);
    let observed_network = network.clone();
    let store = MemoryStore::default();
    let observed_store = store.clone();
    let model_transport = transport(network, store);
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("gate");
    let factory = StaticRequestFactory {
        endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_url: snapshot.resolved_request_url.clone(),
    };
    let (events, mut receiver) = TransportEventChannel::bounded(32);

    let terminal = model_transport
        .execute(ModelTransportExecution {
            snapshot: &snapshot,
            endpoint_timeouts: &timeouts(),
            gate: &gate,
            priority: ModelRequestPriority::InteractiveParent,
            request_factory: &factory,
            adapter: &FixtureAdapter,
            initial_compatibility_overrides: BTreeSet::new(),
            cancellation: &ModelCancellationToken::default(),
            events: &events,
        })
        .await
        .expect("transport execution");
    let failure = match terminal {
        TransportRunTerminal::Failed { failure, attempts, .. } => {
            assert_eq!(attempts.len(), 1);
            assert_eq!(attempts[0].state, TransportAttemptState::Failed);
            failure
        }
        TransportRunTerminal::Completed { .. } => panic!("missing terminal must not succeed"),
        TransportRunTerminal::Cancelled { .. } => {
            panic!("missing terminal must not be reported as cancelled")
        }
    };
    assert_eq!(failure.failure.category, ModelFailureCategory::Protocol);
    assert_eq!(failure.failure.code, "provider_terminal_missing");
    assert_eq!(observed_network.requests.lock().expect("requests").len(), 1);
    assert_eq!(observed_store.state.lock().expect("state").failures.len(), 1);
    assert_eq!(gate.metrics().expect("gate metrics").active_requests, 0);

    let mut assistant_deltas = 0;
    let mut completed = 0;
    let mut failed = 0;
    while let Ok(envelope) = receiver.try_recv() {
        match envelope.event {
            ModelStreamEvent::AssistantTextDelta { .. } => assistant_deltas += 1,
            ModelStreamEvent::ResponseCompleted { .. } => completed += 1,
            ModelStreamEvent::StreamError { .. } => failed += 1,
            _ => {}
        }
    }
    assert_eq!(assistant_deltas, 1);
    assert_eq!(completed, 0);
    assert_eq!(failed, 1);
}

#[tokio::test]
async fn cancelling_while_waiting_for_the_endpoint_emits_one_terminal_failure() {
    let snapshot = snapshot();
    let network = FakeNetwork::new(vec![success_response(true)]);
    let observed_network = network.clone();
    let store = MemoryStore::default();
    let observed_store = store.clone();
    let model_transport = transport(network, store);
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("gate");
    let holder = gate
        .acquire(ModelRequestPriority::InteractiveParent, &ModelCancellationToken::default())
        .await
        .expect("holder permit");
    let factory = StaticRequestFactory {
        endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_url: snapshot.resolved_request_url.clone(),
    };
    let cancellation = ModelCancellationToken::default();
    let task_cancellation = cancellation.clone();
    let task_gate = gate.clone();
    let (events, mut receiver) = TransportEventChannel::bounded(32);
    let task = tokio::spawn(async move {
        model_transport
            .execute(ModelTransportExecution {
                snapshot: &snapshot,
                endpoint_timeouts: &timeouts(),
                gate: &task_gate,
                priority: ModelRequestPriority::Maintenance,
                request_factory: &factory,
                adapter: &FixtureAdapter,
                initial_compatibility_overrides: BTreeSet::new(),
                cancellation: &task_cancellation,
                events: &events,
            })
            .await
    });
    while gate.metrics().expect("gate metrics").queued_requests == 0 {
        tokio::task::yield_now().await;
    }
    cancellation.cancel();
    let terminal = task.await.expect("transport task").expect("transport execution");
    let attempts = match terminal {
        TransportRunTerminal::Cancelled { cancellation, attempts } => {
            assert_eq!(cancellation.category, ModelFailureCategory::Cancelled);
            attempts
        }
        TransportRunTerminal::Failed { .. } => panic!("cancelled request must not be failed"),
        TransportRunTerminal::Completed { .. } => panic!("cancelled request must not complete"),
    };
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].state, TransportAttemptState::Cancelled);
    assert!(attempts[0].failure_id.is_none());
    assert!(observed_network.requests.lock().expect("requests").is_empty());
    assert!(observed_store.state.lock().expect("store state").failures.is_empty());
    assert_eq!(gate.metrics().expect("gate metrics").queued_requests, 0);

    let mut failed = 0;
    while let Ok(envelope) = receiver.try_recv() {
        if matches!(envelope.event, ModelStreamEvent::StreamError { .. }) {
            failed += 1;
        }
    }
    assert_eq!(failed, 1);
    drop(holder);
    assert_eq!(gate.metrics().expect("gate metrics").active_requests, 0);
}

#[tokio::test]
async fn cancelling_during_retry_backoff_stops_before_a_second_request() {
    let snapshot = snapshot();
    let network = FakeNetwork::new(vec![
        ResponseSpec {
            status: 503,
            headers: Vec::new(),
            chunks: vec![br#"{"error":{"message":"temporary"}}"#.to_vec()],
        },
        success_response(true),
    ]);
    let observed_network = network.clone();
    let store = MemoryStore::default();
    let observed_store = store.clone();
    let model_transport = ModelTransport::new(
        network,
        store,
        TransportRetryPolicy::new(
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(30),
        ),
        SseLimits::default(),
    );
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("gate");
    let task_gate = gate.clone();
    let factory = StaticRequestFactory {
        endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_url: snapshot.resolved_request_url.clone(),
    };
    let cancellation = ModelCancellationToken::default();
    let task_cancellation = cancellation.clone();
    let (events, mut receiver) = TransportEventChannel::bounded(32);
    let task = tokio::spawn(async move {
        model_transport
            .execute(ModelTransportExecution {
                snapshot: &snapshot,
                endpoint_timeouts: &timeouts(),
                gate: &task_gate,
                priority: ModelRequestPriority::InteractiveParent,
                request_factory: &factory,
                adapter: &FixtureAdapter,
                initial_compatibility_overrides: BTreeSet::new(),
                cancellation: &task_cancellation,
                events: &events,
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let failure_recorded =
                observed_store.state.lock().expect("store state").failures.len() == 1;
            if failure_recorded && gate.metrics().expect("gate metrics").active_requests == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first attempt entered backoff");

    cancellation.cancel();
    let terminal = task.await.expect("transport task").expect("transport execution");
    match terminal {
        TransportRunTerminal::Cancelled { cancellation, attempts } => {
            assert_eq!(cancellation.category, ModelFailureCategory::Cancelled);
            assert_eq!(attempts.len(), 1);
        }
        TransportRunTerminal::Failed { .. } => panic!("cancelled backoff must not be failed"),
        TransportRunTerminal::Completed { .. } => panic!("cancelled backoff must not complete"),
    }
    assert_eq!(observed_network.requests.lock().expect("requests").len(), 1);
    assert_eq!(observed_store.state.lock().expect("state").failures.len(), 1);
    assert_eq!(gate.metrics().expect("gate metrics").active_requests, 0);

    let mut failed = 0;
    while let Ok(envelope) = receiver.try_recv() {
        if matches!(envelope.event, ModelStreamEvent::StreamError { .. }) {
            failed += 1;
        }
    }
    assert_eq!(failed, 1);
}

/// A provider that never answers exhausts three attempts on the first-event limit.
#[tokio::test]
async fn a_response_that_never_starts_exhausts_three_attempts_on_the_first_event_limit() {
    let timeout_settings = limits(10, 50, 1_000, 1_000);
    let (terminal, request_count, raw_artifact_count, events) =
        execute_stalling_scenario(StallingMode::Connect, timeout_settings).await;

    let code = "model_transport_firstevent_timeout";
    assert_timeout_failure(terminal, code, ModelFailureCategory::Transport, 3);
    assert_eq!(request_count, 3);
    assert_eq!(raw_artifact_count, 0);
    assert_terminal_failure_event(&events, code);
}

/// A gateway answering after the connect limit (gpt-6.1-sol at `max`) is waited for.
#[tokio::test]
async fn a_response_that_starts_after_the_connect_limit_is_still_waited_for() {
    let timeout_settings = limits(10, 1_000, 1_000, 5_000);
    let (terminal, request_count, _, _) =
        execute_stalling_scenario(StallingMode::SlowHeaders, timeout_settings).await;
    assert!(matches!(terminal, TransportRunTerminal::Completed { .. }), "the answer is kept");
    assert_eq!(request_count, 1);
}

#[tokio::test]
async fn first_event_timeout_retries_without_committing_truncated_raw_attempts() {
    let timeout_settings = limits(1_000, 10, 1_000, 1_000);
    let (terminal, request_count, raw_artifact_count, events) =
        execute_stalling_scenario(StallingMode::AwaitingFirstEvent, timeout_settings).await;

    assert_timeout_failure(
        terminal,
        "model_transport_firstevent_timeout",
        ModelFailureCategory::Transport,
        3,
    );
    assert_eq!(request_count, 3);
    assert_eq!(raw_artifact_count, 0);
    assert_terminal_failure_event(&events, "model_transport_firstevent_timeout");
}

#[tokio::test]
async fn stream_idle_timeout_after_a_valid_event_is_not_retried() {
    let timeout_settings = limits(1_000, 1_000, 10, 1_000);
    let (terminal, request_count, raw_artifact_count, events) =
        execute_stalling_scenario(StallingMode::StreamIdle, timeout_settings).await;

    assert_timeout_failure(
        terminal,
        "model_transport_streamidle_timeout",
        ModelFailureCategory::StreamInterrupted,
        1,
    );
    assert_eq!(request_count, 1);
    assert_eq!(raw_artifact_count, 0);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ModelStreamEvent::AssistantTextDelta { .. }))
            .count(),
        1
    );
    assert_terminal_failure_event(&events, "model_transport_streamidle_timeout");
}

#[tokio::test]
async fn empty_body_chunks_do_not_postpone_stream_idle_timeout() {
    let timeout_settings = limits(1_000, 1_000, 35, 80);
    let (terminal, request_count, raw_artifact_count, events) =
        execute_stalling_scenario(StallingMode::StreamIdleWithEmptyChunks, timeout_settings).await;

    assert_timeout_failure(
        terminal,
        "model_transport_streamidle_timeout",
        ModelFailureCategory::StreamInterrupted,
        1,
    );
    assert_eq!(request_count, 1);
    assert_eq!(raw_artifact_count, 0);
    assert_terminal_failure_event(&events, "model_transport_streamidle_timeout");
}

#[tokio::test]
async fn total_timeout_retries_before_the_first_event_and_exhausts_attempt_budget() {
    let timeout_settings = limits(1_000, 1_000, 1_000, 10);
    let (terminal, request_count, raw_artifact_count, events) =
        execute_stalling_scenario(StallingMode::Total, timeout_settings).await;

    assert_timeout_failure(
        terminal,
        "model_transport_total_timeout",
        ModelFailureCategory::Transport,
        3,
    );
    assert_eq!(request_count, 3);
    assert_eq!(raw_artifact_count, 0);
    assert_terminal_failure_event(&events, "model_transport_total_timeout");
}

#[tokio::test]
async fn cancelling_after_a_valid_event_discards_partial_stream_without_retrying() {
    let snapshot = snapshot();
    let network = StallingNetwork::new(StallingMode::StreamIdle);
    let observed_network = network.clone();
    let store = MemoryStore::default();
    let observed_store = store.clone();
    let model_transport = ModelTransport::new(
        network,
        store,
        TransportRetryPolicy::new(Duration::ZERO, Duration::ZERO, Duration::ZERO),
        SseLimits::default(),
    );
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("gate");
    let task_gate = gate.clone();
    let factory = StaticRequestFactory {
        endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_url: snapshot.resolved_request_url.clone(),
    };
    let cancellation = ModelCancellationToken::default();
    let task_cancellation = cancellation.clone();
    let timeout_settings = ModelEndpointTimeouts {
        connect_ms: 5_000,
        first_event_ms: 5_000,
        stream_idle_ms: 5_000,
        total_ms: None,
    };
    let (events, mut receiver) = TransportEventChannel::bounded(32);
    let task = tokio::spawn(async move {
        model_transport
            .execute(ModelTransportExecution {
                snapshot: &snapshot,
                endpoint_timeouts: &timeout_settings,
                gate: &task_gate,
                priority: ModelRequestPriority::InteractiveParent,
                request_factory: &factory,
                adapter: &FixtureAdapter,
                initial_compatibility_overrides: BTreeSet::new(),
                cancellation: &task_cancellation,
                events: &events,
            })
            .await
    });

    let mut observed_events = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let envelope = receiver.recv().await.expect("stream event");
            let received_delta =
                matches!(envelope.event, ModelStreamEvent::AssistantTextDelta { .. });
            observed_events.push(envelope.event);
            if received_delta {
                break;
            }
        }
    })
    .await
    .expect("valid model event");
    cancellation.cancel();

    let terminal = task.await.expect("transport task").expect("transport execution");
    match terminal {
        TransportRunTerminal::Cancelled { cancellation, attempts } => {
            assert_eq!(cancellation.category, ModelFailureCategory::Cancelled);
            assert_eq!(cancellation.code, "model_transport_cancelled");
            assert_eq!(attempts.len(), 1);
            assert_eq!(attempts[0].state, TransportAttemptState::Cancelled);
            assert!(attempts[0].failure_id.is_none());
            assert!(attempts[0].first_byte_at.is_some());
            assert!(attempts[0].first_model_event_at.is_some());
            assert!(attempts[0].raw_stream_artifact_id.is_none());
        }
        TransportRunTerminal::Failed { .. } => panic!("cancelled stream must not be failed"),
        TransportRunTerminal::Completed { .. } => panic!("cancelled stream must not complete"),
    }
    while let Ok(envelope) = receiver.try_recv() {
        observed_events.push(envelope.event);
    }
    assert_eq!(observed_network.requests.lock().expect("requests").len(), 1);
    assert!(observed_store.state.lock().expect("store state").raw_artifacts.is_empty());
    assert!(observed_store.state.lock().expect("store state").failures.is_empty());
    assert_eq!(gate.metrics().expect("gate metrics").active_requests, 0);
    assert_terminal_failure_event(&observed_events, "model_transport_cancelled");
}
