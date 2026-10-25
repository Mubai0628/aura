//! Human-readable validation corrections; argument values never enter feedback.
use jsonschema::error::{ValidationError, ValidationErrorKind as Kind};
use serde_json::Value;

pub(super) fn code(kind: &Kind) -> String {
    let name = match kind {
        Kind::OneOfNotValid { .. } => "oneofnotvalid",
        Kind::OneOfMultipleValid { .. } => "oneofmultiplevalid",
        Kind::Constant { .. } => "constant",
        Kind::BacktrackLimitExceeded { .. } => "backtracklimitexceeded",
        Kind::RegexEngineFailure { .. } => "regexenginefailure",
        Kind::Custom { .. } => "custom",
        Kind::FromUtf8 { .. } => "fromutf8",
        Kind::Referencing(_) => "referencing",
        _ => kind.keyword(),
    };
    name.to_ascii_lowercase()
}

pub(super) fn message(error: &ValidationError<'_>, root: &Value) -> String {
    message_at_depth(error, root, 0)
}

fn message_at_depth(error: &ValidationError<'_>, root: &Value, depth: usize) -> String {
    let actual = shape(error.instance());
    let text = match error.kind() {
        Kind::AdditionalProperties { unexpected } | Kind::UnevaluatedProperties { unexpected } => {
            // The fields the object does take, so a value can move to the right one.
            let parent = error.schema_path().as_str().rsplit_once('/').map_or("", |(p, _)| p);
            let taken = root
                .pointer(parent)
                .and_then(|schema| schema.get("properties"))
                .and_then(Value::as_object)
                .filter(|properties| !properties.is_empty())
                .map(|properties| {
                    properties
                        .keys()
                        .take(12)
                        .map(|name| format!("`{}`", name.chars().take(64).collect::<String>()))
                        .collect::<Vec<_>>()
                        .join(", ")
                });
            format!(
                "expected only the fields {}; remove unexpected field(s) {}",
                taken.as_deref().unwrap_or("the schema lists"),
                names(error.instance_path().as_str(), unexpected)
            )
        }
        Kind::Required { property } => {
            format!(
                "expected required field {}; the field is missing",
                property.as_str().map_or_else(
                    || property.to_string(),
                    |name| field_path(error.instance_path().as_str(), name)
                )
            )
        }
        Kind::Type { .. } => {
            let expected = root
                .pointer(error.schema_path().as_str())
                .map(|value| value.to_string())
                .unwrap_or_else(|| "the declared type".into());
            format!("expected {expected}; received {actual}")
        }
        Kind::OneOfNotValid { context } | Kind::AnyOf { context } => {
            let path = error.schema_path().as_str();
            let variants = root
                .pointer(path)
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .take(6)
                        .map(|v| describe(v, root, 0))
                        .collect::<Vec<_>>()
                        .join(" or ")
                })
                .unwrap_or_else(|| "one of the declared shapes".into());
            let detail = if depth < 2 {
                context
                    .iter()
                    .flatten()
                    .take(3)
                    .map(|issue| {
                        format!(
                            "{}: {}",
                            issue.instance_path(),
                            message_at_depth(issue, root, depth + 1)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            } else {
                String::new()
            };
            format!(
                "expected {variants}; received {actual} matching none of these shapes. {detail}"
            )
        }
        Kind::OneOfMultipleValid { .. } => {
            "expected exactly one declared shape; multiple shapes matched".into()
        }
        Kind::Enum { options } => {
            format!("expected one of {options}; received a different {actual}")
        }
        Kind::Constant { expected_value } => {
            format!("expected {expected_value}; received a different {actual}")
        }
        Kind::MinLength { limit } => {
            format!("expected at least {limit} characters; the string is too short")
        }
        Kind::MaxLength { limit } => {
            format!("expected at most {limit} characters; the string is too long")
        }
        Kind::Minimum { limit } => format!("expected a number >= {limit}; the number is too small"),
        Kind::ExclusiveMinimum { limit } => {
            format!("expected a number > {limit}; the number is too small")
        }
        Kind::Maximum { limit } => format!("expected a number <= {limit}; the number is too large"),
        Kind::ExclusiveMaximum { limit } => {
            format!("expected a number < {limit}; the number is too large")
        }
        Kind::MinItems { limit } => {
            format!("expected at least {limit} items; the array has too few items")
        }
        Kind::MaxItems { limit } => {
            format!("expected at most {limit} items; the array has too many items")
        }
        Kind::UniqueItems => "expected distinct array items; duplicate items were supplied".into(),
        Kind::Format { format } => {
            format!("expected format {format}; the {actual} has an invalid format")
        }
        Kind::Pattern { .. } => {
            "expected a string matching the declared pattern; the string does not match".into()
        }
        _ => format!(
            "expected the declared {} constraint; the {actual} does not satisfy it",
            error.kind().keyword()
        ),
    };
    text.chars().take(512).map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn field_path(parent: &str, name: &str) -> String {
    format!("{parent}/{}", name.replace('~', "~0").replace('/', "~1"))
}

fn names(parent: &str, values: &[String]) -> String {
    values
        .iter()
        .take(8)
        .map(|name| format!("`{}`", field_path(parent, name).chars().take(128).collect::<String>()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn describe(schema: &Value, root: &Value, depth: usize) -> String {
    if depth < 4
        && let Some(reference) = schema.get("$ref").and_then(Value::as_str)
        && let Some(target) = reference.strip_prefix('#').and_then(|p| root.pointer(p))
    {
        return describe(target, root, depth + 1);
    }
    if let Some(constant) = schema.get("const") {
        return constant.to_string();
    }
    if schema.get("type").and_then(Value::as_str) == Some("object") {
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .take(8)
            .map(|name| {
                match schema
                    .get("properties")
                    .and_then(|p| p.get(name))
                    .and_then(|p| p.get("const"))
                {
                    Some(constant) => format!("{name}:{constant}"),
                    None => name.to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        return format!("object {{{required}}}");
    }
    schema.get("type").map_or_else(|| "the declared shape".into(), Value::to_string)
}
