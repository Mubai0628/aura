//! The real Transport branch for a refused Provider tool call.
//!
//! These cases drive `ModelTransport::execute` with a scripted SSE body and a
//! real frozen request: the adapter normalizes the call, the domain check binds
//! it against the frozen tool list, and the run terminal must carry the bounded
//! evidence of the refusal. Nothing here calls the diagnostic builder directly.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ArtifactId, ContentHash, ContentHashAlgorithm, ContextProjectionSnapshotId, EndpointId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, NetworkRequestId,
    PromptBundleSnapshotId, ProviderId, Revision, ToolId, ToolRegistrySnapshotId,
    TransportAttemptId, UtcTimestamp,
};
use aura_model_domain::{
    ChatOutputTokenField, EndpointAuthScheme, ModelAdapterIdentity, ModelApiFormat,
    ModelEndpointTimeouts, ModelExecutionSnapshot, ModelFailureCategory, ModelRequestPriority,
    ModelRetryClass, ModelRuntimeCapabilityFacts, ModelStreamEvent, NormalizedModelFailure,
    NormalizedModelOutput, NormalizedStopReason, PartialModelUsage, PersistedModelFailure,
    ProviderRequestEvidence, ProviderRequestHeaderClass, ProviderRequestHeaderMetadata,
    ProviderRequestTargetMetadata, ProviderToolNameBinding, ResolvedModelRequestOptions,
    TransportAttemptKind, TransportAttemptRecord, TransportOperationContext,
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
use aura_tool_contract::{ToolInputSchemaV1, ToolSchemaSnapshotV1, ToolVersion};

/// The wire name of the one tool the frozen request exposes. It is the name the
/// registry publishes for `fixture.tool0`, never a hand-written guess.
fn frozen_wire_name() -> String {
    aura_model_domain::provider_wire_tool_name("fixture.tool0", ToolId::from_bytes([1; 16]))
}

#[derive(Default)]
struct StoreState {
    failures: Vec<PersistedModelFailure>,
    raw_artifacts: BTreeMap<ArtifactId, Vec<u8>>,
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
        _snapshot: &ModelExecutionSnapshot,
    ) -> Result<(), ModelTransportError> {
        Ok(())
    }

    async fn canonical_request_body_artifact(
        &self,
        _attempt_id: TransportAttemptId,
        _attempt_kind: TransportAttemptKind,
        _request_plan: &ProviderRequestPlan,
    ) -> Result<Option<ArtifactId>, ModelTransportError> {
        Ok(None)
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
        _continuation: &ProviderContinuationArtifact,
    ) -> Result<ArtifactId, ModelTransportError> {
        Err(ModelTransportError::Store("no continuation in this fixture".to_owned()))
    }

    async fn record_attempt(
        &self,
        _attempt: &TransportAttemptRecord,
    ) -> Result<(), ModelTransportError> {
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
        _output: &NormalizedModelOutput,
    ) -> Result<(), ModelTransportError> {
        Ok(())
    }
}

struct FakeBody {
    chunks: VecDeque<Vec<u8>>,
}

impl TransportHttpBody for FakeBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, ModelTransportError> {
        Ok(self.chunks.pop_front())
    }
}

struct FakeNetwork {
    body: Mutex<Option<Vec<u8>>>,
}

impl ModelNetworkExecutor for FakeNetwork {
    type Body = FakeBody;

    async fn open(
        &self,
        _snapshot: &ModelExecutionSnapshot,
        _request_plan: &ProviderRequestPlan,
        _attempt_id: TransportAttemptId,
        _client_request_id: NetworkRequestId,
    ) -> Result<TransportHttpResponse<Self::Body>, ModelTransportError> {
        let body = self
            .body
            .lock()
            .map_err(|_| ModelTransportError::Store("test network poisoned".to_owned()))?
            .take()
            .ok_or_else(|| {
                ModelTransportError::InvalidProviderRequest(
                    "test network has no scripted response".to_owned(),
                )
            })?;
        Ok(TransportHttpResponse {
            status: 200,
            headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
            body: FakeBody { chunks: vec![body].into() },
        })
    }
}

/// A frozen request with exactly one exposed tool.
struct FrozenRequestFactory {
    endpoint_fingerprint: ContentHash,
    request_url: String,
}

