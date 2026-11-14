//! What compatible services send beside the strict OpenAI stream, read the way OpenCode's own
//! client reads it: a finish reason named again with the usage, no `[DONE]`, usage without
//! choices, a choice without its index, `"error": null`, and thinking under other field names.
//! What would lose or change content is still refused.
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, ModelExecutionSnapshotId, ModelProfileId,
    ModelStepId, PromptBundleSnapshotId, ProviderId, UtcTimestamp,
};
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelExecutionSnapshot, ModelStreamEvent, NormalizedModelOutput, NormalizedStopReason,
    ResolvedModelRequestOptions,
};
use aura_model_transport::{
    ModelAdapterError, ModelGenerationProbeProtocol, ModelProtocolDecoder, ProviderStreamFrame,
};
use serde_json::{Value, json};

fn snapshot() -> ModelExecutionSnapshot {
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Compatible Provider".to_owned(),
        endpoint_display_name: "Chat Completions".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Compatible Provider / Chat Completions / fixture-model".to_owned(),
        api_format: ModelApiFormat::OpenAiChatCompletions,
        chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: Some(ChatInstructionRole::System),
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
            effective_max_output_tokens: 512,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: false,
            speed_mode: None,
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::blake3(b"chat-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn frame(value: &Value) -> ProviderStreamFrame {
    ProviderStreamFrame { data: value.to_string(), ..ProviderStreamFrame::default() }
}

fn done() -> ProviderStreamFrame {
    ProviderStreamFrame { data: "[DONE]".to_owned(), ..ProviderStreamFrame::default() }
}

/// Every chunk through one decoder; the events of all of them, then the output or the error.
fn decode(
    chunks: &[Value],
    with_done: bool,
) -> (Vec<ModelStreamEvent>, Result<NormalizedModelOutput, ModelAdapterError>) {
    let mut decoder = OpenAiChatCompletionsAdapter.create_stream_decoder(&snapshot());
    let mut events = Vec::new();
    for chunk in chunks {
        match decoder.push_frame(frame(chunk)) {
            Ok(more) => events.extend(more),
            Err(error) => return (events, Err(error)),
        }
    }
    if with_done && let Err(error) = decoder.push_frame(done()) {
        return (events, Err(error));
    }
    (events, decoder.finish())
}

fn text(output: &NormalizedModelOutput) -> String {
    output.assistant_blocks.iter().map(|block| block.text.as_str()).collect()
}

fn code(result: Result<NormalizedModelOutput, ModelAdapterError>) -> String {
    match result {
        Ok(_) => panic!("the stream was accepted"),
        Err(error) => error.code,
    }
}

fn content(text: &str) -> Value {
    json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {"content": text}}]})
}

/// The last chunk, with the usage every stream must report.
fn finished(reason: &str) -> Value {
    json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {}, "finish_reason": reason}],
           "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12}})
}

#[test]
fn a_finish_reason_named_again_with_the_usage_keeps_the_whole_answer() {
    // OpenCode Go's Kimi K3: the last text with `stop`, then the usage with `stop` once more.
    let (events, output) = decode(
        &[
            content("你好！"),
            json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {"content": "😊"}, "finish_reason": "stop"}]}),
            json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                   "usage": {"prompt_tokens": 120, "completion_tokens": 8, "total_tokens": 128}}),
        ],
        true,
    );
    let output = output.expect("the repeated finish reason ends nothing a second time");
    assert_eq!(text(&output), "你好！😊");
    assert_eq!(output.stop_reason, NormalizedStopReason::EndTurn);
    assert_eq!(output.usage.input_tokens, 120);
    assert_eq!(
        events.iter().filter(|e| matches!(e, ModelStreamEvent::ResponseCompleted { .. })).count(),
        1
    );
}

#[test]
fn content_after_the_finish_reason_is_refused() {
    let (_, output) = decode(&[content("one"), finished("stop"), content("two")], true);
    assert_eq!(code(output), "openai_chat_content_after_finish");
}

#[test]
fn a_tool_call_after_the_finish_reason_is_refused() {
    let late = json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {"tool_calls": [
        {"index": 0, "id": "call_1", "type": "function", "function": {"name": "read", "arguments": "{}"}}
    ]}}]});
    let (_, output) = decode(&[content("one"), finished("stop"), late], true);
    assert_eq!(code(output), "openai_chat_content_after_finish");
}

#[test]
fn a_different_finish_reason_later_keeps_the_first_and_says_so() {
    let (events, output) = decode(&[content("one"), finished("stop"), finished("length")], true);
    assert_eq!(
        output.expect("the first finish reason stands").stop_reason,
        NormalizedStopReason::EndTurn
    );
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ProviderWarning { code: Some(code), .. }
            if code == "openai_chat_finish_reason_changed"
    )));
}

#[test]
fn a_stream_closed_without_done_after_its_finish_reason_completes() {
    let (_, output) = decode(&[content("one"), finished("stop")], false);
    assert_eq!(text(&output.expect("no [DONE] is needed after the finish reason")), "one");
}

#[test]
fn a_stream_without_any_finish_reason_still_fails() {
    let (_, output) = decode(&[content("one")], true);
    assert_eq!(code(output), "openai_chat_terminal_missing");
}

