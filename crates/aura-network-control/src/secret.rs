pub use aura_secret_scanner::detect_text;
mod headers;
mod request;
pub use headers::is_authentication_header;
use headers::is_authentication_header as sensitive_header;

use aura_permission_domain::{NetworkDataReference, NetworkRequestPlan};
use serde_json::Value;
use url::Url;

use crate::NetworkControlError;

const REDACTED_SECRET: &str = "<redacted-secret>";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretDetection {
    pub location: String,
    pub kind: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SecretScanner;

impl SecretScanner {
    pub fn scan_text(&self, location: &str, value: &str) -> Result<(), NetworkControlError> {
        if let Some(kind) = detect_text(value) {
            Err(detected(location.to_owned(), kind))
        } else {
            Ok(())
        }
    }

    pub fn scan_header(&self, name: &str, value: &str) -> Result<(), NetworkControlError> {
        if sensitive_header(name) {
            return Err(detected(
                format!("header.{}", name.to_ascii_lowercase()),
                "credential_header",
            ));
        }
        self.scan_text(&format!("header.{}", name.to_ascii_lowercase()), value)
    }

    pub fn scan_plan(&self, plan: &NetworkRequestPlan) -> Result<(), NetworkControlError> {
        let url = Url::parse(&plan.requested_url).map_err(|_| NetworkControlError::InvalidUrl)?;
        for (name, value) in url.query_pairs() {
            if sensitive_name(&name) || detect_text(&value).is_some() {
                return Err(detected(format!("url.query.{name}"), "credential_like_query"));
            }
        }
        for reference in &plan.request_data {
            match reference {
                NetworkDataReference::InlineText { label, value } => {
                    if let Some(kind) = detect_text(value) {
                        return Err(detected(format!("inline.{label}"), kind));
                    }
                }
                NetworkDataReference::Header { name, value } => {
                    if sensitive_header(name) {
                        return Err(detected(
                            format!("header.{}", name.to_ascii_lowercase()),
                            "credential_header",
                        ));
                    }
                    if let Some(kind) = detect_text(value) {
                        return Err(detected(
                            format!("header.{}", name.to_ascii_lowercase()),
                            kind,
                        ));
                    }
                }
                NetworkDataReference::JsonBody { value } => {
                    scan_json(value, "body")?;
                }
                NetworkDataReference::FileReference { .. }
                | NetworkDataReference::Artifact { .. } => {
                    // File and Artifact bytes are classified from provenance.
                    // The caller's preview scanner handles their contents before
                    // creating this plan; this layer never guesses authorization
                    // from a string-only path.
                }
            }
        }
        Ok(())
    }

    /// Scans a non-secret JSON value at a trusted Core boundary. Sensitive
    /// field names and credential-like string values are both rejected; the
    /// returned error contains only the structural location and detection
    /// class, never the candidate value.
    pub fn scan_json_value(
        &self,
        location: &str,
        value: &Value,
    ) -> Result<(), NetworkControlError> {
        scan_json(value, location)
    }

    #[must_use]
    pub fn redact(value: &str) -> String {
        if detect_text(value).is_some() { REDACTED_SECRET.to_owned() } else { value.to_owned() }
    }

    /// Returns a recursively redacted copy of an untrusted Provider error payload.
    ///
    /// Sensitive field names replace the entire field value. Non-sensitive
    /// strings are still checked by the existing credential-pattern scanner.
    /// The input value is never modified.
    #[must_use]
    pub fn redact_provider_error(value: &Value) -> Value {
        redact_provider_error_value(value)
    }
}

fn redact_provider_error_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, child)| {
                    let child = if sensitive_name(key) || sensitive_header(key) {
                        Value::String(REDACTED_SECRET.to_owned())
                    } else {
                        redact_provider_error_value(child)
                    };
                    (key.clone(), child)
                })
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.iter().map(redact_provider_error_value).collect())
        }
        Value::String(text) if detect_text(text).is_some() => {
            Value::String(REDACTED_SECRET.to_owned())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => value.clone(),
    }
}