impl ProviderRequestFactory for FrozenRequestFactory {
    fn build_request(
        &self,
        operation_context: &TransportOperationContext,
        compatibility_overrides: &BTreeSet<String>,
    ) -> Result<ProviderRequestPlan, ModelAdapterError> {
        let registry_ref = aura_tool_contract::ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"transport-binding-registry"),
        };
        let schema = ToolSchemaSnapshotV1::new(
            ToolId::from_bytes([1; 16]),
            ToolVersion::new(None, Revision::INITIAL).expect("tool version"),
            "fixture.tool0".parse().expect("canonical tool name"),
            "Fixture tool".to_owned(),
            ToolInputSchemaV1::new(serde_json::json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"],
                "additionalProperties": false
            }))
            .expect("tool input schema"),
            true,
        )
        .expect("tool schema snapshot");
        let (_, wrapped_hash) =
            aura_model_domain::provider_arguments_envelope_schema_v3(&schema.input_schema)
                .expect("argument envelope");
        let binding = ProviderToolNameBinding::from_snapshot(&registry_ref, &schema, wrapped_hash);
        assert_eq!(binding.provider_wire_name, frozen_wire_name());
        let body = br#"{"stream":true}"#.to_vec();
        let url = url::Url::parse(&self.request_url).expect("fixture request URL");
        Ok(ProviderRequestPlan {
            method: "POST".to_owned(),
            request_url: self.request_url.clone(),
            headers: vec![("content-type".to_owned(), "application/json".to_owned())],
            request_evidence: ProviderRequestEvidence {
                body_sha256: ContentHash::sha256(&body),
                body_byte_length: u64::try_from(body.len()).expect("fixture body length"),
                target: ProviderRequestTargetMetadata {
                    method: "POST".to_owned(),
                    origin: NormalizedOrigin {
                        scheme: url.scheme().to_owned(),
                        host: url.host_str().expect("fixture host").to_owned(),
                        port: url.port_or_known_default().expect("fixture port"),
                    },
                    path: url.path().to_owned(),
                    query_parameter_names: BTreeSet::new(),
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
                tool_registry_snapshot_ref: Some(registry_ref),
                tool_name_bindings: vec![binding],
                historical_tool_call_wire_bindings: Vec::new(),
                canonical_body_artifact_id: None,
            },
            body,
            safe_optional_parameters: BTreeSet::new(),
        })
    }

    fn current_endpoint_fingerprint(&self) -> Result<ContentHash, ModelTransportError> {
        Ok(self.endpoint_fingerprint.clone())
    }
}

struct ToolCallAdapter;

struct ToolCallDecoder {
    output: NormalizedOutputBuilder,
    started: bool,
}

impl ModelProtocolDecoder for ToolCallAdapter {
    fn api_format(&self) -> ModelApiFormat {
        ModelApiFormat::OpenAiChatCompletions
    }

    /// Adapter-owned evidence: the transport must report exactly this value.
    fn tool_call_item_type(&self) -> &'static str {
        "fixture_function_call"
    }

    fn create_stream_decoder(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Box<dyn ModelStreamDecoder> {
        Box::new(ToolCallDecoder {
            output: NormalizedOutputBuilder::new(snapshot.model_step_id),
            started: false,
        })
    }

    fn classify_http_error(&self, response: &ProviderErrorResponse) -> NormalizedModelFailure {
        NormalizedModelFailure {
            category: ModelFailureCategory::Transport,
            code: format!("fixture_http_{}", response.status),
            message: "fixture HTTP failure".to_owned(),
            retry_class: ModelRetryClass::NotRetryable,
            provider_status: Some(response.status),
            provider_request_id: None,
            provider_error_ref: None,
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
            native_tool_channel_exposed: true,
            native_attachment_parts_present: false,
            reasoning_channel_observed: false,
            native_capability_names: Vec::new(),
        }
    }
}

impl ModelStreamDecoder for ToolCallDecoder {
    fn push_frame(
        &mut self,
        frame: ProviderStreamFrame,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        if frame.data == "[DONE]" {
            return Ok(Vec::new());
        }
        let value: serde_json::Value = serde_json::from_str(&frame.data).map_err(|_| {
            ModelAdapterError::protocol("fixture_json_invalid", "fixture event is invalid JSON")
        })?;
        let name = value.get("name").and_then(serde_json::Value::as_str).ok_or_else(|| {
            ModelAdapterError::protocol("fixture_name_missing", "fixture call has no name")
        })?;
        let arguments = value.get("arguments").and_then(serde_json::Value::as_str).unwrap_or("{}");
        let provider_call_id =
            aura_model_domain::ProviderToolCallId::new("call_fixture").expect("provider call ID");
        self.output.start_tool_call(provider_call_id.clone(), name.to_owned(), 0)?;
        self.output.append_tool_arguments(&provider_call_id, arguments)?;
        self.output.complete_tool_call(&provider_call_id)?;
        self.started = true;
        let usage = PartialModelUsage {
            input_tokens: Some(4),
            output_tokens: Some(2),
            total_tokens: Some(6),
            reasoning_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
        };
        self.output.update_usage(usage);
        self.output.complete_response(NormalizedStopReason::ToolCalls)?;
        Ok(vec![
            ModelStreamEvent::ToolCallStarted {
                provider_call_id: provider_call_id.clone(),
                tool_name: name.to_owned(),
                position: 0,
            },
            ModelStreamEvent::ToolCallCompleted { provider_call_id },
            ModelStreamEvent::ResponseCompleted { stop_reason: NormalizedStopReason::ToolCalls },
        ])
    }

