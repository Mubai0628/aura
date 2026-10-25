use aura_domain_common::ToolRegistrySnapshotId;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId,
};

use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, CompiledInstructionEnvelope, EndpointAuthScheme,
    LogicalConversationItem, LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest,
    ModelApiFormat, ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest,
    ResolvedModelRequestOptions,
};

use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1};

pub(super) fn snapshot() -> ModelExecutionSnapshot {
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::from_bytes([9; 16]),
        model_step_id: ModelStepId::from_bytes([9; 16]),
        provider_id: ProviderId::from_bytes([9; 16]),
        endpoint_id: EndpointId::from_bytes([9; 16]),
        model_profile_id: ModelProfileId::from_bytes([9; 16]),
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
        context_snapshot_id: ContextProjectionSnapshotId::from_bytes([9; 16]),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::from_bytes([9; 16]),
        endpoint_fingerprint: ContentHash::blake3(b"chat-endpoint"),
        created_at: "2026-10-12T00:00:00Z".parse().expect("fixed timestamp"),
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

fn tool_schema_set() -> ToolSchemaSetSnapshotV1 {
    let release = aura_tool_registry::agent_release::release().expect("current released tools");
    let tools = release
        .manifest()
        .entries
        .iter()
        .map(|e| {
            ToolSchemaSnapshotV1::new(
                e.definition.tool_id,
                e.definition.tool_version.clone(),
                e.definition.canonical_name.clone(),
                e.definition.model_description.clone(),
                e.definition.input_schema.clone(),
                e.definition.strict,
            )
            .expect("current definition schema")
        })
        .collect();
    ToolSchemaSetSnapshotV1::new(
        ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::from_bytes([9; 16]),
            registry_revision: aura_domain_common::Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"current-tools-golden"),
        },
        tools,
    )
    .expect("current schema set")
}

pub(super) fn logical_request(snapshot: &ModelExecutionSnapshot) -> LogicalModelRequest {
    LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope: instruction_envelope(),
        conversation_items: vec![LogicalConversationItem::User {
            source_event_id: EventId::from_bytes([9; 16]),
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
    }
}
