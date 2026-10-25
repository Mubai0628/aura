use serde::Serialize;
use serde_json::Value;
use std::sync::LazyLock;

static SENSITIVE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)(?:bearer\s+[a-z0-9._~+/=-]+|(?:sk|key|token)[-_][a-z0-9_-]+|[a-z0-9_+/=-]{32,}|(?:api[_-]?key|token|password|secret)\s*[:=]\s*[^\s,;]+)")
        .expect("fixed diagnostic redaction pattern")
});

pub fn sanitized_text(text: &str, limit: usize) -> String {
    SENSITIVE.replace_all(text, "[redacted]").chars().take(limit).collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderError {
    pub code: Option<String>,
    pub message: Option<String>,
}

/// Retained in the probe report only. Never stored in the budget ledger or returned to Core.
pub(super) fn provider_error(body: &[u8]) -> Option<ProviderError> {
    let error = super::values(body).ok()?.into_iter().find_map(|value| {
        value
            .get("error")
            .or_else(|| value.pointer("/response/error"))
            .filter(|v| !v.is_null())
            .cloned()
    })?;
    let code = error.get("code").filter(|v| v.is_string() || v.is_number()).map(|v| {
        sanitized_text(v.as_str().map_or_else(|| v.to_string(), str::to_owned).as_str(), 300)
    });
    let message = error.get("message").and_then(Value::as_str).map(|v| sanitized_text(v, 300));
    (code.is_some() || message.is_some()).then_some(ProviderError { code, message })
}
