//! Protocol history mapping, including endpoint-bound provider continuation. An image a tool
//! returned follows the run of function outputs as a user message.
use super::*;

/// `items` are the request's conversation, or what followed its closing question.
pub(super) fn compile_responses_history(
    request: &aura_model_domain::LogicalModelRequest,
    items: &[LogicalConversationItem],
    snapshot: &ModelExecutionSnapshot,
    resolved: &[aura_model_domain::ResolvedModelAttachment],
) -> Result<Vec<Value>, ModelAdapterError> {
    let bindings = historical_tool_call_wire_bindings(snapshot);
    let images = aura_model_transport::tool_result_images(request, resolved)?;
    let mut input = Vec::with_capacity(items.len());
    let mut pending_images = Vec::new();
    for item in items {
        if !matches!(item, LogicalConversationItem::ToolResult { .. }) && !pending_images.is_empty()
        {
            input.push(json!({"role": "user", "content": std::mem::take(&mut pending_images)}));
        }
        match item {
            // A message's pictures follow its text.
            LogicalConversationItem::User { source_event_id, content, .. } => {
                let mut parts = vec![json!({"type": "input_text", "text": content})];
                for image in images.get(source_event_id).into_iter().flatten() {
                    parts.push(json!({"type": "input_text", "text": image.user_image_label()}));
                    parts.push(json!({"type": "input_image", "image_url": image.data_url()}));
                }
                input.push(json!({"role": "user", "content": parts}));
            }
            LogicalConversationItem::Assistant {
                content, tool_calls, provider_history, ..
            } => {
                if content.is_none() && tool_calls.is_empty() {
                    return Err(ModelAdapterError::invalid_request(
                        "model_request.empty_assistant_history",
                        "assistant history item has neither visible content nor a tool call",
                    ));
                }
                if let Some(aura_model_domain::ProviderHistoryData::Responses { reasoning_items }) =
                    provider_history.as_ref().and_then(|h| h.for_snapshot(snapshot))
                {
                    input.extend(reasoning_items.clone());
                }
                if let Some(content) = content {
                    input.push(json!({
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": content}],
                    }));
                }
                for call in tool_calls {
                    let binding = bindings.get(&call.tool_call_id).ok_or_else(|| {
                        ModelAdapterError::invalid_request(
                            "model_request.tool_call_mapping_not_resolved",
                            "historical Responses function call has no frozen wire binding",
                        )
                    })?;
                    input.push(json!({
                        "type": "function_call",
                        "call_id": binding.wire_identity.wire_id(),
                        "name": binding.provider_wire_tool_name,
                        "arguments": call.arguments_json,
                    }));
                }
            }
            LogicalConversationItem::ToolResult {
                source_event_id,
                tool_call_id,
                tool_name,
                content,
                ..
            } => {
                let binding = bindings.get(tool_call_id).ok_or_else(|| {
                    ModelAdapterError::invalid_request(
                        "model_request.tool_call_mapping_not_resolved",
                        "historical Responses function result has no frozen wire binding",
                    )
                })?;
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": binding.wire_identity.wire_id(),
                    "output": content,
                }));
                for image in images.get(source_event_id).into_iter().flatten() {
                    let label = aura_model_transport::tool_result_image_label(
                        tool_name,
                        binding.wire_identity.wire_id(),
                    );
                    pending_images.push(json!({"type": "input_text", "text": label}));
                    pending_images
                        .push(json!({"type": "input_image", "image_url": image.data_url()}));
                }
            }
        }
    }
    if !pending_images.is_empty() {
        input.push(json!({"role": "user", "content": pending_images}));
    }
    Ok(input)
}
