//! Token counts, read from the chunk or from its choice: services put them in either place.
use aura_model_domain::PartialModelUsage;
use serde_json::Value;

pub(super) fn parse_usage(value: &Value) -> PartialModelUsage {
    PartialModelUsage {
        input_tokens: value.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: value.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: value
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        cached_input_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .or_else(|| value.get("prompt_cache_hit_tokens").and_then(Value::as_u64)),
        cache_write_tokens: None,
    }
}
