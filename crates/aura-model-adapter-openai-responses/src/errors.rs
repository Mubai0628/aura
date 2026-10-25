use super::*;

pub(super) fn classify_http_error(response: &ProviderErrorResponse) -> NormalizedModelFailure {
    let value = serde_json::from_slice::<Value>(&response.body).ok();
    let error = value.as_ref().and_then(|value| value.get("error")).or(value.as_ref());
    let code = error
        .and_then(|value| string_at(value, &["code"]).or_else(|| string_at(value, &["type"])))
        .unwrap_or("openai_responses_http_error")
        .to_owned();
    let message = error
        .and_then(|value| string_at(value, &["message"]))
        .unwrap_or("OpenAI Responses request failed")
        .to_owned();
    let lower = format!("{} {}", code.to_ascii_lowercase(), message.to_ascii_lowercase());
    let (category, mut retry_class) = classify_status(response.status, &lower, error);
    if let ModelRetryClass::TransparentRetry { retry_after_ms } = &mut retry_class {
        *retry_after_ms = retry_after_milliseconds(&response.headers);
    }
    NormalizedModelFailure {
        category,
        code,
        message,
        retry_class,
        provider_status: Some(response.status),
        provider_request_id: response.provider_request_id.clone(),
        provider_error_ref: response.raw_error_artifact_id,
        partial_output_ref: None,
        transport_attempt_id: None,
    }
}

pub(super) fn classify_status(
    status: u16,
    lower: &str,
    error: Option<&Value>,
) -> (ModelFailureCategory, ModelRetryClass) {
    if status == 401 {
        return (ModelFailureCategory::Authentication, ModelRetryClass::NotRetryable);
    }
    if status == 403 {
        return (ModelFailureCategory::Authorization, ModelRetryClass::NotRetryable);
    }
    if status == 429 {
        if contains_any(lower, &["quota", "balance", "billing", "credit"]) {
            return (ModelFailureCategory::QuotaOrBalance, ModelRetryClass::NotRetryable);
        }
        return (
            ModelFailureCategory::RateLimit,
            ModelRetryClass::TransparentRetry { retry_after_ms: None },
        );
    }
    if matches!(status, 408 | 500 | 502 | 503 | 504) {
        return (
            ModelFailureCategory::Transport,
            ModelRetryClass::TransparentRetry { retry_after_ms: None },
        );
    }
    if contains_any(lower, &["context_length", "context window", "too many tokens"]) {
        return (ModelFailureCategory::ContextWindowExceeded, ModelRetryClass::NotRetryable);
    }
    if contains_any(lower, &["model_not_found", "unknown model"]) {
        return (ModelFailureCategory::UnsupportedModel, ModelRetryClass::NotRetryable);
    }
    if error.and_then(|value| string_at(value, &["param"])).is_some()
        && contains_any(lower, &["unsupported", "unknown parameter", "not supported"])
    {
        return (ModelFailureCategory::UnsupportedParameter, ModelRetryClass::NotRetryable);
    }
    (ModelFailureCategory::InvalidRequest, ModelRetryClass::NotRetryable)
}

pub(super) fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}
