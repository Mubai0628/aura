//! A server's input schema in Aura's dialect (JSON Schema 2020-12). Draft-07 spellings are
//! converted, annotations Aura does not keep are dropped (a default goes into the
//! description so the model still reads it), and property names are never touched.
use serde_json::{Map, Value};

/// Keywords whose value is one schema.
const ONE: [&str; 11] = [
    "additionalProperties",
    "not",
    "if",
    "then",
    "else",
    "contains",
    "propertyNames",
    "unevaluatedItems",
    "unevaluatedProperties",
    "items",
    "additionalItems",
];
/// Keywords whose value maps names to schemas.
const MANY: [&str; 4] = ["properties", "patternProperties", "$defs", "dependentSchemas"];
/// Keywords whose value is a list of schemas.
const LIST: [&str; 4] = ["allOf", "anyOf", "oneOf", "prefixItems"];
/// Validation and the annotations Aura keeps; everything else is dropped.
const KEPT: [&str; 31] = [
    "$ref",
    "title",
    "description",
    "deprecated",
    "examples",
    "type",
    "enum",
    "const",
    "multipleOf",
    "maximum",
    "exclusiveMaximum",
    "minimum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "format",
    "maxContains",
    "minContains",
    "maxItems",
    "minItems",
    "uniqueItems",
    "maxProperties",
    "minProperties",
    "required",
    "dependentRequired",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "prefixItems",
];

/// The formats Aura's validator accepts (`aura-tool-contract` schema policy); another format
/// is an annotation Aura cannot check, so it is dropped rather than refusing the tool.
const FORMATS: [&str; 15] = [
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "hostname",
    "ipv4",
    "ipv6",
    "uuid",
    "uri",
    "uri-reference",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

fn rewrite_ref(reference: &str) -> String {
    reference
        .strip_prefix("#/definitions/")
        .map_or_else(|| reference.to_owned(), |rest| format!("#/$defs/{rest}"))
}

fn convert(schema: &Value) -> Value {
    let Some(object) = schema.as_object() else {
        // `true`/`false` schemas and anything odd pass through for the validator to judge.
        return schema.clone();
    };
    let mut out = Map::new();
    for (key, value) in object {
        let key = match key.as_str() {
            "definitions" => "$defs",
            other => other,
        };
        match key {
            "$ref" => {
                if let Some(reference) = value.as_str() {
                    out.insert(key.into(), Value::String(rewrite_ref(reference)));
                }
            }
            // Draft-07 tuple validation: `items: [..]` is `prefixItems`, `additionalItems`
            // is the 2020-12 `items`.
            "items" if value.is_array() => {
                out.insert(
                    "prefixItems".into(),
                    Value::Array(value.as_array().into_iter().flatten().map(convert).collect()),
                );
            }
            "additionalItems" => {
                if object.get("items").is_some_and(Value::is_array) {
                    out.insert("items".into(), convert(value));
                }
            }
            key if ONE.contains(&key) => {
                out.insert(key.into(), convert(value));
            }
            key if MANY.contains(&key) => {
                if let Some(map) = value.as_object() {
                    out.insert(
                        key.into(),
                        Value::Object(
                            map.iter().map(|(name, s)| (name.clone(), convert(s))).collect(),
                        ),
                    );
                }
            }
            key if LIST.contains(&key) => {
                if let Some(list) = value.as_array() {
                    out.insert(key.into(), Value::Array(list.iter().map(convert).collect()));
                }
            }
            "format" => {
                if value.as_str().is_some_and(|format| FORMATS.contains(&format)) {
                    out.insert(key.into(), value.clone());
                }
            }
            key if KEPT.contains(&key) => {
                out.insert(key.into(), value.clone());
            }
            _ => {}
        }
    }
    if let Some(default) = object.get("default") {
        let note = format!("Default: {default}");
        let description = match out.get("description").and_then(Value::as_str) {
            Some(text) if !text.is_empty() => format!("{text} ({note})"),
            _ => note,
        };
        out.insert("description".into(), Value::String(description));
    }
    Value::Object(out)
}

/// The schema in Aura's dialect; the root must stay an object schema.
pub fn input_schema(schema: &Value) -> Result<Value, String> {
    let converted = convert(schema);
    if converted.get("type").and_then(Value::as_str) != Some("object") {
        return Err("inputSchema 的 type 必须是 object".into());
    }
    Ok(converted)
}
