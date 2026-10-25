//! Protocol history mapping, including endpoint-bound provider continuation. A tool message holds
//! text only, so an image a tool returned follows the run of tool messages as a user message.
use super::*;

pub(super) fn compile_chat_history(
    request: &aura_model_domain::LogicalModelRequest,
    snapshot: &ModelExecutionSnapshot,
    resolved: &[aura_model_domain::ResolvedModelAttachment],
) -> Result<Vec<Value>, ModelAdapterError> {
    let bindings = historical_tool_call_wire_bindings(snapshot);
    let images = aura_model_transport::tool_result_images(request, resolved)?;
    let mut messages = Vec::with_capacity(request.conversation_items.len());
    let mut pending_images = Vec::new();
    for item in &request.conversation_items {
        if !matches!(item, LogicalConversationItem::ToolResult { .. }) && !pending_images.is_empty()
        {
            messages.push(json!({"role": "user", "content": std::mem::take(&mut pending_images)}));
        }
        let message = match item {
            // A message's pictures follow its text; one without them is sent as before.
            LogicalConversationItem::User { source_event_id, content, .. } => {
                match images.get(source_event_id) {
                    None => json!({"role": "user", "content": content}),
                    Some(list) => {
                        let mut parts = vec![json!({"type": "text", "text": content})];
                        for image in list {
                            parts.push(json!({"type": "text", "text": image.user_image_label()}));
                            parts.push(json!({"type": "image_url", "image_url": {"url": image.data_url()}}));
                        }
                        json!({"role": "user", "content": parts})
                    }
                }
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
                let mut assistant = Map::new();
                assistant.insert("role".to_owned(), Value::String("assistant".to_owned()));
                assistant.insert(
                    "content".to_owned(),
                    content.as_ref().map_or(Value::Null, |text| Value::String(text.clone())),
                );
                let history = provider_history.as_ref().and_then(|h| h.for_snapshot(snapshot));
                if let Some(aura_model_domain::ProviderHistoryData::Chat {
                    reasoning_content: Some(reasoning),
                    ..
                }) = history
                {
                    assistant.insert("reasoning_content".into(), json!(reasoning));
                }
                if !tool_calls.is_empty() {
                    let calls = tool_calls
                        .iter()
                        .map(|call| {
                            let binding = bindings.get(&call.tool_call_id).ok_or_else(|| {
                                ModelAdapterError::invalid_request(
                                    "model_request.tool_call_mapping_not_resolved",
                                    "historical Chat tool call has no frozen wire binding",
                                )
                            })?;
                            let mut value = json!({
                                "id": binding.wire_identity.wire_id(),
                                "type": "function",
                                "function": {
                                    "name": binding.provider_wire_tool_name,
                                    "arguments": call.arguments_json,
                                },
                            });
                            if let Some(aura_model_domain::ProviderHistoryData::Chat {
                                thought_signatures,
                                ..
                            }) = history
                                && let Some(signature) =
                                    thought_signatures.get(binding.wire_identity.wire_id())
                            {
                                value["extra_content"] =
                                    json!({"google":{"thought_signature":signature}});
                            }
                            Ok(value)
                        })
                        .collect::<Result<Vec<_>, ModelAdapterError>>()?;
                    assistant.insert("tool_calls".to_owned(), Value::Array(calls));
                }
                Value::Object(assistant)
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
                        "historical Chat tool result has no frozen wire binding",
                    )
                })?;
                for image in images.get(source_event_id).into_iter().flatten() {
                    let label = aura_model_transport::tool_result_image_label(
                        tool_name,
                        binding.wire_identity.wire_id(),
                    );
                    pending_images.push(json!({"type": "text", "text": label}));
                    pending_images
                        .push(json!({"type": "image_url", "image_url": {"url": image.data_url()}}));
                }
                json!({
                    "role": "tool",
                    "tool_call_id": binding.wire_identity.wire_id(),
                    "content": content,
                })
            }
        };
        messages.push(message);
    }
    if !pending_images.is_empty() {
        messages.push(json!({"role": "user", "content": pending_images}));
    }
    Ok(messages)
}
