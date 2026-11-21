use aura_tool_contract::{CompiledToolSchemaV1, ToolSourceKindV1};
use aura_tool_registry::agent_release::{AgentBuiltinManifest, release};
use serde_json::{Value, json};

fn manifest() -> AgentBuiltinManifest {
    match std::env::var_os("AURA_TEST_TOOL_INPUT_MANIFEST") {
        Some(path) => serde_json::from_slice(&std::fs::read(path).expect("candidate manifest"))
            .expect("versioned agent release"),
        None => release().expect("installed definitions").manifest().clone(),
    }
}

fn accepts_shape(schema: &Value, root: &Value, kind: &str) -> bool {
    if let Some(reference) = schema["$ref"].as_str() {
        let target = root
            .pointer(reference.strip_prefix('#').expect("local reference"))
            .expect("definition");
        return accepts_shape(target, root, kind);
    }
    for union in ["anyOf", "oneOf"] {
        if let Some(variants) = schema[union].as_array() {
            return variants.iter().any(|variant| accepts_shape(variant, root, kind));
        }
    }
    let matches = |t: &str| t == kind || (kind == "integer" && t == "number");
    match &schema["type"] {
        Value::String(t) => matches(t),
        Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(matches),
        _ => true,
    }
}

#[test]
fn published_tool_input_matrix() {
    let manifest = manifest();
    let filter = std::env::var("AURA_TEST_TOOL_NAME").ok();
    let mut checked = 0;
    for entry in &manifest.entries {
        let def = &entry.definition;
        let name = def.canonical_name.as_str();
        if filter.as_deref().is_some_and(|f| f != name) {
            continue;
        }
        let input = &def.input_schema;
        let validator = CompiledToolSchemaV1::for_tool_input(input, def.source_kind).expect(name);
        let root = input.document();
        let mut cases = Vec::new();
        let mut unrestricted = Vec::new();
        for (shape, value) in [
            ("null", Value::Null),
            ("array", json!([])),
            ("string", json!("{}")),
            ("integer", json!(42)),
            ("boolean", json!(false)),
        ] {
            let report = validator.validate(&value);
            assert!(
                report
                    .errors
                    .iter()
                    .any(|e| e.instance_path.is_empty() && e.issue_kind.starts_with("type:")),
                "{name} root {shape}: {report:?}"
            );
            cases.push(format!("root/{shape}"));
        }
        if root["additionalProperties"] == false {
            let report = validator.validate(&json!({"aura_unknown_fixture_field":true}));
            assert!(
                report.errors.iter().any(|e| e.issue_kind.contains("additionalproperties")
                    && e.issue_kind.contains("aura_unknown_fixture_field")),
                "{name}: {report:?}"
            );
            cases.push("unknown-field".to_owned());
            if root["properties"].get("max_output_tokens").is_none() {
                let extra = validator.validate(&json!({"max_output_tokens":7000}));
                assert!(
                    extra.errors.iter().any(|e| e.issue_kind.contains("max_output_tokens")),
                    "{name}: {extra:?}"
                );
                cases.push("unexpected/max_output_tokens".to_owned());
            }
        }
        let empty = validator.validate(&json!({}));
        for key in root["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            assert!(
                empty.errors.iter().any(|e| e.issue_kind.starts_with("required:")
                    && e.issue_kind.contains(&format!("field /{key};"))),
                "{name} required {key}: {empty:?}"
            );
            cases.push(format!("missing/{key}"));
        }
        if root.get("required").is_none() && root.get("oneOf").is_none() {
            assert!(empty.valid, "{name}: empty object with only optional fields: {empty:?}");
        }
        for variant in root["oneOf"].as_array().into_iter().flatten() {
            let kind = variant["properties"]["kind"]["const"]
                .as_str()
                .unwrap_or_else(|| panic!("{name}: add a valid fixture for this variant"));
            let mut control = json!({"kind":kind});
            for key in
                variant["required"].as_array().into_iter().flatten().filter_map(Value::as_str)
            {
                if key != "kind" {
                    assert_eq!(
                        variant["properties"][key]["type"], "string",
                        "{name}/{kind}/{key}: add a valid fixture"
                    );
                    control[key] = json!("fixture");
                }
            }
            assert!(validator.validate(&control).valid, "{name}/{kind} control");
            cases.push(format!("variant/{kind}/valid"));
            let mut extra = control.clone();
            extra["aura_unknown_fixture_field"] = json!(true);
            assert!(!validator.validate(&extra).valid, "{name}/{kind} extra");
            cases.push(format!("variant/{kind}/unknown-field"));
            let mut budget = control.clone();
            budget["max_output_tokens"] = json!(7000);
            assert!(!validator.validate(&budget).valid, "{name}/{kind}: model budget accepted");
            cases.push(format!("variant/{kind}/unexpected/max_output_tokens"));
            for field in control.as_object().expect("variant object").keys() {
                let mut missing = control.clone();
                missing.as_object_mut().expect("variant object").remove(field);
                assert!(!validator.validate(&missing).valid, "{name}/{kind} missing {field}");
                cases.push(format!("variant/{kind}/missing/{field}"));
            }
        }
        if empty.valid {
            cases.push("empty-object-valid".to_owned());
        }
        for (key, field) in root["properties"].as_object().into_iter().flatten() {
            let shapes = [
                ("boolean", json!(false)),
                ("array", json!([])),
                ("object", json!({})),
                ("integer", json!(3)),
                ("string", json!("text")),
                ("null", Value::Null),
            ];
            if let Some((shape, value)) =
                shapes.into_iter().find(|(kind, _)| !accepts_shape(field, root, kind))
            {
                let mut args = json!({});
                args[key] = value;
                let report = validator.validate(&args);
                assert!(
                    report.errors.iter().any(|e| e.instance_path == format!("/{key}")),
                    "{name} {key} {shape}: {report:?}"
                );
                cases.push(format!("type/{key}/{shape}"));
            } else {
                unrestricted.push(key);
            }
        }
        checked += 1;
        println!(
            "AURA_TOOL_INPUT_CASES {}",
            json!({"tool":name,"revision":def.tool_version.definition_revision.value(),"schema_hash":input.schema_hash().hex(),"definition_fingerprint":def.definition_fingerprint.hex(),"passed":cases,"no_disallowed_json_type":unrestricted})
        );
    }
    assert!(checked > 0, "the requested tool must exist");
}

