use std::collections::BTreeSet;

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, UtcTimestamp,
};
use aura_domain_common::{ToolId, ToolRegistrySnapshotId};
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
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
        provider_display_name: "OpenAI".to_owned(),
        endpoint_display_name: "Responses".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "OpenAI / Responses / fixture-model".to_owned(),
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
        endpoint_fingerprint: ContentHash::blake3(b"responses-endpoint"),
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
        ToolId::from_bytes([1; 16]),
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
            content_hash: CanonicalHashV1::sha256(b"responses-test-registry"),
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
            format: ModelOutputFormat::JsonObject,
            effective_max_output_tokens: 512,
        },
    }
}

#[test]
fn compiles_frozen_instruction_history_and_output_contract() {
    let snapshot = snapshot();
    let request = logical_request(&snapshot);
    let operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let compatibility_removals = BTreeSet::new();
    let plan = OpenAiResponsesAdapter
        .compile_request(ModelRequestCompilationInput {
            request: &request,
            snapshot: &snapshot,
            resolved_attachments: &[],
            operation_context: &operation_context,
            compatibility_removals: &compatibility_removals,
            applied_output_token_field_alias: None,
        })
        .expect("compile Responses request");
    let body: Value = serde_json::from_slice(&plan.body).expect("request JSON");
    assert_eq!(body["instructions"], aura_model_transport::instruction_layout(&request).stable);
    assert!(!body["instructions"].as_str().expect("stable instructions").contains("<TaskState>"));
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["max_output_tokens"], 512);
    assert_eq!(body["text"]["format"]["type"], "json_object");
    assert_eq!(body["stream"], true);
    let parameters = &body["tools"][0]["parameters"];
    // Envelope 3: the tool's own root object with the required purpose beside its fields.
    assert_eq!(parameters["required"], json!(["aura_purpose", "value"]));
    assert_eq!(parameters["additionalProperties"], false);
    assert_eq!(parameters["properties"]["value"], json!({"type": "string"}));
    assert_eq!(parameters["properties"]["aura_purpose"]["type"], "string");
    assert!(
        parameters["properties"].get("_aura").is_none()
            && parameters["properties"].get("arguments").is_none(),
        "no `_aura`/`arguments` wrapper: {parameters}"
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
        plan.request_evidence.tool_name_bindings[0].tool_definition.registry_snapshot_ref,
        request.tool_schema_set.registry_snapshot_ref
    );
    assert_eq!(plan.request_evidence.body_sha256, ContentHash::sha256(&plan.body));
    assert!(plan.safe_optional_parameters.is_empty());
}

#[test]
fn unknown_provider_events_emit_a_warning_without_panicking() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    let events = decoder
        .push_frame(frame(json!({
            "type": "response.future_event",
            "future_payload": {"value": true}
        })))
        .expect("unknown event is forward-compatible");
    assert!(matches!(
        events.as_slice(),
        [ModelStreamEvent::ProviderWarning { code: Some(code), .. }]
            if code == "openai_responses_event_unknown"
    ));
}

#[test]
fn resolves_provider_tool_id_at_item_done_without_rewriting_it() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());

    decoder
        .push_frame(frame(json!({
            "type": "response.created",
            "response": { "id": "resp_fixture" }
        })))
        .expect("response.created");
    let added = decoder
        .push_frame(frame(json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": { "type": "function_call", "id": "item_fixture" }
        })))
        .expect("output item added");
    assert!(added.is_empty());

    for delta in ["{\"city\":\"", "Paris\"}"] {
        let events = decoder
            .push_frame(frame(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "item_fixture",
                "delta": delta
            })))
            .expect("argument delta");
        assert!(events.is_empty());
    }

    let done = decoder
        .push_frame(frame(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "item_fixture",
                "call_id": "Call_ID-ExactCase",
                "name": "weather.lookup",
                "arguments": "{\"city\":\"Paris\"}"
            }
        })))
        .expect("output item done");
    assert!(matches!(
        &done[0],
        ModelStreamEvent::ToolCallStarted { provider_call_id, tool_name, position }
            if provider_call_id.as_str() == "Call_ID-ExactCase"
                && tool_name == "weather.lookup"
                && *position == 0
    ));
    assert!(matches!(
        &done[1],
        ModelStreamEvent::ToolCallArgumentsDelta { provider_call_id, delta }
            if provider_call_id.as_str() == "Call_ID-ExactCase"
                && delta == "{\"city\":\"Paris\"}"
    ));
    assert!(matches!(
        &done[2],
        ModelStreamEvent::ToolCallCompleted { provider_call_id }
            if provider_call_id.as_str() == "Call_ID-ExactCase"
    ));

    decoder
        .push_frame(frame(json!({
            "type": "response.completed",
            "response": {
                "id": "resp_fixture",
                "usage": { "input_tokens": 12, "output_tokens": 5, "total_tokens": 17 }
            }
        })))
        .expect("response completed");
    decoder
        .push_frame(ProviderStreamFrame {
            data: "[DONE]".to_owned(),
            ..ProviderStreamFrame::default()
        })
        .expect("done marker");

    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.tool_calls[0].provider_call_id.as_str(), "Call_ID-ExactCase");
    assert_eq!(output.tool_calls[0].arguments_json, "{\"city\":\"Paris\"}");
    assert_eq!(output.usage.total_tokens, 17);
}

