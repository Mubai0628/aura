use aura_domain_common::jcs::{
    CanonicalHashV1, canonical_json_jcs, canonical_sha256, parse_json_strict,
};
use aura_tool_contract::{
    MAX_PROVIDER_ARGUMENTS_BYTES, NormalizedToolCallInputDiagnosticV1,
    NormalizedToolCallInputIssueV1, NormalizedToolCallInputStateV1, ToolPurposeTextError,
    ToolPurposeTextV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderToolCallInputNormalizationV1 {
    pub input: NormalizedToolCallInputStateV1,
    pub provider_arguments_envelope_hash: CanonicalHashV1,
}

mod flat;

#[must_use]
pub fn normalize_provider_tool_call_input_v1(
    provider_arguments: &str,
) -> ProviderToolCallInputNormalizationV1 {
    let provider_arguments_envelope_hash = canonical_sha256(provider_arguments)
        .expect("a Rust UTF-8 string always has a canonical JSON string representation");
    let input = normalize_input(provider_arguments);
    ProviderToolCallInputNormalizationV1 { input, provider_arguments_envelope_hash }
}

/// Normalizes a call in the envelope its binding promised the Provider.
///
/// A stored step keeps the envelope it was sent with, so history written before envelope 3
/// still reads as `_aura`/`arguments`; every new binding is envelope 3.
#[must_use]
pub fn normalize_provider_tool_call_input(
    envelope_version: u32,
    provider_arguments: &str,
) -> ProviderToolCallInputNormalizationV1 {
    if envelope_version != crate::PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V3 {
        let mut normalized = normalize_provider_tool_call_input_v1(provider_arguments);
        if !matches!(
            envelope_version,
            crate::PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V1
                | crate::PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V2
        ) {
            normalized.input = invalid(NormalizedToolCallInputIssueV1::EnvelopeShapeInvalid);
        }
        return normalized;
    }
    let provider_arguments_envelope_hash = canonical_sha256(provider_arguments)
        .expect("a Rust UTF-8 string always has a canonical JSON string representation");
    let input = flat::normalize(provider_arguments);
    ProviderToolCallInputNormalizationV1 { input, provider_arguments_envelope_hash }
}

/// Logical history keeps tool arguments as one complete JSON value, and the
/// stored hash must describe what is emitted. A Provider can still stream an
/// argument fragment that is not valid JSON (an unescaped newline inside a long
/// string is enough), and copying that string into history made the whole
/// conversation unreadable: the next step failed its history validation, and a
/// recovery could not assemble the same history either.
///
/// The Provider's bytes stay in the response artifact for audit. History gets a
/// reserved, self-describing marker instead, so the call remains replayable and
/// the recorded Tool result still explains the real failure to the model.
#[must_use]
pub fn history_tool_call_arguments_v1(provider_arguments: &str) -> String {
    if parse_json_strict(provider_arguments.as_bytes()).is_ok() {
        return provider_arguments.to_owned();
    }
    let error = parse_json_strict(provider_arguments.as_bytes())
        .err()
        .map(|failure| failure.to_string())
        .unwrap_or_default();
    let error: String = error.chars().take(200).collect();
    let raw_prefix: String = provider_arguments.chars().take(1024).collect();
    serde_json::json!({
        INVALID_ARGUMENTS_MARKER: {
            "error": error,
            "raw_bytes": provider_arguments.len(),
            "raw_prefix": raw_prefix,
        }
    })
    .to_string()
}

/// Reserved key marking provider arguments that were not a complete JSON value.
pub const INVALID_ARGUMENTS_MARKER: &str = "_aura_invalid_tool_arguments";

/// The most field names one diagnostic reports.
const MAX_DIAGNOSTIC_FIELDS: usize = 8;
/// The longest field name a diagnostic repeats.
const MAX_DIAGNOSTIC_FIELD_CHARS: usize = 64;
/// The longest pointer a diagnostic carries.
const MAX_DIAGNOSTIC_POINTER_CHARS: usize = 96;

/// The envelope's own two fields, in the order the contract names them.
const ENVELOPE_FIELDS: [&str; 2] = ["_aura", "arguments"];

fn normalize_input(provider_arguments: &str) -> NormalizedToolCallInputStateV1 {
    if provider_arguments.len() > MAX_PROVIDER_ARGUMENTS_BYTES {
        return invalid(NormalizedToolCallInputIssueV1::EnvelopeShapeInvalid);
    }
    let value = match parse_json_strict(provider_arguments.as_bytes()) {
        Ok(value) => value,
        // A duplicate key is a structural mistake with a name, and telling the model
        // "invalid JSON" sends it looking for a syntax error it did not make.
        Err(aura_domain_common::jcs::CanonicalJsonError::DuplicateObjectKey { key }) => {
            return invalid_at(
                NormalizedToolCallInputIssueV1::ArgumentsJsonDuplicateKey,
                "$",
                vec![key],
            );
        }
        Err(_) => return invalid(NormalizedToolCallInputIssueV1::ArgumentsJsonInvalid),
    };
    let Value::Object(root) = value else {
        return invalid(NormalizedToolCallInputIssueV1::EnvelopeRootInvalid);
    };
    let Some(aura) = root.get("_aura") else {
        return invalid(NormalizedToolCallInputIssueV1::AuraMetadataMissing);
    };
    let Some(arguments) = root.get("arguments") else {
        return invalid(NormalizedToolCallInputIssueV1::ArgumentsMissing);
    };
    // The envelope is exactly these two fields. Naming the extra ones is the whole
    // correction: the model has to move them, not merely notice it did something wrong.
    let unexpected_root = unexpected_fields(root.keys().map(String::as_str), &ENVELOPE_FIELDS);
    if !unexpected_root.is_empty() {
        return invalid_at(
            NormalizedToolCallInputIssueV1::EnvelopeUnexpectedField,
            "$",
            unexpected_root,
        );
    }
    let Value::Object(aura) = aura else {
        return invalid_at(
            NormalizedToolCallInputIssueV1::AuraMetadataShapeInvalid,
            "/_aura",
            vec![],
        );
    };
    let Some(purpose) = aura.get("purpose") else {
        return invalid(NormalizedToolCallInputIssueV1::PurposeMissing);
    };
    // `_aura` holds the purpose and nothing else — the failure the Shell calls actually
    // hit, where the tool's own fields (or a nested `arguments`) were placed here.
    let unexpected_aura = unexpected_fields(aura.keys().map(String::as_str), &["purpose"]);
    if !unexpected_aura.is_empty() {
        return invalid_at(
            NormalizedToolCallInputIssueV1::AuraMetadataUnexpectedField,
            "/_aura",
            unexpected_aura,
        );
    }
    let Value::String(purpose) = purpose else {
        return invalid_at(
            NormalizedToolCallInputIssueV1::PurposeInvalid,
            "/_aura/purpose",
            vec![],
        );
    };
    let purpose = match ToolPurposeTextV1::new(purpose.clone()) {
        Ok(purpose) => purpose,
        // `InvalidLength` is the only error the constructor raises for a value that has
        // content and is too long; an empty or whitespace-only purpose is its own error
        // and stays its own answer.
        Err(ToolPurposeTextError::InvalidLength) if !purpose.trim().is_empty() => {
            return invalid_at(
                NormalizedToolCallInputIssueV1::PurposeTooLong,
                "/_aura/purpose",
                vec![],
            );
        }
        Err(_) => {
            return invalid_at(
                NormalizedToolCallInputIssueV1::PurposeInvalid,
                "/_aura/purpose",
                vec![],
            );
        }
    };
    let Value::Object(arguments) = arguments else {
        return invalid_at(
            NormalizedToolCallInputIssueV1::ArgumentsRootInvalid,
            "/arguments",
            vec![],
        );
    };
    ready(purpose, arguments)
}

/// The keys present that the contract does not allow, bounded and escaped.
///
/// Field *names* are structure, not content: they are what the model has to move, and
/// they are the only part of the invalid envelope this diagnostic repeats. The bound and
/// the escaping exist so a hostile or merely enormous argument string cannot turn a
/// correction message into a copy of itself.
fn unexpected_fields<'a>(present: impl Iterator<Item = &'a str>, allowed: &[&str]) -> Vec<String> {
    present
        .filter(|name| !allowed.contains(name))
        .take(MAX_DIAGNOSTIC_FIELDS)
        .map(bounded_field_name)
        .collect()
}

