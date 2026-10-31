use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, UtcTimestamp,
};
use aura_domain_common::{ToolId, ToolRegistrySnapshotId};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_domain::{
    CompiledInstructionEnvelope, EndpointAuthScheme, LogicalConversationItem,
    LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest, ModelApiFormat,
    ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest, ModelStreamEvent,
    NormalizedStopReason, ResolvedModelRequestOptions, TransportOperationContext,
};
use aura_model_transport::{
    ModelProtocolCompiler, ModelProtocolDecoder, ModelRequestCompilationInput, ProviderStreamFrame,
};
use aura_tool_contract::{
    ToolInputSchemaV1, ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1,
    ToolVersion,
};
use serde_json::{Value, json};

fn snapshot() -> ModelExecutionSnapshot {
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Anthropic".to_owned(),
        endpoint_display_name: "Messages".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Anthropic / Messages / fixture-model".to_owned(),
        api_format: ModelApiFormat::AnthropicMessages,
        chat_output_token_field: None,
        chat_instruction_role: None,
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: "https://api.example.test/v1/messages".to_owned(),
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
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::blake3(b"anthropic-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn frame(value: Value) -> ProviderStreamFrame {
    ProviderStreamFrame { data: value.to_string(), ..ProviderStreamFrame::default() }
}

fn instruction_envelope() -> CompiledInstructionEnvelope {
    let blocks = [
        LogicalInstructionKind::Core,
        LogicalInstructionKind::Permission,
        LogicalInstructionKind::ToolSummary,
        LogicalInstructionKind::WorkspaceAgents,
        LogicalInstructionKind::HistoricalSnapshot,
        LogicalInstructionKind::AgentMode,
        LogicalInstructionKind::Hook,
        LogicalInstructionKind::TaskState,
    ]
    .into_iter()
    .map(|kind| {
        let content = format!("<{kind:?}>{kind:?}</{kind:?}>");
        LogicalInstructionBlock {
            kind,
            content_hash: ContentHash::sha256(content.as_bytes()),
            content,
            source_event_ids: Vec::new(),
        }
    })
    .collect::<Vec<_>>();
    let canonical_text =
        blocks.iter().map(|block| block.content.as_str()).collect::<Vec<_>>().join("\n");
    CompiledInstructionEnvelope::new(1, blocks, canonical_text).expect("instruction envelope")
}

fn tool_schema_set() -> ToolSchemaSetSnapshotV1 {
    let tool = ToolSchemaSnapshotV1::new(
        ToolId::from_bytes([3; 16]),
        ToolVersion::new(None, aura_domain_common::Revision::INITIAL).expect("tool version"),
        "fixture.inspect".parse().expect("canonical tool name"),
        "Inspect a fixture value".to_owned(),
        ToolInputSchemaV1::new(json!({
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"],
            "additionalProperties": false
        }))
        .expect("tool input schema"),
        true,
    )
    .expect("tool schema snapshot");
    ToolSchemaSetSnapshotV1::new(
        ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: aura_domain_common::Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"anthropic-test-registry"),
        },
        vec![tool],
    )
    .expect("schema set")
}

fn logical_request(snapshot: &ModelExecutionSnapshot) -> LogicalModelRequest {
    LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope: instruction_envelope(),
        conversation_items: vec![LogicalConversationItem::User {
            source_event_id: EventId::new(),
            content: "hello".to_owned(),
            content_hash: ContentHash::sha256(b"hello"),
        }],
        tool_schema_set: tool_schema_set(),
        attachments: Vec::new(),
        provider_continuation: None,
        output_request: ModelOutputRequest {
            format: ModelOutputFormat::Text,
            effective_max_output_tokens: 512,
        },
        closing_question: None,
        closing_exchange: Vec::new(),
    }
}

