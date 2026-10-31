use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, UtcTimestamp,
};
use aura_domain_common::{ToolId, ToolRegistrySnapshotId};
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, CompiledInstructionEnvelope, EndpointAuthScheme,
    LogicalConversationItem, LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest,
    ModelApiFormat, ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest,
    ModelStreamEvent, NormalizedStopReason, ResolvedModelRequestOptions, TransportOperationContext,
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
        ToolId::from_bytes([2; 16]),
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
            content_hash: CanonicalHashV1::sha256(b"chat-test-registry"),
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
fn compiles_configured_chat_role_and_output_token_field() {
    let snapshot = snapshot();
    let request = logical_request(&snapshot);
    let operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let compatibility_removals = std::collections::BTreeSet::new();
    let plan = OpenAiChatCompletionsAdapter
        .compile_request(ModelRequestCompilationInput {
            request: &request,
            snapshot: &snapshot,
            resolved_attachments: &[],
            operation_context: &operation_context,
            compatibility_removals: &compatibility_removals,
            applied_output_token_field_alias: None,
        })
        .expect("compile Chat request");
    let body: Value = serde_json::from_slice(&plan.body).expect("request JSON");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(
        body["messages"][0]["content"],
        aura_model_transport::instruction_layout(&request).stable
    );
    assert!(
        !body["messages"][0]["content"]
            .as_str()
            .expect("stable instructions")
            .contains("<TaskState>")
    );
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["max_tokens"], 512);
    assert!(body.get("max_completion_tokens").is_none());
    assert_eq!(body["stream_options"]["include_usage"], true);
    let parameters = &body["tools"][0]["function"]["parameters"];
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
    assert!(
        body["tools"][0]["function"].get("strict").is_none(),
        "strict outside the strict subset"
    );
    assert_eq!(
        plan.request_evidence.tool_registry_snapshot_ref.as_ref(),
        Some(&request.tool_schema_set.registry_snapshot_ref)
    );
    assert_eq!(plan.request_evidence.tool_name_bindings.len(), 1);
}

#[test]
fn unknown_chat_chunk_fields_are_ignored_without_panicking() {
    let adapter = OpenAiChatCompletionsAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    let events = decoder
        .push_frame(frame(json!({
            "id": "chatcmpl-future",
            "choices": [],
            "future_top_level_field": {"value": true}
        })))
        .expect("future Chat fields are forward-compatible");
    assert!(matches!(
        events.as_slice(),
        [
            ModelStreamEvent::RequestAccepted {
                provider_request_id: Some(id)
            },
            ModelStreamEvent::ResponseStarted
        ] if id == "chatcmpl-future"
    ));
}

#[test]
fn assembles_parallel_tool_fragments_exactly_once() {
    let adapter = OpenAiChatCompletionsAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());

    let first = decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_fixture",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "index": 0,
                    "id": "Call_ID-ExactCase",
                    "extra_content":{"google":{"thought_signature":"signature-ExactCase"}},
                    "type": "function",
                    "function": {
                        "name": "weather.lookup",
                        "arguments": "{\"city\":"
                    }
                }]},
                "finish_reason": null
            }]
        })))
        .expect("first tool delta");
    assert!(first.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ToolCallStarted { provider_call_id, .. }
            if provider_call_id.as_str() == "Call_ID-ExactCase"
    )));

    let second = decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_fixture",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "index": 0,
                    "function": { "arguments": "\"Paris\"}" }
                }]},
                "finish_reason": null
            }]
        })))
        .expect("second tool delta");
    assert!(second.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ToolCallArgumentsDelta { delta, .. }
            if delta == "\"Paris\"}"
    )));

    decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_fixture",
            "usage": {
                "prompt_tokens": 10,
                "prompt_cache_hit_tokens": 8,
                "completion_tokens": 4,
                "total_tokens": 14
            },
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        })))
        .expect("finish reason");
    decoder
        .push_frame(ProviderStreamFrame {
            data: "[DONE]".to_owned(),
            ..ProviderStreamFrame::default()
        })
        .expect("done marker");

    let output = decoder.finish().expect("normalized output");
    assert_eq!(output.usage.cached_input_tokens, Some(8));
    let continuation = decoder
        .take_continuation_artifact()
        .expect("valid protocol fixture")
        .expect("Google signature");
    let value: Value = serde_json::from_slice(&continuation.bytes).expect("valid protocol fixture");
    assert_eq!(value["thought_signatures"]["Call_ID-ExactCase"], "signature-ExactCase");
    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.tool_calls[0].provider_call_id.as_str(), "Call_ID-ExactCase");
    assert_eq!(output.tool_calls[0].arguments_json, "{\"city\":\"Paris\"}");
    assert_eq!(output.usage.total_tokens, 14);
}

