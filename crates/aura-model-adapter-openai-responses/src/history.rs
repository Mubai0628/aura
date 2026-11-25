//! Protocol history mapping, including endpoint-bound provider continuation. An image a tool
//! returned follows the run of function outputs as a user message.
use super::*;

/// `items` are the request's conversation, or what followed its closing question.
pub(super) fn compile_responses_history(
    conversation: &[LogicalConversationItem],
    items: &[LogicalConversationItem],
    snapshot: &ModelExecutionSnapshot,
    resolved: &[aura_model_domain::ResolvedModelAttachment],
    archive_old: bool,
    mut updates: std::slice::Iter<'_, aura_model_domain::TaskStateUpdate>,
) -> Result<Vec<Value>, ModelAdapterError> {
    let bindings = historical_tool_call_wire_bindings(snapshot);
    let images = aura_model_transport::tool_result_images(conversation, resolved)?;
    let mut input = Vec::with_capacity(items.len());
    let mut pending_images = Vec::new();
    let mut archived_until = 0;
    let mut next_update = updates.next();
    for (index, item) in items.iter().enumerate() {
        if !matches!(item, LogicalConversationItem::ToolResult { .. }) && !pending_images.is_empty()
        {
            input.push(json!({"role": "user", "content": std::mem::take(&mut pending_images)}));
        }
        if archive_old
            && index >= archived_until
            && let Some((count, archive)) =
                aura_model_transport::archived_deepseek_group(&items[index..], snapshot)
        {
            input.push(json!({"role":"user","content":[{"type":"input_text","text":archive}]}));
            archived_until = index + count;
            continue;
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
                let replay = provider_history.as_ref().and_then(|history| {
                    aura_model_transport::provider_history_replay(history, snapshot)
                });
                if let Some(aura_model_domain::ProviderHistoryData::Responses { reasoning_items }) =
                    replay.as_deref()
                {
                    input.extend(reasoning_items.iter().cloned());
                }
                if let Some(content) = content {
                    input.push(json!({
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": content}],
                    }));
                }
                for call in tool_calls {
                    input.push(aura_model_transport::provider_tool_call(
                        snapshot.api_format,
                        call,
                        &bindings,
                    )?);
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
                if index >= archived_until {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": binding.wire_identity.wire_id(),
                        "output": content,
                    }));
                }
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
        if let Some(update) = next_update.filter(|update| update.after == item.source_event_id()) {
            if !pending_images.is_empty() {
                input.push(json!({"role": "user", "content": std::mem::take(&mut pending_images)}));
            }
            input.push(json!({
                "role": "developer",
                "content": [{"type": "input_text", "text": update.content}],
            }));
            next_update = updates.next();
        }
    }
    if !pending_images.is_empty() {
        input.push(json!({"role": "user", "content": pending_images}));
    }
    Ok(input)
}