#[test]
fn compiles_top_level_system_array_and_anthropic_max_tokens() {
    let snapshot = snapshot();
    let request = logical_request(&snapshot);
    let operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let compatibility_removals = std::collections::BTreeSet::new();
    let plan = AnthropicMessagesAdapter
        .compile_request(ModelRequestCompilationInput {
            request: &request,
            snapshot: &snapshot,
            resolved_attachments: &[],
            operation_context: &operation_context,
            compatibility_removals: &compatibility_removals,
            applied_output_token_field_alias: None,
        })
        .expect("compile Anthropic request");
    let body: Value = serde_json::from_slice(&plan.body).expect("request JSON");
    assert_eq!(body["system"][0]["type"], "text");
    assert_eq!(
        body["system"][0]["text"],
        aura_model_transport::instruction_layout(&request).stable
    );
    assert!(
        !body["system"][0]["text"].as_str().expect("stable instructions").contains("<TaskState>")
    );
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["max_tokens"], 512);
    assert_eq!(body["stream"], true);
    let input_schema = &body["tools"][0]["input_schema"];
    // Envelope 3: the tool's own root object with the required purpose beside its fields.
    assert_eq!(input_schema["required"], json!(["aura_purpose", "value"]));
    assert_eq!(input_schema["additionalProperties"], false);
    assert_eq!(input_schema["properties"]["value"], json!({"type": "string"}));
    assert_eq!(input_schema["properties"]["aura_purpose"]["type"], "string");
    assert!(
        input_schema["properties"].get("_aura").is_none()
            && input_schema["properties"].get("arguments").is_none(),
        "no `_aura`/`arguments` wrapper: {input_schema}"
    );
    // The purpose carries length bounds, which no strict implementation supports, so the
    // schema is sent without `strict` and Core validates the call.
    assert!(body["tools"][0].get("strict").is_none(), "strict outside the strict subset");
    assert_eq!(
        plan.request_evidence.tool_registry_snapshot_ref.as_ref(),
        Some(&request.tool_schema_set.registry_snapshot_ref)
    );
    assert_eq!(plan.request_evidence.tool_name_bindings.len(), 1);
    assert_eq!(
        plan.request_evidence
            .headers
            .iter()
            .find(|header| header.name == "anthropic-version")
            .and_then(|header| header.non_secret_value.as_deref()),
        Some("2023-06-01")
    );
}

#[test]
fn unknown_provider_events_emit_a_warning_without_panicking() {
    let adapter = AnthropicMessagesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    let events = decoder
        .push_frame(frame(json!({
            "type": "future_message_event",
            "future_payload": {"value": true}
        })))
        .expect("unknown event is forward-compatible");
    assert!(matches!(
        events.as_slice(),
        [ModelStreamEvent::ProviderWarning { code: Some(code), .. }]
            if code == "anthropic_event_unknown"
    ));
}

fn start_message(decoder: &mut Box<dyn aura_model_transport::ModelStreamDecoder>) {
    start_message_with_usage(decoder, json!({ "input_tokens": 7 }));
}

fn start_message_with_usage(
    decoder: &mut Box<dyn aura_model_transport::ModelStreamDecoder>,
    usage: serde_json::Value,
) {
    decoder
        .push_frame(frame(json!({
            "type": "message_start",
            "message": {
                "id": "msg_fixture",
                "usage": usage
            }
        })))
        .expect("message_start");
}

#[test]
fn cached_usage_counts_all_input_and_survives_output_only_delta() {
    let mut decoder = AnthropicMessagesAdapter.create_stream_decoder(&snapshot());
    start_message_with_usage(
        &mut decoder,
        json!({ "input_tokens": 7, "cache_read_input_tokens": 1000,
            "cache_creation_input_tokens": 200 }),
    );
    decoder
        .push_frame(frame(json!({ "type": "content_block_start", "index": 0,
            "content_block": { "type": "text", "text": "ok" } })))
        .expect("visible response");
    decoder
        .push_frame(frame(json!({ "type": "content_block_stop", "index": 0 })))
        .expect("text complete");
    decoder
        .push_frame(frame(json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": { "output_tokens": 3 }
        })))
        .expect("output-only delta");
    decoder.push_frame(frame(json!({ "type": "message_stop" }))).expect("message_stop");
    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.usage.input_tokens, 1207);
    assert_eq!(output.usage.output_tokens, 3);
    assert_eq!(output.usage.total_tokens, 1210);
    assert_eq!(output.usage.cached_input_tokens, Some(1000));
    assert_eq!(output.usage.cache_write_tokens, Some(200));
}

