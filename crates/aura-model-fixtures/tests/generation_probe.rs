use aura_domain_common::{
    EndpointId, ModelAvailabilityTestId, ModelProfileId, ProviderId, Revision, UtcTimestamp,
};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelContextWindowConfig, ModelDiscoveryMode, ModelEndpointConfig, ModelEndpointTimeouts,
    ModelProfileConfig, ModelProfileOrigin, ModelRequestDefaults, TransportOperationContext,
};
use aura_model_transport::{
    MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS, MODEL_GENERATION_PROBE_PROMPT,
    ModelAvailabilityProbeCompilationInput, ModelGenerationProbeProtocol,
};
use serde_json::{Value, json};

fn endpoint_and_profile(api_format: ModelApiFormat) -> (ModelEndpointConfig, ModelProfileConfig) {
    let endpoint_id = EndpointId::new();
    let profile_id = ModelProfileId::new();
    let now = UtcTimestamp::now();
    let endpoint = ModelEndpointConfig {
        id: endpoint_id,
        revision: Revision::INITIAL,
        provider_id: ProviderId::new(),
        display_name: "Fixture endpoint".to_owned(),
        slug: "fixture-endpoint".parse().expect("valid Endpoint slug"),
        api_format,
        base_url: "https://api.example.test/v1".to_owned(),
        discovery_mode: ModelDiscoveryMode::Automatic,
        chat_output_token_field: (api_format == ModelApiFormat::OpenAiChatCompletions)
            .then_some(ChatOutputTokenField::MaxCompletionTokens),
        chat_instruction_role: (api_format == ModelApiFormat::OpenAiChatCompletions)
            .then_some(ChatInstructionRole::Developer),
        auth_scheme: EndpointAuthScheme::None,
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        timeouts: ModelEndpointTimeouts {
            connect_ms: 5_000,
            first_event_ms: 10_000,
            stream_idle_ms: 10_000,
            total_ms: Some(30_000),
        },
        max_parallel_model_requests: 1,
        model_order: vec![profile_id],
        compatibility: None,
        created_at: now,
        updated_at: now,
    };
    let profile = ModelProfileConfig {
        id: profile_id,
        revision: Revision::INITIAL,
        endpoint_id,
        provider_model_name: "fixture-model".to_owned(),
        display_name: "Fixture model".to_owned(),
        notes: None,
        origin: ModelProfileOrigin::Manual,
        enabled: true,
        available_for_subagents: false,
        catalog: None,
        context: ModelContextWindowConfig {
            catalog_window_tokens: None,
            user_override_window_tokens: None,
            user_override_revision: 0,
            runtime_corrected_window_tokens: None,
            runtime_correction_basis: None,
            runtime_effective_window_tokens: 32_000,
            historical_snapshot_max_tokens: 1_024,
        },
        request_defaults: ModelRequestDefaults {
            max_output_tokens: None,
            temperature: Some(0.5),
            top_p: Some(0.8),
            chat_output_token_field: (api_format == ModelApiFormat::OpenAiChatCompletions)
                .then_some(ChatOutputTokenField::MaxTokens),
            chat_instruction_role: None,
            reasoning_effort: None,
            reasoning_efforts: None,
            image_input: None,
        },
        created_at: now,
        updated_at: now,
    };
    (endpoint, profile)
}

fn probe_context(profile_id: ModelProfileId) -> TransportOperationContext {
    TransportOperationContext::ModelAvailabilityTest {
        model_availability_test_id: ModelAvailabilityTestId::new(),
        model_profile_id: profile_id,
    }
}

#[test]
fn responses_probe_is_fixed_non_streaming_plain_text_and_requires_visible_text() {
    let (endpoint, profile) = endpoint_and_profile(ModelApiFormat::OpenAiResponses);
    let context = probe_context(profile.id);
    let adapter = OpenAiResponsesAdapter;
    let plan = adapter
        .compile_generation_probe(ModelAvailabilityProbeCompilationInput {
            endpoint: &endpoint,
            model_profile: &profile,
            resolved_request_url: "https://api.example.test/v1/responses",
            operation_context: &context,
            applied_output_token_field_alias: None,
        })
        .expect("Responses generation probe");
    let body: Value = serde_json::from_slice(&plan.body).expect("probe JSON");
    assert_eq!(body["input"][0]["content"][0]["text"], MODEL_GENERATION_PROBE_PROMPT);
    assert_eq!(body["max_output_tokens"], MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS);
    assert_eq!(body["stream"], false);
    for forbidden in ["tools", "temperature", "top_p", "instructions", "previous_response_id"] {
        assert!(body.get(forbidden).is_none(), "unexpected probe field {forbidden}");
    }
    adapter
        .validate_generation_probe_success_body(
            json!({
                "status": "completed",
                "output": [{
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Acknowledged."}],
                }],
            })
            .to_string()
            .as_bytes(),
        )
        .expect("visible Responses text");
    assert!(
        adapter
            .validate_generation_probe_success_body(
                json!({
                    "status": "completed",
                    "output": [{"type": "reasoning", "content": []}],
                })
                .to_string()
                .as_bytes(),
            )
            .is_err()
    );
}

