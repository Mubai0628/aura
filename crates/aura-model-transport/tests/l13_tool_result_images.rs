//! A screenshot the browser returned reaches a
//! model that takes images with the tool result it belongs to, in each connected format; an image
//! anywhere else, or something that is not an image, is refused, and a request without images is
//! sent exactly as before.
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, EndpointId, EventId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId,
    ToolCallId, ToolId, ToolRegistrySnapshotId, UtcTimestamp,
};
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, CompiledInstructionEnvelope, EndpointAuthScheme,
    LogicalConversationItem, LogicalHistoricalToolCall, LogicalInstructionBlock,
    LogicalInstructionKind, LogicalModelRequest, ModelApiFormat, ModelAttachmentSnapshot,
    ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest, ProviderToolCallWireBinding,
    ProviderToolCallWireIdentity, ProviderWireToolCallId, ResolvedModelAttachment,
    ResolvedModelRequestOptions, TransportOperationContext,
};
use aura_model_transport::{
    ModelAdapterError, ModelProtocolCompiler, ModelRequestCompilationInput,
};
use aura_tool_contract::{
    ToolInputSchemaV1, ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1,
    ToolVersion,
};
use serde_json::{Value, json};

const FORMATS: [ModelApiFormat; 3] = [
    ModelApiFormat::OpenAiChatCompletions,
    ModelApiFormat::OpenAiResponses,
    ModelApiFormat::AnthropicMessages,
];
/// A JPEG's first bytes are enough: the adapters carry bytes, they do not decode them.
const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
const JPEG_BASE64: &str = "/9j/4AAQSkZJRg==";

struct Fixture {
    request: LogicalModelRequest,
    snapshot: ModelExecutionSnapshot,
    result_event: EventId,
    user_event: EventId,
    wire_call_id: String,
}

fn fixture(api_format: ModelApiFormat) -> Fixture {
    let url = match api_format {
        ModelApiFormat::OpenAiChatCompletions => "https://api.example.test/v1/chat/completions",
        ModelApiFormat::OpenAiResponses => "https://api.example.test/v1/responses",
        ModelApiFormat::AnthropicMessages => "https://api.anthropic.test/v1/messages",
    };
    let chat = api_format == ModelApiFormat::OpenAiChatCompletions;
    let call_id = ToolCallId::new();
    let tool_id = ToolId::from_bytes([2; 16]);
    let wire = ProviderWireToolCallId::from_tool_call_id(call_id);
    let wire_call_id = wire.as_str().to_owned();
    let snapshot = ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Compatible Provider".to_owned(),
        endpoint_display_name: "fixture".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Compatible Provider / fixture / fixture-model".to_owned(),
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
            effective_max_output_tokens: 512,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: true,
        },
        historical_tool_call_wire_bindings: vec![ProviderToolCallWireBinding {
            tool_call_id: call_id,
            tool_id: Some(tool_id),
            provider_wire_tool_name: "browser_screenshot".to_owned(),
            wire_identity: ProviderToolCallWireIdentity::AuraGenerated {
                provider_wire_tool_call_id: wire,
            },
        }],
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::blake3(b"fixture-endpoint"),
        created_at: UtcTimestamp::now(),
    };
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
    let canonical = blocks.iter().map(|block| block.content.as_str()).collect::<Vec<_>>();
    let instruction_envelope =
        CompiledInstructionEnvelope::new(1, blocks.clone(), canonical.join("\n"))
            .expect("instruction envelope");
    let tool = ToolSchemaSnapshotV1::new(
        tool_id,
        ToolVersion::new(None, aura_domain_common::Revision::INITIAL).expect("tool version"),
        "browser.screenshot".parse().expect("canonical tool name"),
        "Take a screenshot of the page".to_owned(),
        ToolInputSchemaV1::new(
            json!({"type": "object", "properties": {}, "additionalProperties": false}),
        )
        .expect("tool input schema"),
        true,
    )
    .expect("tool schema snapshot");
    let tool_schema_set = ToolSchemaSetSnapshotV1::new(
        ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: aura_domain_common::Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"l13-registry"),
        },
        vec![tool],
    )
    .expect("schema set");
    let (user_event, result_event) = (EventId::new(), EventId::new());
    let result = r#"{"url":"http://127.0.0.1:9/game","shown_to_model":true}"#;
    let request = LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id: snapshot.model_step_id,
        execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        instruction_envelope,
        conversation_items: vec![
            LogicalConversationItem::User {
                source_event_id: user_event,
                content: "test the game".to_owned(),
                content_hash: ContentHash::sha256(b"test the game"),
            },
            LogicalConversationItem::Assistant {
                source_event_id: EventId::new(),
                content: None,
                content_hash: None,
                tool_calls: vec![LogicalHistoricalToolCall {
                    tool_call_id: call_id,
                    tool_id: Some(tool_id),
                    canonical_name: "browser.screenshot".to_owned(),
                    arguments_hash: ContentHash::sha256(b"{}"),
                    arguments_json: "{}".to_owned(),
                }],
                provider_history: None,
            },
            LogicalConversationItem::ToolResult {
                source_event_id: result_event,
                tool_call_id: call_id,
                tool_name: "browser.screenshot".to_owned(),
                content: result.to_owned(),
                content_hash: ContentHash::sha256(result.as_bytes()),
                is_error: false,
            },
        ],
        tool_schema_set,
        attachments: vec![],
        provider_continuation: None,
        output_request: ModelOutputRequest {
            format: ModelOutputFormat::Text,
            effective_max_output_tokens: 512,
        },
    };
    Fixture { request, snapshot, result_event, user_event, wire_call_id }
}