fn finish_message(decoder: &mut Box<dyn aura_model_transport::ModelStreamDecoder>) {
    decoder
        .push_frame(frame(json!({
            "type": "message_delta",
            "delta": { "stop_reason": "tool_use", "stop_sequence": null },
            "usage": { "output_tokens": 3 }
        })))
        .expect("message_delta");
    decoder.push_frame(frame(json!({ "type": "message_stop" }))).expect("message_stop");
}

#[test]
fn assembles_tool_input_json_and_preserves_provider_id() {
    let adapter = AnthropicMessagesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut decoder);
    decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "tool_use",
                "id": "ToolUse_ID-ExactCase",
                "name": "weather.lookup",
                "input": {}
            }
        })))
        .expect("content_block_start");
    for partial_json in ["{\"city\":\"", "Paris\"}"] {
        decoder
            .push_frame(frame(json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": partial_json
                }
            })))
            .expect("input_json_delta");
    }
    decoder
        .push_frame(frame(json!({ "type": "content_block_stop", "index": 0 })))
        .expect("content_block_stop");
    finish_message(&mut decoder);

    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.tool_calls[0].provider_call_id.as_str(), "ToolUse_ID-ExactCase");
    assert_eq!(output.tool_calls[0].arguments_json, "{\"city\":\"Paris\"}");
    assert_eq!(output.usage.input_tokens, 7);
    assert_eq!(output.usage.output_tokens, 3);
    assert_eq!(output.usage.total_tokens, 10);
}

#[test]
fn normalizes_empty_tool_input_as_an_empty_json_object() {
    let adapter = AnthropicMessagesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut decoder);
    decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "tool_use",
                "id": "empty-tool-input",
                "name": "clock.now",
                "input": {}
            }
        })))
        .expect("content_block_start");
    decoder
        .push_frame(frame(json!({ "type": "content_block_stop", "index": 0 })))
        .expect("an empty input object is still valid JSON");
    finish_message(&mut decoder);

    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.tool_calls[0].arguments_json, "{}");
}

#[test]
fn rejects_missing_blank_and_duplicate_provider_tool_ids() {
    let adapter = AnthropicMessagesAdapter;
    let mut missing_decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut missing_decoder);
    let missing = missing_decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "tool_use", "name": "missing", "input": {} }
        })))
        .expect_err("missing provider ID must fail the response");
    assert_eq!(missing.code, "provider_tool_call_id_missing");

    let mut blank_decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut blank_decoder);
    let blank = blank_decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "tool_use",
                "id": " \n ",
                "name": "blank",
                "input": {}
            }
        })))
        .expect_err("blank provider ID must fail the response");
    assert_eq!(blank.code, "provider_tool_call_id_invalid");

    let mut duplicate_decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut duplicate_decoder);
    duplicate_decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {
                "type": "tool_use",
                "id": "duplicate-id",
                "name": "first",
                "input": {}
            }
        })))
        .expect("first provider tool ID");
    let duplicate = duplicate_decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "tool_use",
                "id": "duplicate-id",
                "name": "second",
                "input": {}
            }
        })))
        .expect_err("duplicate provider ID must fail the response");
    assert_eq!(duplicate.code, "provider_tool_call_duplicate");
}

