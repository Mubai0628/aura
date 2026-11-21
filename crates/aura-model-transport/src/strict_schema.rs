//! Whether a compiled tool schema may be sent with `strict: true`.
//!
//! The adapters opt in only for the common subset of strict function schemas.
//! Provider support for numeric bounds, length and pattern constraints varies.
//! Core validates calls against the original tool schema independently.
use serde_json::Value;

/// Keywords every strict implementation Aura targets understands.
const STRICT_KEYWORDS: [&str; 10] = [
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "const",
    "anyOf",
    "description",
    "title",
];

/// True when `schema` uses only the strict subset described above.
#[must_use]
pub fn strict_schema_compatible(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object")
        && schema.get("anyOf").is_none()
        && compatible(schema, 0)
}

fn compatible(node: &Value, depth: usize) -> bool {
    let Value::Object(object) = node else { return false };
    if depth > 64 || object.keys().any(|key| !STRICT_KEYWORDS.contains(&key.as_str())) {
        return false;
    }
    if let Some(properties) = object.get("properties") {
        let Value::Object(properties) = properties else { return false };
        let required = object.get("required").and_then(Value::as_array);
        let all_required = required.is_some_and(|required| {
            required.len() == properties.len()
                && properties.keys().all(|key| required.iter().any(|name| name == key))
        });
        if !all_required
            || object.get("additionalProperties") != Some(&Value::Bool(false))
            || !properties.values().all(|value| compatible(value, depth + 1))
        {
            return false;
        }
    } else if object.get("type").and_then(Value::as_str) == Some("object")
        && object.get("additionalProperties") != Some(&Value::Bool(false))
    {
        return false;
    }
    if let Some(items) = object.get("items")
        && !compatible(items, depth + 1)
    {
        return false;
    }
    if let Some(branches) = object.get("anyOf") {
        let Value::Array(branches) = branches else { return false };
        if !branches.iter().all(|branch| compatible(branch, depth + 1)) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_fully_required_closed_schema_is_strict() {
        let schema = json!({"type": "object", "additionalProperties": false,
            "properties": {"id": {"type": "string", "description": "x"},
                           "mode": {"anyOf": [{"type": "string", "enum": ["a"]}, {"type": "null"}]},
                           "count": {"type": "integer"}},
            "required": ["id", "mode", "count"]});
        assert!(strict_schema_compatible(&schema));
        assert!(strict_schema_compatible(
            &json!({"type": "object", "additionalProperties": false})
        ));
    }

    #[test]
    fn optional_fields_open_objects_and_unsupported_constraints_are_not_strict() {
        let closed = |properties: serde_json::Value, required: serde_json::Value| {
            json!({"type": "object", "additionalProperties": false,
                   "properties": properties, "required": required})
        };
        for schema in [
            closed(json!({"a": {"type": "string"}, "b": {"type": "string"}}), json!(["a"])),
            json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]}),
            closed(json!({"a": {"type": "string", "minLength": 1}}), json!(["a"])),
            closed(json!({"a": {"type": "string", "pattern": "x"}}), json!(["a"])),
            closed(
                json!({"a": {"type": "array", "items": {"type": "string"}, "maxItems": 3}}),
                json!(["a"]),
            ),
            closed(json!({"a": {"oneOf": [{"type": "string"}]}}), json!(["a"])),
            closed(
                json!({"a": {"type": "object", "properties": {"b": {"type": "string"}}, "required": []}}),
                json!(["a"]),
            ),
            json!({"type": "object"}),
            json!(true),
        ] {
            assert!(!strict_schema_compatible(&schema), "{schema}");
        }
    }

    #[test]
    fn provider_specific_numeric_limits_and_root_alternatives_are_not_portable() {
        for limit in [json!({"minimum": 0}), json!({"maximum": 9})] {
            let mut count = limit;
            count["type"] = json!("integer");
            let schema = json!({"type": "object", "additionalProperties": false,
                "properties": {"count": count}, "required": ["count"]});
            assert!(!strict_schema_compatible(&schema), "{schema}");
        }
        let root_alternative = json!({"type": "object", "additionalProperties": false,
            "anyOf": [{"type": "object", "additionalProperties": false}]});
        assert!(!strict_schema_compatible(&root_alternative));
    }
}
