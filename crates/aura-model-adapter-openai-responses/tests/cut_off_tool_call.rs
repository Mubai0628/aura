//! A response cut off by the output limit while the model was writing a function call.
//!
//! DeepSeek's thinking mode spent 8146 of 8192 output tokens reasoning in one evaluation sample, then
//! opened a function call and closed it without arguments before `response.incomplete`. The
//! decoder called that a protocol error, the step failed as not retryable, and the task was
//! interrupted. The cut-off call is not an executable call (its arguments are partial), so it is
//! dropped and the step ends as a truncated response; Core then reminds the model why.
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, ModelExecutionSnapshotId, ModelProfileId,
    ModelStepId, PromptBundleSnapshotId, ProviderId, UtcTimestamp,
};
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    EndpointAuthScheme, ModelApiFormat, ModelExecutionSnapshot, NormalizedModelOutput,
    NormalizedStopReason, ResolvedModelRequestOptions,
};
use aura_model_transport::{
    ModelAdapterError, ModelProtocolDecoder, ProviderStreamFrame, SseDecoder, SseLimits,
};
use serde_json::{Value, json};

fn snapshot() -> ModelExecutionSnapshot {
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "DeepSeek".to_owned(),
        endpoint_display_name: "Responses".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "DeepSeek / Responses / fixture-model".to_owned(),
        api_format: ModelApiFormat::OpenAiResponses,
        chat_output_token_field: None,
        chat_instruction_role: None,
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: "https://api.example.test/v1/responses".to_owned(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 256_000,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions {
            effective_max_output_tokens: 8_192,
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
        endpoint_fingerprint: ContentHash::blake3(b"responses-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn frame(value: &Value) -> String {
    let kind = value["type"].as_str().expect("a Responses event has a type");
    format!("event: {kind}\ndata: {value}\n\n")
}

fn decode(raw: &str) -> Result<NormalizedModelOutput, ModelAdapterError> {
    let mut sse = SseDecoder::new(SseLimits::default());
    let mut decoder = OpenAiResponsesAdapter.create_stream_decoder(&snapshot());
    let mut frames: Vec<ProviderStreamFrame> = sse.push(raw.as_bytes()).expect("valid SSE");
    frames.extend(sse.finish().expect("SSE terminator"));
    for frame in frames {
        decoder.push_frame(frame)?;
    }
    decoder.finish()
}

/// Reasoning, then a function call the Provider closes as `done_item`, then `terminal`.
fn stream(done_item: Value, terminal: &Value) -> String {
    [
        frame(&json!({"type": "response.created", "response": {"id": "resp_cut"}})),
        frame(&json!({
            "type": "response.reasoning_text.delta",
            "item_id": "rs_cut", "output_index": 0, "content_index": 0,
            "delta": "Thinking about which file to write first."
        })),
        frame(&json!({
            "type": "response.output_item.added",
            "output_index": 1,
            "item": {"type": "function_call", "id": "fc_cut", "call_id": "call_cut", "name": "fs_write__01"}
        })),
        frame(&json!({"type": "response.output_item.done", "output_index": 1, "item": done_item})),
        frame(terminal),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat()
}

fn incomplete() -> Value {
    json!({
        "type": "response.incomplete",
        "response": {
            "id": "resp_cut",
            "incomplete_details": {"reason": "max_output_tokens"},
            "usage": {"input_tokens": 31955, "output_tokens": 8192, "total_tokens": 40147,
                      "output_tokens_details": {"reasoning_tokens": 8146}}
        }
    })
}

fn completed() -> Value {
    json!({
        "type": "response.completed",
        "response": {"id": "resp_cut", "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}}
    })
}

fn item(extra: Value) -> Value {
    let mut item = json!({"type": "function_call", "id": "fc_cut", "call_id": "call_cut", "name": "fs_write__01"});
    item.as_object_mut().expect("object").extend(extra.as_object().expect("object").clone());
    item
}

#[test]
fn a_function_call_cut_off_at_the_output_limit_is_dropped_not_a_protocol_error() {
    for done in [
        item(json!({"status": "incomplete", "arguments": ""})),
        item(json!({"status": "incomplete", "arguments": "{\"path\":\"src/li"})),
        item(json!({"arguments": ""})),
        item(json!({})),
    ] {
        let output = decode(&stream(done.clone(), &incomplete()))
            .unwrap_or_else(|error| panic!("{done}: the response must decode: {error:?}"));
        assert_eq!(output.stop_reason, NormalizedStopReason::MaxOutputTokens, "{done}");
        assert!(output.tool_calls.is_empty(), "{done}: a cut-off call is never executable");
        assert!(!output.reasoning_blocks.is_empty(), "{done}: the reasoning is kept");
    }
}

#[test]
fn an_unfinished_call_in_a_response_that_claims_completion_is_still_refused() {
    // Only the output limit explains a missing argument list; a completed response does not.
    let error =
        decode(&stream(item(json!({"status": "incomplete", "arguments": ""})), &completed()))
            .expect_err("a completed response cannot end with an unfinished call");
    assert_eq!(error.code, "provider_tool_call_incomplete");
    let error =
        decode(&stream(item(json!({"status": "completed", "arguments": ""})), &incomplete()))
            .expect_err("a call the Provider says it completed must carry its arguments");
    assert_eq!(error.code, "openai_responses_field_missing");
}