#[test]
fn preserves_thinking_and_signature_as_continuation_artifact() {
    let adapter = AnthropicMessagesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut decoder);
    decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "thinking", "thinking": "", "signature": "" }
        })))
        .expect("thinking start");
    decoder
        .push_frame(frame(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "thinking_delta", "thinking": "internal reasoning" }
        })))
        .expect("thinking delta");
    decoder
        .push_frame(frame(json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "signature_delta", "signature": "Signature-ExactCase" }
        })))
        .expect("signature delta");
    decoder
        .push_frame(frame(json!({ "type": "content_block_stop", "index": 0 })))
        .expect("thinking stop");
    decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": { "type": "text", "text": "answer" }
        })))
        .expect("text start");
    decoder
        .push_frame(frame(json!({ "type": "content_block_stop", "index": 1 })))
        .expect("text stop");
    decoder
        .push_frame(frame(json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": { "output_tokens": 6 }
        })))
        .expect("message delta");
    decoder.push_frame(frame(json!({ "type": "message_stop" }))).expect("message stop");

    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.reasoning_blocks[0].text, "internal reasoning");
    let artifact = decoder
        .take_continuation_artifact()
        .expect("continuation extraction")
        .expect("thinking continuation");
    let value: Value = serde_json::from_slice(&artifact.bytes).expect("continuation JSON");
    assert_eq!(value["format"], "anthropic_messages_thinking_v1");
    assert_eq!(value["blocks"][0]["thinking"], "internal reasoning");
    assert_eq!(value["blocks"][0]["signature"], "Signature-ExactCase");
}

#[test]
fn rejects_thinking_block_without_integrity_signature() {
    let adapter = AnthropicMessagesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut decoder);
    decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "thinking", "thinking": "visible" }
        })))
        .expect("thinking start");
    let error = decoder
        .push_frame(frame(json!({ "type": "content_block_stop", "index": 0 })))
        .expect_err("thinking without a signature must fail closed");
    assert_eq!(error.code, "anthropic_thinking_signature_missing");
}

#[test]
fn preserves_partial_text_and_normalizes_classifier_refusal_details() {
    let adapter = AnthropicMessagesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut decoder);
    decoder
        .push_frame(frame(json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "text", "text": "Partial answer before classifier." }
        })))
        .expect("partial text start");
    decoder
        .push_frame(frame(json!({ "type": "content_block_stop", "index": 0 })))
        .expect("partial text stop");
    let events = decoder
        .push_frame(frame(json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "refusal",
                "stop_sequence": null,
                "stop_details": {
                    "type": "refusal",
                    "category": "cyber",
                    "explanation": "This request was declined."
                }
            },
            "usage": { "output_tokens": 5 }
        })))
        .expect("refusal message delta");
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::RefusalDelta { text } if text == "This request was declined."
    )));
    decoder.push_frame(frame(json!({ "type": "message_stop" }))).expect("message stop");

    let output = decoder.finish().expect("normalized refusal");
    assert_eq!(output.stop_reason, NormalizedStopReason::Refusal);
    assert_eq!(output.assistant_blocks[0].text, "Partial answer before classifier.");
    let refusal = output.refusal.expect("structured refusal");
    assert_eq!(refusal.message, "This request was declined.");
    assert_eq!(refusal.provider_code.as_deref(), Some("cyber"));
}

#[test]
fn rejects_classifier_refusal_without_stop_details() {
    let adapter = AnthropicMessagesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    start_message(&mut decoder);
    let error = decoder
        .push_frame(frame(json!({
            "type": "message_delta",
            "delta": { "stop_reason": "refusal", "stop_sequence": null },
            "usage": { "output_tokens": 0 }
        })))
        .expect_err("refusal without official stop_details must fail closed");
    assert_eq!(error.code, "anthropic_refusal_details_missing");
}

fn wire_body(request: &LogicalModelRequest, snapshot: &ModelExecutionSnapshot) -> Value {
    let operation = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let plan = AnthropicMessagesAdapter
        .compile_request(ModelRequestCompilationInput {
            request,
            snapshot,
            resolved_attachments: &[],
            operation_context: &operation,
            compatibility_removals: &Default::default(),
            applied_output_token_field_alias: None,
        })
        .expect("compile continuation");
    serde_json::from_slice(&plan.body).expect("wire body")
}