#[test]
fn rejects_duplicate_provider_tool_ids_within_one_response() {
    let adapter = OpenAiChatCompletionsAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_fixture",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "index": 0,
                    "id": "duplicate-id",
                    "function": { "name": "first", "arguments": "{}" }
                }]},
                "finish_reason": null
            }]
        })))
        .expect("first tool call");
    let error = decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_fixture",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "index": 1,
                    "id": "duplicate-id",
                    "function": { "name": "second", "arguments": "{}" }
                }]},
                "finish_reason": null
            }]
        })))
        .expect_err("duplicate provider ID must fail the response");
    assert_eq!(error.code, "provider_tool_call_duplicate");
}

#[test]
fn rejects_blank_or_unresolved_provider_tool_ids() {
    let adapter = OpenAiChatCompletionsAdapter;
    let mut blank_decoder = adapter.create_stream_decoder(&snapshot());
    let blank = blank_decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_blank",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "index": 0,
                    "id": " \t ",
                    "function": { "name": "tool", "arguments": "{}" }
                }]},
                "finish_reason": null
            }]
        })))
        .expect_err("blank provider ID must fail the response");
    assert_eq!(blank.code, "provider_tool_call_id_invalid");

    let mut missing_decoder = adapter.create_stream_decoder(&snapshot());
    missing_decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_missing",
            "choices": [{
                "index": 0,
                "delta": { "tool_calls": [{
                    "index": 0,
                    "function": { "name": "tool", "arguments": "{}" }
                }]},
                "finish_reason": null
            }]
        })))
        .expect("protocol index may provisionally assemble a streaming call");
    let missing = missing_decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_missing",
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        })))
        .expect_err("response completion requires a provider tool ID");
    assert_eq!(missing.code, "provider_tool_call_id_missing");
}

#[test]
fn normalizes_structured_chat_refusal_without_conflating_content_filter() {
    let adapter = OpenAiChatCompletionsAdapter;
    let mut decoder = adapter.create_stream_decoder(&snapshot());
    decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_refusal",
            "choices": [{
                "index": 0,
                "delta": { "role": "assistant", "refusal": "I cannot help with that." },
                "finish_reason": null
            }]
        })))
        .expect("refusal delta");
    decoder
        .push_frame(frame(json!({
            "id": "chatcmpl_refusal",
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 4,
                "total_tokens": 9
            },
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        })))
        .expect("refusal finish reason");
    decoder
        .push_frame(ProviderStreamFrame {
            data: "[DONE]".to_owned(),
            ..ProviderStreamFrame::default()
        })
        .expect("done marker");

    let output = decoder.finish().expect("normalized refusal");
    assert_eq!(output.stop_reason, NormalizedStopReason::Refusal);
    assert_eq!(output.refusal.expect("refusal").message, "I cannot help with that.");
}

