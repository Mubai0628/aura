use aura_tool_eval::snapshots::current_requests;

#[test]
fn reconcile_keeps_its_conditional_schema_in_all_three_protocols() {
    let release = aura_tool_registry::agent_release::release().expect("tool release");
    let definition = &release.entry("task_lineage.reconcile").expect("reconcile tool").definition;
    let name = aura_model_domain::provider_wire_tool_name(
        definition.canonical_name.as_str(),
        definition.tool_id,
    );
    for (protocol, request) in current_requests().expect("compile real tool declarations") {
        let tool = request["body"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool.get("function").unwrap_or(tool))
            .find(|tool| tool["name"] == name)
            .expect("reconcile is advertised");
        let schema = tool.get("parameters").unwrap_or(&tool["input_schema"]);
        for keyword in ["if", "then", "else", "allOf"] {
            assert_eq!(
                schema.get(keyword).expect("reconcile condition"),
                &definition.input_schema.document()[keyword],
                "{protocol} changed {keyword}"
            );
        }
        match protocol.as_str() {
            "openai-responses" => assert_eq!(tool["strict"], false),
            "openai-chat" | "anthropic" => assert!(tool.get("strict").is_none()),
            _ => panic!("unexpected protocol {protocol}"),
        }
    }
}

#[test]
fn three_final_adapter_requests_match_the_current_green_baseline() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/golden");
    for (name, current) in current_requests().expect("compile the three actual adapters") {
        let stored = std::fs::read(root.join(format!("{name}.json")))
            .expect("frozen pre-interface-change golden request");
        let expected: serde_json::Value = serde_json::from_slice(&stored).expect("golden JSON");
        assert_eq!(current, expected, "final request changed for {name}");
    }
}