fn image(source_event_id: EventId, media_type: &str) -> ResolvedModelAttachment {
    ResolvedModelAttachment {
        snapshot: ModelAttachmentSnapshot {
            source_event_id,
            artifact_id: ArtifactId::new(),
            file_reference_id: None,
            display_name: "browser screenshot".to_owned(),
            media_type: media_type.to_owned(),
            byte_length: JPEG.len() as u64,
            content_hash: ContentHash::sha256(JPEG),
        },
        bytes: JPEG.into(),
    }
}

fn compile(
    fixture: &Fixture,
    images: &[ResolvedModelAttachment],
) -> Result<Value, ModelAdapterError> {
    let mut request = fixture.request.clone();
    request.attachments = images.iter().map(|image| image.snapshot.clone()).collect();
    let snapshot = &fixture.snapshot;
    let operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: snapshot.model_step_id,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: snapshot.context_snapshot_id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    let input = ModelRequestCompilationInput {
        request: &request,
        snapshot,
        resolved_attachments: images,
        operation_context: &operation_context,
        compatibility_removals: &std::collections::BTreeSet::new(),
        applied_output_token_field_alias: None,
    };
    let plan = match snapshot.api_format {
        ModelApiFormat::OpenAiChatCompletions => {
            aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter.compile_request(input)
        }
        ModelApiFormat::OpenAiResponses => {
            aura_model_adapter_openai_responses::OpenAiResponsesAdapter.compile_request(input)
        }
        ModelApiFormat::AnthropicMessages => {
            aura_model_adapter_anthropic::AnthropicMessagesAdapter.compile_request(input)
        }
    }?;
    Ok(serde_json::from_slice(&plan.body).expect("JSON body"))
}

