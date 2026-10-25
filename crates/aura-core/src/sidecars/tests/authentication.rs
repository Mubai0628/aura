use super::*;

#[test]
fn proof_comparison_rejects_length_and_value_mismatches() {
    assert!(constant_time_equal(b"same", b"same"));
    assert!(!constant_time_equal(b"same", b"diff"));
    assert!(!constant_time_equal(b"same", b"longer"));
}

#[test]
fn sidecar_failure_preserves_the_complete_typed_error_envelope() {
    let id = CorrelationId::new();
    let envelope = IpcErrorEnvelope {
        code: aura_domain_common::ErrorCode::new("sandbox.appcontainer_network_denied")
            .expect("error code"),
        category: aura_domain_common::ErrorCategory::Permission,
        message: "network access requires an explicit native-networked replan".to_owned(),
        retryable: false,
        correlation_id: id,
        terminal: aura_runtime_contract::IpcRequestTerminal::Failed,
        details: BTreeMap::from([(
            "network_policy".to_owned(),
            json!({
                "required_declaration": "required",
                "allowed_networked_modes": [{
                    "mode_kind": "native",
                    "mode": "observed_native_networked"
                }]
            }),
        )]),
    };
    let response = JsonRpcResponse::failure(id, envelope.clone());
    let frame = WireFrame::Control(serde_json::to_vec(&response).expect("response frame"));
    let (reply_tx, reply_rx) = oneshot::channel();
    let mut pending = BTreeMap::from([(id, PendingRequest::Caller(reply_tx))]);
    let mut heartbeat = None;
    let (events, _) = broadcast::channel(1);

    handle_inbound(frame, &mut pending, &mut heartbeat, &events).expect("handle Sidecar failure");
    let received =
        reply_rx.blocking_recv().expect("caller terminal").expect_err("failure envelope");
    assert_eq!(received.code, envelope.code);
    assert_eq!(received.details, envelope.details);
    assert_eq!(received.terminal, envelope.terminal);
}
