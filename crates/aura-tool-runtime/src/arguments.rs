use aura_domain_common::jcs::canonical_json_jcs;
use aura_tool_contract::{
    ArgumentJsonPointer, CommandArgumentV1, HttpHeaderValueSourceV1, HttpJsonTemplateV1,
    HttpValueSourceV1,
};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolArgumentResolutionErrorV1 {
    #[error("the declared argument pointer does not resolve")]
    Missing,
    #[error("the declared argument is null where a value is required")]
    Null,
    #[error("the declared argument is not a scalar")]
    ScalarRequired,
    #[error("the declared repeated argument is not an array of scalars")]
    ScalarArrayRequired,
    #[error("the declared argument cannot be represented as canonical JSON")]
    CanonicalJson,
}

pub fn render_command_arguments_v1(
    arguments: &Value,
    mapping: &[CommandArgumentV1],
) -> Result<Vec<String>, ToolArgumentResolutionErrorV1> {
    let mut rendered = Vec::new();
    for item in mapping {
        match item {
            CommandArgumentV1::Literal { value } => rendered.push(value.as_str().to_owned()),
            CommandArgumentV1::Scalar { source } => {
                rendered.push(required_scalar_text(arguments, source)?);
            }
            CommandArgumentV1::Repeated { source } => {
                let value = required_pointer(arguments, source)?;
                let Value::Array(values) = value else {
                    return Err(ToolArgumentResolutionErrorV1::ScalarArrayRequired);
                };
                for value in values {
                    rendered.push(scalar_text(value).map_err(|error| match error {
                        ToolArgumentResolutionErrorV1::Null
                        | ToolArgumentResolutionErrorV1::ScalarRequired => {
                            ToolArgumentResolutionErrorV1::ScalarArrayRequired
                        }
                        other => other,
                    })?);
                }
            }
            CommandArgumentV1::ConditionalFlag { source, flag } => {
                let value = required_pointer(arguments, source)?;
                let Value::Bool(enabled) = value else {
                    return Err(ToolArgumentResolutionErrorV1::ScalarRequired);
                };
                if *enabled {
                    rendered.push(flag.as_str().to_owned());
                }
            }
            CommandArgumentV1::NamedOption { name, source, omit_when_missing_or_null } => {
                match optional_pointer(arguments, source) {
                    None | Some(Value::Null) if *omit_when_missing_or_null => {}
                    None => return Err(ToolArgumentResolutionErrorV1::Missing),
                    Some(Value::Null) => return Err(ToolArgumentResolutionErrorV1::Null),
                    Some(value) => {
                        rendered.push(name.as_str().to_owned());
                        rendered.push(scalar_text(value)?);
                    }
                }
            }
        }
    }
    Ok(rendered)
}

pub fn render_http_value_source_v1(
    arguments: &Value,
    source: &HttpValueSourceV1,
) -> Result<Vec<String>, ToolArgumentResolutionErrorV1> {
    match source {
        HttpValueSourceV1::Literal { value } => Ok(vec![value.as_str().to_owned()]),
        HttpValueSourceV1::Scalar { source } => Ok(vec![required_scalar_text(arguments, source)?]),
        HttpValueSourceV1::Repeated { source } => {
            let value = required_pointer(arguments, source)?;
            let Value::Array(values) = value else {
                return Err(ToolArgumentResolutionErrorV1::ScalarArrayRequired);
            };
            values
                .iter()
                .map(|value| {
                    scalar_text(value).map_err(|error| match error {
                        ToolArgumentResolutionErrorV1::Null
                        | ToolArgumentResolutionErrorV1::ScalarRequired => {
                            ToolArgumentResolutionErrorV1::ScalarArrayRequired
                        }
                        other => other,
                    })
                })
                .collect()
        }
    }
}

pub fn render_http_header_source_v1(
    arguments: &Value,
    source: &HttpHeaderValueSourceV1,
) -> Result<String, ToolArgumentResolutionErrorV1> {
    match source {
        HttpHeaderValueSourceV1::Literal { value } => Ok(value.as_str().to_owned()),
        HttpHeaderValueSourceV1::Scalar { source } => required_scalar_text(arguments, source),
    }
}

pub fn render_http_json_template_v1(
    arguments: &Value,
    template: &HttpJsonTemplateV1,
) -> Result<Value, ToolArgumentResolutionErrorV1> {
    match template {
        HttpJsonTemplateV1::Literal { value } => Ok(value.clone()),
        HttpJsonTemplateV1::Argument { source } => optional_pointer(arguments, source)
            .cloned()
            .ok_or(ToolArgumentResolutionErrorV1::Missing),
        HttpJsonTemplateV1::Array { items } => items
            .iter()
            .map(|item| render_http_json_template_v1(arguments, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        HttpJsonTemplateV1::Object { properties } => {
            let mut object = serde_json::Map::new();
            for property in properties.as_slice() {
                object.insert(
                    property.name.as_str().to_owned(),
                    render_http_json_template_v1(arguments, &property.value)?,
                );
            }
            Ok(Value::Object(object))
        }
    }
}

pub fn optional_pointer<'a>(
    arguments: &'a Value,
    pointer: &ArgumentJsonPointer,
) -> Option<&'a Value> {
    if pointer.as_str().is_empty() { Some(arguments) } else { arguments.pointer(pointer.as_str()) }
}

