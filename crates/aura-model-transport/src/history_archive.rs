//! Readable results for a new DeepSeek chain whose older assistant state cannot be replayed.
use crate::provider_history_replay;
use aura_model_domain::{
    CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION, CompiledInstructionEnvelope,
    LogicalConversationItem, ModelApiFormat, ModelExecutionSnapshot, ProviderHistoryData,
};
use aura_tool_contract::ToolSchemaSetSnapshotV1;
use serde_json::json;

/// Budgeting and both OpenAI-shaped adapters use the same history representation decision.
#[must_use]
pub fn uses_deepseek_history_archive(
    snapshot: &ModelExecutionSnapshot,
    envelope: &CompiledInstructionEnvelope,
    schemas: &ToolSchemaSetSnapshotV1,
) -> bool {
    envelope.serializer_version == CHECKPOINT_INSTRUCTION_SERIALIZER_VERSION
        && !schemas.tools.is_empty()
        && deepseek_history_requires_reasoning(snapshot)
}

/// Only the upstream DeepSeek protocol and the known OpenCode DeepSeek Chat path require this
/// projection. The normal paired wire form remains available whenever reasoning can be reused.
#[must_use]
pub fn deepseek_history_requires_reasoning(snapshot: &ModelExecutionSnapshot) -> bool {
    let Ok(url) = url::Url::parse(&snapshot.resolved_request_url) else { return false };
    snapshot.model_name.starts_with("deepseek-")
        && snapshot.request_options.reasoning_effort
            != Some(aura_model_domain::ReasoningEffort::Off)
        && match snapshot.api_format {
            ModelApiFormat::OpenAiChatCompletions => {
                matches!(url.host_str(), Some("api.deepseek.com" | "opencode.ai"))
            }
            ModelApiFormat::OpenAiResponses => url.host_str() == Some("api.deepseek.com"),
            ModelApiFormat::AnthropicMessages => false,
        }
}

/// Projects a closed response group as historical data. Do not synthesize reasoning, a new user
/// request, or a tool result for a call that does not have a verified result.
#[must_use]
pub fn archived_deepseek_group(
    items: &[LogicalConversationItem],
    snapshot: &ModelExecutionSnapshot,
) -> Option<(usize, String)> {
    let (
        LogicalConversationItem::Assistant {
            source_event_id,
            content,
            tool_calls,
            provider_history,
            ..
        },
        rest,
    ) = items.split_first()?
    else {
        return None;
    };
    let replay =
        provider_history.as_ref().and_then(|history| provider_history_replay(history, snapshot));
    if match replay.as_deref() {
        Some(ProviderHistoryData::Chat { reasoning_content: Some(text), .. }) => !text.is_empty(),
        Some(ProviderHistoryData::Responses { reasoning_items }) => !reasoning_items.is_empty(),
        _ => false,
    } {
        return None;
    }
    let results = rest.get(..tool_calls.len())?;
    let receipts = tool_calls
        .iter()
        .zip(results)
        .map(|(call, result)| {
            let LogicalConversationItem::ToolResult {
                source_event_id,
                tool_call_id,
                tool_name,
                content,
                is_error,
                ..
            } = result
            else {
                return None;
            };
            (*tool_call_id == call.tool_call_id).then(|| {
                json!({
                    "source_event": source_event_id, "call_id": tool_call_id, "tool": tool_name,
                    "tool_id": call.tool_id, "called_tool": call.canonical_name,
                    "arguments_json": call.arguments_json,
                    "result": content, "error": is_error,
                })
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let archive = json!({"historical_assistant_event":source_event_id, "reply":content,"completed_tool_receipts":receipts});
    Some((
        1 + results.len(),
        format!(
            "Aura historical data. The recorded assistant reply and tool receipts below are evidence from completed work.\n{archive}"
        ),
    ))
}
