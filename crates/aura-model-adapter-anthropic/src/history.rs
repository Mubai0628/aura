//! Protocol history mapping, including endpoint-bound provider continuation.
use super::*;

/// Cache the stable history before the changing current-state block is appended.
/// Signed/redacted thinking is retained verbatim and never marked directly.
pub(super) fn mark_cacheable_tail(messages: &mut [Value]) {
    for message in messages.iter_mut().rev() {
        if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
            for block in blocks.iter_mut().rev() {
                if matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("text" | "tool_use" | "tool_result")
                ) {
                    block["cache_control"] = json!({"type":"ephemeral"});
                    return;
                }
            }
        }
    }
}

/// A tool result carries the images its tool returned beside its text. `items` are the request's
/// conversation, or what followed its closing question.
pub(super) fn compile_anthropic_history(
    request: &aura_model_domain::LogicalModelRequest,
    items: &[LogicalConversationItem],
    snapshot: &ModelExecutionSnapshot,
    resolved: &[aura_model_domain::ResolvedModelAttachment],
) -> Result<Vec<Value>, ModelAdapterError> {
    let bindings = historical_tool_call_wire_bindings(snapshot);
    let images = aura_model_transport::tool_result_images(request, resolved)?;
    let mut messages = Vec::with_capacity(items.len());
    let mut pending_tool_results = Vec::new();
    for item in items {
        match item {
            // Messages rejects consecutive user roles. Text that follows a tool
            // result shares that tool_result user message; other user messages
            // stay separate.
            LogicalConversationItem::User { source_event_id, content, .. } => {
                // A message's pictures follow its text.
                let mut user_blocks = vec![json!({"type": "text", "text": content})];
                for image in images.get(source_event_id).into_iter().flatten() {
                    user_blocks.push(json!({"type": "text", "text": image.user_image_label()}));
                    user_blocks.push(json!({"type": "image", "source": {
                        "type": "base64",
                        "media_type": image.media_type,
                        "data": image.data_base64,
                    }}));
                }
                if !pending_tool_results.is_empty() {
                    pending_tool_results.extend(user_blocks);
                } else if messages.last().is_some_and(|message: &Value| {
                    message["role"] == "user"
                        && message["content"].as_array().is_some_and(|blocks| {
                            blocks.iter().any(|block| block["type"] == "tool_result")
                        })
                }) {
                    let Some(blocks) =
                        messages.last_mut().and_then(|message| message["content"].as_array_mut())
                    else {
                        return Err(ModelAdapterError::invalid_request(
                            "model_request.anthropic_history_invalid",
                            "Anthropic tool result history is not a user content array",
                        ));
                    };
                    blocks.extend(user_blocks);
                } else {
                    messages.push(json!({"role": "user", "content": user_blocks}));
                }
            }
            LogicalConversationItem::Assistant {
                content, tool_calls, provider_history, ..
            } => {
                if !pending_tool_results.is_empty() {
                    messages.push(json!({
                        "role": "user",
                        "content": std::mem::take(&mut pending_tool_results),
                    }));
                }
                if content.is_none() && tool_calls.is_empty() {
                    return Err(ModelAdapterError::invalid_request(
                        "model_request.empty_assistant_history",
                        "assistant history item has neither visible content nor a tool call",
                    ));
                }
                let mut blocks =
                    Vec::with_capacity(usize::from(content.is_some()) + tool_calls.len());
                if let Some(aura_model_domain::ProviderHistoryData::Anthropic { thinking_blocks }) =
                    provider_history.as_ref().and_then(|h| h.for_snapshot(snapshot))
                {
                    blocks.extend(thinking_blocks.clone());
                }
                if let Some(content) = content {
                    blocks.push(json!({"type": "text", "text": content}));
                }
                for call in tool_calls {
                    let binding = bindings.get(&call.tool_call_id).ok_or_else(|| {
                        ModelAdapterError::invalid_request(
                            "model_request.tool_call_mapping_not_resolved",
                            "historical Anthropic tool call has no frozen wire binding",
                        )
                    })?;
                    let arguments: Value =
                        serde_json::from_str(&call.arguments_json).map_err(|_| {
                            ModelAdapterError::invalid_request(
                                "model_request.tool_arguments_invalid",
                                "historical Anthropic tool arguments are not valid JSON",
                            )
                        })?;
                    if !arguments.is_object() {
                        return Err(ModelAdapterError::compatibility(
                            "model_request.anthropic_tool_input_not_object",
                            "Anthropic tool_use input requires a JSON object",
                        ));
                    }
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": binding.wire_identity.wire_id(),
                        "name": binding.provider_wire_tool_name,
                        "input": arguments,
                    }));
                }
                messages.push(json!({"role": "assistant", "content": blocks}));
            }
            LogicalConversationItem::ToolResult {
                source_event_id,
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let binding = bindings.get(tool_call_id).ok_or_else(|| {
                    ModelAdapterError::invalid_request(
                        "model_request.tool_call_mapping_not_resolved",
                        "historical Anthropic tool result has no frozen wire binding",
                    )
                })?;
                let content = match images.get(source_event_id) {
                    None => json!(content),
                    Some(list) => {
                        let mut blocks = vec![json!({"type": "text", "text": content})];
                        blocks.extend(list.iter().map(|image| {
                            json!({"type": "image", "source": {
                                "type": "base64",
                                "media_type": image.media_type,
                                "data": image.data_base64,
                            }})
                        }));
                        Value::Array(blocks)
                    }
                };
                pending_tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": binding.wire_identity.wire_id(),
                    "content": content,
                    "is_error": is_error,
                }));
            }
        }
    }
    if !pending_tool_results.is_empty() {
        messages.push(json!({"role": "user", "content": pending_tool_results}));
    }
    Ok(messages)
}
