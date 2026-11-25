//! Through OpenAI or a local gateway the task state goes before the conversation, where a provider
//! that caches whole earlier requests kept the conversation cached only while each step's input
//! began with the one before. A state that changes during a conversation is added where it changed
//! and the first stays in front, so a change costs the new state and not the whole conversation.
//! Another provider's endpoint keeps the state after the conversation, replaced as it changes.
use std::collections::BTreeSet;

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, Revision, SessionId,
    ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    CompiledInstructionEnvelope, EndpointAuthScheme, LogicalConversationItem,
    LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest, ModelApiFormat,
    ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest, ResolvedModelRequestOptions,
    TransportOperationContext,
};
use aura_model_transport::{ModelProtocolCompiler, ModelRequestCompilationInput};
use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1};
use serde_json::Value;

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

fn user(text: &str) -> LogicalConversationItem {
    LogicalConversationItem::User {
        source_event_id: EventId::new(),
        content: text.to_owned(),
        content_hash: ContentHash::sha256(text.as_bytes()),
    }
}

/// A step's request: the same instructions, these conversation items and this task state.
fn request(
    snapshot: &ModelExecutionSnapshot,
    items: &[LogicalConversationItem],
    state: &str,
) -> LogicalModelRequest {
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
        let content = if kind == LogicalInstructionKind::TaskState {
            state.to_owned()
        } else {
            format!("<{kind:?}>{kind:?}</{kind:?}>")
        };
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
    LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope: CompiledInstructionEnvelope::new(1, blocks, canonical)
            .expect("instruction envelope"),
        conversation_items: items.to_vec(),
        tool_schema_set: ToolSchemaSetSnapshotV1::new(
            ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"task-state-registry"),
            },
            vec![],
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

/// The request's input as compiled for `session`.
fn input(
    request: &LogicalModelRequest,
    snapshot: &ModelExecutionSnapshot,
    session: SessionId,
) -> Vec<Value> {
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
            session_id: Some(session),
        })
        .expect("compile Responses request");
    let body: Value = serde_json::from_slice(&plan.body).expect("request JSON");
    body["input"].as_array().expect("input").clone()
}

fn text(item: &Value) -> &str {
    item["content"][0]["text"].as_str().unwrap_or_default()
}

#[test]
fn a_changed_state_is_added_where_it_changed_and_each_step_extends_the_one_before() {
    let snapshot = snapshot("http://127.0.0.1:8317/v1/responses");
    let session = SessionId::new();
    let items = [user("hello"), user("and then?"), user("more")];
    let first = input(&request(&snapshot, &items[..1], "<State>one</State>"), &snapshot, session);
    assert_eq!(first[0]["role"], "developer");
    assert_eq!(text(&first[0]), "<State>one</State>");
    let second = input(&request(&snapshot, &items[..2], "<State>two</State>"), &snapshot, session);
    assert_eq!(&second[..first.len()], first.as_slice(), "the step before stays as it was sent");
    let added = second.last().expect("the change");
    assert_eq!((added["role"].as_str(), text(added)), (Some("developer"), "<State>two</State>"));
    let third = input(&request(&snapshot, &items, "<State>two</State>"), &snapshot, session);
    assert_eq!(&third[..second.len()], second.as_slice());
    assert_eq!(third.len(), second.len() + 1, "an unchanged state adds nothing");
}

#[test]
fn another_providers_endpoint_keeps_the_state_after_the_conversation() {
    let snapshot = snapshot("https://api.example.test/v1/responses");
    let session = SessionId::new();
    let items = [user("hello"), user("and then?")];
    input(&request(&snapshot, &items[..1], "<State>one</State>"), &snapshot, session);
    let second = input(&request(&snapshot, &items, "<State>two</State>"), &snapshot, session);
    assert_eq!(second.len(), 3, "{second:?}");
    assert_eq!([text(&second[0]), text(&second[1])], ["hello", "and then?"]);
    assert_eq!(text(second.last().expect("the state")), "<State>two</State>");
    assert!(!second.iter().any(|item| text(item) == "<State>one</State>"));
}
