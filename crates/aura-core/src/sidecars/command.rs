use super::*;

#[allow(
    dead_code,
    reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
)]
impl ToolSandboxExecutionService {
    /// Drives one authorized declarative command while retaining the Host
    /// permit until the response and all queued execution evidence have been
    /// reconciled. Invalid or missing event evidence is never treated as a
    /// successful command result.
    pub(crate) async fn execute_authorized_command_tool<F>(
        &self,
        host_permit: ToolSandboxHostPermit,
        execution: AuthorizedToolSandboxExecutionV1,
        secret: Option<ToolRuntimeSecretFrameV1>,
        success_exit_codes: Vec<i32>,
        mut runtime_started: F,
    ) -> std::result::Result<ToolCommandDispatchEvidenceV1, ToolSandboxExecutionError>
    where
        F: FnMut() -> std::result::Result<(), ToolSandboxExecutionError>,
    {
        let execution_id = execution.binding.sandbox_execution_id;
        let cancellation_id = execution.plan.cancellation_id;
        let expected_network_enforcement = execution.plan.network_enforcement;
        let mut tracker = CommandSandboxHostTrackerV1::new(execution_id);
        let credential_canary =
            secret.as_ref().map(|frame| Zeroizing::new(frame.secret().expose().to_vec()));
        let mut events = self.subscribe_events();
        let execute = self.execute_authorized_tool(host_permit, execution, secret);
        tokio::pin!(execute);
        let mut event_error = None;
        let mut cancellation_requested = false;
        let mut runtime_started_committed = false;

        let outcome = loop {
            tokio::select! {
                result = &mut execute => break result?,
                event = events.recv() => {
                    let result = match event {
                        Ok(value) => accept_command_host_notification(&mut tracker, value).and_then(
                            |execution_started| {
                                if execution_started && !runtime_started_committed {
                                    runtime_started().map_err(
                                        CommandDispatchStreamErrorV1::RuntimeStartedCommit,
                                    )?;
                                    runtime_started_committed = true;
                                }
                                Ok(())
                            },
                        ),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            Err(CommandDispatchStreamErrorV1::EventStreamLagged)
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            Err(CommandDispatchStreamErrorV1::EventStreamClosed)
                        }
                    };
                    if let Err(error) = result
                        && event_error.is_none()
                    {
                        event_error = Some(error);
                        if !cancellation_requested {
                            let _ = self.cancel(cancellation_id).await;
                            cancellation_requested = true;
                        }
                    }
                }
            }
        };

        loop {
            match events.try_recv() {
                Ok(value) if event_error.is_none() => {
                    if let Err(error) = accept_command_host_notification(&mut tracker, value)
                        .and_then(|execution_started| {
                            if execution_started && !runtime_started_committed {
                                runtime_started()
                                    .map_err(CommandDispatchStreamErrorV1::RuntimeStartedCommit)?;
                                runtime_started_committed = true;
                            }
                            Ok(())
                        })
                    {
                        event_error = Some(error);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    event_error.get_or_insert(CommandDispatchStreamErrorV1::EventStreamLagged);
                    break;
                }
            }
        }

        if !runtime_started_committed && event_error.is_none() {
            event_error = Some(CommandDispatchStreamErrorV1::RuntimeStartedEvidenceMissing);
        }

        let mut candidate = reconcile_command_dispatch_candidate(
            &outcome,
            expected_network_enforcement,
            &tracker,
            event_error.as_ref(),
            success_exit_codes,
        );
        let mut stderr = tracker.stderr().to_vec();
        let mut internal_event_error = event_error.map(|error| error.to_string());
        let credential_canary_detected = credential_canary.as_ref().is_some_and(|canary| {
            command_candidate_contains_bytes(&candidate, canary) || contains_bytes(&stderr, canary)
        });
        if credential_canary_detected {
            candidate = ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
            stderr.zeroize();
            stderr.clear();
            internal_event_error = Some(
                "Sandbox output contained the attempt Credential canary and was quarantined"
                    .to_owned(),
            );
        }
        Ok(ToolCommandDispatchEvidenceV1 {
            candidate,
            stderr,
            internal_event_error,
            credential_canary_detected,
        })
    }

