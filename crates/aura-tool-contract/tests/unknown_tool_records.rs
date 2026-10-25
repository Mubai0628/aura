//! A call whose Provider name was absent from the
//! frozen request has no Tool definition. Its record says so with an explicit null; a record that
//! omits the field is damaged and is refused, never read as an unknown Tool.
use aura_domain_common::ToolCallId;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_tool_contract::{
    NormalizedToolCallInputIssueV1, NormalizedToolCallInputStateV1, NormalizedToolCallV1,
};

fn unknown_call() -> NormalizedToolCallV1 {
    NormalizedToolCallV1 {
        tool_call_id: ToolCallId::new(),
        ordinal: 0,
        definition_ref: None,
        input: NormalizedToolCallInputStateV1::InvalidInput {
            issue: NormalizedToolCallInputIssueV1::PurposeMissing,
            detail: None,
        },
        provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"provider-envelope"),
    }
}

#[test]
fn an_unknown_call_states_its_missing_definition_as_null() {
    let call = unknown_call();
    let value = serde_json::to_value(&call).expect("serialize");
    assert!(value.get("definition_ref").is_some_and(serde_json::Value::is_null), "{value}");
    assert_eq!(serde_json::from_value::<NormalizedToolCallV1>(value).expect("explicit null"), call);
}

#[test]
fn a_call_record_that_omits_its_definition_is_refused() {
    let mut value = serde_json::to_value(unknown_call()).expect("serialize");
    value.as_object_mut().expect("object").remove("definition_ref");
    assert!(serde_json::from_value::<NormalizedToolCallV1>(value).is_err());
}