#[test]
fn retains_bound_protocol_history_and_keeps_changing_state_out_of_prefix() {
    use aura_model_domain::{
        LogicalProviderHistory, ProviderHistoryData, ProviderToolCallReuseDomain,
    };
    let snapshot = snapshot();
    let mut request = logical_request(&snapshot);
    request.conversation_items.push(LogicalConversationItem::Assistant {
        source_event_id:EventId::new(), content:Some("visible answer".into()),
        content_hash:Some(ContentHash::sha256(b"visible answer")),tool_calls:vec![],
        provider_history:Some(LogicalProviderHistory { reuse_domain:ProviderToolCallReuseDomain {
            endpoint_id:snapshot.endpoint_id,endpoint_security_fingerprint:snapshot.endpoint_fingerprint.clone(),api_format:snapshot.api_format,
        },model_name:snapshot.model_name.clone(),data:ProviderHistoryData::Anthropic { thinking_blocks: vec![json!({"type":"thinking","thinking":"retained reasoning","signature":"sig_ExactCase"}),json!({"type":"redacted_thinking","data":"opaque_redacted"})] } }),
    });
    let body = wire_body(&request, &snapshot);
    let stable = body["system"].clone();
    assert_eq!(
        body["messages"]
            .as_array()
            .expect("compiled protocol array")
            .last()
            .expect("current state at request tail")["content"]
            .as_array()
            .expect("valid protocol fixture")
            .last()
            .expect("valid protocol fixture")["text"],
        "<TaskState>TaskState</TaskState>"
    );
    assert_eq!(body["messages"][1]["content"][0]["signature"], "sig_ExactCase");
    assert_eq!(body["messages"][1]["content"][1]["data"], "opaque_redacted");
    let mut blocks = request.instruction_envelope.ordered_blocks.clone();
    let state = blocks.last_mut().expect("task state");
    state.content = "<TaskState>new step and revision</TaskState>".into();
    state.content_hash = ContentHash::sha256(state.content.as_bytes());
    let canonical = blocks.iter().map(|b| b.content.as_str()).collect::<Vec<_>>().join("\n");
    request.instruction_envelope =
        CompiledInstructionEnvelope::new(1, blocks, canonical).expect("valid protocol fixture");
    let body = wire_body(&request, &snapshot);
    assert_eq!(body["system"], stable);
    assert_eq!(
        body["messages"]
            .as_array()
            .expect("compiled protocol array")
            .last()
            .expect("current state at request tail")["content"]
            .as_array()
            .expect("valid protocol fixture")
            .last()
            .expect("valid protocol fixture")["text"],
        "<TaskState>new step and revision</TaskState>"
    );
    // Endpoint and model changes cannot leak provider-owned continuation to another domain.
    for change_endpoint in [true, false] {
        let mut other = snapshot.clone();
        if change_endpoint {
            other.endpoint_id = EndpointId::new();
        } else {
            other.model_name = "another-model".into();
        }
        let body = wire_body(&request, &other);
        assert_eq!(body["messages"][1]["content"][0]["type"], "text");
    }
}

#[test]
fn native_cache_marks_stable_prefix_and_history_but_not_compatible_endpoints_or_current_state() {
    let mut native = snapshot();
    native.base_url = "https://api.anthropic.com/v1".into();
    native.resolved_request_url = "https://api.anthropic.com/v1/messages".into();
    let request = logical_request(&native);
    let body = wire_body(&request, &native);
    assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
    assert_eq!(body["messages"][0]["content"][0]["cache_control"]["type"], "ephemeral");
    let tail =
        body["messages"].as_array().expect("messages").last().expect("current message")["content"]
            .as_array()
            .expect("content")
            .last()
            .expect("current state");
    assert!(tail.get("cache_control").is_none());
    for base in [
        "https://api.example.test/v1",
        "https://api.deepseek.com/anthropic/v1",
        "https://api.anthropic.com.example.test/v1",
    ] {
        let mut compatible = native.clone();
        compatible.base_url = base.into();
        compatible.resolved_request_url = format!("{base}/messages");
        let body = wire_body(&request, &compatible);
        assert!(!body.to_string().contains("cache_control"));
    }
}

