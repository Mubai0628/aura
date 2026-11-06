//! Tool calls arrive in fragments keyed by index: an ID and a name once, arguments in pieces.
use aura_model_domain::{ModelStreamEvent, ProviderToolCallId};
use aura_model_transport::ModelAdapterError;
use serde_json::Value;

use super::{ChatDecoder, state::PendingChatToolCall};

impl ChatDecoder {
    pub(super) fn apply_tool_delta(
        &mut self,
        value: &Value,
        events: &mut Vec<ModelStreamEvent>,
    ) -> Result<(), ModelAdapterError> {
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                ModelAdapterError::protocol(
                    "openai_chat_tool_index_missing",
                    "OpenAI Chat Completions tool delta has no valid index",
                )
            })?;
        let pending = self.tool_calls.entry(index).or_insert(PendingChatToolCall {
            provider_call_id: None,
            name: None,
            buffered_arguments: String::new(),
            started: false,
            completed: false,
            thought_signature: None,
        });
        if pending.completed {
            return Err(ModelAdapterError::protocol(
                "openai_chat_tool_completed",
                "tool delta arrived after the call completed",
            ));
        }
        if let Some(signature) =
            value.pointer("/extra_content/google/thought_signature").and_then(Value::as_str)
        {
            if pending.thought_signature.as_deref().is_some_and(|old| old != signature) {
                return Err(ModelAdapterError::protocol(
                    "openai_chat_thought_signature_changed",
                    "Thought signature changed within one call",
                ));
            }
            pending.thought_signature = Some(signature.to_owned());
        }
        if let Some(raw_id) = value.get("id").and_then(Value::as_str) {
            let id = ProviderToolCallId::new(raw_id).map_err(|_| {
                ModelAdapterError::protocol(
                    "provider_tool_call_id_invalid",
                    "OpenAI Chat Completions tool call ID is blank",
                )
            })?;
            if pending.provider_call_id.as_ref().is_some_and(|current| current != &id) {
                return Err(ModelAdapterError::protocol(
                    "openai_chat_tool_identity_changed",
                    "tool call ID changed for one choice/index",
                ));
            }
            if pending.provider_call_id.is_none() && !self.provider_tool_ids.insert(id.clone()) {
                return Err(ModelAdapterError::protocol(
                    "provider_tool_call_duplicate",
                    "OpenAI Chat Completions duplicated a provider tool call ID",
                ));
            }
            pending.provider_call_id = Some(id);
        }
        if let Some(function) = value.get("function") {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                if name.trim().is_empty() {
                    return Err(ModelAdapterError::protocol(
                        "provider_tool_name_missing",
                        "OpenAI Chat Completions tool name is blank",
                    ));
                }
                if pending.name.as_ref().is_some_and(|current| current != name) {
                    return Err(ModelAdapterError::protocol(
                        "openai_chat_tool_name_changed",
                        "tool name changed for one choice/index",
                    ));
                }
                pending.name = Some(name.to_owned());
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                pending.buffered_arguments.push_str(arguments);
            }
        }
        if !pending.started
            && let (Some(provider_call_id), Some(name)) =
                (pending.provider_call_id.clone(), pending.name.clone())
        {
            self.output.start_tool_call(provider_call_id.clone(), name.clone(), index)?;
            pending.started = true;
            events.push(ModelStreamEvent::ToolCallStarted {
                provider_call_id: provider_call_id.clone(),
                tool_name: name,
                position: index,
            });
            if !pending.buffered_arguments.is_empty() {
                self.output
                    .append_tool_arguments(&provider_call_id, &pending.buffered_arguments)?;
                events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                    provider_call_id,
                    delta: pending.buffered_arguments.clone(),
                });
            }
        } else if pending.started
            && let Some(arguments) = value
                .get("function")
                .and_then(|function| function.get("arguments"))
                .and_then(Value::as_str)
            && !arguments.is_empty()
        {
            let provider_call_id = pending.provider_call_id.as_ref().ok_or_else(|| {
                ModelAdapterError::protocol(
                    "provider_tool_call_id_missing",
                    "started tool call lost its provider ID",
                )
            })?;
            self.output.append_tool_arguments(provider_call_id, arguments)?;
            events.push(ModelStreamEvent::ToolCallArgumentsDelta {
                provider_call_id: provider_call_id.clone(),
                delta: arguments.to_owned(),
            });
        }
        Ok(())
    }

    pub(super) fn complete_tool_calls(
        &mut self,
        events: &mut Vec<ModelStreamEvent>,
    ) -> Result<(), ModelAdapterError> {
        for call in self.tool_calls.values_mut() {
            if !call.started {
                return Err(ModelAdapterError::protocol(
                    "provider_tool_call_id_missing",
                    "OpenAI Chat Completions response ended before a tool call had ID and name",
                ));
            }
            let provider_call_id = call.provider_call_id.as_ref().ok_or_else(|| {
                ModelAdapterError::protocol(
                    "provider_tool_call_id_missing",
                    "OpenAI Chat Completions tool call has no provider ID",
                )
            })?;
            self.output.complete_tool_call(provider_call_id)?;
            call.completed = true;
            events.push(ModelStreamEvent::ToolCallCompleted {
                provider_call_id: provider_call_id.clone(),
            });
        }
        Ok(())
    }
}
