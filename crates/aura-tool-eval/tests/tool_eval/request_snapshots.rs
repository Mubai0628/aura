use aura_tool_eval::snapshots::current_requests;

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
