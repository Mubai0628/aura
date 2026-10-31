//! A question asked after the request a step would have sent - a compaction - keeps that request
//! as it was, so Anthropic reads it from its cache: the question is a text block of its own after
//! the task state in the last user turn, the cache breakpoints stay where the step put them, and no
//! `tool_choice` is sent - changing it drops the cache of the messages. When the model called tools
//! instead of answering, its turn, thinking included, and a result for each call follow the
//! question.
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, Revision, ToolCallId, ToolId,
    ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_domain::{
    CompiledInstructionEnvelope, EndpointAuthScheme, LogicalConversationItem,
    LogicalHistoricalToolCall, LogicalInstructionBlock, LogicalInstructionKind,
    LogicalModelRequest, LogicalProviderHistory, ModelApiFormat, ModelExecutionSnapshot,
    ModelOutputFormat, ModelOutputRequest, ProviderHistoryData, ProviderToolCallId,
    ProviderToolCallReuseDomain, ProviderToolCallWireBinding, ProviderToolCallWireIdentity,
    ResolvedModelRequestOptions, TransportOperationContext,
};
use aura_model_transport::{ModelProtocolCompiler, ModelRequestCompilationInput};
use aura_tool_contract::{
    ToolInputSchemaV1, ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1,
    ToolVersion,
};
use serde_json::{Value, json};

const QUESTION: &str = "Compaction request: write the summary in place of the older part.";
const ARGUMENTS: &str = r#"{"aura_purpose":"look first","value":"x"}"#;
const RESULT: &str = "No tool runs while the summary is written; write it now.";
const TOOL: [u8; 16] = [3; 16];

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
        base_url: "https://api.anthropic.com/v1".to_owned(),
        resolved_request_url: "https://api.anthropic.com/v1/messages".to_owned(),
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
        endpoint_fingerprint: ContentHash::blake3(b"anthropic-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn request(snapshot: &ModelExecutionSnapshot) -> LogicalModelRequest {
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
    let canonical = blocks.iter().map(|b| b.content.as_str()).collect::<Vec<_>>().join("\n");
    let tool = ToolSchemaSnapshotV1::new(
        ToolId::from_bytes(TOOL),
        ToolVersion::new(None, Revision::INITIAL).expect("tool version"),
        "fixture.inspect".parse().expect("canonical tool name"),
        "Inspect a fixture value".to_owned(),
        ToolInputSchemaV1::new(json!({"type": "object"})).expect("tool input schema"),
        false,
    )
    .expect("tool schema snapshot");
    LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope: CompiledInstructionEnvelope::new(1, blocks, canonical)
            .expect("instruction envelope"),
        conversation_items: vec![LogicalConversationItem::User {
            source_event_id: EventId::new(),
            content: "hello".to_owned(),
            content_hash: ContentHash::sha256(b"hello"),
        }],
        tool_schema_set: ToolSchemaSetSnapshotV1::new(
            ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"closing-question-registry"),
            },
            vec![tool],
        )
        .expect("schema set"),
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

fn body(request: &LogicalModelRequest, snapshot: &ModelExecutionSnapshot) -> Value {
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
        .expect("compile Anthropic request");
    serde_json::from_slice(&plan.body).expect("request JSON")
}

fn messages(body: &Value) -> Vec<Value> {
    body["messages"].as_array().expect("messages").clone()
}

fn asked(mut request: LogicalModelRequest) -> LogicalModelRequest {
    request.closing_question = Some(QUESTION.to_owned());
    request
}

