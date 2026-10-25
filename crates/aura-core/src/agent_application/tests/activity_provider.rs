//! Actual loopback Provider response bytes for the three configured adapter protocols.
use super::*;
use tokio::io::AsyncWriteExt;

pub(super) async fn respond(
    stream: &mut tokio::net::TcpStream,
    format: ModelApiFormat,
    step: usize,
    body: &Value,
) {
    let release = aura_tool_registry::agent_release::release()
        .expect("valid local activity Provider fixture");
    let target =
        release.entry("agent.activity.query").expect("valid local activity Provider fixture");
    // A tool loaded during a turn joins the request's list at the next turn, so this turn
    // reaches the query through the unified entry.
    let name = match step {
        0 => "tools.search",
        1 => "tools.load",
        _ => "tools.call",
    };
    let entry = release.entry(name).expect("valid local activity Provider fixture");
    let wire = provider_wire_tool_name(name, entry.definition.tool_id);
    let tools = body["tools"].as_array().expect("the real request exposes tools");
    assert!(tools.iter().any(|tool| tool["name"] == wire || tool["function"]["name"] == wire));
    let arguments = match step {
        0 => json!({"query":"agent.activity.query","limit":5,"cursor":null}),
        1 => json!({"tool_ids":[target.definition.tool_id],"replace":[]}),
        _ => json!({"tool":"agent.activity.query","arguments":{"by":"session","limit":20}}),
    };
    let args = super::provider_fixture::flat_call(
        "Inspect running tools without controlling execution",
        arguments,
    );
    let call_id = if step == 2 {
        "activity_call".to_owned()
    } else {
        format!("activity_discovery_call_{step}")
    };
    let text = if step == 3 {
        "The query is recorded; this bounded intermediate reply reached its output limit."
    } else {
        "The read-only activity query completed; no file was changed."
    };
    let calls = step <= 2;
    let mut events = Vec::new();
    match format {
        ModelApiFormat::OpenAiChatCompletions => {
            let delta = if calls {
                json!({"role":"assistant","tool_calls":[{"index":0,"id":call_id,"type":"function","function":{"name":wire,"arguments":args}}]})
            } else {
                json!({"role":"assistant","content":text})
            };
            let finish = if calls {
                "tool_calls"
            } else if step == 3 {
                "length"
            } else {
                "stop"
            };
            events.push(json!({"id":format!("activity-{step}"),"object":"chat.completion.chunk","model":"fixture",
                "choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}}));
        }
        ModelApiFormat::OpenAiResponses => {
            events.push(
                json!({"type":"response.created","response":{"id":format!("activity-{step}")}}),
            );
            if calls {
                events.push(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"activity_item","call_id":call_id,"name":wire}}));
                events.push(json!({"type":"response.function_call_arguments.delta","item_id":"activity_item","output_index":0,"delta":args}));
                events.push(json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"activity_item","call_id":call_id,"name":wire,"arguments":args}}));
            } else if !text.is_empty() {
                events.push(json!({"type":"response.output_text.delta","item_id":"activity_message","output_index":0,"content_index":0,"delta":text}));
            }
            if step == 3 {
                events.push(json!({"type":"response.incomplete","response":{"id":format!("activity-{step}"),"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":20,"output_tokens":10,"total_tokens":30}}}));
            } else {
                events.push(json!({"type":"response.completed","response":{"id":format!("activity-{step}"),"usage":{"input_tokens":20,"output_tokens":10,"total_tokens":30}}}));
            }
        }
        ModelApiFormat::AnthropicMessages => {
            events.push(json!({"type":"message_start","message":{"id":format!("activity-{step}"),"usage":{"input_tokens":20}}}));
            if calls {
                events.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":call_id,"name":wire,"input":{}}}));
                events.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args}}));
                events.push(json!({"type":"content_block_stop","index":0}));
            } else if !text.is_empty() {
                events.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":text}}));
                events.push(json!({"type":"content_block_stop","index":0}));
            }
            events.push(json!({"type":"message_delta","delta":{"stop_reason":if calls {"tool_use"} else if step==3 {"max_tokens"} else {"end_turn"},"stop_sequence":null},"usage":{"output_tokens":10}}));
            events.push(json!({"type":"message_stop"}));
        }
    }
    let mut content = String::new();
    for event in events {
        if let Some(kind) = event["type"].as_str() {
            content.push_str(&format!("event: {kind}\n"));
        }
        content.push_str(&format!("data: {event}\n\n"));
    }
    if format != ModelApiFormat::AnthropicMessages {
        content.push_str("data: [DONE]\n\n");
    }
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        content.len()
    );
    stream.write_all(head.as_bytes()).await.expect("valid local activity Provider fixture");
    stream.write_all(content.as_bytes()).await.expect("valid local activity Provider fixture");
}