fn wire_body(request: &LogicalModelRequest, snapshot: &ModelExecutionSnapshot) -> Value {
    let operation = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let plan = OpenAiChatCompletionsAdapter
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
        source_event_id: EventId::new(),
        content: Some("visible answer".into()),
        content_hash: Some(ContentHash::sha256(b"visible answer")),
        tool_calls: vec![],
        provider_history: Some(LogicalProviderHistory {
            reuse_domain: ProviderToolCallReuseDomain {
                endpoint_id: snapshot.endpoint_id,
                endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
                api_format: snapshot.api_format,
            },
            model_name: snapshot.model_name.clone(),
            data: ProviderHistoryData::Chat {
                reasoning_content: Some("retained reasoning".into()),
                thought_signatures: Default::default(),
            },
        }),
    });
    let body = wire_body(&request, &snapshot);
    let stable = body["messages"][0].clone();
    assert_eq!(
        body["messages"]
            .as_array()
            .expect("compiled protocol array")
            .last()
            .expect("current state at request tail")["content"],
        "<TaskState>TaskState</TaskState>"
    );
    assert_eq!(body["messages"][2]["reasoning_content"], "retained reasoning");
    let mut blocks = request.instruction_envelope.ordered_blocks.clone();
    let state = blocks.last_mut().expect("task state");
    state.content = "<TaskState>new step and revision</TaskState>".into();
    state.content_hash = ContentHash::sha256(state.content.as_bytes());
    let canonical = blocks.iter().map(|b| b.content.as_str()).collect::<Vec<_>>().join("\n");
    request.instruction_envelope =
        CompiledInstructionEnvelope::new(1, blocks, canonical).expect("valid protocol fixture");
    let body = wire_body(&request, &snapshot);
    assert_eq!(body["messages"][0], stable);
    assert_eq!(
        body["messages"]
            .as_array()
            .expect("compiled protocol array")
            .last()
            .expect("current state at request tail")["content"],
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
        assert!(body["messages"][2].get("reasoning_content").is_none());
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
        body["tools"][0]["function"]["name"]
            .as_str()
            .expect("compiled tool name or message text")
            .starts_with("fixture_fixed")
    );
    assert!(body["tools"][0]["function"].get("strict").is_none());
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
fn an_addition_stays_a_separate_user_message_after_the_tool_result() {
    use aura_domain_common::ToolCallId;
    use aura_model_domain::{
        LogicalHistoricalToolCall, LogicalProviderHistory, ProviderHistoryData,
        ProviderToolCallReuseDomain, ProviderToolCallWireBinding, ProviderToolCallWireIdentity,
        ProviderWireToolCallId,
    };
    use std::collections::BTreeMap;
    let mut snapshot = snapshot();
    let tool_call_id = ToolCallId::new();
    let tool_id = ToolId::new();
    let wire = ProviderWireToolCallId::from_tool_call_id(tool_call_id);
    let mut signatures = BTreeMap::new();
    signatures.insert(wire.as_str().to_owned(), "thought_exact".to_owned());
    snapshot.historical_tool_call_wire_bindings = vec![ProviderToolCallWireBinding {
        tool_call_id,
        tool_id: Some(tool_id),
        provider_wire_tool_name: "fixture_read".to_owned(),
        wire_identity: ProviderToolCallWireIdentity::AuraGenerated {
            provider_wire_tool_call_id: wire,
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
            data: ProviderHistoryData::Chat {
                reasoning_content: Some("kept".to_owned()),
                thought_signatures: signatures,
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
    let messages = body["messages"].as_array().expect("messages");
    let roles = messages
        .iter()
        .map(|message| message["role"].as_str().expect("message role"))
        .collect::<Vec<_>>();
    assert_eq!(&roles[roles.len() - 4..roles.len() - 1], ["assistant", "tool", "user"]);
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .expect("assistant tool call");
    assert_eq!(assistant["reasoning_content"], "kept");
    assert_eq!(
        assistant["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
        "thought_exact"
    );
    let addition =
        messages.iter().rev().find(|message| message["role"] == "user").expect("user addition");
    assert_eq!(addition["content"], "Record the path.");
}

/// A profile's thinking level is `reasoning_effort`; none named sends none.
#[test]
fn a_thinking_level_is_sent_as_reasoning_effort() {
    let mut snapshot = snapshot();
    let request = logical_request(&snapshot);
    assert!(wire_body(&request, &snapshot).get("reasoning_effort").is_none());
    snapshot.request_options.reasoning_effort = Some(aura_model_domain::ReasoningEffort::Low);
    assert_eq!(wire_body(&request, &snapshot)["reasoning_effort"], json!("low"));
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
    snapshot.resolved_request_url = "https://api.openai.com/v1/chat/completions".to_owned();
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
