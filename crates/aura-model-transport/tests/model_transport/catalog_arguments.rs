use aura_domain_common::{Revision, ToolRegistrySnapshotId, jcs::CanonicalHashV1};
use aura_model_domain::ModelApiFormat;
use aura_model_transport::ProviderTools;
use aura_tool_contract::{
    CanonicalJsonSchemaDocumentV1, CompiledToolSchemaV1, ToolRegistrySnapshotRef,
    ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1,
};
use serde_json::{Value, json};

fn release_schemas() -> ToolSchemaSetSnapshotV1 {
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../../resources/tools/builtin-agent-release.v1.json"
    ))
    .expect("published manifest");
    let tools = manifest["entries"]
        .as_array()
        .expect("definitions")
        .iter()
        .map(|entry| {
            let d = &entry["definition"];
            ToolSchemaSnapshotV1::new(
                serde_json::from_value(d["tool_id"].clone()).expect("tool ID"),
                serde_json::from_value(d["tool_version"].clone()).expect("tool revision"),
                serde_json::from_value(d["canonical_name"].clone()).expect("canonical name"),
                d["model_description"].as_str().expect("description").into(),
                serde_json::from_value(d["input_schema"].clone()).expect("input schema"),
                d["strict"].as_bool().expect("strict preference"),
            )
            .expect("snapshot")
        })
        .collect();
    ToolSchemaSetSnapshotV1::new(
        ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"tool argument audit"),
        },
        tools,
    )
    .expect("ordered release")
}

#[test]
fn published_tools_keep_their_argument_names_units_and_reject_model_budgets_on_all_wires() {
    let schemas = release_schemas();
    for format in [
        ModelApiFormat::OpenAiResponses,
        ModelApiFormat::OpenAiChatCompletions,
        ModelApiFormat::AnthropicMessages,
    ] {
        let compiled = ProviderTools::compile(&schemas, format).expect("provider definitions");
        let (wire, bindings) = compiled.into_wire(&[]);
        assert_eq!(wire.len(), schemas.tools.len());
        for ((tool, entry), binding) in schemas.tools.iter().zip(&wire).zip(&bindings) {
            let (function, parameters) = match format {
                ModelApiFormat::OpenAiResponses => (entry, &entry["parameters"]),
                ModelApiFormat::OpenAiChatCompletions => {
                    (&entry["function"], &entry["function"]["parameters"])
                }
                ModelApiFormat::AnthropicMessages => (entry, &entry["input_schema"]),
            };
            assert_eq!(binding.tool_definition.tool_id, tool.tool_id);
            assert_eq!(function["name"], binding.provider_wire_name.as_str());
            assert_eq!(function["description"], tool.description);
            let props = parameters["properties"].as_object().expect("wire arguments");
            let declared = tool.input_schema["properties"].as_object();
            assert_eq!(props.len(), declared.map_or(0, |p| p.len()) + 1, "{}", tool.canonical_name);
            for (key, expected) in declared.into_iter().flatten() {
                assert!(props.contains_key(key), "{}.{}", tool.canonical_name, key);
                if let Some(text) = expected.get("description") {
                    assert!(
                        has_field_description(&props[key], text),
                        "{}.{key} ({format:?})",
                        tool.canonical_name
                    );
                }
            }
            assert!(props.contains_key("aura_purpose"));
            assert!(!props.contains_key("max_output_tokens"));
            // Compare admissible inputs on the actual wire for all closed union branches.
            if let Some(branches) = tool.input_schema["oneOf"].as_array() {
                let validator = CompiledToolSchemaV1::compile(
                    &CanonicalJsonSchemaDocumentV1::new(parameters.clone()).expect("wire schema"),
                )
                .expect("validator");
                for branch in branches {
                    let mut call = json!({"aura_purpose":"Check the repository"});
                    for field in branch["required"].as_array().expect("branch fields") {
                        let key = field.as_str().expect("name");
                        call[key] = branch["properties"][key]
                            .get("const")
                            .cloned()
                            .unwrap_or_else(|| json!("fixture"));
                    }
                    assert!(
                        validator.validate(&call).valid,
                        "{} {format:?}: {call}",
                        tool.canonical_name
                    );
                    call["max_output_tokens"] = json!(7000);
                    assert!(!validator.validate(&call).valid);
                }
            } else {
                assert_eq!(
                    parameters["additionalProperties"], false,
                    "{} {format:?}",
                    tool.canonical_name
                );
            }
        }
        println!("catalog wire {format:?}: {} definitions", wire.len());
    }
}

// A local reference can have its own field description beside that of its target.
// The adapter preserves both at this field using allOf when their annotations differ.
fn has_field_description(schema: &Value, text: &Value) -> bool {
    schema.get("description") == Some(text)
        || schema["allOf"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| has_field_description(part, text)))
}