/// A valid startup configuration is a normal product input. These wire tests do not test
/// configuration edits, and do not replace the existing model-configuration route tests.
pub(super) fn configure_startup(
    host: &crate::r03_tests::Fixture,
    url: &str,
    format: ModelApiFormat,
) -> ModelProfileId {
    use aura_domain_common::{EndpointId, ProviderId, Revision};
    use aura_model_domain::*;
    use std::collections::BTreeMap;
    let provider_id = ProviderId::new();
    let endpoint_id = EndpointId::new();
    let model_id = ModelProfileId::new();
    let now = UtcTimestamp::now();
    let chat = format == ModelApiFormat::OpenAiChatCompletions;
    let provider = ModelProviderConfig {
        id: provider_id,
        display_name: "Local wire fixture".into(),
        slug: "wire-fixture".parse().expect("valid local activity Provider fixture"),
        source: ModelProviderSource::Custom { catalog_provider_id: None },
        endpoint_order: vec![endpoint_id],
        created_at: now,
        updated_at: now,
        default_endpoint_id: None,
        backup_endpoint_ids: vec![],
    };
    let endpoint = ModelEndpointConfig {
        id: endpoint_id,
        revision: Revision::INITIAL,
        provider_id,
        display_name: "Local HTTP".into(),
        slug: "local-http".parse().expect("valid local activity Provider fixture"),
        api_format: format,
        base_url: url.into(),
        discovery_mode: ModelDiscoveryMode::Disabled,
        chat_output_token_field: chat.then_some(ChatOutputTokenField::MaxTokens),
        chat_instruction_role: chat.then_some(ChatInstructionRole::System),
        auth_scheme: EndpointAuthScheme::None,
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        timeouts: ModelEndpointTimeouts {
            connect_ms: 5000,
            first_event_ms: 10000,
            stream_idle_ms: 10000,
            total_ms: Some(30000),
        },
        max_parallel_model_requests: 2,
        model_order: vec![model_id],
        compatibility: None,
        created_at: now,
        updated_at: now,
    };
    let model = ModelProfileConfig {
        id: model_id,
        revision: Revision::INITIAL,
        endpoint_id,
        provider_model_name: "fixture".into(),
        display_name: "Fixture".into(),
        notes: None,
        origin: ModelProfileOrigin::Manual,
        enabled: true,
        available_for_subagents: false,
        catalog: None,
        context: ModelContextWindowConfig {
            catalog_window_tokens: None,
            user_override_window_tokens: Some(256000),
            user_override_revision: 1,
            runtime_corrected_window_tokens: None,
            runtime_correction_basis: None,
            runtime_effective_window_tokens: 256000,
            historical_snapshot_max_tokens: 1024,
        },
        request_defaults: ModelRequestDefaults {
            max_output_tokens: Some(4096),
            ..Default::default()
        },
        created_at: now,
        updated_at: now,
    };
    let registry = ModelRegistryConfig {
        provider_order: vec![provider_id],
        providers: BTreeMap::from([(provider_id, provider)]),
        endpoints: BTreeMap::from([(endpoint_id, endpoint)]),
        models: BTreeMap::from([(model_id, model)]),
    };
    let document = aura_config::ModuleConfigDocument {
        schema_version: 1,
        revision: 1,
        values: serde_json::from_value(
            serde_json::to_value(registry).expect("valid local activity Provider fixture"),
        )
        .expect("valid local activity Provider fixture"),
    };
    let directory = host.root.path().join("config");
    std::fs::create_dir_all(&directory).expect("valid local activity Provider fixture");
    std::fs::write(
        directory.join("models.json"),
        serde_json::to_vec(&document).expect("valid local activity Provider fixture"),
    )
    .expect("valid local activity Provider fixture");
    model_id
}
