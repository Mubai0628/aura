use std::sync::OnceLock;

use aura_domain_common::NormalizedIssueCode;
use aura_model_domain::ProviderErrorSummaryView;
use aura_network_control::SecretScanner;
use regex::Regex;

const MAX_PROVIDER_MESSAGE_BYTES: usize = 4096;
const MAX_PROVIDER_REQUEST_ID_BYTES: usize = 512;

pub(crate) fn summarize_provider_error(
    issue_code: NormalizedIssueCode,
    provider_status: Option<u16>,
    body: Option<&[u8]>,
) -> ProviderErrorSummaryView {
    let parsed = body.and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok());
    let message = parsed.as_ref().and_then(extract_provider_message);
    let provider_request_id = parsed
        .as_ref()
        .and_then(extract_provider_request_id)
        .and_then(sanitize_provider_request_id);
    let (provider_message, message_was_redacted, message_was_truncated) =
        message.map_or((None, false, false), |message| {
            let (message, redacted, truncated) = sanitize_provider_message(message);
            (Some(message), redacted, truncated)
        });
    ProviderErrorSummaryView {
        issue_code,
        provider_status,
        provider_message,
        message_was_redacted,
        message_was_truncated,
        provider_request_id,
    }
}

fn extract_provider_message(value: &serde_json::Value) -> Option<&str> {
    let error = value.get("error");
    [
        error.and_then(|error| error.get("message")),
        error.and_then(|error| error.get("detail")),
        value.get("message"),
        value.get("detail"),
    ]
    .into_iter()
    .flatten()
    .find_map(serde_json::Value::as_str)
}

fn extract_provider_request_id(value: &serde_json::Value) -> Option<&str> {
    let error = value.get("error");
    ["request_id", "requestId", "request-id"]
        .into_iter()
        .flat_map(|name| [value.get(name), error.and_then(|error| error.get(name))])
        .flatten()
        .find_map(serde_json::Value::as_str)
}

fn sanitize_provider_message(value: &str) -> (String, bool, bool) {
    let (control_safe, controls_changed) = replace_prohibited_controls(value);
    let (path_safe, paths_changed) = redact_local_paths(&control_safe);
    let secret_safe = SecretScanner::redact(&path_safe);
    let secret_changed = secret_safe != path_safe;
    let (truncated, was_truncated) = truncate_utf8(&secret_safe, MAX_PROVIDER_MESSAGE_BYTES);
    (truncated, controls_changed || paths_changed || secret_changed, was_truncated)
}

fn sanitize_provider_request_id(value: &str) -> Option<String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let redacted = SecretScanner::redact(value);
    if redacted != value {
        return None;
    }
    let (value, _) = truncate_utf8(value, MAX_PROVIDER_REQUEST_ID_BYTES);
    (!value.is_empty()).then_some(value)
}

fn replace_prohibited_controls(value: &str) -> (String, bool) {
    let mut changed = false;
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                changed = true;
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                }
                output.push('\n');
            }
            '\n' | '\t' => output.push(character),
            character if character.is_control() => {
                changed = true;
                output.push('\u{fffd}');
            }
            _ => output.push(character),
        }
    }
    (output, changed)
}

fn redact_local_paths(value: &str) -> (String, bool) {
    static WINDOWS_PATH: OnceLock<Regex> = OnceLock::new();
    static UNIX_PRIVATE_PATH: OnceLock<Regex> = OnceLock::new();
    let windows = WINDOWS_PATH.get_or_init(|| {
        Regex::new(r#"(?i)(?:[a-z]:[\\/]|\\\\)[^\r\n\t\"'<>|]+"#)
            .expect("static Windows path redaction regex is valid")
    });
    let unix = UNIX_PRIVATE_PATH.get_or_init(|| {
        Regex::new(r#"/(?:home|users|tmp|var/tmp)/[^\r\n\t\"'<>]+"#)
            .expect("static Unix path redaction regex is valid")
    });
    let windows_redacted = windows.replace_all(value, "<redacted-path>");
    let unix_redacted = unix.replace_all(&windows_redacted, "<redacted-path>");
    let changed = unix_redacted.as_ref() != value;
    (unix_redacted.into_owned(), changed)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_owned(), false);
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (value[..boundary].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use aura_domain_common::NormalizedIssueCode;

    use super::summarize_provider_error;

    #[test]
    fn summary_preserves_safe_provider_text_and_redacts_local_paths() {
        let body = br#"{"error":{"message":"bad request at C:\\Users\\alice\\secret.txt","request_id":"req_123"}}"#;
        let summary = summarize_provider_error(
            NormalizedIssueCode::new("provider.request_rejected").expect("issue code"),
            Some(400),
            Some(body),
        );
        assert_eq!(summary.provider_status, Some(400));
        assert_eq!(summary.provider_request_id.as_deref(), Some("req_123"));
        assert_eq!(summary.provider_message.as_deref(), Some("bad request at <redacted-path>"));
        assert!(summary.message_was_redacted);
        assert!(!summary.message_was_truncated);
    }

    #[test]
    fn summary_truncates_on_a_utf8_boundary() {
        let message = "界".repeat(2000);
        let body = serde_json::to_vec(&serde_json::json!({"error": {"detail": message}}))
            .expect("provider body");
        let summary = summarize_provider_error(
            NormalizedIssueCode::new("provider.invalid_response").expect("issue code"),
            Some(200),
            Some(&body),
        );
        let message = summary.provider_message.expect("summary message");
        assert!(message.len() <= 4096);
        assert!(message.is_char_boundary(message.len()));
        assert!(summary.message_was_truncated);
    }
}
