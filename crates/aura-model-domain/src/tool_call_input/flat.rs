//! Provider envelope 3: the tool's own fields at the top level, beside `aura_purpose`.
//!
//! Envelopes 1 and 2 asked for `{"_aura":{"purpose":…},"arguments":{…}}`. In the first real
//! Samples 7 of 48 calls (15%) put the tool's fields inside `_aura`, and each one cost a
//! correction round. One flat object leaves nothing to put in the wrong place.
use super::{invalid, invalid_at, ready};
use aura_domain_common::jcs::{CanonicalJsonError, parse_json_strict};
use aura_tool_contract::{
    MAX_PROVIDER_ARGUMENTS_BYTES, NormalizedToolCallInputIssueV1 as Issue,
    NormalizedToolCallInputStateV1, PROVIDER_PURPOSE_FIELD, ToolPurposeTextError,
    ToolPurposeTextV1,
};
use serde_json::Value;

/// The metadata object of envelopes 1 and 2. It is reserved in every tool schema, so a call
/// that carries it copied the earlier shape. `arguments` alone is not treated that way: an
/// external tool may have an ordinary field of that name, and its schema decides.
const EARLIER_METADATA: &str = "_aura";

pub(super) fn normalize(provider_arguments: &str) -> NormalizedToolCallInputStateV1 {
    if provider_arguments.len() > MAX_PROVIDER_ARGUMENTS_BYTES {
        return invalid(Issue::EnvelopeShapeInvalid);
    }
    let value = match parse_json_strict(provider_arguments.as_bytes()) {
        Ok(value) => value,
        Err(CanonicalJsonError::DuplicateObjectKey { key }) => {
            return invalid_at(Issue::ArgumentsJsonDuplicateKey, "$", vec![key]);
        }
        Err(_) => return invalid(Issue::ArgumentsJsonInvalid),
    };
    let Value::Object(mut root) = value else {
        return invalid(Issue::EnvelopeRootInvalid);
    };
    if root.contains_key(EARLIER_METADATA) {
        return invalid_at(Issue::EnvelopeUnexpectedField, "$", vec![EARLIER_METADATA.to_owned()]);
    }
    let pointer = format!("/{PROVIDER_PURPOSE_FIELD}");
    let Some(purpose) = root.remove(PROVIDER_PURPOSE_FIELD) else {
        return invalid_at(Issue::PurposeMissing, &pointer, vec![]);
    };
    let Value::String(purpose) = purpose else {
        return invalid_at(Issue::PurposeInvalid, &pointer, vec![]);
    };
    let purpose = match ToolPurposeTextV1::new(purpose.clone()) {
        Ok(purpose) => purpose,
        // Only a value with content can be too long; an empty or blank one is invalid.
        Err(ToolPurposeTextError::InvalidLength) if !purpose.trim().is_empty() => {
            return invalid_at(Issue::PurposeTooLong, &pointer, vec![]);
        }
        Err(_) => return invalid_at(Issue::PurposeInvalid, &pointer, vec![]),
    };
    ready(purpose, &root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize_provider_tool_call_input_v1;

    fn state(value: &str) -> NormalizedToolCallInputStateV1 {
        normalize(value)
    }

    fn issue(value: &str) -> (Issue, Option<String>, Vec<String>) {
        match state(value) {
            NormalizedToolCallInputStateV1::InvalidInput { issue, detail } => {
                let pointer = detail.as_ref().map(|d| d.pointer.clone());
                (issue, pointer, detail.map(|d| d.fields).unwrap_or_default())
            }
            NormalizedToolCallInputStateV1::Ready { .. } => panic!("invalid input expected"),
        }
    }

    #[test]
    fn numbers_are_held_as_a_stored_record_gives_them_back() {
        let NormalizedToolCallInputStateV1::Ready { arguments, arguments_hash, .. } = state(
            r#"{"aura_purpose":"note","confidence":1.0,"weight":2.50,"limit":10,"zero":-0.0}"#,
        ) else {
            panic!("ready input expected");
        };
        assert_eq!(arguments["confidence"], serde_json::json!(1), "1.0 is the stored 1");
        assert_eq!(arguments["weight"], serde_json::json!(2.5));
        assert_eq!(arguments["zero"], serde_json::json!(0));
        let stored = stored_copy(&arguments);
        assert_eq!(stored, arguments, "a stored record compares equal to the call");
        assert_eq!(
            arguments_hash,
            aura_domain_common::jcs::canonical_sha256(&stored).expect("hash")
        );
    }

    /// What a store does with a record: canonical JSON written, plain JSON read back.
    fn stored_copy(value: &Value) -> Value {
        let bytes = aura_domain_common::jcs::canonical_json_jcs(value).expect("canonical");
        serde_json::from_slice(&bytes).expect("parses")
    }

    #[test]
    fn the_tool_fields_beside_the_purpose_are_the_arguments() {
        let NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash } =
            state(r#"{"path":".","aura_purpose":"  list\r\nthe root  ","limit":50}"#)
        else {
            panic!("ready input expected");
        };
        assert_eq!(purpose.as_str(), "  list\nthe root  ");
        assert_eq!(arguments, serde_json::json!({"path": ".", "limit": 50}));
        // The arguments of the two envelopes are the same value and hash, so an operation
        // is identified by what it does, not by the envelope that carried it.
        let NormalizedToolCallInputStateV1::Ready { arguments_hash: earlier, .. } =
            normalize_provider_tool_call_input_v1(
                r#"{"_aura":{"purpose":"list the root"},"arguments":{"limit":50,"path":"."}}"#,
            )
            .input
        else {
            panic!("ready input expected");
        };
        assert_eq!(arguments_hash, earlier);
    }

    #[test]
    fn a_tool_without_fields_needs_only_the_purpose() {
        let NormalizedToolCallInputStateV1::Ready { arguments, .. } =
            state(r#"{"aura_purpose":"read the TODO list"}"#)
        else {
            panic!("ready input expected");
        };
        assert_eq!(arguments, serde_json::json!({}));
    }

    #[test]
    fn an_ordinary_arguments_field_is_left_to_the_tool_schema() {
        let NormalizedToolCallInputStateV1::Ready { arguments, .. } =
            state(r#"{"aura_purpose":"call","arguments":{"a":1}}"#)
        else {
            panic!("ready input expected");
        };
        assert_eq!(arguments, serde_json::json!({"arguments": {"a": 1}}));
    }

    /// The shapes the earlier envelope produced, sent to a tool bound to envelope 3.
    #[test]
    fn the_earlier_envelope_is_named_as_the_field_to_remove() {
        for value in [
            r#"{"_aura":{"purpose":"list"},"arguments":{"path":"."}}"#,
            r#"{"_aura":{"purpose":"list","path":"."}}"#,
            r#"{"aura_purpose":"list","_aura":{"purpose":"list"},"path":"."}"#,
        ] {
            let (issue, pointer, fields) = issue(value);
            assert_eq!(issue, Issue::EnvelopeUnexpectedField, "{value}");
            assert_eq!(pointer.as_deref(), Some("$"));
            assert_eq!(fields, vec!["_aura".to_owned()]);
        }
    }

    #[test]
    fn a_missing_or_malformed_purpose_is_its_own_issue() {
        assert_eq!(issue(r#"{"path":"."}"#).0, Issue::PurposeMissing);
        assert_eq!(issue(r#"{"path":"."}"#).1.as_deref(), Some("/aura_purpose"));
        assert_eq!(issue(r#"{"aura_purpose":7}"#).0, Issue::PurposeInvalid);
        assert_eq!(issue(r#"{"aura_purpose":""}"#).0, Issue::PurposeInvalid);
        assert_eq!(issue(r#"{"aura_purpose":" \t "}"#).0, Issue::PurposeInvalid);
        let long = "p".repeat(aura_tool_contract::MAX_TOOL_PURPOSE_BYTES + 1);
        assert_eq!(issue(&format!(r#"{{"aura_purpose":"{long}"}}"#)).0, Issue::PurposeTooLong);
    }

    #[test]
    fn json_errors_keep_their_names() {
        assert_eq!(issue("{").0, Issue::ArgumentsJsonInvalid);
        assert_eq!(issue("[]").0, Issue::EnvelopeRootInvalid);
        let (duplicate, _, fields) = issue(r#"{"aura_purpose":"x","a":1,"a":2}"#);
        assert_eq!(duplicate, Issue::ArgumentsJsonDuplicateKey);
        assert_eq!(fields, vec!["a".to_owned()]);
        let oversized = format!(
            r#"{{"aura_purpose":"x","text":"{}"}}"#,
            "x".repeat(MAX_PROVIDER_ARGUMENTS_BYTES)
        );
        assert_eq!(issue(&oversized).0, Issue::EnvelopeShapeInvalid);
    }
}
