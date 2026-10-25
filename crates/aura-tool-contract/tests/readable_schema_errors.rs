use aura_tool_contract::{CanonicalJsonSchemaDocumentV1, CompiledToolSchemaV1};
use serde_json::json;

#[test]
fn schema_feedback_names_the_field_shape_and_problem_without_dumping_values() {
    let schema = CanonicalJsonSchemaDocumentV1::new(json!({
        "type":"object", "additionalProperties":false,
        "properties":{"limit":{"type":"integer"},"expected":{"oneOf":[
            {"type":"object","properties":{"expected_kind":{"const":"absent"}},
                "required":["expected_kind"],"additionalProperties":false},
            {"type":"object","properties":{"algorithm":{"type":"string"},"hex":{"type":"string"}},
                "required":["algorithm","hex"],"additionalProperties":false}
        ]}}, "required":["limit"]
    }))
    .expect("input schema");
    let validator = CompiledToolSchemaV1::compile(&schema).expect("compiled schema");
    let report = serde_json::to_value(validator.validate(&json!({
        "limit":"PRIVATE_VALUE", "expected":42, "extra":"PRIVATE_VALUE"
    })))
    .expect("report");
    let messages = report["errors"]
        .as_array()
        .expect("errors")
        .iter()
        .map(|issue| issue["issue_kind"].as_str().expect("readable issue"))
        .collect::<Vec<_>>();
    assert!(
        messages.iter().any(|message| message.contains("integer") && message.contains("string"))
    );
    assert!(
        messages.iter().any(|message| message.contains("/extra") && message.contains("remove"))
    );
    assert!(
        messages.iter().any(|message| message.contains("object") && message.contains("number"))
    );
    assert!(
        messages
            .iter()
            .all(|message| !message.contains("PRIVATE_VALUE") && !message.contains("context:"))
    );
    assert!(report["errors"].as_array().expect("errors").iter().all(|issue| {
        !issue["issue_kind"].as_str().expect("structured issue retained").is_empty()
    }));
    assert!(messages.iter().any(|message| message.starts_with("type:")));
    assert!(messages.iter().any(|message| message.starts_with("additionalproperties:")));
    assert!(messages.iter().any(|message| message.starts_with("oneofnotvalid:")));
}

/// A field the schema does not take is answered with the fields it
/// does take, so the model can move its value to the right one.
#[test]
fn an_unexpected_field_is_answered_with_the_fields_the_schema_takes() {
    let schema = CanonicalJsonSchemaDocumentV1::new(json!({
        "type":"object", "additionalProperties":false,
        "properties":{"pattern":{"type":"string"},"root":{"type":"string"},
            "path_glob":{"type":["string","null"]},"limit":{"type":"integer"}},
        "required":["pattern"]
    }))
    .expect("input schema");
    let validator = CompiledToolSchemaV1::compile(&schema).expect("compiled schema");
    let report =
        serde_json::to_value(validator.validate(&json!({"pattern":"x","path":"game/index.html"})))
            .expect("report");
    let message = report["errors"][0]["issue_kind"].as_str().expect("readable issue");
    assert!(message.contains("remove unexpected field(s) `/path`"), "{message}");
    for field in ["pattern", "root", "path_glob", "limit"] {
        assert!(message.contains(&format!("`{field}`")), "{field} is named: {message}");
    }
    assert!(!message.contains("game/index.html"), "values never enter feedback: {message}");
}
