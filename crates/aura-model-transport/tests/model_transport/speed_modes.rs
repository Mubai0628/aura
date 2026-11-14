//! A speed mode is sent the same way in every connected format: the mode's body fields go into
//! the request body and its headers into the request, as the catalog lists them. A request with
//! no mode carries neither. A field or header off Aura's list, or a value that is not a plain
//! word, is never sent, even when a snapshot written elsewhere carries it; a header the request
//! already has keeps its own value and gets the mode's after it.
use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelExecutionSnapshotId,
    ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId, Revision,
    ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, CompiledInstructionEnvelope, EndpointAuthScheme,
    LogicalConversationItem, LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest,
    ModelApiFormat, ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest, ModelSpeedMode,
    ProviderRequestHeaderClass, ResolvedModelRequestOptions, TransportOperationContext,
};
use aura_model_transport::{
    ModelProtocolCompiler, ModelRequestCompilationInput, NonSecretProviderHeader,
    ProviderRequestPlan, build_provider_request_plan,
};
use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1};
use serde_json::Value;

const FORMATS: [(ModelApiFormat, &str); 3] = [
    (ModelApiFormat::OpenAiResponses, "https://api.example.test/v1/responses"),
    (ModelApiFormat::OpenAiChatCompletions, "https://api.example.test/v1/chat/completions"),
    (ModelApiFormat::AnthropicMessages, "https://api.example.test/v1/messages"),
];

fn mode(name: &str, body: &[(&str, &str)], headers: &[(&str, &str)]) -> ModelSpeedMode {
    let pairs = |items: &[(&str, &str)]| -> BTreeMap<String, String> {
        items.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
    };
    ModelSpeedMode { name: name.to_owned(), body: pairs(body), headers: pairs(headers) }
}

fn snapshot(
    api_format: ModelApiFormat,
    url: &str,
    speed_mode: Option<ModelSpeedMode>,
) -> ModelExecutionSnapshot {
    let chat = api_format == ModelApiFormat::OpenAiChatCompletions;
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Example".to_owned(),
        endpoint_display_name: "fixture".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Example / fixture / fixture-model".to_owned(),
        api_format,
        chat_output_token_field: chat.then_some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: chat.then_some(ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: url.to_owned(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 256_000,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions {
            speed_mode,
            ..ResolvedModelRequestOptions::output_only(512)
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::blake3(b"fixture-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn logical_request(snapshot: &ModelExecutionSnapshot) -> LogicalModelRequest {
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
        let content = format!("<{kind:?}>stable</{kind:?}>");
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
        conversation_items: vec![LogicalConversationItem::User {
            source_event_id: EventId::new(),
            content: "hello".to_owned(),
            content_hash: ContentHash::sha256(b"hello"),
        }],
        tool_schema_set: ToolSchemaSetSnapshotV1::new(
            ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"speed-mode-registry"),
            },
            Vec::new(),
        )
        .expect("schema set"),
        attachments: vec![],
        provider_continuation: None,
        output_request: ModelOutputRequest {
            format: ModelOutputFormat::Text,
            effective_max_output_tokens: 512,
        },
        closing_question: None,
        closing_exchange: Vec::new(),
    }
}

fn operation(snapshot: &ModelExecutionSnapshot) -> TransportOperationContext {
    TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    }
}

fn compile(snapshot: &ModelExecutionSnapshot) -> ProviderRequestPlan {
    let request = logical_request(snapshot);
    let operation = operation(snapshot);
    let removals = BTreeSet::new();
    let input = ModelRequestCompilationInput {
        request: &request,
        snapshot,
        resolved_attachments: &[],
        operation_context: &operation,
        compatibility_removals: &removals,
        applied_output_token_field_alias: None,
        session_id: None,
    };
    match snapshot.api_format {
        ModelApiFormat::OpenAiChatCompletions => {
            OpenAiChatCompletionsAdapter.compile_request(input)
        }
        ModelApiFormat::OpenAiResponses => OpenAiResponsesAdapter.compile_request(input),
        ModelApiFormat::AnthropicMessages => AnthropicMessagesAdapter.compile_request(input),
    }
    .expect("compiled request")
}

fn body(plan: &ProviderRequestPlan) -> Value {
    serde_json::from_slice(&plan.body).expect("JSON body")
}