pub fn required_pointer<'a>(
    arguments: &'a Value,
    pointer: &ArgumentJsonPointer,
) -> Result<&'a Value, ToolArgumentResolutionErrorV1> {
    optional_pointer(arguments, pointer).ok_or(ToolArgumentResolutionErrorV1::Missing)
}

pub fn required_scalar_text(
    arguments: &Value,
    pointer: &ArgumentJsonPointer,
) -> Result<String, ToolArgumentResolutionErrorV1> {
    scalar_text(required_pointer(arguments, pointer)?)
}

pub fn scalar_text(value: &Value) -> Result<String, ToolArgumentResolutionErrorV1> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(_) | Value::Bool(_) => String::from_utf8(
            canonical_json_jcs(value).map_err(|_| ToolArgumentResolutionErrorV1::CanonicalJson)?,
        )
        .map_err(|_| ToolArgumentResolutionErrorV1::CanonicalJson),
        Value::Null => Err(ToolArgumentResolutionErrorV1::Null),
        Value::Array(_) | Value::Object(_) => Err(ToolArgumentResolutionErrorV1::ScalarRequired),
    }
}

#[cfg(test)]
mod tests {
    use aura_tool_contract::{CommandFlagV1, CommandOptionNameV1};
    use serde_json::json;

    use super::*;

    #[test]
    fn command_dsl_preserves_manifest_and_repeated_order_without_coercion() {
        let mapping = vec![
            CommandArgumentV1::Literal {
                value: aura_tool_contract::CommandLiteralV1::new("literal").expect("literal"),
            },
            CommandArgumentV1::Scalar { source: "/number".parse().expect("pointer") },
            CommandArgumentV1::Repeated { source: "/items".parse().expect("pointer") },
            CommandArgumentV1::ConditionalFlag {
                source: "/enabled".parse().expect("pointer"),
                flag: CommandFlagV1::new("--enabled").expect("flag"),
            },
            CommandArgumentV1::NamedOption {
                name: CommandOptionNameV1::new("--name").expect("option"),
                source: "/name".parse().expect("pointer"),
                omit_when_missing_or_null: false,
            },
        ];
        assert_eq!(
            render_command_arguments_v1(
                &json!({
                    "number": 1.25,
                    "items": ["first", 2, false],
                    "enabled": true,
                    "name": "  unchanged  "
                }),
                &mapping,
            )
            .expect("render"),
            ["literal", "1.25", "first", "2", "false", "--enabled", "--name", "  unchanged  ",]
        );
    }

    #[test]
    fn null_missing_object_and_non_scalar_repeated_fail_closed() {
        let scalar = [CommandArgumentV1::Scalar { source: "/value".parse().expect("pointer") }];
        assert_eq!(
            render_command_arguments_v1(&json!({}), &scalar),
            Err(ToolArgumentResolutionErrorV1::Missing)
        );
        assert_eq!(
            render_command_arguments_v1(&json!({"value": null}), &scalar),
            Err(ToolArgumentResolutionErrorV1::Null)
        );
        assert_eq!(
            render_command_arguments_v1(&json!({"value": {}}), &scalar),
            Err(ToolArgumentResolutionErrorV1::ScalarRequired)
        );
        let repeated = [CommandArgumentV1::Repeated { source: "/value".parse().expect("pointer") }];
        assert_eq!(
            render_command_arguments_v1(&json!({"value": [1, null]}), &repeated),
            Err(ToolArgumentResolutionErrorV1::ScalarArrayRequired)
        );
    }

    #[test]
    fn named_option_omission_is_explicit_and_atomic() {
        let omitted = [CommandArgumentV1::NamedOption {
            name: CommandOptionNameV1::new("--optional").expect("option"),
            source: "/value".parse().expect("pointer"),
            omit_when_missing_or_null: true,
        }];
        assert!(render_command_arguments_v1(&json!({}), &omitted).expect("omit").is_empty());
        assert!(
            render_command_arguments_v1(&json!({"value": null}), &omitted)
                .expect("omit null")
                .is_empty()
        );
    }

    #[test]
    fn json_template_preserves_native_types_and_explicit_null() {
        let template = HttpJsonTemplateV1::Array {
            items: vec![
                HttpJsonTemplateV1::Argument { source: "/object".parse().expect("pointer") },
                HttpJsonTemplateV1::Argument { source: "/null".parse().expect("pointer") },
            ],
        };
        assert_eq!(
            render_http_json_template_v1(
                &json!({"object": {"value": 1}, "null": null}),
                &template,
            )
            .expect("render JSON"),
            json!([{"value": 1}, null])
        );
    }
}
