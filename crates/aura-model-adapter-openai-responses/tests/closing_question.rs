//! A question asked after the request a step would have sent - a compaction - keeps that request
//! as it was, so the provider reads it from its cache: the question is a user message of its own
//! after the step's input, wherever the task state goes, and no tool choice is named - a provider
//! told that no tool may be called can leave the tools out of the prompt it reads from its cache,
//! and thinking models refuse the forced forms. When the model called tools instead of answering,
//! its answer and a result for each call follow the question.
use std::collections::BTreeSet;

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, Revision, ToolCallId, ToolId,
    ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
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

fn snapshot(url: &str) -> ModelExecutionSnapshot {
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
        base_url: url.trim_end_matches("/responses").to_owned(),
        resolved_request_url: url.to_owned(),
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
        endpoint_fingerprint: ContentHash::blake3(b"responses-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn request(snapshot: &ModelExecutionSnapshot, tools: bool) -> LogicalModelRequest {
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
    let canonical =
        blocks.iter().map(|block| block.content.as_str()).collect::<Vec<_>>().join("\n");
    let tool = ToolSchemaSnapshotV1::new(
        ToolId::from_bytes([1; 16]),
        ToolVersion::new(None, Revision::INITIAL).expect("tool version"),
        "fixture.inspect".parse().expect("canonical tool name"),
        "Inspect a fixture value".to_owned(),
        ToolInputSchemaV1::new(json!({"type": "object"})).expect("tool input schema"),
        false,
    )
    .expect("tool schema snapshot");
    let user = |text: &str| LogicalConversationItem::User {
        source_event_id: EventId::new(),
        content: text.to_owned(),
        content_hash: ContentHash::sha256(text.as_bytes()),
    };
    LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope: CompiledInstructionEnvelope::new(1, blocks, canonical)
            .expect("instruction envelope"),
        conversation_items: vec![user("hello"), user("and then?")],
        tool_schema_set: ToolSchemaSetSnapshotV1::new(
            ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"closing-question-registry"),
            },
            if tools { vec![tool] } else { vec![] },
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
    let plan = OpenAiResponsesAdapter
        .compile_request(ModelRequestCompilationInput {
            request,
            snapshot,
            resolved_attachments: &[],
            operation_context: &operation,
            compatibility_removals: &BTreeSet::new(),
            applied_output_token_field_alias: None,
        })
        .expect("compile Responses request");
    serde_json::from_slice(&plan.body).expect("request JSON")
}

fn asked(mut request: LogicalModelRequest) -> LogicalModelRequest {
    request.closing_question = Some(QUESTION.to_owned());
    request
}

fn input(body: &Value) -> Vec<Value> {
    body["input"].as_array().expect("input").clone()
}

fn question() -> Value {
    json!({"role":"user","content":[{"type":"input_text","text":QUESTION}]})
}

/// OpenAI's API and a local gateway in front of it, where the task state leads, and another
/// provider's endpoint, where it follows the conversation.
const URLS: [&str; 3] = [
    "http://127.0.0.1:8317/v1/responses",
    "https://api.openai.com/v1/responses",
    "https://api.example.test/v1/responses",
];

const ARGUMENTS: &str = r#"{"aura_purpose":"look first"}"#;
const RESULT: &str = "No tool runs while the summary is written; write it now.";

/// The model's answer to the question that called the request's tool, and the result that says
/// no tool runs; the snapshot gets the call's wire binding.
fn exchange(snapshot: &mut ModelExecutionSnapshot) -> Vec<LogicalConversationItem> {
    let tool_id = ToolId::from_bytes([1; 16]);
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
                data: ProviderHistoryData::Responses {
                    reasoning_items: vec![json!({"type":"reasoning","id":"rs_1","summary":[]})],
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
fn the_question_is_a_user_message_after_the_steps_input_and_names_no_tool_choice() {
    for url in URLS {
        let snapshot = snapshot(url);
        let step = request(&snapshot, true);
        let (sent, compaction) = (body(&step, &snapshot), body(&asked(step), &snapshot));
        let (before, now) = (input(&sent), input(&compaction));
        let state = json!([{"type":"input_text","text":"<TaskState>TaskState</TaskState>"}]);
        let at = if url.contains("example") { before.len() - 1 } else { 0 };
        assert_eq!(before[at]["content"], state, "the task state where the step puts it: {url}");
        assert_eq!(now.len(), before.len() + 1, "{now:?}");
        assert_eq!(&now[..before.len()], before.as_slice(), "the step's input, unchanged: {url}");
        assert_eq!(now[before.len()], question(), "{url}");
        for field in ["instructions", "tools", "prompt_cache_key", "model", "max_output_tokens"] {
            assert_eq!(compaction.get(field), sent.get(field), "{field} for {url}");
        }
        assert!(compaction.get("tool_choice").is_none(), "{url}: {compaction}");
    }
}

#[test]
fn after_tool_calls_the_answer_and_a_result_for_each_call_follow_the_question() {
    let wire =
        aura_model_domain::provider_wire_tool_name("fixture.inspect", ToolId::from_bytes([1; 16]));
    for url in URLS {
        let snapshot = snapshot(url);
        let step = request(&snapshot, true);
        let before = input(&body(&step, &snapshot));
        let mut follow = snapshot.clone();
        let mut again = asked(step);
        again.closing_exchange = exchange(&mut follow);
        let sent = body(&again, &follow);
        let now = input(&sent);
        assert_eq!(&now[..before.len()], before.as_slice(), "the step's input, unchanged: {url}");
        assert_eq!(
            &now[before.len()..],
            [
                question(),
                json!({"type":"reasoning","id":"rs_1","summary":[]}),
                json!({"type":"function_call","call_id":"call_closing","name":wire,
                    "arguments":ARGUMENTS}),
                json!({"type":"function_call_output","call_id":"call_closing","output":RESULT}),
            ],
            "{url}"
        );
        assert!(sent.get("tool_choice").is_none(), "{url}: {sent}");
    }
}

#[test]
fn a_question_without_tools_is_the_last_user_message() {
    let snapshot = snapshot("http://127.0.0.1:8317/v1/responses");
    let compaction = body(&asked(request(&snapshot, false)), &snapshot);
    assert!(compaction.get("tools").is_none() && compaction.get("tool_choice").is_none());
    assert_eq!(input(&compaction).last(), Some(&question()));
}