    fn finish(&mut self) -> Result<NormalizedModelOutput, ModelAdapterError> {
        let _ = self.started;
        self.output.finish()
    }
}

fn snapshot() -> ModelExecutionSnapshot {
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
        chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: None,
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
        request_options: ResolvedModelRequestOptions {
            effective_max_output_tokens: 128,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: false,
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::blake3(b"transport-binding-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn sse_body(name: &str) -> Vec<u8> {
    let event = serde_json::json!({
        "type": "tool_call",
        "name": name,
        "arguments": "{\"aura_purpose\":\"fixture\",\"value\":\"x\"}"
    });
    format!("data: {event}\n\ndata: [DONE]\n\n").into_bytes()
}

async fn execute(name: &str) -> (TransportRunTerminal, MemoryStore, ModelStepId) {
    let snapshot = snapshot();
    let model_step_id = snapshot.model_step_id;
    let network = FakeNetwork { body: Mutex::new(Some(sse_body(name))) };
    let store = MemoryStore::default();
    let transport = ModelTransport::new(
        network,
        store.clone(),
        TransportRetryPolicy::new(Duration::ZERO, Duration::ZERO, Duration::ZERO),
        SseLimits::default(),
    );
    let gate = EndpointGate::new(snapshot.endpoint_id, 1).expect("gate");
    let factory = FrozenRequestFactory {
        endpoint_fingerprint: snapshot.endpoint_fingerprint.clone(),
        request_url: snapshot.resolved_request_url.clone(),
    };
    let (events, mut receiver) = TransportEventChannel::bounded(32);
    let terminal = transport
        .execute(ModelTransportExecution {
            snapshot: &snapshot,
            endpoint_timeouts: &ModelEndpointTimeouts {
                connect_ms: 1_000,
                first_event_ms: 1_000,
                stream_idle_ms: 1_000,
                total_ms: Some(5_000),
            },
            gate: &gate,
            priority: ModelRequestPriority::InteractiveParent,
            request_factory: &factory,
            adapter: &ToolCallAdapter,
            initial_compatibility_overrides: BTreeSet::new(),
            cancellation: &ModelCancellationToken::default(),
            events: &events,
        })
        .await
        .expect("transport execution");
    while receiver.try_recv().is_ok() {}
    (terminal, store, model_step_id)
}

/// An illegal function name is refused by the real transport branch, and the run
/// terminal carries the bounded evidence with the step and attempt identity.
#[tokio::test]
async fn the_transport_carries_bounded_evidence_for_a_refused_tool_call() {
    let (terminal, store, model_step_id) = execute("_aura").await;
    let TransportRunTerminal::Failed { failure, attempts, diagnostic } = terminal else {
        panic!("an illegal function name must fail the run");
    };
    assert_eq!(failure.failure.code, "provider_tool_call_binding_invalid");
    assert_eq!(failure.failure.category, ModelFailureCategory::Protocol);
    assert!(failure.failure.message.contains("wire name _aura"));
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].state, aura_model_domain::TransportAttemptState::Failed);
    // The raw stream of a refused response is not retained.
    assert!(
        store.state.lock().expect("store").raw_artifacts.is_empty(),
        "a refused response body must not be persisted"
    );

    let diagnostic = diagnostic.expect("a binding refusal carries structured evidence");
    assert_eq!(diagnostic.code, "provider_tool_call_binding_invalid");
    assert_eq!(diagnostic.error_name, "unknown_provider_wire_tool_name");
    assert_eq!(diagnostic.refused_wire_name.as_deref(), Some("_aura"));
    assert_eq!(diagnostic.provider_item_type, "fixture_function_call");
    assert_eq!(diagnostic.adapter, "openai_chat_completions@1.0");
    assert_eq!(diagnostic.frozen_tool_count, 1);
    assert_eq!(diagnostic.frozen_tool_list_digest.len(), 64);
    assert_eq!(diagnostic.model_step_id, model_step_id.to_string());
    assert_eq!(diagnostic.transport_attempt_id, attempts[0].id.to_string());
    let summary = diagnostic.summary();
    assert!(summary.contains("wire name _aura"), "{summary}");
    assert!(summary.contains("fixture_function_call"), "{summary}");
    assert!(summary.len() < 512, "{summary}");
    assert_eq!(diagnostic.to_json()["frozen_tool_list_digest"].as_str().expect("digest").len(), 64);
    assert_eq!(
        failure.fingerprint.0.algorithm,
        ContentHashAlgorithm::Blake3,
        "the failure fingerprint still uses its own algorithm"
    );
}

/// The same harness accepts a legal frozen name, so the rejection above is the
/// frozen tool list and not the fixture.
#[tokio::test]
async fn the_same_transport_accepts_the_frozen_name() {
    let (terminal, _, _) = execute(&frozen_wire_name()).await;
    let TransportRunTerminal::Completed { output, .. } = terminal else {
        panic!("a frozen tool name must bind");
    };
    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.tool_calls[0].tool_name, frozen_wire_name());
}