fn bounded_field_name(name: &str) -> String {
    name.chars()
        .take(MAX_DIAGNOSTIC_FIELD_CHARS)
        .flat_map(|character| character.escape_default())
        .collect()
}

fn bounded_pointer(pointer: &str) -> String {
    pointer.chars().take(MAX_DIAGNOSTIC_POINTER_CHARS).collect()
}

fn ready(
    purpose: ToolPurposeTextV1,
    arguments: &Map<String, Value>,
) -> NormalizedToolCallInputStateV1 {
    // The arguments exactly as a stored record gives them back: records are canonical JSON, so
    // a number the provider wrote as `1.0` is stored as `1`, and serde_json holds `1.0` and `1`
    // unequal. A call held in another form than its stored copy failed every later comparison
    //.
    let canonical = canonical_json_jcs(&Value::Object(arguments.clone()))
        .expect("strictly parsed JSON is valid canonical JSON material");
    let arguments: Value =
        serde_json::from_slice(&canonical).expect("canonical JSON is valid JSON");
    let arguments_hash = canonical_sha256(&arguments)
        .expect("strictly parsed JSON is valid canonical JSON material");
    NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash }
}

const fn invalid(issue: NormalizedToolCallInputIssueV1) -> NormalizedToolCallInputStateV1 {
    NormalizedToolCallInputStateV1::InvalidInput { issue, detail: None }
}

