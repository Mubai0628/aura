//! Synthetic raw OpenAI Responses streams for the tool-call binding boundary.
//!
//! Every case here starts from bytes a Provider could send — real SSE framing,
//! `event:` and `data:` lines, chunk boundaries in the middle of a frame — and
//! runs the production chain: `SseDecoder` -> adapter stream decoder ->
//! `derive_provider_tool_call_protocol_bindings` against the evidence of the
//! request the adapter itself compiled.
//!
//! The point of these cases is the boundary the incident crossed: the parameter
//! envelope's metadata key (`_aura` in envelopes 1-2, `aura_purpose` in envelope 3) is
//! an argument key and must never become a function name, while a Provider that really
//! names its function call `_aura` must be rejected because `_aura` is not in the frozen
//! tool list.

use std::collections::BTreeSet;

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, ToolId,
    ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    CompiledInstructionEnvelope, EndpointAuthScheme, LogicalConversationItem,
    LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest, ModelApiFormat,
    ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest, NormalizedModelOutput,
    ProviderToolCallBindingError, ResolvedModelRequestOptions, TransportOperationContext,
    derive_p07_normalized_tool_calls_v1, derive_provider_tool_call_protocol_bindings,
    provider_wire_tool_name,
};
use aura_model_transport::{
    ModelAdapterError, ModelProtocolCompiler, ModelProtocolDecoder, ModelRequestCompilationInput,
    ProviderRequestPlan, ProviderStreamFrame, SseDecoder, SseLimits,
};
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, ToolInputSchemaV1, ToolRegistrySnapshotRef,
    ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1, ToolVersion,
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

/// One frozen tool, exactly as the registry publishes it.
fn tool(id_byte: u8, canonical_name: &str) -> ToolSchemaSnapshotV1 {
    ToolSchemaSnapshotV1::new(
        ToolId::from_bytes([id_byte; 16]),
        ToolVersion::new(None, aura_domain_common::Revision::INITIAL).expect("tool version"),
        canonical_name.parse().expect("canonical tool name"),
        format!("Fixture tool {canonical_name}"),
        ToolInputSchemaV1::new(json!({
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"],
            "additionalProperties": false
        }))
        .expect("tool input schema"),
        true,
    )
    .expect("tool schema snapshot")
}

fn schema_set(tools: Vec<ToolSchemaSnapshotV1>) -> ToolSchemaSetSnapshotV1 {
    ToolSchemaSetSnapshotV1::new(
        ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: aura_domain_common::Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"responses-binding-registry"),
        },
        tools,
    )
    .expect("schema set")
}

fn logical_request(
    snapshot: &ModelExecutionSnapshot,
    tools: Vec<ToolSchemaSnapshotV1>,
) -> LogicalModelRequest {
    LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope: instruction_envelope(),
        conversation_items: vec![LogicalConversationItem::User {
            source_event_id: EventId::new(),
            content: "Write the fixture value.".to_owned(),
            content_hash: ContentHash::sha256(b"Write the fixture value."),
        }],
        tool_schema_set: schema_set(tools),
        attachments: Vec::new(),
        provider_continuation: None,
        output_request: ModelOutputRequest {
            format: ModelOutputFormat::JsonObject,
            effective_max_output_tokens: 512,
        },
    }
}

/// The request plan the adapter really compiles, so the binding check reads the
/// frozen list of the request the Provider answered.
fn compiled_plan(
    snapshot: &ModelExecutionSnapshot,
    tools: Vec<ToolSchemaSnapshotV1>,
) -> (LogicalModelRequest, ProviderRequestPlan) {
    let request = logical_request(snapshot, tools);
    let operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let mut plan = OpenAiResponsesAdapter
        .compile_request(ModelRequestCompilationInput {
            request: &request,
            snapshot,
            resolved_attachments: &[],
            operation_context: &operation_context,
            compatibility_removals: &BTreeSet::new(),
            applied_output_token_field_alias: None,
        })
        .expect("compile Responses request");
    plan.request_evidence.adapter =
        aura_model_domain::ModelAdapterIdentity::v1(snapshot.api_format);
    (request, plan)
}

/// Frames one Responses event the way the wire carries it.
fn event_frame(value: &Value) -> String {
    let kind = value["type"].as_str().expect("a Responses event has a type");
    format!("event: {kind}\ndata: {value}\n\n")
}