#[test]
fn nested_builtin_formats_and_external_schemas_keep_their_own_semantics() {
    let manifest = manifest();
    let entry = |name: &str| {
        &manifest
            .entries
            .iter()
            .find(|e| e.handler.name() == name)
            .expect("registered tool")
            .definition
    };
    let digest = "a".repeat(64);
    let hash = |hex: &str| json!({"algorithm":"blake3","hex":hex});
    let stale = "a".repeat(60);
    let expected = |h: Value| json!({"expected_kind":"file_hash","hash":h});
    let target = json!({"kind":"id","id":aura_domain_common::MemoryId::new()});
    let init = json!({"language":"chinese","expected":{"expected_kind":"absent"},"project_structure":"ROOT","commands":{"entries":[]},"coding_style":"style","testing":"tests","commits_and_pull_requests":"commits","guides":[{"name":"guide","title":"Title","body":"Body","expected":expected(hash(&digest))}]});
    let mut root_init = init.clone();
    root_init["expected"] = hash(&digest);
    for (name, mut args, pointer, good, bad, help) in [
        (
            "fs.patch",
            json!({"path":"f","unified_diff":"diff","expected_hash":hash(&digest)}),
            "/expected_hash/hex",
            json!(digest),
            json!("Z".repeat(64)),
            "fs.read",
        ),
        (
            "fs.write",
            json!({"path":"f","text":"v","expected":expected(hash(&digest))}),
            "/expected/hash/hex",
            json!(digest),
            json!(stale),
            "logical_hash",
        ),
        (
            "workspace_init.complete",
            root_init,
            "/expected/hex",
            json!(digest),
            json!("a".repeat(49)),
            "logical_hash",
        ),
        (
            "workspace_init.complete",
            init,
            "/guides/0/expected/hash/hex",
            json!(digest),
            json!("a".repeat(82)),
            "logical_hash",
        ),
        (
            "memory.update",
            json!({"scope":"project","items":[{"target":target,"expected_hash":digest}]}),
            "/items/0/expected_hash",
            json!(digest),
            json!("A".repeat(64)),
            "memory.read",
        ),
        (
            "memory.delete",
            json!({"scope":"project","items":[{"target":target,"expected_hash":digest}]}),
            "/items/0/expected_hash",
            json!(digest),
            json!("wrong"),
            "memory.read",
        ),
        (
            "todo.replace",
            json!({"expected_revision":"0","items":[]}),
            "/expected_revision",
            json!("0"),
            json!("01"),
            "无符号十进制",
        ),
        (
            "context.read_events",
            json!({"selectors":[{"type":"sequence_range","start":"0","end":"1"}]}),
            "/selectors/0/end",
            json!("1"),
            json!("18446744073709551616"),
            "无符号十进制",
        ),
        (
            "history.list_turns",
            json!({"since":"2026-11-20T00:00:00Z"}),
            "/since",
            json!("2026-11-20T00:00:00Z"),
            json!("yesterday"),
            "RFC 3339",
        ),
        (
            "tools.search",
            json!({"query":"files","limit":2}),
            "/query",
            json!("files"),
            json!("\u{007f}"),
            "关键词",
        ),
        (
            "tools.search",
            json!({"query":"files","limit":2,"cursor":"abc.1"}),
            "/cursor",
            json!("abc.1"),
            json!("invalid cursor"),
            "上一页",
        ),
        (
            "context.read_tool_result",
            json!({"tool_call_id":aura_domain_common::ToolCallId::new()}),
            "/tool_call_id",
            json!(aura_domain_common::ToolCallId::new()),
            json!(aura_domain_common::SessionId::new()),
            aura_domain_common::ToolCallId::PREFIX,
        ),
    ] {
        let schema = &entry(name).input_schema;
        let builtin =
            CompiledToolSchemaV1::for_tool_input(schema, ToolSourceKindV1::BuiltIn).expect(name);
        let external =
            CompiledToolSchemaV1::for_tool_input(schema, ToolSourceKindV1::Plugin).expect(name);
        *args.pointer_mut(pointer).expect("fixture path") = good;
        let valid = builtin.validate(&args);
        assert!(valid.valid, "{name} control: {valid:?}");
        *args.pointer_mut(pointer).expect("fixture path") = bad.clone();
        let report = builtin.validate(&args);
        assert!(
            report.errors.iter().any(|e| e.instance_path == pointer && e.issue_kind.contains(help)),
            "{name} {pointer}: {report:?}"
        );
        assert!(external.validate(&args).valid, "an external tool owns its schema types: {name}");
        assert!(!format!("{:?}", report.errors).contains(bad.as_str().expect("string fixture")));
    }
}

#[test]
fn empty_input_names_extra_fields_once_without_echoing_their_content() {
    let manifest = manifest();
    let def = &manifest
        .entries
        .iter()
        .find(|e| e.handler.name() == "todo.get")
        .expect("todo.get entry")
        .definition;
    let validator = CompiledToolSchemaV1::for_tool_input(&def.input_schema, def.source_kind)
        .expect("compiled todo.get input");
    let report = validator.validate(&json!({"a/b": "private fixture value", "a~b": 17}));
    assert!(!report.valid);
    assert_eq!(report.errors.len(), 1);
    let hint = &report.errors[0].issue_kind;
    for text in ["additionalproperties", "/a~1b", "/a~0b", "请移除"] {
        assert!(hint.contains(text), "{hint}");
    }
    assert!(!hint.contains("private fixture value"));
}