fn invalid_at(
    issue: NormalizedToolCallInputIssueV1,
    pointer: &str,
    fields: Vec<String>,
) -> NormalizedToolCallInputStateV1 {
    NormalizedToolCallInputStateV1::invalid_input_at(
        issue,
        NormalizedToolCallInputDiagnosticV1::new(bounded_pointer(pointer), fields),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_purpose_and_canonical_package_arguments() {
        let first = normalize_provider_tool_call_input_v1(
            r#"{"_aura":{"purpose":"  inspect\r\nfile  "},"arguments":{"z":1,"a":2}}"#,
        );
        let second = normalize_provider_tool_call_input_v1(
            r#"{ "arguments": {"a":2,"z":1}, "_aura": {"purpose":"  inspect\r\nfile  "} }"#,
        );
        let NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash } =
            first.input
        else {
            panic!("ready input expected");
        };
        assert_eq!(purpose.as_str(), "  inspect\nfile  ");
        assert_eq!(arguments, serde_json::json!({"a": 2, "z": 1}));
        let NormalizedToolCallInputStateV1::Ready { arguments_hash: second_arguments_hash, .. } =
            second.input
        else {
            panic!("ready input expected");
        };
        assert_eq!(arguments_hash, second_arguments_hash);
        assert_ne!(
            first.provider_arguments_envelope_hash, second.provider_arguments_envelope_hash,
            "the audit hash preserves the exact Provider argument string"
        );
    }

    #[test]
    fn malformed_inputs_map_to_the_closed_p05_input_state() {
        let state = |value: &str| normalize_provider_tool_call_input_v1(value).input;
        let issue = |value: &str| match state(value) {
            NormalizedToolCallInputStateV1::InvalidInput { issue, .. } => issue,
            NormalizedToolCallInputStateV1::Ready { .. } => panic!("invalid input expected"),
        };
        assert_eq!(issue("{"), NormalizedToolCallInputIssueV1::ArgumentsJsonInvalid);
        assert_eq!(issue("[]"), NormalizedToolCallInputIssueV1::EnvelopeRootInvalid);
        assert_eq!(
            issue(r#"{"arguments":{}}"#),
            NormalizedToolCallInputIssueV1::AuraMetadataMissing
        );
        assert_eq!(
            issue(r#"{"_aura":{"purpose":"x"}}"#),
            NormalizedToolCallInputIssueV1::ArgumentsMissing
        );
        assert_eq!(
            issue(r#"{"_aura":{},"arguments":{}}"#),
            NormalizedToolCallInputIssueV1::PurposeMissing
        );
        assert_eq!(
            issue(r#"{"_aura":{"purpose":" \t "},"arguments":{}}"#),
            NormalizedToolCallInputIssueV1::PurposeInvalid
        );
        assert_eq!(
            issue(r#"{"_aura":{"purpose":""},"arguments":{}}"#),
            NormalizedToolCallInputIssueV1::PurposeInvalid,
            "an empty purpose is not a length problem"
        );
        assert_eq!(
            issue(r#"{"_aura":{"purpose":"x"},"arguments":[],"arguments":{}}"#),
            NormalizedToolCallInputIssueV1::ArgumentsJsonDuplicateKey,
            "a repeated key is a named structural mistake, not a syntax error"
        );
        assert_eq!(
            issue(r#"{"_aura":[1],"arguments":{}}"#),
            NormalizedToolCallInputIssueV1::AuraMetadataShapeInvalid
        );
        assert_eq!(
            issue(r#"{"_aura":{"purpose":"x"},"arguments":[]}"#),
            NormalizedToolCallInputIssueV1::ArgumentsRootInvalid
        );
    }

    /// The three shapes the user's Shell calls actually produced.
    ///
    /// Each one carries the fields that have to move, so the correction can name them
    /// instead of restating the whole envelope rule.
    #[test]
    fn the_observed_metadata_mistakes_name_the_fields_that_must_move() {
        let diagnostic = |value: &str| match normalize_provider_tool_call_input_v1(value).input {
            NormalizedToolCallInputStateV1::InvalidInput { issue, detail } => {
                (issue, detail.expect("the issue names where it was found"))
            }
            NormalizedToolCallInputStateV1::Ready { .. } => panic!("invalid input expected"),
        };

        let (issue, detail) = diagnostic(
            r#"{"_aura":{"purpose":"list","path":".","limit":50},"arguments":{"path":".","limit":50}}"#,
        );
        assert_eq!(issue, NormalizedToolCallInputIssueV1::AuraMetadataUnexpectedField);
        assert_eq!(detail.pointer, "/_aura");
        assert_eq!(detail.fields, vec!["limit".to_owned(), "path".to_owned()]);

        let (issue, detail) = diagnostic(
            r#"{"_aura":{"purpose":"list","arguments":{"path":".","limit":50}},"arguments":{"path":".","limit":50}}"#,
        );
        assert_eq!(issue, NormalizedToolCallInputIssueV1::AuraMetadataUnexpectedField);
        assert_eq!(detail.fields, vec!["arguments".to_owned()]);

        let (issue, detail) =
            diagnostic(r#"{"_aura":{"purpose":"x"},"arguments":{},"extra":true}"#);
        assert_eq!(issue, NormalizedToolCallInputIssueV1::EnvelopeUnexpectedField);
        assert_eq!(detail.pointer, "$");
        assert_eq!(detail.fields, vec!["extra".to_owned()]);
    }

    #[test]
    fn a_diagnostic_is_bounded_and_never_repeats_argument_content() {
        let long_name = "n".repeat(200);
        let value = format!(
            r#"{{"_aura":{{"purpose":"x"}},"arguments":{{}},"{long_name}":{{"secret":"do-not-echo"}}}}"#
        );
        let NormalizedToolCallInputStateV1::InvalidInput { detail, .. } =
            normalize_provider_tool_call_input_v1(&value).input
        else {
            panic!("invalid input expected");
        };
        let detail = detail.expect("a named field");
        assert!(detail.fields[0].len() <= MAX_DIAGNOSTIC_FIELD_CHARS);
        let rendered = serde_json::to_string(&detail).expect("wire");
        assert!(
            !rendered.contains("do-not-echo"),
            "the diagnostic repeated argument content: {rendered}"
        );
    }

    #[test]
    fn a_duplicate_key_is_reported_by_name_and_control_characters_are_escaped() {
        let NormalizedToolCallInputStateV1::InvalidInput { issue, detail } =
            normalize_provider_tool_call_input_v1(
                r#"{"_aura":{"purpose":"x"},"arguments":{"a":1,"a":2}}"#,
            )
            .input
        else {
            panic!("invalid input expected");
        };
        assert_eq!(issue, NormalizedToolCallInputIssueV1::ArgumentsJsonDuplicateKey);
        assert_eq!(detail.expect("the key is named").fields, vec!["a".to_owned()]);

        let NormalizedToolCallInputStateV1::InvalidInput { detail, .. } =
            normalize_provider_tool_call_input_v1(
                "{\"_aura\":{\"purpose\":\"x\"},\"arguments\":{},\"a\\u0007b\":1}",
            )
            .input
        else {
            panic!("invalid input expected");
        };
        let field = &detail.expect("a named field").fields[0];
        assert!(
            !field.chars().any(char::is_control),
            "a control character reached the diagnostic: {field:?}"
        );
    }

    #[test]
    fn an_over_long_purpose_is_its_own_issue() {
        let purpose = "p".repeat(aura_tool_contract::MAX_TOOL_PURPOSE_BYTES + 1);
        let NormalizedToolCallInputStateV1::InvalidInput { issue, detail } =
            normalize_provider_tool_call_input_v1(&format!(
                r#"{{"_aura":{{"purpose":"{purpose}"}},"arguments":{{}}}}"#
            ))
            .input
        else {
            panic!("invalid input expected");
        };
        assert_eq!(issue, NormalizedToolCallInputIssueV1::PurposeTooLong);
        assert_eq!(detail.expect("a pointer").pointer, "/_aura/purpose");
    }
}
