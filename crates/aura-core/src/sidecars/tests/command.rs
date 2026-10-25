use super::*;

#[test]
fn w7_host_adapter_command_requires_matching_host_terminal_and_network_mode() {
    let execution_id = SandboxExecutionId::new();
    let mut tracker = CommandSandboxHostTrackerV1::new(execution_id);
    tracker
        .accept_host_event(SandboxHostEvent::StdoutChunk {
            execution_id,
            sequence: 0,
            bytes: b"ok".to_vec(),
        })
        .expect("stdout");
    tracker.accept_host_event(SandboxHostEvent::Exit { execution_id, exit_code: 0 }).expect("exit");
    let outcome = ExternalProcessExecutionOutcome {
        execution_id,
        state: SandboxExecutionState::Exited,
        process_id: Some(42),
        exit_code: Some(0),
        network_enforcement: NetworkEnforcementKind::AppContainerNetworkDenied,
    };
    assert_eq!(
        reconcile_command_dispatch_candidate(
            &outcome,
            NetworkEnforcementKind::AppContainerNetworkDenied,
            &tracker,
            None,
            vec![0],
        ),
        ToolRuntimeCandidateV1::CommandExited {
            exit_code: 0,
            success_exit_codes: vec![0],
            stdout: b"ok".to_vec(),
            process_tree_terminated: true,
        }
    );
    assert_eq!(
        reconcile_command_dispatch_candidate(
            &outcome,
            NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
            &tracker,
            None,
            vec![0],
        ),
        ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid
    );
}

#[test]
fn w7_runtime_started_callback_is_keyed_to_the_bound_host_started_event() {
    let execution_id = SandboxExecutionId::new();
    let mut tracker = CommandSandboxHostTrackerV1::new(execution_id);
    assert!(
        accept_command_host_notification(
            &mut tracker,
            serde_json::to_value(SandboxHostEvent::Started { execution_id })
                .expect("Started event"),
        )
        .expect("accept Started")
    );
    assert!(
        !accept_command_host_notification(
            &mut tracker,
            serde_json::to_value(SandboxHostEvent::ProcessStarted { execution_id, process_id: 42 })
                .expect("ProcessStarted event"),
        )
        .expect("accept ProcessStarted")
    );
    let wrong_execution = SandboxExecutionId::new();
    assert!(
        accept_command_host_notification(
            &mut tracker,
            serde_json::to_value(SandboxHostEvent::Started { execution_id: wrong_execution })
                .expect("wrong Started event"),
        )
        .is_err(),
        "a different execution must never advance the bound Attempt",
    );
}
