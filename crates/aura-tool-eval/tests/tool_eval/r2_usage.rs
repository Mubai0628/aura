use aura_tool_eval::meter::read_usage;
use serde_json::json;

fn event(kind: &str, input: u64, output: u64) -> String {
    let value = json!({"type":kind,"response":{"usage":{"input_tokens":input,"input_tokens_details":{"cached_tokens":7},"output_tokens":output,"output_tokens_details":{"reasoning_tokens":3}}}});
    format!("event: {kind}\ndata: {value}\n\n")
}

#[test]
fn r2_responses_usage_only_comes_from_completed_or_incomplete_events() {
    for terminal in ["response.completed", "response.incomplete"] {
        let stream = event("response.created", 999, 40) + &event(terminal, 10, 4);
        let usage =
            read_usage(stream.as_bytes()).expect("only the terminal usage is authoritative");
        assert_eq!(usage.input, 10);
        assert_eq!(usage.output, 4);
        assert_eq!(usage.cached_input, 7);
        assert_eq!(usage.reasoning_tokens, Some(3));
    }
    assert!(
        read_usage(event("response.created", 10, 4).as_bytes()).is_err(),
        "nonterminal telemetry cannot settle the ledger"
    );
}

#[test]
fn r2_conflicting_terminal_usage_still_fails_closed() {
    let stream = event("response.completed", 10, 4) + &event("response.incomplete", 11, 4);
    assert!(read_usage(stream.as_bytes()).is_err());
}
