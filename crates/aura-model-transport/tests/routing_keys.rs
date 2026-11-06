//! Each request carries the routing key its host's profile names, as the Provider documents it:
//! - OpenCode's gateway refuses a request without `x-opencode-session` ("cannot be routed
//!   efficiently"): it routes and caches by a stable id for each conversation, and it asks each
//!   client to name itself in User-Agent. Every request to opencode.ai carries both, in each
//!   connected format and for the availability test and the model list too;
//! - xAI's API is sent `prompt_cache_key` in a Responses body, with the task's state after the
//!   conversation, and `x-grok-conv-id` with Chat Completions;
//! - Vercel's AI Gateway is sent `x-session-id` in each format;
//! - no other Provider is sent any of them.
use std::collections::BTreeSet;

use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelAvailabilityTestId,
    ModelDiscoveryRunId, ModelExecutionSnapshotId, ModelProfileId, ModelStepId,
    PromptBundleSnapshotId, ProviderId, Revision, SessionId, ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, CompiledInstructionEnvelope, EndpointAuthScheme,
    LogicalConversationItem, LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest,
    ModelApiFormat, ModelContextWindowConfig, ModelDiscoveryMode, ModelEndpointConfig,
    ModelEndpointTimeouts, ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest,
    ModelProfileConfig, ModelProfileOrigin, ModelRequestDefaults, ProviderRequestHeaderClass,
    ResolvedModelRequestOptions, TransportOperationContext,
};
use aura_model_transport::{
    ModelAvailabilityProbeCompilationInput, ModelDiscoveryCompilationInput,
    ModelGenerationProbeProtocol, ModelProtocolCompiler, ModelRequestCompilationInput,
    ProviderRequestPlan, build_model_discovery_request_plan,
};
use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1};

const SESSION: &str = "x-opencode-session";
const FORMATS: [(ModelApiFormat, &str); 3] = [
    (ModelApiFormat::OpenAiChatCompletions, "https://opencode.ai/zen/go/v1/chat/completions"),
    (ModelApiFormat::OpenAiResponses, "https://opencode.ai/inference/go/openai/v1/responses"),
    (ModelApiFormat::AnthropicMessages, "https://opencode.ai/inference/go/anthropic/v1/messages"),
];

fn user_agent() -> String {
    format!("Aura/{}", env!("CARGO_PKG_VERSION"))
}

