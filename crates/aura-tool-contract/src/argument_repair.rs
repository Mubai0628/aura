//! Arguments a model wrote with an object or an array as a string of JSON. The tool's schema says
//! what each top-level field takes: a string in a field that takes an object or an array, and
//! never a string, is read as the object or array it holds. When the object in such a string goes
//! on with `,"field":value…`, with the call's closing brace or without it, the model wrote the rest
//! of the call into the string; those fields move back beside it when the schema takes them there
//! and the call does not have them already.
//! Anything the schema takes as a string stays a string, and a string that cannot be read for
//! certain is left for validation to refuse.
use serde_json::{Map, Value};

/// How deep `$ref` and single `allOf` wrappers are followed.
const MAX_DEPTH: usize = 8;

/// A call's arguments as its schema takes them, and what was changed to get there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgumentRepairV1 {
    pub arguments: Value,
    /// The top-level fields read from a string of JSON, in name order.
    pub parsed: Vec<String>,
    /// The fields that came after the object in such a string, moved beside it, in name order.
    pub lifted: Vec<String>,
}

impl ArgumentRepairV1 {
    /// What the model is told with the call's result. It names fields, never their values.
    #[must_use]
    pub fn note(&self) -> String {
        let named = |fields: &[String]| {
            fields.iter().map(|field| format!("`{field}`")).collect::<Vec<_>>().join(", ")
        };
        let mut note = format!(
            "Note: {} came as a string holding JSON; Aura read it as JSON.",
            named(&self.parsed)
        );
        if !self.lifted.is_empty() {
            note.push_str(&format!(
                " {} were inside that string; Aura moved them to the top level.",
                named(&self.lifted)
            ));
        }
        note.push_str(
            " Write objects and arrays as JSON values, not as strings, and every field at the top level of the arguments.",
        );
        note
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Takes {
    Object,
    Array,
}

impl Takes {
    fn holds(self, value: &Value) -> bool {
        match self {
            Self::Object => value.is_object(),
            Self::Array => value.is_array(),
        }
    }
}

/// `schema` with its local `$ref` followed.
fn resolved<'a>(root: &'a Value, schema: &'a Value, depth: usize) -> Option<&'a Value> {
    match schema.get("$ref").and_then(Value::as_str) {
        Some(reference) if depth < MAX_DEPTH => {
            resolved(root, root.pointer(reference.strip_prefix('#')?)?, depth + 1)
        }
        Some(_) => None,
        None => Some(schema),
    }
}

/// What a field whose schema is `schema` takes, when that is an object or an array and never a
/// string.
fn takes(root: &Value, schema: &Value, depth: usize) -> Option<Takes> {
    let schema = resolved(root, schema, depth)?;
    if schema.get("anyOf").is_some() || schema.get("oneOf").is_some() {
        return None;
    }
    if let Some([only]) = schema.get("allOf").and_then(Value::as_array).map(Vec::as_slice)
        && schema.get("type").is_none()
        && depth < MAX_DEPTH
    {
        return takes(root, only, depth + 1);
    }
    match schema.get("type") {
        Some(Value::String(name)) => match name.as_str() {
            "object" => Some(Takes::Object),
            "array" => Some(Takes::Array),
            _ => None,
        },
        Some(Value::Array(names)) => {
            let has = |wanted: &str| names.iter().any(|name| name.as_str() == Some(wanted));
            match (has("string"), has("object"), has("array")) {
                (false, true, false) => Some(Takes::Object),
                (false, false, true) => Some(Takes::Array),
                _ => None,
            }
        }
        None => schema.get("properties").is_some().then_some(Takes::Object),
        Some(_) => None,
    }
}

/// The JSON value `text` starts with, and what follows it.
fn first_value(text: &str) -> Option<(Value, &str)> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    let value = stream.next()?.ok()?;
    Some((value, &text[stream.byte_offset()..]))
}

/// The fields of `,"a":1,"b":2}`, as written after an object that should have ended the call. A
/// model that closed the call outside the string leaves its brace out: `,"a":1,"b":2`.
fn trailing_fields(rest: &str) -> Option<Map<String, Value>> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Some(Map::new());
    }
    let fields = rest.strip_prefix(',')?;
    serde_json::from_str(&format!("{{{fields}"))
        .or_else(|_| serde_json::from_str(&format!("{{{fields}}}")))
        .ok()
}

/// `arguments` with each top-level string of JSON read as the object or array `schema` takes
/// there; `None` when nothing is to be changed.
#[must_use]
pub fn repair_string_encoded_arguments(
    schema: &Value,
    arguments: &Value,
) -> Option<ArgumentRepairV1> {
    let sent = arguments.as_object()?;
    let properties = schema.get("properties")?.as_object()?;
    let mut repaired = sent.clone();
    let mut parsed = Vec::new();
    let mut lifted = Vec::new();
    for (field, value) in sent {
        let Some(text) = value.as_str().map(str::trim) else { continue };
        if !text.starts_with(['{', '[']) {
            continue;
        }
        let Some(wanted) = properties.get(field).and_then(|property| takes(schema, property, 0))
        else {
            continue;
        };
        let Some((value, rest)) = first_value(text) else { continue };
        let Some(after) = trailing_fields(rest) else { continue };
        if !wanted.holds(&value) || (!after.is_empty() && wanted != Takes::Object) {
            continue;
        }
        // A field after the object is moved only to a place the schema has for it and nothing
        // in the call holds.
        if after.keys().any(|name| !properties.contains_key(name) || repaired.contains_key(name)) {
            continue;
        }
        repaired.insert(field.clone(), value);
        for (name, value) in after {
            lifted.push(name.clone());
            repaired.insert(name, value);
        }
        parsed.push(field.clone());
    }
    if parsed.is_empty() {
        return None;
    }
    parsed.sort();
    lifted.sort();
    Some(ArgumentRepairV1 { arguments: Value::Object(repaired), parsed, lifted })
}
