//! Authentication is unchanged; the lane only orders each terminal's inputs.
use super::*;
use aura_domain_common::{CorrelationId, SessionId};

fn input(session: SessionId, terminal: &str, value: &str) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: aura_ipc::JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: None,
        method: "terminal.user.write".into(),
        params: serde_json::json!({"session_id":session,"terminal_id":terminal,"input":value,"interrupt":false}),
    }
}

#[tokio::test]
async fn l03_b0_one_terminal_inputs_are_ordered_and_other_terminals_stay_independent() {
    let session = SessionId::new();
    let first = input(session, "terminal-A", "first");
    let second = input(session, "terminal-A", "second");
    let other = input(session, "terminal-B", "other");
    assert_eq!(lane_key_for(&first), lane_key_for(&second));
    assert_ne!(lane_key_for(&first), lane_key_for(&other));
    let mut close = first.clone();
    close.method = "terminal.user.close".into();
    close.params = serde_json::json!({"session_id":session,"terminal_id":"terminal-A"});
    assert_eq!(lane_key_for(&first), lane_key_for(&close), "closing orders after accepted inputs");
    let mut lanes = IpcSession::new();
    assert_eq!(lanes.admit(first.clone()), Admission::Accepted);
    let ready = lanes.take_ready();
    let (ticket, request) = ready.into_iter().next().expect("first admitted").into_parts();
    assert_eq!(request.id, first.id);
    lanes.attach(ticket, tokio::spawn(std::future::pending::<HandlerTerminal>()));
    assert_eq!(lanes.admit(second), Admission::Accepted);
    assert!(lanes.take_ready().is_empty(), "same terminal cannot overtake the pending first input");
    assert_eq!(lanes.admit(other.clone()), Admission::Accepted);
    let ready = lanes.take_ready();
    assert_eq!(ready.len(), 1, "different terminal can make progress");
    assert_eq!(ready.into_iter().next().expect("other admitted").into_parts().1.id, other.id);
    lanes.shutdown().await;
}

#[test]
fn l03_b0_malformed_input_cannot_gain_an_independent_terminal_lane() {
    let mut request = input(SessionId::new(), "terminal-A", "typed");
    request.params["input"] = serde_json::json!(42);
    assert_eq!(lane_key_for(&request), LaneKey::Connection(Lane::Serial));
}