#[test]
fn a_chunk_of_usage_without_choices_is_read() {
    let usage = json!({"id": "chatcmpl-1", "usage": {"prompt_tokens": 90, "completion_tokens": 9, "total_tokens": 99}});
    let (_, output) = decode(&[content("one"), finished("stop"), usage], true);
    assert_eq!(output.expect("usage alone is accepted").usage.total_tokens, 99);
}

#[test]
fn usage_inside_the_choice_is_read() {
    let last = json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop",
        "usage": {"prompt_tokens": 40, "completion_tokens": 5, "total_tokens": 45}}]});
    let (_, output) = decode(&[content("one"), last], true);
    assert_eq!(output.expect("usage in the choice").usage.input_tokens, 40);
}

#[test]
fn a_choice_without_its_index_is_the_only_choice() {
    let (_, output) = decode(
        &[
            json!({"id": "chatcmpl-1", "choices": [{"delta": {"content": "one"}}]}),
            json!({"id": "chatcmpl-1", "choices": [{"delta": {}, "finish_reason": "stop"}],
                   "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12}}),
        ],
        true,
    );
    assert_eq!(text(&output.expect("a missing index is index 0")), "one");
}

#[test]
fn a_choice_with_another_index_is_still_refused() {
    let second =
        json!({"id": "chatcmpl-1", "choices": [{"index": 1, "delta": {"content": "two"}}]});
    let (_, output) = decode(&[second], true);
    assert_eq!(code(output), "openai_chat_choice_index_unsupported");
}

#[test]
fn choices_that_are_not_a_list_are_still_refused() {
    for choices in [json!({"index": 0, "delta": {"content": "one"}}), json!("one")] {
        let (_, output) = decode(&[json!({"id": "chatcmpl-1", "choices": choices})], true);
        assert_eq!(code(output), "openai_chat_choices_invalid");
    }
}

#[test]
fn an_error_field_that_is_null_is_not_an_error() {
    let (_, output) = decode(
        &[
            json!({"id": "chatcmpl-1", "error": null, "choices": [{"index": 0, "delta": {"content": "one"}}]}),
            json!({"id": "chatcmpl-1", "error": null, "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                   "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12}}),
        ],
        true,
    );
    assert_eq!(text(&output.expect("a null error is no error")), "one");
}

#[test]
fn an_error_object_still_fails_the_stream() {
    let failed = json!({"error": {"message": "upstream failed", "code": "server_error"}});
    let (_, output) = decode(&[content("one"), failed], true);
    assert_eq!(code(output), "server_error");
}

fn thinking_chunks(delta: Value) -> Vec<Value> {
    vec![
        json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": delta}]}),
        content("answer"),
        finished("stop"),
    ]
}

#[test]
fn thinking_under_reasoning_or_reasoning_text_is_shown_and_marked() {
    for field in ["reasoning", "reasoning_text"] {
        let (_, output) = decode(&thinking_chunks(json!({field: "Reading the file first."})), true);
        let output = output.expect("thinking under another name");
        let block = &output.reasoning_blocks[0];
        assert_eq!(block.text, "Reading the file first.", "{field}");
        assert_eq!(
            block.provider_reference.as_ref().and_then(|r| r.block_id.as_deref()),
            Some(field),
            "the block names the field, so it is not sent back as reasoning_content"
        );
    }
}

#[test]
fn thinking_as_reasoning_content_is_left_unmarked() {
    let (_, output) =
        decode(&thinking_chunks(json!({"reasoning_content": "Reading the file first."})), true);
    let output = output.expect("thinking as reasoning_content");
    let block = &output.reasoning_blocks[0];
    assert_eq!(block.text, "Reading the file first.");
    assert_eq!(block.provider_reference.as_ref().and_then(|r| r.block_id.as_deref()), None);
}

#[test]
fn reasoning_details_give_their_text_and_summary_but_no_encrypted_part() {
    let (_, output) = decode(
        &thinking_chunks(json!({"reasoning_details": [
            {"type": "reasoning.text", "text": "Step one."},
            {"type": "reasoning.summary", "summary": "Looked at one file."},
            {"type": "reasoning.encrypted", "data": "opaque"}
        ]})),
        true,
    );
    let output = output.expect("structured thinking");
    let block = &output.reasoning_blocks[0];
    assert_eq!(block.text, "Step one.");
    assert_eq!(block.summary.as_deref(), Some("Looked at one file."));
}

#[test]
fn a_scalar_thinking_field_wins_over_the_same_text_in_reasoning_details() {
    let (_, output) = decode(
        &thinking_chunks(json!({
            "reasoning": "Once.",
            "reasoning_details": [{"type": "reasoning.text", "text": "Once."}]
        })),
        true,
    );
    assert_eq!(output.expect("thinking twice over").reasoning_blocks[0].text, "Once.");
}

#[test]
fn a_probe_reply_without_its_index_passes() {
    let body = json!({"choices": [{"message": {"role": "assistant", "content": "OK"}, "finish_reason": "stop"}]});
    OpenAiChatCompletionsAdapter
        .validate_generation_probe_success_body(body.to_string().as_bytes())
        .expect("a missing index is index 0");
}
