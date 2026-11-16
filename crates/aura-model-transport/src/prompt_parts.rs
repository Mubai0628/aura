//! Prompt-bearing structures shared by request rendering and context budgeting.
use std::{borrow::Cow, collections::BTreeMap};

use aura_domain_common::{ToolCallId, ToolId};
use aura_model_domain::{
    LogicalHistoricalToolCall, LogicalProviderHistory, ModelApiFormat, ModelExecutionSnapshot,
    ProviderHistoryData, ProviderToolCallWireBinding, ProviderToolNameBinding,
    provider_arguments_envelope_schema_v3,
};
use aura_tool_contract::ToolSchemaSetSnapshotV1;
use serde_json::{Value, json};

use crate::{ModelAdapterError, store_off, strict_schema_compatible};

pub struct ProviderTools {
    bindings: Vec<ProviderToolNameBinding>,
    definitions: Vec<(ToolId, Value)>,
}

impl ProviderTools {
    pub fn compile(
        schemas: &ToolSchemaSetSnapshotV1,
        format: ModelApiFormat,
    ) -> Result<Self, ModelAdapterError> {
        let mut bindings = Vec::with_capacity(schemas.tools.len());
        let mut definitions = Vec::with_capacity(schemas.tools.len());
        for tool in &schemas.tools {
            let (schema, hash) = provider_arguments_envelope_schema_v3(&tool.input_schema)
                .map_err(|error| {
                    ModelAdapterError::invalid_request(
                        "model_request.tool_schema_envelope_invalid",
                        format!("provider tool schema envelope is invalid: {error}"),
                    )
                })?;
            let binding =
                ProviderToolNameBinding::from_snapshot(&schemas.registry_snapshot_ref, tool, hash);
            binding.validate_contract_shape().map_err(|error| {
                ModelAdapterError::invalid_request(
                    "model_request.tool_binding_invalid",
                    format!("provider tool name binding is invalid: {error}"),
                )
            })?;
            let mut definition = json!({
                "name": binding.provider_wire_name,
                "description": tool.description,
            });
            if tool.strict && strict_schema_compatible(&schema) {
                definition["strict"] = Value::Bool(true);
            }
            match format {
                ModelApiFormat::OpenAiResponses => {
                    definition["type"] = Value::from("function");
                    definition["parameters"] = schema;
                }
                ModelApiFormat::OpenAiChatCompletions => {
                    definition["parameters"] = schema;
                    definition = json!({"type": "function", "function": definition});
                }
                ModelApiFormat::AnthropicMessages => definition["input_schema"] = schema,
            }
            bindings.push(binding);
            definitions.push((tool.tool_id, definition));
        }
        Ok(Self { bindings, definitions })
    }

    pub fn definitions(&self) -> impl Iterator<Item = &Value> {
        self.definitions.iter().map(|(_, definition)| definition)
    }

    pub fn into_wire(
        self,
        fixed_tool_ids: &[ToolId],
    ) -> (Vec<Value>, Vec<ProviderToolNameBinding>) {
        let mut definitions = self.definitions;
        definitions.sort_by_key(|(id, _)| (!fixed_tool_ids.contains(id), *id));
        (definitions.into_iter().map(|(_, definition)| definition).collect(), self.bindings)
    }
}

pub fn provider_tool_call(
    format: ModelApiFormat,
    call: &LogicalHistoricalToolCall,
    bindings: &BTreeMap<ToolCallId, &ProviderToolCallWireBinding>,
) -> Result<Value, ModelAdapterError> {
    let binding = bindings.get(&call.tool_call_id).ok_or_else(|| {
        ModelAdapterError::invalid_request(
            "model_request.tool_call_mapping_not_resolved",
            "historical tool call has no frozen wire binding",
        )
    })?;
    let id = binding.wire_identity.wire_id();
    let name = &binding.provider_wire_tool_name;
    Ok(match format {
        ModelApiFormat::OpenAiResponses => json!({
            "type": "function_call", "call_id": id, "name": name, "arguments": call.arguments_json,
        }),
        ModelApiFormat::OpenAiChatCompletions => json!({
            "type": "function", "id": id,
            "function": { "name": name, "arguments": call.arguments_json },
        }),
        ModelApiFormat::AnthropicMessages => {
            let arguments: Value = serde_json::from_str(&call.arguments_json).map_err(|_| {
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
            json!({"type": "tool_use", "id": id, "name": name, "input": arguments})
        }
    })
}

/// Applies the endpoint's replay rules before either budgeting or rendering a native record.
pub fn provider_history_replay<'a>(
    history: &'a LogicalProviderHistory,
    snapshot: &ModelExecutionSnapshot,
) -> Option<Cow<'a, ProviderHistoryData>> {
    let data = history.for_snapshot(snapshot)?;
    if let ProviderHistoryData::Responses { reasoning_items } = data
        && store_off(snapshot)
    {
        let reasoning_items: Vec<_> =
            reasoning_items.iter().filter_map(unstored_reasoning).collect();
        return (!reasoning_items.is_empty())
            .then_some(Cow::Owned(ProviderHistoryData::Responses { reasoning_items }));
    }
    Some(Cow::Borrowed(data))
}

fn unstored_reasoning(item: &Value) -> Option<Value> {
    // A provider with storage disabled can only replay encrypted state. There is no stored ID.
    let encrypted = item.get("encrypted_content").and_then(Value::as_str);
    if encrypted.is_none_or(str::is_empty) {
        return None;
    }
    let mut item = item.clone();
    item.as_object_mut()?.remove("id");
    Some(item)
}

/// Visible reasoning and opaque payload lengths, as distinct inputs to the token estimator.
/// Signatures accompanying visible thinking are authentication metadata.
pub fn provider_reasoning_content(history: &ProviderHistoryData) -> (Vec<&str>, u64) {
    fn length(text: &Value) -> u64 {
        text.as_str().map_or(0, |value| value.len() as u64)
    }

    match history {
        ProviderHistoryData::Chat { reasoning_content, thought_signatures } => (
            reasoning_content.as_deref().into_iter().collect(),
            if reasoning_content.is_some() {
                0
            } else {
                thought_signatures.values().map(|s| s.len() as u64).sum()
            },
        ),
        ProviderHistoryData::Responses { reasoning_items } => (
            reasoning_items
                .iter()
                .flat_map(|item| item["summary"].as_array().into_iter().flatten())
                .filter_map(|piece| piece["text"].as_str())
                .collect(),
            reasoning_items.iter().map(|item| length(&item["encrypted_content"])).sum(),
        ),
        ProviderHistoryData::Anthropic { thinking_blocks } => (
            thinking_blocks.iter().filter_map(|block| block["thinking"].as_str()).collect(),
            thinking_blocks.iter().map(|block| length(&block["data"])).sum(),
        ),
    }
}