#[test]
fn chat_probe_uses_configured_token_field_without_optional_sampling_fields() {
    let (endpoint, profile) = endpoint_and_profile(ModelApiFormat::OpenAiChatCompletions);
    let context = probe_context(profile.id);
    let adapter = OpenAiChatCompletionsAdapter;
    let plan = adapter
        .compile_generation_probe(ModelAvailabilityProbeCompilationInput {
            endpoint: &endpoint,
            model_profile: &profile,
            resolved_request_url: "https://api.example.test/v1/chat/completions",
            operation_context: &context,
            applied_output_token_field_alias: None,
        })
        .expect("Chat generation probe");
    let body: Value = serde_json::from_slice(&plan.body).expect("probe JSON");
    assert_eq!(
        body["messages"],
        json!([{"role": "user", "content": MODEL_GENERATION_PROBE_PROMPT}])
    );
    assert_eq!(body["max_tokens"], MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS);
    assert!(body.get("max_completion_tokens").is_none());
    assert_eq!(body["stream"], false);
    for forbidden in ["tools", "temperature", "top_p", "response_format"] {
        assert!(body.get(forbidden).is_none(), "unexpected probe field {forbidden}");
    }
    adapter
        .validate_generation_probe_success_body(
            json!({
                "choices": [{
                    "index": 0,
                    "finish_reason": "stop",
                    "message": {"role": "assistant", "content": "Acknowledged."},
                }],
            })
            .to_string()
            .as_bytes(),
        )
        .expect("visible Chat text");
    assert!(
        adapter
            .validate_generation_probe_success_body(
                json!({
                    "choices": [{
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {"role": "assistant", "content": "", "refusal": "no"},
                    }],
                })
                .to_string()
                .as_bytes(),
            )
            .is_err()
    );
}

#[test]
fn anthropic_probe_has_no_system_or_reasoning_controls_and_requires_text() {
    let (endpoint, profile) = endpoint_and_profile(ModelApiFormat::AnthropicMessages);
    let context = probe_context(profile.id);
    let adapter = AnthropicMessagesAdapter;
    let plan = adapter
        .compile_generation_probe(ModelAvailabilityProbeCompilationInput {
            endpoint: &endpoint,
            model_profile: &profile,
            resolved_request_url: "https://api.example.test/v1/messages",
            operation_context: &context,
            applied_output_token_field_alias: None,
        })
        .expect("Anthropic generation probe");
    let body: Value = serde_json::from_slice(&plan.body).expect("probe JSON");
    assert_eq!(body["messages"][0]["content"][0]["text"], MODEL_GENERATION_PROBE_PROMPT);
    assert_eq!(body["max_tokens"], MODEL_GENERATION_PROBE_MAX_OUTPUT_TOKENS);
    assert_eq!(body["stream"], false);
    for forbidden in ["tools", "temperature", "top_p", "system", "thinking"] {
        assert!(body.get(forbidden).is_none(), "unexpected probe field {forbidden}");
    }
    adapter
        .validate_generation_probe_success_body(
            json!({
                "type": "message",
                "stop_reason": "end_turn",
                "content": [{"type": "text", "text": "Acknowledged."}],
            })
            .to_string()
            .as_bytes(),
        )
        .expect("visible Anthropic text");
    assert!(
        adapter
            .validate_generation_probe_success_body(
                json!({
                    "type": "message",
                    "stop_reason": "tool_use",
                    "content": [{"type": "tool_use", "id": "tool", "name": "x"}],
                })
                .to_string()
                .as_bytes(),
            )
            .is_err()
    );
}

#[test]
fn probe_rejects_non_test_operation_context() {
    let (endpoint, profile) = endpoint_and_profile(ModelApiFormat::OpenAiResponses);
    let context = TransportOperationContext::ModelDiscovery {
        discovery_run_id: aura_domain_common::ModelDiscoveryRunId::new(),
    };
    let error = OpenAiResponsesAdapter
        .compile_generation_probe(ModelAvailabilityProbeCompilationInput {
            endpoint: &endpoint,
            model_profile: &profile,
            resolved_request_url: "https://api.example.test/v1/responses",
            operation_context: &context,
            applied_output_token_field_alias: None,
        })
        .expect_err("probe must not borrow a Discovery or Agent context");
    assert_eq!(error.code, "model_generation_probe.operation_context_mismatch");
}
