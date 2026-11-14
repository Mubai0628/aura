//! The frozen bindings and history of a call whose
//! Provider name was unknown state the missing Tool with an explicit null. A record that omits the
//! field is damaged and is refused, never read as an unknown Tool.
use aura_domain_common::{ContentHash, ModelExecutionSnapshotId, ToolCallId};
use aura_model_domain::{
    LogicalHistoricalToolCall, ProviderToolCallId, ProviderToolCallProtocolBindingRecord,
    ProviderToolCallWireBinding, ProviderToolCallWireIdentity, ProviderWireToolCallId,
};
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;

fn explicit_null_round_trips_and_omission_fails<T>(record: T, field: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let value = serde_json::to_value(&record).expect("serialize");
    assert!(value.get(field).is_some_and(serde_json::Value::is_null), "{value}");
    assert_eq!(serde_json::from_value::<T>(value.clone()).expect("explicit null"), record);
    let mut omitted = value;
    omitted.as_object_mut().expect("object").remove(field);
    assert!(serde_json::from_value::<T>(omitted).is_err(), "{field} may not be omitted");
}

#[test]
fn unknown_call_bindings_and_history_require_an_explicit_null() {
    let tool_call_id = ToolCallId::new();
    explicit_null_round_trips_and_omission_fails(
        ProviderToolCallWireBinding {
            tool_call_id,
            tool_id: None,
            provider_wire_tool_name: "_aura".to_owned(),
            wire_identity: ProviderToolCallWireIdentity::AuraGenerated {
                provider_wire_tool_call_id: ProviderWireToolCallId::from_tool_call_id(tool_call_id),
            },
        },
        "tool_id",
    );
    explicit_null_round_trips_and_omission_fails(
        ProviderToolCallProtocolBindingRecord {
            model_execution_snapshot_id: ModelExecutionSnapshotId::new(),
            tool_call_id,
            tool_definition: None,
            provider_wire_tool_name: "_aura".to_owned(),
            provider_tool_call_id: ProviderToolCallId::new("call_unknown").expect("provider ID"),
        },
        "tool_definition",
    );
    explicit_null_round_trips_and_omission_fails(
        LogicalHistoricalToolCall {
            tool_call_id,
            tool_id: None,
            canonical_name: "_aura".to_owned(),
            arguments_json: "{}".to_owned(),
            arguments_hash: ContentHash::sha256(b"{}"),
        },
        "tool_id",
    );
}
