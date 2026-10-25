use anyhow::Result;
use aura_model_adapter_anthropic::AnthropicMessagesAdapter;
use aura_model_adapter_openai_chat::OpenAiChatCompletionsAdapter;
use aura_model_adapter_openai_responses::OpenAiResponsesAdapter;
use aura_model_domain::{ModelApiFormat, TransportOperationContext};
use aura_model_transport::{ModelProtocolCompiler, ModelRequestCompilationInput};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Actual adapter compilation with all current tools, stable IDs, synthetic instructions and no credential.
pub fn current_requests() -> Result<BTreeMap<String, Value>> {
    let mut result = BTreeMap::new();
    let adapters: [(ModelApiFormat, &dyn ModelProtocolCompiler, &str, &str); 3] = [
        (
            ModelApiFormat::OpenAiChatCompletions,
            &OpenAiChatCompletionsAdapter,
            "openai-chat",
            "chat/completions",
        ),
        (ModelApiFormat::OpenAiResponses, &OpenAiResponsesAdapter, "openai-responses", "responses"),
        (ModelApiFormat::AnthropicMessages, &AnthropicMessagesAdapter, "anthropic", "messages"),
    ];
    for (format, adapter, name, path) in adapters {
        let mut snapshot = super::support::snapshot();
        snapshot.api_format = format;
        if format != ModelApiFormat::OpenAiChatCompletions {
            snapshot.chat_output_token_field = None;
            snapshot.chat_instruction_role = None;
        }
        snapshot.resolved_request_url = format!("https://api.example.test/v1/{path}");
        let request = super::support::logical_request(&snapshot);
        let context = TransportOperationContext::AgentModelAttempt {
            model_step_id: snapshot.model_step_id,
            model_execution_snapshot_id: snapshot.id,
            context_snapshot_id: snapshot.context_snapshot_id,
            prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
        };
        let plan = adapter.compile_request(ModelRequestCompilationInput {
            request: &request,
            snapshot: &snapshot,
            resolved_attachments: &[],
            operation_context: &context,
            compatibility_removals: &Default::default(),
            applied_output_token_field_alias: None,
        })?;
        result.insert(name.into(),json!({"method":plan.method,"url":plan.request_url,"body":serde_json::from_slice::<Value>(&plan.body)?}));
    }
    Ok(result)
}
