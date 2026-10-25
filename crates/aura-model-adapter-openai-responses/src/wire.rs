use super::*;

pub(super) fn usage_from_response(
    response: &Value,
) -> Result<PartialModelUsage, ModelAdapterError> {
    let usage = response.get("usage").ok_or_else(|| {
        ModelAdapterError::protocol(
            "openai_responses_usage_missing",
            "OpenAI Responses terminal event has no usage",
        )
    })?;
    Ok(PartialModelUsage {
        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_u64),
        reasoning_tokens: usage
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        cached_input_tokens: usage
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_write_tokens: None,
    })
}

pub(super) fn provider_stream_error(value: &Value) -> ModelAdapterError {
    let error = value.get("error").unwrap_or(value);
    ModelAdapterError::protocol(
        string_at(error, &["code"]).unwrap_or("openai_responses_stream_error"),
        string_at(error, &["message"]).unwrap_or("OpenAI Responses stream reported an error"),
    )
}

pub(super) fn has_native_attachments(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(has_native_attachments),
        Value::Object(object) => {
            object.get("type").and_then(Value::as_str).is_some_and(|kind| {
                matches!(kind, "input_image" | "input_file" | "computer_screenshot")
            }) || object.values().any(has_native_attachments)
        }
        _ => false,
    }
}

pub(super) fn required_string<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a str, ModelAdapterError> {
    required_nested_string(value, &[field], field)
}

pub(super) fn required_nested_string<'a>(
    value: &'a Value,
    path: &[&str],
    label: &str,
) -> Result<&'a str, ModelAdapterError> {
    string_at(value, path).filter(|value| !value.is_empty()).ok_or_else(|| {
        ModelAdapterError::protocol(
            "openai_responses_field_missing",
            format!("OpenAI Responses event is missing {label}"),
        )
    })
}

pub(super) fn required_u32(value: &Value, field: &str) -> Result<u32, ModelAdapterError> {
    value.get(field).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()).ok_or_else(
        || {
            ModelAdapterError::protocol(
                "openai_responses_field_missing",
                format!("OpenAI Responses event is missing valid {field}"),
            )
        },
    )
}

pub(super) fn string_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(value, |current, key| current.get(*key)).and_then(Value::as_str)
}
