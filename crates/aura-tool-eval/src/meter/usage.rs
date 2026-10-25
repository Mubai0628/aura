use crate::budget::Usage;
use crate::owner::ApiFormat;
use anyhow::{Result, ensure};
use serde_json::Value;

/// Duplicate identical usage frames are harmless; conflicting usage fails closed.
pub fn read_usage(body: &[u8]) -> Result<Usage> {
    let format = if super::values(body)?
        .iter()
        .any(|v| v["type"].as_str().is_some_and(|t| t.starts_with("response.")))
    {
        ApiFormat::OpenAiResponses
    } else {
        ApiFormat::OpenAiChatCompletions
    };
    read_usage_for(format, body)
}

pub fn read_usage_for(format: ApiFormat, body: &[u8]) -> Result<Usage> {
    let mut seen = None;
    for value in super::values(body)? {
        let usage = match format {
            ApiFormat::OpenAiResponses => {
                if !matches!(
                    value["type"].as_str(),
                    Some("response.completed" | "response.incomplete")
                ) {
                    continue;
                }
                value.pointer("/response/usage")
            }
            ApiFormat::OpenAiChatCompletions => value.get("usage"),
        };
        let Some(usage) = usage else {
            continue;
        };
        if usage.is_null() {
            continue;
        }
        let input = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(Value::as_u64);
        let output = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(Value::as_u64);
        let (Some(input), Some(output)) = (input, output) else {
            anyhow::bail!("eval.incomplete_usage_frame");
        };
        let cached_input = usage
            .get("prompt_cache_hit_tokens")
            .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
            .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
            .map(optional_number)
            .transpose()?
            .flatten();
        let total =
            input.checked_add(output).ok_or_else(|| anyhow::anyhow!("eval.usage_overflow"))?;
        if let Some(reported) = usage.get("total_tokens") {
            ensure!(reported.as_u64() == Some(total), "eval.conflicting_usage_total");
        }
        let reasoning_tokens = usage
            .pointer("/completion_tokens_details/reasoning_tokens")
            .or_else(|| usage.pointer("/output_tokens_details/reasoning_tokens"))
            .map(optional_number)
            .transpose()?
            .flatten();
        let cache_write_tokens =
            usage.get("cache_creation_input_tokens").map(optional_number).transpose()?.flatten();
        let current = Usage {
            input,
            output,
            cached_input: cached_input.unwrap_or(0),
            cached_input_reported: cached_input.is_some(),
            reasoning_tokens,
            cache_write_tokens,
        };
        ensure!(
            current.cached_input <= input
                && reasoning_tokens.is_none_or(|v| v <= output)
                && cache_write_tokens.is_none_or(|v| v <= input),
            "eval.invalid_cached_usage"
        );
        if let Some(previous) = seen {
            ensure!(previous == current, "eval.conflicting_usage_frames");
        }
        seen = Some(current);
    }
    seen.ok_or_else(|| anyhow::anyhow!("eval.usage_missing"))
}

fn optional_number(value: &Value) -> Result<Option<u64>> {
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(value.as_u64().ok_or_else(|| anyhow::anyhow!("eval.invalid_usage_number"))?))
}