/// A new snapshot is a new model step.
fn snapshot(api_format: ModelApiFormat, url: &str) -> ModelExecutionSnapshot {
    let chat = api_format == ModelApiFormat::OpenAiChatCompletions;
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "OpenCode Go".to_owned(),
        endpoint_display_name: "fixture".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "OpenCode Go / fixture / fixture-model".to_owned(),
        api_format,
        chat_output_token_field: chat.then_some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: chat.then_some(ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::None,
        base_url: url.rsplit_once("/v1/").map_or(url, |(base, _)| base).to_owned() + "/v1",
        resolved_request_url: url.to_owned(),
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
            speed_mode: None,
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
                content_hash: CanonicalHashV1::sha256(b"opencode-registry"),
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

fn compile(
    snapshot: &ModelExecutionSnapshot,
    session_id: Option<SessionId>,
) -> ProviderRequestPlan {
    let request = logical_request(snapshot);
    let operation = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let removals = BTreeSet::new();
    let input = ModelRequestCompilationInput {
        request: &request,
        snapshot,
        resolved_attachments: &[],
        operation_context: &operation,
        compatibility_removals: &removals,
        applied_output_token_field_alias: None,
        session_id,
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

/// The header as the plan sends it; its evidence must hold the same value in the given class,
/// or the transport refuses the plan.
fn sent(
    plan: &ProviderRequestPlan,
    name: &str,
    class: ProviderRequestHeaderClass,
) -> Option<String> {
    let mut values = plan.headers.iter().filter(|(sent, _)| sent == name).map(|(_, value)| value);
    let value = values.next()?.clone();
    assert!(values.next().is_none(), "{name} is sent once");
    let evidence = plan
        .request_evidence
        .headers
        .iter()
        .find(|header| header.name == name)
        .expect("the header's evidence");
    assert_eq!((evidence.class, evidence.non_secret_value.as_deref()), (class, Some(&*value)));
    Some(value)
}

fn session(plan: &ProviderRequestPlan) -> Option<String> {
    sent(plan, SESSION, ProviderRequestHeaderClass::SemanticRouting)
}

fn agent(plan: &ProviderRequestPlan) -> Option<String> {
    sent(plan, "user-agent", ProviderRequestHeaderClass::ProtocolFixed)
}

#[test]
fn each_request_to_opencode_carries_its_conversations_id_and_auras_name() {
    for (format, url) in FORMATS {
        let conversation = SessionId::new();
        let first = compile(&snapshot(format, url), Some(conversation));
        let key = session(&first).unwrap_or_else(|| panic!("{format:?}: no {SESSION}"));
        assert!(
            key.len() == 37
                && key.starts_with("aura-")
                && key[5..].bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{format:?}: an opaque id, not an Aura record id: {key}"
        );
        let later = compile(&snapshot(format, url), Some(conversation));
        assert_eq!(session(&later), Some(key.clone()), "{format:?}: the conversation's next step");
        let other = compile(&snapshot(format, url), Some(SessionId::new()));
        assert_ne!(session(&other), Some(key), "{format:?}: another conversation");
        assert_eq!(agent(&first), Some(user_agent()), "{format:?}");
    }
}

/// The value stands for the conversation as it did when the header was first sent, so a
/// conversation's requests keep reaching the same cache after an update.
#[test]
fn the_conversation_header_is_derived_as_it_was_first_sent() {
    let conversation = SessionId::new();
    let hash = ContentHash::sha256(format!("{SESSION}\n{conversation}").as_bytes());
    let expected = format!("aura-{}", &hash.hex[..32]);
    for (format, url) in FORMATS {
        let plan = compile(&snapshot(format, url), Some(conversation));
        assert_eq!(session(&plan), Some(expected.clone()), "{format:?}");
    }
}

#[test]
fn a_subdomain_a_capitalised_host_and_a_trailing_dot_are_opencode_too() {
    for url in [
        "https://api.opencode.ai/v1/chat/completions",
        "https://OpenCode.AI/zen/go/v1/chat/completions",
        "https://opencode.ai./zen/go/v1/chat/completions",
    ] {
        let plan =
            compile(&snapshot(ModelApiFormat::OpenAiChatCompletions, url), Some(SessionId::new()));
        assert!(session(&plan).is_some(), "{url}");
        assert_eq!(agent(&plan), Some(user_agent()), "{url}");
    }
}

/// The header `name` a conversation's requests carry: the same derivation as OpenCode's, by the
/// header's own name.
fn conversation_key(name: &str, conversation: SessionId) -> String {
    let hash = ContentHash::sha256(
        format!(
            "{name}
{conversation}"
        )
        .as_bytes(),
    );
    format!("aura-{}", &hash.hex[..32])
}

fn body(plan: &ProviderRequestPlan) -> serde_json::Value {
    serde_json::from_slice(&plan.body).expect("a JSON body")
}

#[test]
fn xais_requests_carry_its_cache_key_by_each_protocols_own_name() {
    let conversation = SessionId::new();
    let responses = compile(
        &snapshot(ModelApiFormat::OpenAiResponses, "https://api.x.ai/v1/responses"),
        Some(conversation),
    );
    let sent = body(&responses);
    let key = sent["prompt_cache_key"].as_str().expect("prompt_cache_key");
    assert!(key.len() == 37 && key.starts_with("aura-"), "{key}");
    assert_eq!(sent["input"][0]["role"], "user", "the state stays after the conversation");
    assert!(responses.headers.iter().all(|(name, _)| name != "x-grok-conv-id"));

    let chat = compile(
        &snapshot(ModelApiFormat::OpenAiChatCompletions, "https://api.x.ai/v1/chat/completions"),
        Some(conversation),
    );
    assert_eq!(
        sent_header(&chat, "x-grok-conv-id"),
        Some(conversation_key("x-grok-conv-id", conversation))
    );
    assert!(body(&chat).get("prompt_cache_key").is_none());

    let messages = compile(
        &snapshot(ModelApiFormat::AnthropicMessages, "https://api.x.ai/v1/messages"),
        Some(conversation),
    );
    assert!(messages.headers.iter().all(|(name, _)| name != "x-grok-conv-id"));
    assert!(body(&messages).get("prompt_cache_key").is_none());
    for plan in [&responses, &chat, &messages] {
        assert_eq!(agent(plan), None);
    }
}

#[test]
fn vercels_ai_gateway_is_sent_the_conversations_session_header_in_each_format() {
    for (format, url) in [
        (ModelApiFormat::OpenAiChatCompletions, "https://ai-gateway.vercel.sh/v1/chat/completions"),
        (ModelApiFormat::OpenAiResponses, "https://ai-gateway.vercel.sh/v1/responses"),
        (ModelApiFormat::AnthropicMessages, "https://ai-gateway.vercel.sh/v1/messages"),
    ] {
        let conversation = SessionId::new();
        let plan = compile(&snapshot(format, url), Some(conversation));
        assert_eq!(
            sent_header(&plan, "x-session-id"),
            Some(conversation_key("x-session-id", conversation)),
            "{format:?}"
        );
        assert!(body(&plan).get("prompt_cache_key").is_none(), "{format:?}");
        assert_eq!(agent(&plan), None, "{format:?}");
    }
}

fn sent_header(plan: &ProviderRequestPlan, name: &str) -> Option<String> {
    sent(plan, name, ProviderRequestHeaderClass::SemanticRouting)
}

#[test]
fn no_other_provider_is_sent_any_of_them() {
    for url in [
        "https://api.example.test/v1/chat/completions",
        "https://opencode.ai.example.test/v1/chat/completions",
        "https://notopencode.ai/v1/chat/completions",
        "http://127.0.0.1:8317/v1/chat/completions",
        "https://x.ai/v1/chat/completions",
        "https://vercel.sh/v1/chat/completions",
    ] {
        let plan =
            compile(&snapshot(ModelApiFormat::OpenAiChatCompletions, url), Some(SessionId::new()));
        for name in [SESSION, "user-agent", "x-grok-conv-id", "x-session-id"] {
            assert!(
                plan.headers.iter().all(|(sent, _)| sent != name)
                    && plan.request_evidence.headers.iter().all(|header| header.name != name),
                "{url}: {name}"
            );
        }
    }
}

fn endpoint_and_profile(base_url: &str) -> (ModelEndpointConfig, ModelProfileConfig) {
    let endpoint_id = EndpointId::new();
    let profile_id = ModelProfileId::new();
    let now = UtcTimestamp::now();
    let endpoint = ModelEndpointConfig {
        id: endpoint_id,
        revision: Revision::INITIAL,
        provider_id: ProviderId::new(),
        display_name: "OpenCode Go".to_owned(),
        slug: "opencode-go".parse().expect("valid Endpoint slug"),
        api_format: ModelApiFormat::OpenAiChatCompletions,
        base_url: base_url.to_owned(),
        discovery_mode: ModelDiscoveryMode::Automatic,
        chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: Some(ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::None,
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        timeouts: ModelEndpointTimeouts {
            connect_ms: 5_000,
            first_event_ms: 10_000,
            stream_idle_ms: 10_000,
            total_ms: Some(30_000),
        },
        max_parallel_model_requests: 1,
        model_order: vec![profile_id],
        compatibility: None,
        created_at: now,
        updated_at: now,
    };
    let profile = ModelProfileConfig {
        id: profile_id,
        revision: Revision::INITIAL,
        endpoint_id,
        provider_model_name: "fixture-model".to_owned(),
        display_name: "Fixture model".to_owned(),
        notes: None,
        origin: ModelProfileOrigin::Manual,
        enabled: true,
        available_for_subagents: false,
        catalog: None,
        context: ModelContextWindowConfig {
            catalog_window_tokens: None,
            user_override_window_tokens: None,
            user_override_revision: 0,
            runtime_corrected_window_tokens: None,
            runtime_correction_basis: None,
            runtime_effective_window_tokens: 32_000,
            historical_snapshot_max_tokens: 1_024,
        },
        request_defaults: ModelRequestDefaults {
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            chat_output_token_field: None,
            chat_instruction_role: None,
            reasoning_effort: None,
            reasoning_efforts: None,
            image_input: None,
            allow_speed_modes: false,
            speed_mode: None,
        },
        created_at: now,
        updated_at: now,
    };
    (endpoint, profile)
}

#[test]
fn a_request_outside_a_conversation_is_keyed_by_its_own_operation() {
    let (format, url) = FORMATS[0];
    let step = snapshot(format, url);
    let first = session(&compile(&step, None)).expect("a step without a conversation");
    assert_eq!(session(&compile(&step, None)), Some(first.clone()), "the step's next attempt");
    assert_ne!(
        session(&compile(&snapshot(format, url), None)),
        Some(first.clone()),
        "another step"
    );

    let (endpoint, profile) = endpoint_and_profile("https://opencode.ai/zen/go/v1");
    let availability_test = || TransportOperationContext::ModelAvailabilityTest {
        model_availability_test_id: ModelAvailabilityTestId::new(),
        model_profile_id: profile.id,
    };
    let test = availability_test();
    let probe = |operation_context| {
        OpenAiChatCompletionsAdapter
            .compile_generation_probe(ModelAvailabilityProbeCompilationInput {
                endpoint: &endpoint,
                model_profile: &profile,
                resolved_request_url: url,
                operation_context,
                applied_output_token_field_alias: None,
            })
            .expect("availability probe")
    };
    let tested = probe(&test);
    let key = session(&tested).expect("the availability test");
    assert_eq!(session(&probe(&test)), Some(key.clone()), "the test's next attempt");
    assert_ne!(session(&probe(&availability_test())), Some(key.clone()), "another test");
    assert_eq!(agent(&tested), Some(user_agent()));

    let list = || {
        build_model_discovery_request_plan(ModelDiscoveryCompilationInput {
            endpoint: &endpoint,
            resolved_request_url: "https://opencode.ai/zen/go/v1/models",
            operation_context: &TransportOperationContext::ModelDiscovery {
                discovery_run_id: ModelDiscoveryRunId::new(),
            },
        })
        .expect("model list")
    };
    let listing = list();
    let listed = session(&listing).expect("the model list");
    assert_ne!(session(&list()), Some(listed.clone()), "another listing");
    assert_eq!(agent(&listing), Some(user_agent()));
    assert!(first != key && key != listed && listed != first, "each operation has its own id");
}