#[test]
fn rejects_missing_provider_tool_id_at_response_completion() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": { "type": "function_call", "id": "item_fixture" }
        })))
        .expect("output item added");
    let error = decoder
        .push_frame(frame(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "item_fixture",
                "name": "weather.lookup",
                "arguments": "{}"
            }
        })))
        .expect_err("missing provider call ID must fail");
    assert_eq!(error.code, "openai_responses_field_missing");
}

#[test]
fn rejects_blank_and_duplicate_provider_tool_ids() {
    let adapter = OpenAiResponsesAdapter;
    let mut blank_decoder = adapter.create_stream_decoder(&snapshot());
    let blank = blank_decoder
        .push_frame(frame(json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "item_blank",
                "call_id": "  ",
                "name": "tool"
            }
        })))
        .expect_err("blank provider ID must fail the response");
    assert_eq!(blank.code, "provider_tool_call_id_invalid");

    let mut duplicate_decoder = adapter.create_stream_decoder(&snapshot());
    duplicate_decoder
        .push_frame(frame(json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "item_first",
                "call_id": "duplicate-id",
                "name": "first"
            }
        })))
        .expect("first provider tool ID");
    let duplicate = duplicate_decoder
        .push_frame(frame(json!({
            "type": "response.output_item.added",
            "output_index": 1,
            "item": {
                "type": "function_call",
                "id": "item_second",
                "call_id": "duplicate-id",
                "name": "second"
            }
        })))
        .expect_err("duplicate provider ID must fail the response");
    assert_eq!(duplicate.code, "provider_tool_call_duplicate");
}

#[test]
fn preserves_completed_reasoning_item_as_continuation_artifact() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "type": "response.created",
            "response": { "id": "resp_reasoning" }
        })))
        .expect("response.created");
    decoder
        .push_frame(frame(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "id": "rs_exact",
                "type": "reasoning",
                "encrypted_content": "opaque-ExactCase",
                "summary": []
            }
        })))
        .expect("reasoning item done");
    decoder
        .push_frame(frame(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_fixture",
            "output_index": 1,
            "content_index": 0,
            "delta": "done"
        })))
        .expect("text delta");
    decoder
        .push_frame(frame(json!({
            "type": "response.completed",
            "response": {
                "id": "resp_reasoning",
                "usage": { "input_tokens": 8, "output_tokens": 4, "total_tokens": 12 }
            }
        })))
        .expect("response completed");

    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.assistant_blocks[0].text, "done");
    let artifact = decoder
        .take_continuation_artifact()
        .expect("continuation extraction")
        .expect("reasoning continuation");
    let value: Value = serde_json::from_slice(&artifact.bytes).expect("continuation JSON");
    assert_eq!(value["format"], "openai_responses_reasoning_items_v1");
    assert_eq!(value["items"][0]["id"], "rs_exact");
    assert_eq!(value["items"][0]["encrypted_content"], "opaque-ExactCase");
}

#[test]
fn keeps_reasoning_text_distinct_from_reasoning_summary() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "type": "response.created",
            "response": { "id": "resp_reasoning_text" }
        })))
        .expect("response.created");

    for delta in ["hidden ", "reasoning"] {
        decoder
            .push_frame(frame(json!({
                "type": "response.reasoning_text.delta",
                "item_id": "rs_text",
                "output_index": 0,
                "content_index": 0,
                "delta": delta
            })))
            .expect("reasoning text delta");
    }
    decoder
        .push_frame(frame(json!({
            "type": "response.reasoning_text.done",
            "item_id": "rs_text",
            "output_index": 0,
            "content_index": 0,
            "text": "hidden reasoning"
        })))
        .expect("reasoning text done");
    decoder
        .push_frame(frame(json!({
            "type": "response.reasoning_summary_text.delta",
            "item_id": "rs_summary",
            "output_index": 0,
            "summary_index": 0,
            "delta": "short summary"
        })))
        .expect("reasoning summary delta");
    decoder
        .push_frame(frame(json!({
            "type": "response.completed",
            "response": {
                "id": "resp_reasoning_text",
                "usage": { "input_tokens": 9, "output_tokens": 5, "total_tokens": 14 }
            }
        })))
        .expect("response completed");

    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.reasoning_blocks.len(), 2);
    assert_eq!(output.reasoning_blocks[0].text, "hidden reasoning");
    assert_eq!(output.reasoning_blocks[0].summary, None);
    assert_eq!(output.reasoning_blocks[1].text, "");
    assert_eq!(output.reasoning_blocks[1].summary.as_deref(), Some("short summary"));
}

