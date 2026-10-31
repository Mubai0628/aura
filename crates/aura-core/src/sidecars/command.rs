use super::*;

#[derive(Debug, thiserror::Error)]
pub(super) enum CommandDispatchStreamErrorV1 {
    #[error("Sandbox Host event is not a valid typed event")]
    EventContract(#[source] serde_json::Error),
    #[error(transparent)]
    HostEvent(#[from] CommandHostEventErrorV1),
}

pub(super) fn accept_command_host_notification(
    tracker: &mut CommandSandboxHostTrackerV1,
    value: Value,
) -> Result<bool, CommandDispatchStreamErrorV1> {
    let event = serde_json::from_value::<SandboxHostEvent>(value)
        .map_err(CommandDispatchStreamErrorV1::EventContract)?;
    let execution_started = matches!(event, SandboxHostEvent::Started { .. });
    tracker.accept_host_event(event).map_err(CommandDispatchStreamErrorV1::from)?;
    Ok(execution_started)
}

pub(super) fn reconcile_command_dispatch_candidate(
    outcome: &ExternalProcessExecutionOutcome,
    expected_network_enforcement: NetworkEnforcementKind,
    tracker: &CommandSandboxHostTrackerV1,
    event_error: Option<&CommandDispatchStreamErrorV1>,
    success_exit_codes: Vec<i32>,
) -> ToolRuntimeCandidateV1 {
    if outcome.state == SandboxExecutionState::StateUnknown {
        return ToolRuntimeCandidateV1::RuntimeTerminationUnknown;
    }
    if outcome.execution_id != tracker.sandbox_execution_id()
        || outcome.network_enforcement != expected_network_enforcement
        || event_error.is_some()
        || tracker.cleanup_failed().is_some()
        || !command_host_terminal_matches_outcome(outcome, tracker.terminal_event())
    {
        return ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
    }
    match outcome.state {
        SandboxExecutionState::Exited => {
            let Some(exit_code) = outcome.exit_code else {
                return ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
            };
            ToolRuntimeCandidateV1::CommandExited {
                exit_code: i32::from_le_bytes(exit_code.to_le_bytes()),
                success_exit_codes,
                stdout: tracker.stdout().to_vec(),
                process_tree_terminated: true,
            }
        }
        SandboxExecutionState::Cancelled => ToolRuntimeCandidateV1::CancelledAndTerminated,
        SandboxExecutionState::TimedOut => ToolRuntimeCandidateV1::TimedOutAndTerminated,
        SandboxExecutionState::PolicyViolation => ToolRuntimeCandidateV1::DeterministicFailure {
            code: aura_tool_contract::ToolRuntimeErrorCode::DispatchFailed,
        },
        SandboxExecutionState::Planned
        | SandboxExecutionState::Starting
        | SandboxExecutionState::Running => ToolRuntimeCandidateV1::DispatchOutcomeUnknown,
        SandboxExecutionState::CleanupFailed => {
            ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid
        }
        SandboxExecutionState::StateUnknown => ToolRuntimeCandidateV1::RuntimeTerminationUnknown,
    }
}

pub(super) fn command_host_terminal_matches_outcome(
    outcome: &ExternalProcessExecutionOutcome,
    terminal: Option<&CommandHostTerminalEventV1>,
) -> bool {
    match (outcome.state, outcome.exit_code, terminal) {
        (
            SandboxExecutionState::Exited,
            Some(actual),
            Some(CommandHostTerminalEventV1::Exited { exit_code }),
        ) => actual == *exit_code,
        (
            SandboxExecutionState::Cancelled,
            Some(_),
            Some(CommandHostTerminalEventV1::Cancelled),
        )
        | (SandboxExecutionState::TimedOut, Some(_), Some(CommandHostTerminalEventV1::TimedOut))
        | (
            SandboxExecutionState::PolicyViolation,
            Some(_),
            Some(CommandHostTerminalEventV1::PolicyViolation { .. }),
        ) => true,
        _ => false,
    }
}

pub(super) fn command_candidate_contains_bytes(
    candidate: &ToolRuntimeCandidateV1,
    needle: &[u8],
) -> bool {
    matches!(
        candidate,
        ToolRuntimeCandidateV1::CommandExited { stdout, .. } if contains_bytes(stdout, needle)
    )
}