fn scan_json(value: &Value, path: &str) -> Result<(), NetworkControlError> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let location = format!("{path}.{key}");
                if sensitive_name(key) && !child.is_null() {
                    return Err(detected(location, "credential_field"));
                }
                scan_json(child, &location)?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                scan_json(child, &format!("{path}[{index}]"))?;
            }
        }
        Value::String(text) => {
            if let Some(kind) = detect_text(text) {
                return Err(detected(path.to_owned(), kind));
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn detected(location: String, kind: impl Into<String>) -> NetworkControlError {
    NetworkControlError::SecretDetected { location, kind: kind.into() }
}

fn sensitive_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "api_key"
            | "apikey"
            | "access_token"
            | "refresh_token"
            | "auth_token"
            | "token"
            | "password"
            | "passwd"
            | "secret"
            | "client_secret"
            | "private_key"
            | "session_cookie"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_secrets_without_returning_the_secret_value() {
        let secret = "sk-abcdefghijklmnopqrstuvwxyz012345";
        let detection = detect_text(secret);
        assert_eq!(detection, Some("api_token"));
        assert_eq!(SecretScanner::redact(secret), "<redacted-secret>");
    }

    #[test]
    fn ordinary_source_text_is_not_treated_as_a_secret() {
        assert_eq!(detect_text("pub fn main() { println!(\"hello\"); }"), None);
    }

    #[test]
    fn provider_error_redaction_is_recursive_and_preserves_diagnostics() {
        let canary_secret = "sk-provider-error-canary-0123456789";
        let private_key =
            "-----BEGIN PRIVATE KEY-----\nprovider-error-canary\n-----END PRIVATE KEY-----";
        let jwt = "eyJabcdefghijk.abcdefghijk.abcdefghijk";
        let input = serde_json::json!({
            "api_key": canary_secret,
            "message": "request rejected",
            "code": 401,
            "type": "authentication_error",
            "request_id": "req_diagnostic",
            "nested": {
                " API-Key ": { "unexpected": canary_secret },
                "authorization": format!("Bearer {canary_secret}"),
                "ordinary": {
                    "enabled": true,
                    "count": 2,
                    "empty": null
                }
            },
            "items": [
                { "x-api-key": canary_secret },
                { "token": canary_secret },
                { "client_secret": canary_secret },
                { "diagnostic_copy": canary_secret },
                { "key_material": private_key },
                { "jwt_copy": jwt }
            ]
        });
        let original = input.clone();

        let redacted = SecretScanner::redact_provider_error(&input);

        assert_eq!(input, original);
        assert_eq!(redacted["api_key"], REDACTED_SECRET);
        assert_eq!(redacted["nested"][" API-Key "], REDACTED_SECRET);
        assert_eq!(redacted["nested"]["authorization"], REDACTED_SECRET);
        assert_eq!(redacted["items"][0]["x-api-key"], REDACTED_SECRET);
        assert_eq!(redacted["items"][1]["token"], REDACTED_SECRET);
        assert_eq!(redacted["items"][2]["client_secret"], REDACTED_SECRET);
        assert_eq!(redacted["items"][3]["diagnostic_copy"], REDACTED_SECRET);
        assert_eq!(redacted["items"][4]["key_material"], REDACTED_SECRET);
        assert_eq!(redacted["items"][5]["jwt_copy"], REDACTED_SECRET);
        assert_eq!(redacted["message"], "request rejected");
        assert_eq!(redacted["code"], 401);
        assert_eq!(redacted["type"], "authentication_error");
        assert_eq!(redacted["request_id"], "req_diagnostic");
        assert_eq!(redacted["nested"]["ordinary"], input["nested"]["ordinary"]);
    }

    #[test]
    fn provider_error_renderings_never_expose_canary_secrets() {
        let canary_secret = "sk-provider-error-canary-9876543210";
        let redacted = SecretScanner::redact_provider_error(&serde_json::json!({
            "error": {
                "client_secret": canary_secret,
                "message": format!("upstream copied {canary_secret}")
            }
        }));
        let error = NetworkControlError::Transport(redacted.to_string());
        let renderings = [
            redacted.to_string(),
            format!("{redacted:?}"),
            serde_json::to_string_pretty(&redacted).expect("redacted provider error snapshot"),
            error.to_string(),
            format!("{error:?}"),
        ];

        for rendering in renderings {
            assert!(!rendering.contains(canary_secret));
            assert!(rendering.contains(REDACTED_SECRET));
        }
    }
}