#[test]
fn a_screenshot_goes_with_its_tool_result_in_each_format() {
    for format in FORMATS {
        let fixture = fixture(format);
        let body =
            compile(&fixture, &[image(fixture.result_event, "image/jpeg")]).expect("compiled");
        let label =
            format!("The image browser.screenshot returned for call {}:", fixture.wire_call_id);
        match format {
            ModelApiFormat::OpenAiChatCompletions => {
                let messages = body["messages"].as_array().expect("messages");
                let at = messages.iter().position(|m| m["role"] == "tool").expect("tool message");
                assert_eq!(
                    messages[at]["content"],
                    fixture.request.conversation_items[2].clone().content(),
                    "{format:?}"
                );
                assert_eq!(messages[at + 1]["role"], "user");
                assert_eq!(
                    messages[at + 1]["content"],
                    json!([
                        {"type": "text", "text": label},
                        {"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{JPEG_BASE64}")}}
                    ])
                );
            }
            ModelApiFormat::OpenAiResponses => {
                let input = body["input"].as_array().expect("input");
                let at = input
                    .iter()
                    .position(|item| item["type"] == "function_call_output")
                    .expect("function output");
                assert_eq!(input[at + 1]["role"], "user");
                assert_eq!(
                    input[at + 1]["content"],
                    json!([
                        {"type": "input_text", "text": label},
                        {"type": "input_image", "image_url": format!("data:image/jpeg;base64,{JPEG_BASE64}")}
                    ])
                );
            }
            ModelApiFormat::AnthropicMessages => {
                let messages = body["messages"].as_array().expect("messages");
                let result = messages
                    .iter()
                    .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
                    .find(|block| block["type"] == "tool_result")
                    .expect("tool result");
                assert_eq!(result["content"][0]["type"], "text");
                assert_eq!(
                    result["content"][1],
                    json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": JPEG_BASE64}})
                );
            }
        }
    }
}

/// A picture the user sent with a message goes
/// with that message, after its text.
#[test]
fn a_picture_sent_with_a_user_message_goes_with_that_message() {
    let data_url = format!("data:image/jpeg;base64,{JPEG_BASE64}");
    for format in FORMATS {
        let fixture = fixture(format);
        let body = compile(&fixture, &[image(fixture.user_event, "image/jpeg")]).expect("sent");
        let first = match format {
            ModelApiFormat::OpenAiResponses => body["input"][0].clone(),
            _ => body["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .find(|m| m["role"] == "user")
                .cloned()
                .expect("user"),
        };
        let expected = match format {
            ModelApiFormat::OpenAiChatCompletions => json!([
                {"type": "text", "text": "test the game"},
                {"type": "text", "text": "Picture `browser screenshot` attached by the user:"},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]),
            ModelApiFormat::OpenAiResponses => json!([
                {"type": "input_text", "text": "test the game"},
                {"type": "input_text", "text": "Picture `browser screenshot` attached by the user:"},
                {"type": "input_image", "image_url": data_url}
            ]),
            ModelApiFormat::AnthropicMessages => json!([
                {"type": "text", "text": "test the game"},
                {"type": "text", "text": "Picture `browser screenshot` attached by the user:"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": JPEG_BASE64}}
            ]),
        };
        assert_eq!(first["role"], "user", "{format:?}");
        assert_eq!(first["content"], expected, "{format:?}");
    }
}

#[test]
fn an_image_elsewhere_or_a_file_that_is_not_one_is_refused() {
    for format in FORMATS {
        let fixture = fixture(format);
        let unplaced =
            compile(&fixture, &[image(EventId::new(), "image/jpeg")]).expect_err("refused");
        assert_eq!(unplaced.code, "model_attachment.unplaced", "{format:?}");
        let pdf = compile(&fixture, &[image(fixture.result_event, "application/pdf")])
            .expect_err("refused");
        assert_eq!(pdf.code, "model_attachment.media_unsupported", "{format:?}");
    }
}

#[test]
fn a_request_without_images_is_sent_as_before() {
    for format in FORMATS {
        let fixture = fixture(format);
        let body = compile(&fixture, &[]).expect("compiled").to_string();
        assert!(!body.contains("image"), "{format:?}: {body}");
    }
}

trait Content {
    fn content(self) -> Value;
}
impl Content for LogicalConversationItem {
    fn content(self) -> Value {
        match self {
            LogicalConversationItem::ToolResult { content, .. } => json!(content),
            _ => Value::Null,
        }
    }
}
