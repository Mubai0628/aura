use crate::{meter::values, owner::ApiFormat};
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) struct Answer {
    pub produced_function_call: bool,
    pub valid: bool,
    pub incomplete_reason: Option<String>,
}
pub(super) fn inspect(response: &[u8], schema: &Value, format: ApiFormat) -> Result<Answer> {
    let mut calls = BTreeMap::<String, String>::new();
    let mut incomplete_reason = None;
    for value in values(response)? {
        match format {
            ApiFormat::OpenAiChatCompletions => {
                if let Some(items) =
                    value.pointer("/choices/0/delta/tool_calls").and_then(Value::as_array)
                {
                    for item in items {
                        let key = item["index"].to_string();
                        let arguments = calls.entry(key).or_default();
                        if let Some(delta) =
                            item.pointer("/function/arguments").and_then(Value::as_str)
                        {
                            arguments.push_str(delta);
                        }
                    }
                }
                if value.pointer("/choices/0/finish_reason").and_then(Value::as_str)
                    == Some("length")
                {
                    incomplete_reason = Some("length".into());
                }
            }
            ApiFormat::OpenAiResponses => match value["type"].as_str() {
                Some("response.function_call_arguments.delta") => {
                    if let Some(id) = value["item_id"].as_str() {
                        calls
                            .entry(id.into())
                            .or_default()
                            .push_str(value["delta"].as_str().unwrap_or(""));
                    }
                }
                Some("response.function_call_arguments.done") => {
                    if let (Some(id), Some(arguments)) =
                        (value["item_id"].as_str(), value["arguments"].as_str())
                    {
                        calls.insert(id.into(), arguments.into());
                    }
                }
                Some("response.output_item.done") if value["item"]["type"] == "function_call" => {
                    let item = &value["item"];
                    if let Some(id) = item["id"].as_str().or_else(|| item["call_id"].as_str()) {
                        calls.insert(id.into(), item["arguments"].as_str().unwrap_or("").into());
                    }
                }
                Some("response.incomplete") => {
                    incomplete_reason = value
                        .pointer("/response/incomplete_details/reason")
                        .and_then(Value::as_str)
                        .map(|v| crate::meter::sanitized_text(v, 300));
                }
                _ => {}
            },
        }
    }
    let valid = if calls.len() == 1 {
        calls.values().next().and_then(|v| serde_json::from_str::<Value>(v).ok()).is_some_and(|v| {
            jsonschema::options()
                .offline()
                .build(schema)
                .is_ok_and(|validator| validator.is_valid(&v))
        })
    } else {
        false
    };
    Ok(Answer { produced_function_call: !calls.is_empty(), valid, incomplete_reason })
}

pub fn arguments_valid(response: &[u8], schema: &Value) -> Result<bool> {
    let format = if values(response)?
        .iter()
        .any(|v| v["type"].as_str().is_some_and(|v| v.starts_with("response.")))
    {
        ApiFormat::OpenAiResponses
    } else {
        ApiFormat::OpenAiChatCompletions
    };
    Ok(inspect(response, schema, format)?.valid)
}