/// Every value the plan sends under `name`; its evidence must hold each one, as a fixed
/// protocol header.
fn header(plan: &ProviderRequestPlan, name: &str) -> Vec<String> {
    let values = plan
        .headers
        .iter()
        .filter(|(sent, _)| sent == name)
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    for value in &values {
        assert!(
            plan.request_evidence.headers.iter().any(|evidence| {
                evidence.name == name
                    && evidence.class == ProviderRequestHeaderClass::ProtocolFixed
                    && evidence.non_secret_value.as_deref() == Some(value)
            }),
            "{name}: {value} has no evidence"
        );
    }
    values
}

#[test]
fn a_mode_sets_its_fields_and_headers_in_every_format() {
    for (format, url) in FORMATS {
        // OpenAI's `fast`, as a body field.
        let priority = compile(&snapshot(
            format,
            url,
            Some(mode("fast", &[("service_tier", "priority")], &[])),
        ));
        assert_eq!(body(&priority)["service_tier"], "priority", "{format:?}");
        assert!(body(&priority).get("speed").is_none(), "{format:?}");
        assert!(header(&priority, "anthropic-beta").is_empty(), "{format:?}");
        // Anthropic's `fast`, as a body field and a header.
        let fast = compile(&snapshot(
            format,
            url,
            Some(mode("fast", &[("speed", "fast")], &[("anthropic-beta", "fast-mode-2026-02-01")])),
        ));
        assert_eq!(body(&fast)["speed"], "fast", "{format:?}");
        assert_eq!(header(&fast, "anthropic-beta"), ["fast-mode-2026-02-01"], "{format:?}");
        // The standard mode sends neither.
        let standard = compile(&snapshot(format, url, None));
        for field in ["service_tier", "speed"] {
            assert!(body(&standard).get(field).is_none(), "{format:?}: {field}");
        }
        assert!(header(&standard, "anthropic-beta").is_empty(), "{format:?}");
    }
}

#[test]
fn a_field_or_header_off_the_list_is_never_sent() {
    let written_elsewhere = mode(
        "odd",
        &[("reasoning", "pro"), ("store", "true"), ("service_tier", "priority\r\nx-evil: 1")],
        &[("x-api-key", "anything"), ("authorization", "Bearer x"), ("anthropic-beta", "a b")],
    );
    for (format, url) in FORMATS {
        let plan = compile(&snapshot(format, url, Some(written_elsewhere.clone())));
        let sent = body(&plan);
        assert!(sent.get("reasoning").is_none_or(|value| value != "pro"), "{format:?}");
        assert!(sent.get("store").is_none_or(|value| value != "true"), "{format:?}");
        assert!(sent.get("service_tier").is_none(), "{format:?}");
        for name in ["x-api-key", "authorization", "anthropic-beta"] {
            assert!(header(&plan, name).is_empty(), "{format:?}: {name}");
        }
        // The rest of a mode is still sent.
        let partly = mode("partly", &[("service_tier", "flex"), ("store", "true")], &[]);
        let plan = compile(&snapshot(format, url, Some(partly)));
        assert_eq!(body(&plan)["service_tier"], "flex", "{format:?}");
    }
}

#[test]
fn a_header_the_request_already_has_gets_the_modes_value_after_its_own() {
    let (format, url) = FORMATS[2];
    let snapshot = snapshot(
        format,
        url,
        Some(mode("fast", &[("speed", "fast")], &[("anthropic-beta", "fast-mode-2026-02-01")])),
    );
    let request = logical_request(&snapshot);
    let operation = operation(&snapshot);
    let removals = BTreeSet::new();
    let input = ModelRequestCompilationInput {
        request: &request,
        snapshot: &snapshot,
        resolved_attachments: &[],
        operation_context: &operation,
        compatibility_removals: &removals,
        applied_output_token_field_alias: None,
        session_id: None,
    };
    let own = NonSecretProviderHeader {
        name: "anthropic-beta".to_owned(),
        value: "context-1m-2025-08-07".to_owned(),
        class: ProviderRequestHeaderClass::ProtocolFixed,
    };
    let plan = build_provider_request_plan(input, b"{}".to_vec(), vec![own], Vec::new())
        .expect("request plan");
    assert_eq!(header(&plan, "anthropic-beta"), ["context-1m-2025-08-07,fast-mode-2026-02-01"]);
}