/// Streams raw SSE bytes in small chunks, so a frame is split across chunk
/// boundaries exactly as a real socket delivers it.
fn decode_raw(
    snapshot: &ModelExecutionSnapshot,
    raw: &str,
) -> Result<NormalizedModelOutput, ModelAdapterError> {
    let mut sse = SseDecoder::new(SseLimits::default());
    let mut decoder = OpenAiResponsesAdapter.create_stream_decoder(snapshot);
    let bytes = raw.as_bytes();
    let mut frames: Vec<ProviderStreamFrame> = Vec::new();
    for chunk in bytes.chunks(7) {
        frames.extend(sse.push(chunk).expect("valid SSE bytes"));
    }
    frames.extend(sse.finish().expect("SSE terminator"));
    for frame in frames {
        // Decoded events are not the subject of these cases; the normalized
        // output and the binding decision are.
        decoder.push_frame(frame)?;
    }
    decoder.finish()
}

fn completed_event() -> Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": "resp_binding",
            "usage": {"input_tokens": 20, "output_tokens": 8, "total_tokens": 28}
        }
    })
}

/// The envelope a model really sends (envelope 3): `aura_purpose` is a top-level key
/// inside the function arguments beside the tool's own fields, never a function name.
fn envelope_arguments() -> String {
    json!({
        "aura_purpose": "Write the fixture value",
        "value": "written-once"
    })
    .to_string()
}

/// The streamed shape: `output_item.added` opens the item, argument deltas
/// follow, `output_item.done` completes it.
fn streamed_item_stream(wire_name: &str, arguments: &str) -> String {
    let (first, second) = arguments.split_at(arguments.len() / 2);
    [
        event_frame(&json!({
            "type": "response.created",
            "response": {"id": "resp_binding"}
        })),
        event_frame(&json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {"type": "function_call", "id": "item_binding", "call_id": "call_binding", "name": wire_name}
        })),
        event_frame(&json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "item_binding",
            "output_index": 0,
            "delta": first
        })),
        event_frame(&json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "item_binding",
            "output_index": 0,
            "delta": second
        })),
        event_frame(&json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "item_binding",
                "call_id": "call_binding",
                "name": wire_name,
                "arguments": arguments
            }
        })),
        event_frame(&completed_event()),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat()
}

/// The completed-item shape: the Provider announces the item without its
/// identity, then resolves call ID, name and arguments in `output_item.done`.
fn resolved_at_completion_stream(wire_name: &str, arguments: &str) -> String {
    [
        event_frame(&json!({
            "type": "response.created",
            "response": {"id": "resp_binding"}
        })),
        event_frame(&json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": {"type": "function_call", "id": "item_binding"}
        })),
        event_frame(&json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "item_binding",
            "output_index": 0,
            "delta": arguments
        })),
        event_frame(&json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "item_binding",
                "call_id": "call_binding",
                "name": wire_name,
                "arguments": arguments
            }
        })),
        event_frame(&completed_event()),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat()
}

fn wire_name(canonical_name: &str, tool_id_byte: u8) -> String {
    provider_wire_tool_name(canonical_name, ToolId::from_bytes([tool_id_byte; 16]))
}

/// A legal function name with the purpose inside its arguments binds to the frozen
/// tool, and the envelope key never becomes a tool name.
#[test]
fn a_legal_function_name_binds_while_the_aura_key_stays_an_argument() {
    let snapshot = snapshot();
    let write = tool(3, "fixture.write");
    let write_wire = wire_name("fixture.write", 3);
    let (_, plan) = compiled_plan(&snapshot, vec![write.clone()]);

    let output = decode_raw(&snapshot, &streamed_item_stream(&write_wire, &envelope_arguments()))
        .expect("a well-formed stream normalizes");

    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.tool_calls[0].tool_name, write_wire);
    assert_ne!(
        output.tool_calls[0].tool_name, "_aura",
        "the envelope metadata key must never be read as a function name"
    );
    assert_eq!(output.tool_calls[0].arguments_json, envelope_arguments());

    let bindings =
        derive_provider_tool_call_protocol_bindings(&snapshot, &plan.request_evidence, &output)
            .expect("a frozen tool binds");
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].tool_definition.as_ref().expect("known binding").tool_id, write.tool_id);
    assert_eq!(bindings[0].provider_wire_tool_name, write_wire);

    // The same call normalizes into the P05 -> P07 input with the envelope
    // removed: the tool input is the tool's fields, without the purpose beside them.
    let calls = derive_p07_normalized_tool_calls_v1(&snapshot, &plan.request_evidence, &output)
        .expect("normalized tool calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].definition_ref.as_ref().expect("resolved call").tool_id, write.tool_id);
    match &calls[0].input {
        NormalizedToolCallInputStateV1::Ready { purpose, arguments, .. } => {
            assert_eq!(arguments, &json!({"value": "written-once"}));
            assert_eq!(purpose.as_str(), "Write the fixture value");
        }
        other => panic!("the envelope must normalize into a ready input: {other:?}"),
    }
}