#[test]
fn rejects_reasoning_done_that_does_not_match_streamed_text() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "type": "response.reasoning_text.delta",
            "item_id": "rs_mismatch",
            "output_index": 0,
            "content_index": 0,
            "delta": "exact"
        })))
        .expect("reasoning text delta");
    let error = decoder
        .push_frame(frame(json!({
            "type": "response.reasoning_text.done",
            "item_id": "rs_mismatch",
            "output_index": 0,
            "content_index": 0,
            "text": "changed"
        })))
        .expect_err("mismatched reasoning text must fail closed");
    assert_eq!(error.code, "openai_responses_reasoning_mismatch");
}

#[test]
fn normalizes_refusal_and_validates_final_text() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "type": "response.created",
            "response": { "id": "resp_refusal" }
        })))
        .expect("response.created");
    for delta in ["I cannot ", "help with that."] {
        decoder
            .push_frame(frame(json!({
                "type": "response.refusal.delta",
                "item_id": "msg_refusal",
                "output_index": 0,
                "content_index": 0,
                "delta": delta
            })))
            .expect("refusal delta");
    }
    decoder
        .push_frame(frame(json!({
            "type": "response.refusal.done",
            "item_id": "msg_refusal",
            "output_index": 0,
            "content_index": 0,
            "refusal": "I cannot help with that."
        })))
        .expect("refusal done");
    decoder
        .push_frame(frame(json!({
            "type": "response.completed",
            "response": {
                "id": "resp_refusal",
                "usage": { "input_tokens": 6, "output_tokens": 5, "total_tokens": 11 }
            }
        })))
        .expect("response completed");

    let output = decoder.finish().expect("normalized refusal");
    assert_eq!(output.stop_reason, NormalizedStopReason::Refusal);
    assert_eq!(output.refusal.expect("refusal").message, "I cannot help with that.");
}

#[test]
fn rejects_refusal_done_that_does_not_match_streamed_text() {
    let adapter = OpenAiResponsesAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "type": "response.refusal.delta",
            "item_id": "msg_refusal_mismatch",
            "output_index": 0,
            "content_index": 0,
            "delta": "exact"
        })))
        .expect("refusal delta");
    let error = decoder
        .push_frame(frame(json!({
            "type": "response.refusal.done",
            "item_id": "msg_refusal_mismatch",
            "output_index": 0,
            "content_index": 0,
            "refusal": "changed"
        })))
        .expect_err("mismatched refusal text must fail closed");
    assert_eq!(error.code, "openai_responses_refusal_mismatch");
}

