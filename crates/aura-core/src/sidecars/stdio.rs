use super::*;

#[derive(Debug, thiserror::Error)]
pub(super) enum StdioDispatchStreamErrorV1 {
    #[error("Sandbox Host did not provide its execution Started boundary")]
    RuntimeStartedEvidenceMissing,
}

pub(super) fn reconcile_stdio_dispatch_candidate(
    outcome: &ExternalProcessExecutionOutcome,
    expected_network_enforcement: NetworkEnforcementKind,
    tracker: &StdioSandboxHostTrackerV1,
    protocol_error: Option<&StdioDispatchStreamErrorV1>,
) -> ToolRuntimeCandidateV1 {
    if outcome.state == SandboxExecutionState::StateUnknown {
        return ToolRuntimeCandidateV1::RuntimeTerminationUnknown;
    }
    if outcome.execution_id != tracker.sandbox_execution_id()
        || outcome.network_enforcement != expected_network_enforcement
        || protocol_error.is_some()
        || !stdio_host_terminal_matches_outcome(outcome, tracker.terminal_event())
    {
        return ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
    }
    match outcome.state {
        SandboxExecutionState::Cancelled => ToolRuntimeCandidateV1::CancelledAndTerminated,
        SandboxExecutionState::TimedOut => ToolRuntimeCandidateV1::TimedOutAndTerminated,
        SandboxExecutionState::PolicyViolation => ToolRuntimeCandidateV1::DeterministicFailure {
            code: aura_tool_contract::ToolRuntimeErrorCode::DispatchFailed,
        },
        SandboxExecutionState::CleanupFailed => {
            ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid
        }
        SandboxExecutionState::Exited => {
            if tracker.cleanup_failed().is_some() {
                return ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
            }
            match tracker.finish() {
                Ok(()) => ToolRuntimeCandidateV1::StdioCompleted {
                    terminal: tracker.terminal_frame().cloned(),
                    process_tree_terminated: true,
                },
                Err(StdioHostEventErrorV1::Protocol(StdioProtocolErrorV1::TerminalMissing)) => {
                    ToolRuntimeCandidateV1::StdioCompleted {
                        terminal: None,
                        process_tree_terminated: true,
                    }
                }
                Err(_) => ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid,
            }
        }
        SandboxExecutionState::Planned
        | SandboxExecutionState::Starting
        | SandboxExecutionState::Running => ToolRuntimeCandidateV1::DispatchOutcomeUnknown,
        SandboxExecutionState::StateUnknown => ToolRuntimeCandidateV1::RuntimeTerminationUnknown,
    }
}

pub(super) fn stdio_host_terminal_matches_outcome(
    outcome: &ExternalProcessExecutionOutcome,
    terminal: Option<&StdioHostTerminalEventV1>,
) -> bool {
    match (outcome.state, outcome.exit_code, terminal) {
        (
            SandboxExecutionState::Exited,
            Some(actual),
            Some(StdioHostTerminalEventV1::Exited { exit_code }),
        ) => actual == *exit_code,
        (SandboxExecutionState::Cancelled, Some(_), Some(StdioHostTerminalEventV1::Cancelled))
        | (SandboxExecutionState::TimedOut, Some(_), Some(StdioHostTerminalEventV1::TimedOut))
        | (
            SandboxExecutionState::PolicyViolation,
            Some(_),
            Some(StdioHostTerminalEventV1::PolicyViolation { .. }),
        ) => true,
        _ => false,
    }
}

pub(super) fn stdio_candidate_contains_bytes(
    candidate: &ToolRuntimeCandidateV1,
    needle: &[u8],
) -> bool {
    let ToolRuntimeCandidateV1::StdioCompleted { terminal: Some(terminal), .. } = candidate else {
        return false;
    };
    serde_json::to_vec(terminal).is_ok_and(|wire| contains_bytes(&wire, needle))
}
