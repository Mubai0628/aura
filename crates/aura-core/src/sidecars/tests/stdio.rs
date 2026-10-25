use super::*;

#[test]
fn stdio_canary_scanner_covers_split_raw_stdout_before_progress_publication() {
    let execution_id = SandboxExecutionId::new();
    let canary = b"aura-conformance-canary-v1-deadbeef";
    let mut scanner = CredentialCanaryOutputScannerV1::new(Some(canary));
    assert!(!scanner.observe(&SandboxHostEvent::StdoutChunk {
        execution_id,
        sequence: 0,
        bytes: b"prefix aura-conformance-canary-v1-".to_vec(),
    }));
    assert!(scanner.observe(&SandboxHostEvent::StdoutChunk {
        execution_id,
        sequence: 1,
        bytes: b"deadbeef suffix".to_vec(),
    }));
    assert!(scanner.detected());
}

#[test]
fn stdio_canary_scanner_does_not_join_independent_streams() {
    let execution_id = SandboxExecutionId::new();
    let canary = b"split-secret";
    let mut scanner = CredentialCanaryOutputScannerV1::new(Some(canary));
    assert!(!scanner.observe(&SandboxHostEvent::StdoutChunk {
        execution_id,
        sequence: 0,
        bytes: b"split-".to_vec(),
    }));
    assert!(!scanner.observe(&SandboxHostEvent::StderrChunk {
        execution_id,
        sequence: 0,
        bytes: b"secret".to_vec(),
    }));
    assert!(!scanner.detected());
}

#[test]
fn w7_host_adapter_stdio_requires_matching_host_and_protocol_terminals() {
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use aura_tool_contract::{
        STDIO_PROTOCOL_VERSION_V1, StdioFinalResultPayloadV1, StdioHelloPayloadV1,
        StdioProtocolFrameV1,
    };
    use aura_tool_runtime::encode_stdio_frame_bytes;

    let execution_id = SandboxExecutionId::new();
    let tool_call_id = ToolCallId::new();
    let attempt_id = ToolExecutionAttemptId::new();
    let mut tracker = StdioSandboxHostTrackerV1::new(
        execution_id,
        tool_call_id,
        attempt_id,
        json!({}),
        ContentHash::sha256(b"arguments"),
    );
    let hello = StdioProtocolFrameV1::Hello {
        protocol_version: STDIO_PROTOCOL_VERSION_V1,
        tool_call_id,
        tool_execution_attempt_id: attempt_id,
        sequence: 1,
        payload: StdioHelloPayloadV1 {
            runtime_name: "fixture".to_owned(),
            runtime_version: "1.0.0".to_owned(),
        },
    };
    let outbound = tracker
        .accept_host_event(SandboxHostEvent::StdoutChunk {
            execution_id,
            sequence: 0,
            bytes: encode_stdio_frame_bytes(&hello).expect("hello"),
        })
        .expect("accept hello");
    assert_eq!(outbound.len(), 1);
    let terminal = StdioProtocolFrameV1::FinalResult {
        protocol_version: STDIO_PROTOCOL_VERSION_V1,
        tool_call_id,
        tool_execution_attempt_id: attempt_id,
        sequence: 2,
        payload: StdioFinalResultPayloadV1 { result: json!({"ok": true}) },
    };
    tracker
        .accept_host_event(SandboxHostEvent::StdoutChunk {
            execution_id,
            sequence: 1,
            bytes: encode_stdio_frame_bytes(&terminal).expect("terminal"),
        })
        .expect("accept terminal");
    tracker
        .accept_host_event(SandboxHostEvent::Exit { execution_id, exit_code: 0 })
        .expect("accept exit");
    let outcome = ExternalProcessExecutionOutcome {
        execution_id,
        state: SandboxExecutionState::Exited,
        process_id: Some(42),
        exit_code: Some(0),
        network_enforcement: NetworkEnforcementKind::AppContainerNetworkDenied,
    };
    assert!(matches!(
        reconcile_stdio_dispatch_candidate(
            &outcome,
            NetworkEnforcementKind::AppContainerNetworkDenied,
            &tracker,
            None,
        ),
        ToolRuntimeCandidateV1::StdioCompleted {
            terminal: Some(StdioProtocolFrameV1::FinalResult { .. }),
            process_tree_terminated: true,
        }
    ));

    let changed_outcome = ExternalProcessExecutionOutcome { exit_code: Some(1), ..outcome };
    assert_eq!(
        reconcile_stdio_dispatch_candidate(
            &changed_outcome,
            NetworkEnforcementKind::AppContainerNetworkDenied,
            &tracker,
            None,
        ),
        ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid
    );

    let cancelled_execution_id = SandboxExecutionId::new();
    let mut cancelled_tracker = StdioSandboxHostTrackerV1::new(
        cancelled_execution_id,
        tool_call_id,
        attempt_id,
        json!({}),
        ContentHash::sha256(b"cancelled arguments"),
    );
    cancelled_tracker
        .accept_host_event(SandboxHostEvent::Started { execution_id: cancelled_execution_id })
        .expect("accept cancelled execution start");
    cancelled_tracker
        .accept_host_event(SandboxHostEvent::Cancelled { execution_id: cancelled_execution_id })
        .expect("accept cancelled terminal");
    let cancelled_outcome = ExternalProcessExecutionOutcome {
        execution_id: cancelled_execution_id,
        state: SandboxExecutionState::Cancelled,
        process_id: Some(43),
        exit_code: Some(1),
        network_enforcement: NetworkEnforcementKind::AppContainerNetworkDenied,
    };
    assert_eq!(
        reconcile_stdio_dispatch_candidate(
            &cancelled_outcome,
            NetworkEnforcementKind::AppContainerNetworkDenied,
            &cancelled_tracker,
            Some(&StdioDispatchStreamErrorV1::RuntimeStartedEvidenceMissing),
        ),
        ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid,
        "a Host cancellation cannot hide missing RuntimeStarted evidence",
    );
}

#[test]
fn w7_attempt_credential_canary_is_detected_without_logging_the_secret() {
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use aura_tool_contract::{
        STDIO_PROTOCOL_VERSION_V1, StdioFinalResultPayloadV1, StdioProtocolFrameV1,
    };

    let canary = b"aura-fixture-credential-canary";
    let command = ToolRuntimeCandidateV1::CommandExited {
        exit_code: 0,
        success_exit_codes: vec![0],
        stdout: [b"prefix:".as_slice(), canary].concat(),
        process_tree_terminated: true,
    };
    assert!(command_candidate_contains_bytes(&command, canary));

    let stdio = ToolRuntimeCandidateV1::StdioCompleted {
        terminal: Some(StdioProtocolFrameV1::FinalResult {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id: ToolCallId::new(),
            tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            sequence: 2,
            payload: StdioFinalResultPayloadV1 {
                result: json!({"leak": "aura-fixture-credential-canary"}),
            },
        }),
        process_tree_terminated: true,
    };
    assert!(stdio_candidate_contains_bytes(&stdio, canary));
    assert!(!contains_bytes(b"safe output", canary));
    assert!(!contains_bytes(canary, b""));
}
