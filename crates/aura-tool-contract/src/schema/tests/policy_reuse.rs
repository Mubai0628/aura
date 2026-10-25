use super::*;
use serde_json::json;

#[test]
fn a_known_schema_reuses_policy_checks_when_it_is_read_from_the_store_again() {
    let before = POLICY_VALIDATIONS.with(std::cell::Cell::get);
    let schema = CanonicalJsonSchemaDocumentV1::new(json!({
        "title": "s08-policy-reuse-persisted-schema",
        "type": "object",
        "properties": { "script": { "type": "string" } },
        "required": ["script"],
        "additionalProperties": false
    }))
    .expect("valid persisted input schema");
    let wire = serde_json::to_string(&schema).expect("persist schema");
    for _ in 0..5 {
        let reopened: CanonicalJsonSchemaDocumentV1 =
            serde_json::from_str(&wire).expect("read and validate the same stored schema");
        assert_eq!(reopened, schema);
    }
    let checks = POLICY_VALIDATIONS.with(std::cell::Cell::get) - before;
    assert_eq!(checks, 1, "identical stored schemas should not repeat the policy walk");
}

#[test]
fn warmed_policy_checks_never_trust_a_supplied_hash_or_a_changed_policy_version() {
    let schema = CanonicalJsonSchemaDocumentV1::new(json!({
        "title": "s08-policy-reuse-mutation-guards",
        "type": "object",
        "properties": { "value": { "type": "string" } }
    }))
    .expect("warm a valid schema");
    let mut forged = schema.clone();
    forged.document["properties"]["value"]["$ref"] = json!("https://example.invalid/schema");
    assert_eq!(forged.validate(), Err(ToolSchemaError::ExternalReference));
    let mut changed = schema.clone();
    changed.document["properties"]["value"]["type"] = json!("integer");
    assert_eq!(changed.validate(), Err(ToolSchemaError::HashMismatch));
    let mut wrong_version = schema;
    wrong_version.schema_policy_version += 1;
    assert_eq!(wrong_version.validate(), Err(ToolSchemaError::UnsupportedMetaSchema));
}