fn wire_body(request: &LogicalModelRequest, snapshot: &ModelExecutionSnapshot) -> Value {
    let operation = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let plan = OpenAiResponsesAdapter
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
        },model_name:snapshot.model_name.clone(),data:ProviderHistoryData::Responses { reasoning_items: vec![json!({"type":"reasoning","id":"rs_retained","summary":[],"encrypted_content":"opaque_ExactCase"})] } }),
    });
    let body = wire_body(&request, &snapshot);
    let stable = body["instructions"].clone();
    assert_eq!(
        body["input"]
            .as_array()
            .expect("compiled protocol array")
            .last()
            .expect("current state at request tail")["content"][0]["text"],
        "<TaskState>TaskState</TaskState>"
    );
    assert_eq!(body["input"][1]["encrypted_content"], "opaque_ExactCase");
    let mut blocks = request.instruction_envelope.ordered_blocks.clone();
    let state = blocks.last_mut().expect("task state");
    state.content = "<TaskState>new step and revision</TaskState>".into();
    state.content_hash = ContentHash::sha256(state.content.as_bytes());
    let canonical = blocks.iter().map(|b| b.content.as_str()).collect::<Vec<_>>().join("\n");
    request.instruction_envelope =
        CompiledInstructionEnvelope::new(1, blocks, canonical).expect("valid protocol fixture");
    let body = wire_body(&request, &snapshot);
    assert_eq!(body["instructions"], stable);
    assert_eq!(
        body["input"]
            .as_array()
            .expect("compiled protocol array")
            .last()
            .expect("current state at request tail")["content"][0]["text"],
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
        assert_eq!(body["input"][1]["role"], "assistant");
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
fn an_addition_stays_a_separate_user_message_after_function_output() {
    use aura_domain_common::ToolCallId;
    use aura_model_domain::{
        LogicalHistoricalToolCall, LogicalProviderHistory, ProviderHistoryData,
        ProviderToolCallReuseDomain, ProviderToolCallWireBinding, ProviderToolCallWireIdentity,
        ProviderWireToolCallId,
    };
    let mut snapshot = snapshot();
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
            data: ProviderHistoryData::Responses {
                reasoning_items: vec![json!({"type":"reasoning","encrypted_content":"kept"})],
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
    request.conversation_items.push(LogicalConversationItem::User {
        source_event_id: EventId::new(),
        content: "Record the path.".to_owned(),
        content_hash: ContentHash::sha256(b"Record the path."),
    });
    let body = wire_body(&request, &snapshot);
    let input = body["input"].as_array().expect("input");
    let output_at =
        input.iter().position(|item| item["type"] == "function_call_output").expect("output");
    let addition_at = input
        .iter()
        .position(|item| item["role"] == "user" && item["content"][0]["text"] == "Record the path.")
        .expect("addition");
    assert!(addition_at > output_at);
    assert_ne!(input[addition_at]["type"], "function_call_output");
    let reasoning_at =
        input.iter().position(|item| item["encrypted_content"] == "kept").expect("reasoning");
    let assistant_at =
        input.iter().position(|item| item["role"] == "assistant").expect("assistant");
    assert!(reasoning_at < assistant_at);
    assert_eq!(input.last().expect("current state")["role"], "developer");
    assert_ne!(body["instructions"], input.last().expect("current state")["content"][0]["text"]);
}

/// A profile's thinking level is `reasoning.effort`; none named sends none.
#[test]
fn a_thinking_level_is_sent_as_reasoning_effort() {
    let mut snapshot = snapshot();
    let request = logical_request(&snapshot);
    assert!(wire_body(&request, &snapshot).get("reasoning").is_none());
    snapshot.request_options.reasoning_effort = Some(aura_model_domain::ReasoningEffort::High);
    assert_eq!(wire_body(&request, &snapshot)["reasoning"], json!({"effort": "high"}));
}

/// The request with one instruction block's text replaced, the envelope compiled again.
fn with_block(
    request: &LogicalModelRequest,
    kind: LogicalInstructionKind,
    text: &str,
) -> LogicalModelRequest {
    let blocks = request
        .instruction_envelope
        .ordered_blocks
        .iter()
        .cloned()
        .map(|mut block| {
            if block.kind == kind {
                block.content = text.to_owned();
                block.content_hash = ContentHash::sha256(text.as_bytes());
            }
            block
        })
        .collect::<Vec<_>>();
    let canonical =
        blocks.iter().map(|block| block.content.as_str()).collect::<Vec<_>>().join("\n");
    let mut changed = request.clone();
    changed.instruction_envelope =
        CompiledInstructionEnvelope::new(1, blocks, canonical).expect("instruction envelope");
    changed
}

/// OpenAI's own API is sent a cache key
/// naming the stable instructions. A later step whose task state changed keeps it; changed stable
/// instructions change it; any other endpoint is not sent it.
#[test]
fn openai_itself_is_sent_a_cache_key_for_the_stable_instructions() {
    let mut snapshot = snapshot();
    snapshot.base_url = "https://api.openai.com/v1".to_owned();
    snapshot.resolved_request_url = "https://api.openai.com/v1/responses".to_owned();
    let request = logical_request(&snapshot);
    let key = wire_body(&request, &snapshot)["prompt_cache_key"]
        .as_str()
        .expect("a cache key")
        .to_owned();
    assert!(key.starts_with("aura-") && key.len() == 37, "{key}");
    let later =
        with_block(&request, LogicalInstructionKind::TaskState, "<TaskState>later</TaskState>");
    assert_eq!(wire_body(&later, &snapshot)["prompt_cache_key"], json!(key));
    let summarized =
        with_block(&request, LogicalInstructionKind::HistoricalSnapshot, "<Summary>new</Summary>");
    assert_ne!(wire_body(&summarized, &snapshot)["prompt_cache_key"], json!(key));
    let other = self::snapshot();
    assert!(wire_body(&logical_request(&other), &other).get("prompt_cache_key").is_none());
}
