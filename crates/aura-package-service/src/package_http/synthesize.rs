//! A small, conservative minimal-instance generator for declared result schemas.
//! `None` means "not synthesized"; the caller reports the result shape as unchecked
//! instead of guessing. Every produced value is validated again by the caller.
use serde_json::{Map, Value};

const MAX_DEPTH: usize = 16;

#[must_use]
pub(crate) fn minimal_instance(schema: &Value) -> Option<Value> {
    instance(schema, 0)
}

fn instance(schema: &Value, depth: usize) -> Option<Value> {
    if depth > MAX_DEPTH {
        return None;
    }
    let object = match schema {
        Value::Bool(true) => return Some(Value::Null),
        Value::Object(object) => object,
        _ => return None,
    };
    if object.contains_key("$ref") || object.contains_key("not") || object.contains_key("if") {
        return None;
    }
    if let Some(value) = object.get("const") {
        return Some(value.clone());
    }
    if let Some(Value::Array(values)) = object.get("enum") {
        return values.first().cloned();
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(branches)) = object.get(key) {
            return branches.iter().find_map(|branch| instance(branch, depth + 1));
        }
    }
    if let Some(Value::Array(parts)) = object.get("allOf") {
        return match parts.as_slice() {
            [single] => instance(single, depth + 1),
            _ => None,
        };
    }
    let types: Vec<&str> = match object.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        None => return Some(Value::Null),
        _ => return None,
    };
    if types.contains(&"null") {
        return Some(Value::Null);
    }
    for kind in ["object", "array", "string", "integer", "number", "boolean"] {
        if types.contains(&kind) {
            return typed(kind, object, depth);
        }
    }
    None
}

fn typed(kind: &str, object: &Map<String, Value>, depth: usize) -> Option<Value> {
    match kind {
        "object" => {
            let mut out = Map::new();
            let properties = object.get("properties").and_then(Value::as_object);
            if let Some(Value::Array(required)) = object.get("required") {
                for name in required {
                    let name = name.as_str()?;
                    let schema = properties.and_then(|all| all.get(name));
                    let value = match schema {
                        Some(schema) => instance(schema, depth + 1)?,
                        None => match object.get("additionalProperties") {
                            Some(Value::Bool(false)) => return None,
                            Some(schema @ Value::Object(_)) => instance(schema, depth + 1)?,
                            _ => Value::Null,
                        },
                    };
                    out.insert(name.to_owned(), value);
                }
            }
            Some(Value::Object(out))
        }
        "array" => {
            let count = object.get("minItems").and_then(Value::as_u64).unwrap_or(0);
            if count == 0 {
                return Some(Value::Array(vec![]));
            }
            if count > 8 {
                return None;
            }
            let item = object
                .get("items")
                .map_or(Some(Value::Null), |items| instance(items, depth + 1))?;
            Some(Value::Array(vec![item; count as usize]))
        }
        "string" => {
            if object.contains_key("pattern") || object.contains_key("format") {
                return None;
            }
            let length = object.get("minLength").and_then(Value::as_u64).unwrap_or(0);
            (length <= 64).then(|| Value::String("a".repeat(length as usize)))
        }
        "integer" | "number" => {
            if object.contains_key("multipleOf") {
                return None;
            }
            let minimum = object
                .get("minimum")
                .and_then(Value::as_i64)
                .or_else(|| object.get("exclusiveMinimum").and_then(Value::as_i64).map(|v| v + 1))
                .unwrap_or(0)
                .max(0);
            Some(Value::from(minimum))
        }
        "boolean" => Some(Value::Bool(false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::minimal_instance;
    use serde_json::json;

    #[test]
    fn simple_shapes_are_synthesized_and_unsupported_ones_are_not_guessed() {
        assert_eq!(minimal_instance(&json!({"type":["object","null"]})), Some(json!(null)));
        assert_eq!(
            minimal_instance(&json!({"type":"object","required":["id","tags"],
                "properties":{"id":{"type":"integer","minimum":3},"tags":{"type":"array","minItems":1,"items":{"type":"string","minLength":2}}}})),
            Some(json!({"id":3,"tags":["aa"]}))
        );
        assert_eq!(minimal_instance(&json!({"enum":["x","y"]})), Some(json!("x")));
        assert_eq!(minimal_instance(&json!({"type":"string","pattern":"^[0-9]+$"})), None);
        assert_eq!(minimal_instance(&json!({"$ref":"#/$defs/item"})), None);
    }
}
