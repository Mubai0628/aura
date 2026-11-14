#[test]
fn strict_probe_is_bounded_and_covers_all_six_questions_without_network() {
    let cases = aura_tool_eval::probe::cases("synthetic-owner-model");
    assert_eq!(cases.len(), 6);
    for case in cases {
        assert_eq!(case.request["tools"][0]["function"]["strict"], true);
        assert_eq!(case.request["max_tokens"], 8192);
        assert!(case.request.get("tool_choice").is_none(), "the product never forces a tool");
        assert!(serde_json::to_vec(&case.request).expect("probe JSON").len() + 4096 <= 8192);
        assert!(case.request.get("Authorization").is_none());
    }
}
