#![allow(clippy::unwrap_used)]
use aura_tool_contract::CompiledToolSchemaV1;
use aura_tool_registry::agent_release::release;
use serde_json::json;

#[test]
fn output_limit_corrections_use_each_tools_own_unit() {
    let release = release().unwrap();
    for (name, key, unit) in [
        ("shell.powershell", "max_output_bytes", "bytes"),
        ("shell.cmd", "max_output_bytes", "bytes"),
        ("shell.query", "max_bytes", "bytes"),
        ("web.read", "max_chars", "characters"),
        ("git.diff", "max_bytes", "bytes"),
        ("terminal.read", "max_lines", "lines"),
        ("chrome.console", "max_entries", "entries"),
        ("web.search", "max_results", "results"),
        ("computer.snapshot", "max_elements", "elements"),
    ] {
        let d = &release.entry(name).unwrap().definition;
        let validator =
            CompiledToolSchemaV1::for_tool_input(&d.input_schema, d.source_kind).unwrap();
        for args in [json!({"max_output_tokens":7000}), json!({key:200,"max_output_tokens":7000})] {
            let result = validator.validate(&args);
            let error =
                result.errors.iter().find(|e| e.issue_kind.contains("max_output_tokens")).unwrap();
            assert!(
                error.issue_kind.contains(&format!("{key}` ({unit})")),
                "{name}: {}",
                error.issue_kind
            );
            assert!(!error.issue_kind.contains("7000"));
        }
    }
}

#[test]
fn published_hard_limits_match_the_execution_boundary() {
    let release = release().unwrap();
    for (name, key, min, max) in [
        ("shell.powershell", "max_output_bytes", 1, 1048576),
        ("shell.cmd", "timeout_ms", 1, 86400000),
        ("shell.query", "max_bytes", 1, 65536),
        ("media.frames", "max_side", 64, 2048),
    ] {
        let d = &release.entry(name).unwrap().definition;
        let validator =
            CompiledToolSchemaV1::for_tool_input(&d.input_schema, d.source_kind).unwrap();
        for (value, valid) in [(min - 1, false), (min, true), (max, true), (max + 1, false)] {
            let report = validator.validate(&json!({key:value}));
            assert_eq!(
                !report.errors.iter().any(|e| e.instance_path == format!("/{key}")),
                valid,
                "{name}.{key}={value}"
            );
        }
    }
}

#[test]
fn chrome_fields_describe_the_units_used_by_the_shared_tab() {
    let release = release().unwrap();
    for (name, field, words) in [
        ("chrome.scroll", "amount", &["viewport", "20"][..]),
        ("chrome.press", "repeat", &["50"][..]),
        ("chrome.press", "hold_ms", &["5000"][..]),
        ("chrome.wait", "time_ms", &["60000"][..]),
        ("chrome.wait", "timeout_ms", &["60000"][..]),
        ("chrome.console", "max_entries", &["500"][..]),
        ("chrome.snapshot", "max_chars", &["2000", "60000"][..]),
        ("browser.snapshot", "max_chars", &["2000", "60000"][..]),
    ] {
        let schema = release.entry(name).unwrap().definition.input_schema.document();
        let description = schema["properties"][field]["description"].as_str().unwrap();
        assert!(
            words.iter().all(|word| description.contains(word)),
            "{name}.{field}: {description}"
        );
    }
}

#[test]
fn a_tool_with_its_own_token_budget_keeps_its_declared_units() {
    let schema = aura_tool_contract::ToolInputSchemaV1::new(json!({
        "type":"object","additionalProperties":false,
        "properties":{"max_tokens":{"type":"integer"},"max_bytes":{"type":"integer"}}
    }))
    .expect("external tool with independent controls");
    let validator =
        CompiledToolSchemaV1::compile(schema.canonical_document()).expect("published input");
    assert!(validator.validate(&json!({"max_tokens":500,"max_bytes":4096})).valid);
    let refused = validator.validate(&json!({"max_output_tokens":500}));
    assert!(!refused.valid);
    assert!(
        refused.errors.iter().all(|error| !error.issue_kind.contains("This tool's output limit"))
    );
}
