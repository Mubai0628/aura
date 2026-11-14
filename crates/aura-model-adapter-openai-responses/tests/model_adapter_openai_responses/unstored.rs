//! Where a Responses request's profile asks the Provider to keep nothing (`store: false`), at
//! OpenAI's API and at Amazon Bedrock, the conversation goes on only by what each request
//! carries: a reasoning item is sent back only as the Provider returned it encrypted, without the
//! id it would be looked up by, and one returned otherwise is left out. Where the profile asks for
//! the reasoning encrypted, at OpenAI's API, a model that reasons asks for it; a model that does
//! not, which refuses the field, does not. Everywhere else a request asks for neither and sends
//! the reasoning back as it came.
use std::collections::BTreeSet;

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, Revision,
    ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    CompiledInstructionEnvelope, EndpointAuthScheme, LogicalConversationItem,
    LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest, LogicalProviderHistory,
    ModelApiFormat, ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest,
    ProviderHistoryData, ProviderToolCallReuseDomain, ReasoningEffort, ResolvedModelRequestOptions,
    TransportOperationContext,
};
use aura_model_transport::{ModelProtocolCompiler, ModelRequestCompilationInput};
use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1};
use serde_json::{Value, json};

const OPENAI: &str = "https://api.openai.com/v1/responses";
const BEDROCK: [&str; 2] = [
    "https://bedrock-mantle.us-east-1.api.aws/openai/v1/responses",
    "https://bedrock-mantle.eu-west-1.api.aws/v1/responses",
];
const ELSEWHERE: [&str; 3] = [
    "http://127.0.0.1:8317/v1/responses",
    "https://api.deepseek.com/responses",
    "https://api.example.test/v1/responses",
];

fn encrypted() -> Value {
    json!({
        "type": "reasoning",
        "id": "rs_encrypted",
        "summary": [{"type": "summary_text", "text": "Thought it through."}],
        "encrypted_content": "opaque_ExactCase",
    })
}

fn by_reference() -> Value {
    json!({"type": "reasoning", "id": "rs_reference", "summary": []})
}

fn blank() -> Value {
    json!({"type": "reasoning", "id": "rs_blank", "summary": [], "encrypted_content": ""})
}

/// A step to `url` of a model that reasons by its catalog, or that does not.
fn snapshot(url: &str, reasons: bool) -> ModelExecutionSnapshot {
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Fixture".to_owned(),
        endpoint_display_name: "Responses".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Fixture / Responses / fixture-model".to_owned(),
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
        request_options: ResolvedModelRequestOptions {
            reasoning_by_effort: reasons,
            ..ResolvedModelRequestOptions::output_only(512)
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

/// A conversation whose answer came with `reasoning_items` from the same endpoint and model.
fn request(snapshot: &ModelExecutionSnapshot, reasoning_items: Vec<Value>) -> LogicalModelRequest {
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
    let user = |text: &str| LogicalConversationItem::User {
        source_event_id: EventId::new(),
        content: text.to_owned(),
        content_hash: ContentHash::sha256(text.as_bytes()),
    };
    let answer = LogicalConversationItem::Assistant {
        source_event_id: EventId::new(),
        content: Some("Hello.".to_owned()),
        content_hash: Some(ContentHash::sha256(b"Hello.")),
        tool_calls: Vec::new(),
        provider_history: Some(LogicalProviderHistory {
            reuse_domain: ProviderToolCallReuseDomain {
                endpoint_id: snapshot.endpoint_id,
                endpoint_security_fingerprint: snapshot.endpoint_fingerprint.clone(),
                api_format: snapshot.api_format,
            },
            model_name: snapshot.model_name.clone(),
            data: ProviderHistoryData::Responses { reasoning_items },
        }),
    };
    LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope: CompiledInstructionEnvelope::new(1, blocks, canonical)
            .expect("instruction envelope"),
        conversation_items: vec![user("hello"), answer, user("and then?")],
        tool_schema_set: ToolSchemaSetSnapshotV1::new(
            ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"unstored-registry"),
            },
            Vec::new(),
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

fn body(snapshot: &ModelExecutionSnapshot, reasoning_items: Vec<Value>) -> Value {
    let request = request(snapshot, reasoning_items);
    let operation = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let plan = OpenAiResponsesAdapter
        .compile_request(ModelRequestCompilationInput {
            request: &request,
            snapshot,
            resolved_attachments: &[],
            operation_context: &operation,
            compatibility_removals: &BTreeSet::new(),
            applied_output_token_field_alias: None,
            session_id: None,
        })
        .expect("compile Responses request");
    serde_json::from_slice(&plan.body).expect("request JSON")
}

/// The reasoning items the request sends back, in order.
fn reasoning(body: &Value) -> Vec<Value> {
    body["input"]
        .as_array()
        .expect("input")
        .iter()
        .filter(|item| item["type"] == "reasoning")
        .cloned()
        .collect()
}

#[test]
fn openais_api_keeps_nothing_and_a_reasoning_model_asks_for_its_reasoning_encrypted() {
    let sent = body(&snapshot(OPENAI, true), Vec::new());
    assert_eq!(sent["store"], json!(false), "{sent}");
    assert_eq!(sent["include"], json!(["reasoning.encrypted_content"]), "{sent}");

    let mut leveled = snapshot(OPENAI, false);
    leveled.request_options.reasoning_effort = Some(ReasoningEffort::High);
    let sent = body(&leveled, Vec::new());
    assert_eq!(sent["include"], json!(["reasoning.encrypted_content"]), "a thinking level: {sent}");

    let sent = body(&snapshot(OPENAI, false), Vec::new());
    assert_eq!(sent["store"], json!(false), "{sent}");
    assert!(sent.get("include").is_none(), "a model that does not reason: {sent}");
}

#[test]
fn bedrock_keeps_nothing_and_is_not_asked_for_encrypted_reasoning() {
    for url in BEDROCK {
        let sent = body(&snapshot(url, true), Vec::new());
        assert_eq!(sent["store"], json!(false), "{url}: {sent}");
        assert!(sent.get("include").is_none(), "{url}: {sent}");
    }
}

#[test]
fn where_nothing_is_kept_reasoning_goes_back_only_encrypted_and_without_its_id() {
    let mut expected = encrypted();
    expected.as_object_mut().expect("an item").remove("id");
    for url in [OPENAI, BEDROCK[0], BEDROCK[1]] {
        let sent = body(&snapshot(url, true), vec![encrypted(), by_reference(), blank()]);
        assert_eq!(reasoning(&sent), [expected.clone()], "{url}");
    }
}

#[test]
fn everywhere_else_a_request_asks_for_neither_and_sends_the_reasoning_back_as_it_came() {
    for url in ELSEWHERE {
        let sent = body(&snapshot(url, true), vec![encrypted(), by_reference(), blank()]);
        assert!(sent.get("store").is_none() && sent.get("include").is_none(), "{url}: {sent}");
        assert_eq!(reasoning(&sent), [encrypted(), by_reference(), blank()], "{url}");
    }
}