#[test]
fn fixed_tools_precede_dynamic_tools_and_default_requests_have_no_vendor_options() {
    let snapshot = snapshot();
    let mut request = logical_request(&snapshot);
    let old = &request.tool_schema_set.tools[0];
    let fixed = ToolSchemaSnapshotV1::new(
        ToolId::from_bytes([9; 16]),
        old.tool_version.clone(),
        "fixture.fixed".parse().expect("valid protocol fixture"),
        "Fixed tool".into(),
        ToolInputSchemaV1::new(old.input_schema.clone()).expect("valid protocol fixture"),
        false,
    )
    .expect("valid protocol fixture");
    request.fixed_tool_ids = vec![fixed.tool_id];
    request.tool_schema_set = ToolSchemaSetSnapshotV1::new(
        request.tool_schema_set.registry_snapshot_ref.clone(),
        vec![old.clone(), fixed],
    )
    .expect("valid protocol fixture");
    let body = wire_body(&request, &snapshot);
    assert!(
        body["tools"][0]["name"]
            .as_str()
            .expect("compiled tool name or message text")
            .starts_with("fixture_fixed")
    );
    assert!(body["tools"][0].get("strict").is_none());
    for field in [
        "thinking",
        "reasoning_effort",
        "extra_body",
        "previous_response_id",
        "conversation",
        "prompt_cache_key",
    ] {
        assert!(body.get(field).is_none(), "unsolicited provider parameter: {field}");
    }
}

#[test]
fn additions_after_a_tool_result_share_one_user_message_and_keep_thinking() {
    use aura_domain_common::ToolCallId;
    use aura_model_domain::{
        LogicalHistoricalToolCall, LogicalProviderHistory, ProviderHistoryData,
        ProviderToolCallReuseDomain, ProviderToolCallWireBinding, ProviderToolCallWireIdentity,
        ProviderWireToolCallId,
    };
    let mut snapshot = snapshot();
    snapshot.resolved_request_url = "https://api.anthropic.com/v1/messages".to_owned();
    let tool_call_id = ToolCallId::new();
    let tool_id = ToolId::new();
    snapshot.historical_tool_call_wire_bindings = vec![ProviderToolCallWireBinding {
        tool_call_id,
        tool_id: Some(tool_id),
        provider_wire_tool_name: "fixture_read".to_owned(),
        wire_identity: ProviderToolCallWireIdentity::AuraGenerated {
            provider_wire_tool_call_id: ProviderWireToolCallId::from_tool_call_id(tool_call_id),
        },
    }];
    let mut request = logical_request(&snapshot);
    request.conversation_items.push(LogicalConversationItem::Assistant {
        source_event_id: EventId::new(),
        content: Some("Looking.".to_owned()),
        content_hash: Some(ContentHash::sha256(b"Looking.")),
        tool_calls: vec![LogicalHistoricalToolCall {
            tool_call_id,
            tool_id: Some(tool_id),
            canonical_name: "fixture.read".to_owned(),
            arguments_hash: ContentHash::sha256(b"{}"),
            arguments_json: "{}".to_owned(),
        }],
        provider_history: Some(LogicalProviderHistory {
            reuse_domain: ProviderToolCallReuseDomain {
                endpoint_id: snapshot.endpoint_id,
                endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
                api_format: snapshot.api_format,
            },
            model_name: snapshot.model_name.clone(),
            data: ProviderHistoryData::Anthropic {
                thinking_blocks: vec![
                    json!({"type":"thinking","thinking":"kept","signature":"sig"}),
                ],
            },
        }),
    });
    request.conversation_items.push(LogicalConversationItem::ToolResult {
        source_event_id: EventId::new(),
        tool_call_id,
        tool_name: "fixture.read".to_owned(),
        content_hash: ContentHash::sha256(b"found"),
        content: "found".to_owned(),
        is_error: false,
    });
    for text in ["Record the path.", "Record the line."] {
        request.conversation_items.push(LogicalConversationItem::User {
            source_event_id: EventId::new(),
            content: text.to_owned(),
            content_hash: ContentHash::sha256(text.as_bytes()),
        });
    }
    let body = wire_body(&request, &snapshot);
    let messages = body["messages"].as_array().expect("messages");
    let roles = messages
        .iter()
        .map(|message| message["role"].as_str().expect("message role"))
        .collect::<Vec<_>>();
    assert_eq!(roles, vec!["user", "assistant", "user"]);
    let assistant = &messages[1]["content"];
    assert_eq!(assistant[0]["type"], "thinking");
    assert!(assistant[0].get("cache_control").is_none());
    let tail = messages[2]["content"].as_array().expect("tail blocks");
    assert_eq!(tail[0]["type"], "tool_result");
    assert_eq!(tail[1]["text"], "Record the path.");
    assert_eq!(tail[2]["text"], "Record the line.");
    assert_eq!(tail[2]["cache_control"]["type"], "ephemeral");
    assert!(tail.last().expect("current state").get("cache_control").is_none());

    let mut plain_snapshot = snapshot.clone();
    plain_snapshot.historical_tool_call_wire_bindings.clear();
    let mut plain = logical_request(&plain_snapshot);
    plain.conversation_items.push(LogicalConversationItem::User {
        source_event_id: EventId::new(),
        content: "second".to_owned(),
        content_hash: ContentHash::sha256(b"second"),
    });
    let plain_body = wire_body(&plain, &plain_snapshot);
    let plain_roles = plain_body["messages"]
        .as_array()
        .expect("plain messages")
        .iter()
        .map(|message| message["role"].as_str().expect("plain message role"))
        .collect::<Vec<_>>();
    assert_eq!(plain_roles, vec!["user", "user"]);
}

