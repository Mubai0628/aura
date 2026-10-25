use aura_mcp::input_schema;
use serde_json::json;

#[test]
fn draft_07_spellings_become_aura_dialect_and_property_names_are_kept() {
    let converted = input_schema(&json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "default": {"type": "string", "default": "x", "description": "a property named default"},
            "pair": {"type": "array", "items": [{"type": "string"}], "additionalItems": false},
            "node": {"$ref": "#/definitions/Node"},
            "count": {"type": "integer", "format": "int32"},
            "when": {"type": "string", "format": "date-time"}
        },
        "required": ["pair"],
        "additionalProperties": false,
        "definitions": {"Node": {"type": "object", "x-vendor": 1, "readOnly": true}}
    }))
    .expect("object schema");
    assert!(converted.get("$schema").is_none());
    assert_eq!(
        converted["properties"]["default"]["description"],
        "a property named default (Default: \"x\")"
    );
    assert!(converted["properties"]["default"].get("default").is_none());
    assert_eq!(converted["properties"]["pair"]["prefixItems"], json!([{"type": "string"}]));
    assert_eq!(converted["properties"]["pair"]["items"], json!(false));
    assert_eq!(converted["properties"]["node"]["$ref"], "#/$defs/Node");
    assert_eq!(converted["$defs"]["Node"], json!({"type": "object"}));
    assert_eq!(
        converted["properties"]["count"],
        json!({"type": "integer"}),
        "an unchecked format is dropped"
    );
    assert_eq!(converted["properties"]["when"]["format"], "date-time");
    assert_eq!(converted["required"], json!(["pair"]));
    assert_eq!(converted["additionalProperties"], json!(false));
}

#[test]
fn only_object_roots_are_usable() {
    assert!(input_schema(&json!({"type": "array"})).is_err());
    assert!(input_schema(&json!({"properties": {}})).is_err());
    assert!(input_schema(&json!(true)).is_err());
}