/// When the Provider resolves the call identity only at completion, the same
/// legal name still binds to the frozen tool.
#[test]
fn a_call_resolved_at_completion_binds_the_same_way() {
    let snapshot = snapshot();
    let write = tool(3, "fixture.write");
    let write_wire = wire_name("fixture.write", 3);
    let (_, plan) = compiled_plan(&snapshot, vec![write.clone()]);

    let output =
        decode_raw(&snapshot, &resolved_at_completion_stream(&write_wire, &envelope_arguments()))
            .expect("a call resolved at completion normalizes");

    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.tool_calls[0].tool_name, write_wire);
    assert_eq!(output.tool_calls[0].arguments_json, envelope_arguments());
    let bindings =
        derive_provider_tool_call_protocol_bindings(&snapshot, &plan.request_evidence, &output)
            .expect("a frozen tool binds");
    assert_eq!(bindings[0].tool_definition.as_ref().expect("known binding").tool_id, write.tool_id);
}

/// A Provider that really names its function call `_aura` is rejected in every
/// shape the adapter accepts, and the rejection names it.
#[test]
fn a_function_named_aura_is_rejected_in_both_stream_shapes() {
    let snapshot = snapshot();
    let write = tool(3, "fixture.write");
    let (_, plan) = compiled_plan(&snapshot, vec![write]);

    for raw in [
        streamed_item_stream("_aura", &envelope_arguments()),
        resolved_at_completion_stream("_aura", &envelope_arguments()),
    ] {
        let output = decode_raw(&snapshot, &raw).expect("the stream itself is well formed");
        assert_eq!(output.tool_calls.len(), 1);
        assert_eq!(output.tool_calls[0].tool_name, "_aura");
        let error =
            derive_provider_tool_call_protocol_bindings(&snapshot, &plan.request_evidence, &output)
                .expect_err("an illegal function name must be rejected");
        assert_eq!(error.diagnostic_name(), "unknown_provider_wire_tool_name");
        assert_eq!(error.offending_wire_name(), Some("_aura"));
        assert!(matches!(error, ProviderToolCallBindingError::UnknownProviderWireToolName { .. }));
        let message = error.to_string();
        assert!(message.contains("wire name _aura"), "{message}");
        assert!(message.contains(&snapshot.model_step_id.to_string()), "{message}");
        assert!(message.contains(&snapshot.id.to_string()), "{message}");
    }
}

/// The frozen list is the authority, before and after `tools.load`: the same
/// wire name is unknown in the request that did not expose it and binds in the
/// request that did.
#[test]
fn a_tool_is_unknown_before_tools_load_and_binds_after() {
    let snapshot = snapshot();
    let read = tool(2, "fixture.read");
    let write = tool(3, "fixture.write");
    let write_wire = wire_name("fixture.write", 3);
    let stream = streamed_item_stream(&write_wire, &envelope_arguments());

    let (_, before) = compiled_plan(&snapshot, vec![read.clone()]);
    let output = decode_raw(&snapshot, &stream).expect("the stream itself is well formed");
    let error =
        derive_provider_tool_call_protocol_bindings(&snapshot, &before.request_evidence, &output)
            .expect_err("a tool the frozen request did not expose cannot bind");
    assert_eq!(error.diagnostic_name(), "unknown_provider_wire_tool_name");
    assert_eq!(error.offending_wire_name(), Some(write_wire.as_str()));
    assert_eq!(before.request_evidence.tool_name_bindings.len(), 1);

    // After tools.load the same response binds to the very tool it named.
    let (_, after) = compiled_plan(&snapshot, vec![read, write.clone()]);
    assert_eq!(after.request_evidence.tool_name_bindings.len(), 2);
    let output = decode_raw(&snapshot, &stream).expect("the stream itself is well formed");
    let bindings =
        derive_provider_tool_call_protocol_bindings(&snapshot, &after.request_evidence, &output)
            .expect("the loaded tool binds");
    assert_eq!(bindings[0].tool_definition.as_ref().expect("known binding").tool_id, write.tool_id);

    // And `_aura` stays unknown against the wider list too: the check is the
    // frozen list, never a name pattern or a guess about what was meant.
    let aura = decode_raw(&snapshot, &streamed_item_stream("_aura", &envelope_arguments()))
        .expect("the stream itself is well formed");
    let error =
        derive_provider_tool_call_protocol_bindings(&snapshot, &after.request_evidence, &aura)
            .expect_err("_aura is not a tool in any frozen list");
    assert_eq!(error.offending_wire_name(), Some("_aura"));
}

