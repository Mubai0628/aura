//! The user-only cancellation bridge has no arbitrary Core-request authority.
use aura_windows_user_contract::{
    TerminalOpeningChangeV1, TerminalOpeningProgressV1, TerminalOpeningRefV1,
    TerminalOpeningStartV1,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn boundary<T: DeserializeOwned + Serialize>(schema: &Value, name: &str, value: &Value) {
    let target = json!({"$defs":schema["$defs"],"$ref":format!("#/$defs/{name}")});
    let validator = jsonschema::validator_for(&target).expect("owning DTO schema");
    assert!(validator.is_valid(value), "valid full {name} fixture");
    let decoded: T = serde_json::from_value(value.clone()).expect("owning Rust DTO");
    assert_eq!(serde_json::to_value(decoded).expect("DTO serializes"), *value);
    for extra in ["request_id", "credential", "arbitrary_target"] {
        // request_id is legitimate only on Core progress, never renderer ref/start/change.
        if value.get(extra).is_some() {
            continue;
        }
        let mut mutated = value.clone();
        mutated[extra] = json!("unapproved");
        assert!(!validator.is_valid(&mutated), "{name} rejects {extra}");
        assert!(serde_json::from_value::<T>(mutated).is_err());
    }
    let mut wrong_version = value.clone();
    wrong_version["contract_version"] = json!(2);
    assert!(!validator.is_valid(&wrong_version));
    assert!(serde_json::from_value::<T>(wrong_version).is_err());
    if value.get("opening_id").is_some() {
        let mut core_target = value.clone();
        core_target["opening_id"] = json!("cor_01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert!(!validator.is_valid(&core_target), "Core IDs cannot be renderer tokens");
    }
}

#[test]
fn full_opening_samples_cross_the_rust_and_schema_boundaries() {
    let schema = aura_contract_schema::canonical_schema_value().expect("aggregate schema");
    let fixture: Value =
        serde_json::from_str(include_str!("../../../fixtures/contracts/terminal-opening.v1.json"))
            .expect("source-owned opening fixture");
    boundary::<TerminalOpeningStartV1>(&schema, "TerminalOpeningStartV1", &fixture["start"]);
    boundary::<TerminalOpeningRefV1>(&schema, "TerminalOpeningRefV1", &fixture["reference"]);
    let progress = fixture["progress"].as_array().expect("progress variants");
    let changes = fixture["changes"].as_array().expect("change variants");
    assert_eq!(progress.len(), 4);
    assert_eq!(changes.len(), 4);
    for value in progress {
        boundary::<TerminalOpeningProgressV1>(&schema, "TerminalOpeningProgressV1", value);
    }
    for value in changes {
        boundary::<TerminalOpeningChangeV1>(&schema, "TerminalOpeningChangeV1", value);
    }
}