/// A thinking level is a thinking budget that leaves room for the answer
/// inside the output limit; thinking takes the provider's own sampling, and "off" is no thinking.
#[test]
fn a_thinking_level_is_a_budget_inside_the_output_limit() {
    let body = |snapshot: &ModelExecutionSnapshot| {
        let mut request = logical_request(snapshot);
        request.output_request.effective_max_output_tokens =
            snapshot.request_options.effective_max_output_tokens;
        wire_body(&request, snapshot)
    };
    let mut snapshot = snapshot();
    snapshot.request_options.effective_max_output_tokens = 8_192;
    snapshot.request_options.temperature = Some(0.2);
    snapshot.request_options.reasoning_effort = Some(aura_model_domain::ReasoningEffort::High);
    let sent = body(&snapshot);
    assert_eq!(sent["thinking"], json!({"type": "enabled", "budget_tokens": 7_168}));
    assert!(sent.get("temperature").is_none());
    snapshot.request_options.reasoning_effort = Some(aura_model_domain::ReasoningEffort::Off);
    let sent = body(&snapshot);
    assert!(sent.get("thinking").is_none());
    assert_eq!(sent["temperature"], json!(0.2));
    // An output limit with no room for a budget sends no thinking at all.
    snapshot.request_options.effective_max_output_tokens = 512;
    snapshot.request_options.reasoning_effort = Some(aura_model_domain::ReasoningEffort::Medium);
    assert!(body(&snapshot).get("thinking").is_none());
}

/// A model whose catalog lists effort levels takes the level
/// as `output_config.effort`, `max` included, and no thinking budget.
#[test]
fn a_level_is_an_effort_for_a_model_that_takes_one() {
    let body = |snapshot: &ModelExecutionSnapshot| {
        let mut request = logical_request(snapshot);
        request.output_request.effective_max_output_tokens =
            snapshot.request_options.effective_max_output_tokens;
        wire_body(&request, snapshot)
    };
    let mut snapshot = snapshot();
    snapshot.request_options.effective_max_output_tokens = 32_000;
    snapshot.request_options.temperature = Some(0.2);
    snapshot.request_options.reasoning_by_effort = true;
    snapshot.request_options.reasoning_effort = Some(aura_model_domain::ReasoningEffort::Max);
    let sent = body(&snapshot);
    assert_eq!(sent["output_config"]["effort"], json!("max"));
    assert!(sent.get("thinking").is_none());
    assert!(sent.get("temperature").is_none());
    snapshot.request_options.reasoning_effort = None;
    let sent = body(&snapshot);
    assert!(sent.get("output_config").is_none());
    assert_eq!(sent["temperature"], json!(0.2));
}