/// A function call that was never announced is refused, not inferred: an item
/// Aura cannot tie to an announced identity fails the response closed, and the
/// name inside its arguments object is still never read as a function name.
#[test]
fn an_unannounced_completed_function_call_is_refused_not_inferred() {
    let snapshot = snapshot();
    let raw = [
        event_frame(&json!({
            "type": "response.created",
            "response": {"id": "resp_binding"}
        })),
        event_frame(&json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "item_never_announced",
                "call_id": "call_binding",
                "name": "_aura",
                "arguments": envelope_arguments()
            }
        })),
        event_frame(&completed_event()),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat();
    let error = decode_raw(&snapshot, &raw)
        .expect_err("an item that was never announced must not be accepted late");
    assert_eq!(error.code, "openai_responses_tool_item_unknown");
}

/// A function call that appears only inside the final `response.completed`
/// object is not a function call: the adapter reads names from item events, so
/// the completion object can neither supply a name nor hide one.
#[test]
fn the_final_response_object_cannot_supply_a_function_name() {
    let snapshot = snapshot();
    let write = tool(3, "fixture.write");
    let write_wire = wire_name("fixture.write", 3);
    let (_, plan) = compiled_plan(&snapshot, vec![write]);

    let raw = [
        event_frame(&json!({
            "type": "response.created",
            "response": {"id": "resp_binding"}
        })),
        event_frame(&json!({
            "type": "response.completed",
            "response": {
                "id": "resp_binding",
                "output": [{
                    "type": "function_call",
                    "id": "item_only_in_completion",
                    "call_id": "call_binding",
                    "name": write_wire,
                    "arguments": envelope_arguments()
                }],
                "usage": {"input_tokens": 20, "output_tokens": 8, "total_tokens": 28}
            }
        })),
        "data: [DONE]\n\n".to_owned(),
    ]
    .concat();

    let error = decode_raw(&snapshot, &raw)
        .expect_err("a stream with no item event has no model output to normalize");
    assert_eq!(error.code, "provider_output_empty");

    // Nothing was invented from the completion object: no output means no
    // binding decision, and no unrelated tool is blamed.
    let empty = NormalizedModelOutput {
        model_step_id: snapshot.model_step_id,
        assistant_blocks: Vec::new(),
        reasoning_blocks: Vec::new(),
        tool_calls: Vec::new(),
        refusal: None,
        stop_reason: aura_model_domain::NormalizedStopReason::EndTurn,
        usage: aura_model_domain::ModelUsage {
            input_tokens: 20,
            output_tokens: 8,
            total_tokens: 28,
            reasoning_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
            source: aura_model_domain::UsageSource::ProviderReported,
        },
        provider_continuation: None,
        provider_request_id: None,
        raw_response_ref: None,
    };
    derive_provider_tool_call_protocol_bindings(&snapshot, &plan.request_evidence, &empty)
        .expect("no tool call means nothing to bind");
}

/// The adapter reports the item kind it parses a function call from, which is
/// what a refusal diagnostic names.
#[test]
fn the_adapter_names_its_function_call_item_kind() {
    assert_eq!(OpenAiResponsesAdapter.api_format(), ModelApiFormat::OpenAiResponses);
    assert_eq!(OpenAiResponsesAdapter.tool_call_item_type(), "function_call");
}

#[test]
fn an_unknown_name_is_unbound_only_under_the_new_frozen_policy() {
    let snapshot = snapshot();
    let (_, mut plan) = compiled_plan(&snapshot, vec![tool(3, "fixture.write")]);
    let output = decode_raw(&snapshot, &streamed_item_stream("_aura", &envelope_arguments()))
        .expect("stream");
    assert!(
        derive_p07_normalized_tool_calls_v1(&snapshot, &plan.request_evidence, &output).is_err()
    );
    let legacy = serde_json::to_value(&plan.request_evidence).expect("legacy evidence");
    assert_eq!(legacy["adapter"]["contract_version"], serde_json::json!({"major":1,"minor":0}));
    let reopened: aura_model_domain::ProviderRequestEvidence =
        serde_json::from_value(legacy).expect("old record");
    assert!(!reopened.allows_unknown_tool_names());
    plan.request_evidence.adapter =
        aura_model_domain::ModelAdapterIdentity::current(snapshot.api_format);
    let calls = derive_p07_normalized_tool_calls_v1(&snapshot, &plan.request_evidence, &output)
        .expect("call-local rejection");
    assert_eq!(calls.len(), 1);
    assert!(calls[0].definition_ref.is_none());
}