    /// W3 Command counterpart. The Host consumes the dedicated `tcr_`-bound
    /// AllowOnce relation and never observes an installed Registry/Batch
    /// identity.
    pub(crate) async fn execute_authorized_conformance_command_tool<F>(
        &self,
        host_permit: ToolSandboxHostPermit,
        execution: AuthorizedToolConformanceSandboxExecutionV1,
        secret: Option<ToolConformanceRuntimeSecretFrameV1>,
        success_exit_codes: Vec<i32>,
        mut runtime_started: F,
    ) -> std::result::Result<ToolCommandDispatchEvidenceV1, ToolSandboxExecutionError>
    where
        F: FnMut() -> std::result::Result<(), ToolSandboxExecutionError>,
    {
        let execution_id = execution.binding.allow_once.sandbox_execution_id;
        let cancellation_id = execution.plan.cancellation_id;
        let expected_network_enforcement = execution.plan.network_enforcement;
        let mut tracker = CommandSandboxHostTrackerV1::new(execution_id);
        let credential_canary =
            secret.as_ref().map(|frame| Zeroizing::new(frame.secret().expose().to_vec()));
        let mut events = self.subscribe_events();
        let execute = self.execute_authorized_tool_conformance(host_permit, execution, secret);
        tokio::pin!(execute);
        let mut event_error = None;
        let mut cancellation_requested = false;
        let mut runtime_started_committed = false;

        let outcome = loop {
            tokio::select! {
                result = &mut execute => break result?,
                event = events.recv() => {
                    let result = match event {
                        Ok(value) => accept_command_host_notification(&mut tracker, value).and_then(
                            |execution_started| {
                                if execution_started && !runtime_started_committed {
                                    runtime_started().map_err(
                                        CommandDispatchStreamErrorV1::RuntimeStartedCommit,
                                    )?;
                                    runtime_started_committed = true;
                                }
                                Ok(())
                            },
                        ),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            Err(CommandDispatchStreamErrorV1::EventStreamLagged)
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            Err(CommandDispatchStreamErrorV1::EventStreamClosed)
                        }
                    };
                    if let Err(error) = result
                        && event_error.is_none()
                    {
                        event_error = Some(error);
                        if !cancellation_requested {
                            let _ = self.cancel(cancellation_id).await;
                            cancellation_requested = true;
                        }
                    }
                }
            }
        };

        loop {
            match events.try_recv() {
                Ok(value) if event_error.is_none() => {
                    if let Err(error) = accept_command_host_notification(&mut tracker, value)
                        .and_then(|execution_started| {
                            if execution_started && !runtime_started_committed {
                                runtime_started()
                                    .map_err(CommandDispatchStreamErrorV1::RuntimeStartedCommit)?;
                                runtime_started_committed = true;
                            }
                            Ok(())
                        })
                    {
                        event_error = Some(error);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    event_error.get_or_insert(CommandDispatchStreamErrorV1::EventStreamLagged);
                    break;
                }
            }
        }

        if !runtime_started_committed && event_error.is_none() {
            event_error = Some(CommandDispatchStreamErrorV1::RuntimeStartedEvidenceMissing);
        }
        let mut candidate = reconcile_command_dispatch_candidate(
            &outcome,
            expected_network_enforcement,
            &tracker,
            event_error.as_ref(),
            success_exit_codes,
        );
        let mut stderr = tracker.stderr().to_vec();
        let mut internal_event_error = event_error.map(|error| error.to_string());
        let credential_canary_detected = credential_canary.as_ref().is_some_and(|canary| {
            command_candidate_contains_bytes(&candidate, canary) || contains_bytes(&stderr, canary)
        });
        if credential_canary_detected {
            candidate = ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
            stderr.zeroize();
            stderr.clear();
            internal_event_error = Some(
                "Sandbox output contained the Conformance Credential canary and was quarantined"
                    .to_owned(),
            );
        }
        Ok(ToolCommandDispatchEvidenceV1 {
            candidate,
            stderr,
            internal_event_error,
            credential_canary_detected,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CommandDispatchStreamErrorV1 {
    #[error("Sandbox Host event stream lagged and lost command execution evidence")]
    EventStreamLagged,
    #[error("Sandbox Host event stream closed before command execution reconciliation")]
    EventStreamClosed,
    #[error("Sandbox Host event is not a valid typed event")]
    EventContract(#[source] serde_json::Error),
    #[error("Sandbox Host did not provide its execution Started boundary")]
    RuntimeStartedEvidenceMissing,
    #[error("Core could not persist the RuntimeStarted boundary")]
    RuntimeStartedCommit(#[source] ToolSandboxExecutionError),
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