/// The model's turn that called the request's tool after thinking, and the result that says no
/// tool runs; the snapshot gets the call's wire binding.
fn exchange(snapshot: &mut ModelExecutionSnapshot) -> Vec<LogicalConversationItem> {
    let tool_id = ToolId::from_bytes(TOOL);
    let call = ToolCallId::new();
    let reuse_domain = ProviderToolCallReuseDomain {
        endpoint_id: snapshot.endpoint_id,
        endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
        api_format: snapshot.api_format,
    };
    snapshot.historical_tool_call_wire_bindings.push(ProviderToolCallWireBinding {
        tool_call_id: call,
        tool_id: Some(tool_id),
        provider_wire_tool_name: aura_model_domain::provider_wire_tool_name(
            "fixture.inspect",
            tool_id,
        ),
        wire_identity: ProviderToolCallWireIdentity::ProviderOpaque {
            provider_tool_call_id: ProviderToolCallId::new("call_closing").expect("call id"),
            reuse_domain: reuse_domain.clone(),
        },
    });
    vec![
        LogicalConversationItem::Assistant {
            source_event_id: EventId::new(),
            content: None,
            content_hash: None,
            tool_calls: vec![LogicalHistoricalToolCall {
                tool_call_id: call,
                tool_id: Some(tool_id),
                canonical_name: "fixture.inspect".to_owned(),
                arguments_json: ARGUMENTS.to_owned(),
                arguments_hash: ContentHash::sha256(ARGUMENTS.as_bytes()),
            }],
            provider_history: Some(LogicalProviderHistory {
                reuse_domain,
                model_name: snapshot.model_name.clone(),
                data: ProviderHistoryData::Anthropic {
                    thinking_blocks: vec![json!({"type":"thinking",
                        "thinking":"Reading the file first.","signature":"sig"})],
                },
            }),
        },
        LogicalConversationItem::ToolResult {
            source_event_id: EventId::new(),
            tool_call_id: call,
            tool_name: "fixture.inspect".to_owned(),
            content: RESULT.to_owned(),
            content_hash: ContentHash::sha256(RESULT.as_bytes()),
            is_error: true,
        },
    ]
}

#[test]
fn the_question_follows_the_task_state_in_the_last_user_turn_and_names_no_tool_choice() {
    let snapshot = snapshot();
    let step = request(&snapshot);
    let (sent, compaction) = (body(&step, &snapshot), body(&asked(step), &snapshot));
    let (before, now) = (messages(&sent), messages(&compaction));
    assert_eq!(now.len(), before.len(), "{now:?}");
    let blocks = |messages: &[Value]| messages[0]["content"].as_array().expect("blocks").clone();
    let (sent_blocks, asked_blocks) = (blocks(&before), blocks(&now));
    assert_eq!(
        sent_blocks,
        [
            json!({"type":"text","text":"hello","cache_control":{"type":"ephemeral"}}),
            json!({"type":"text","text":"<TaskState>TaskState</TaskState>"}),
        ]
    );
    assert_eq!(&asked_blocks[..sent_blocks.len()], sent_blocks.as_slice(), "the step's turn");
    assert_eq!(asked_blocks[sent_blocks.len()..], [json!({"type":"text","text":QUESTION})]);
    for field in ["model", "system", "tools", "max_tokens", "thinking"] {
        assert_eq!(compaction.get(field), sent.get(field), "{field}");
    }
    assert!(compaction.get("tool_choice").is_none(), "{compaction}");
}

#[test]
fn after_tool_calls_the_turn_and_a_result_for_each_call_follow_the_question() {
    let snapshot = snapshot();
    let step = request(&snapshot);
    let alone = messages(&body(&asked(step.clone()), &snapshot));
    let mut follow = snapshot.clone();
    let mut again = asked(step);
    again.closing_exchange = exchange(&mut follow);
    let sent = body(&again, &follow);
    let now = messages(&sent);
    let wire =
        aura_model_domain::provider_wire_tool_name("fixture.inspect", ToolId::from_bytes(TOOL));
    assert_eq!(&now[..alone.len()], alone.as_slice(), "the request with its question, unchanged");
    assert_eq!(
        &now[alone.len()..],
        [
            json!({"role":"assistant","content":[
                {"type":"thinking","thinking":"Reading the file first.","signature":"sig"},
                {"type":"tool_use","id":"call_closing","name":wire,
                    "input":{"aura_purpose":"look first","value":"x"}}]}),
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"call_closing",
                "content":RESULT,"is_error":true}]}),
        ]
    );
    assert!(sent.get("tool_choice").is_none(), "{sent}");
}
