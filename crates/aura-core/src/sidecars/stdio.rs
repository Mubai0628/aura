use super::*;

#[allow(
    dead_code,
    reason = "the W7 authorization-bound adapter is committed before its gated fixture-runtime wiring"
)]
// UNWIRED: no production caller dispatches work to the sandbox host; only tests reach this.
impl ToolSandboxExecutionService {
    /// Drives the complete Core side of one authorized Aura Stdio execution.
    /// The event subscription is created before the launch request, and the
    /// Host permit remains held until the terminal response is reconciled.
    pub(crate) async fn execute_authorized_stdio_tool<F>(
        &self,
        host_permit: ToolSandboxHostPermit,
        execution: AuthorizedToolSandboxExecutionV1,
        secret: Option<ToolRuntimeSecretFrameV1>,
        mut lifecycle_update: F,
    ) -> std::result::Result<ToolStdioDispatchEvidenceV1, ToolSandboxExecutionError>
    where
        F: FnMut(
            ToolRuntimeDispatchLifecycleUpdateV1,
        ) -> std::result::Result<(), ToolSandboxExecutionError>,
    {
        let execution_id = execution.binding.sandbox_execution_id;
        let tool_call_id = execution.binding.tool_call_id;
        let tool_execution_attempt_id = execution.binding.tool_execution_attempt_id;
        let cancellation_id = execution.plan.cancellation_id;
        let expected_network_enforcement = execution.plan.network_enforcement;
        let arguments_hash = ContentHash::new(
            aura_domain_common::ContentHashAlgorithm::Sha256,
            execution.operation.arguments_hash.hex(),
        )
        .map_err(|error| ToolSandboxExecutionError::Unavailable(anyhow::Error::new(error)))?;
        let mut tracker = StdioSandboxHostTrackerV1::new(
            execution_id,
            tool_call_id,
            tool_execution_attempt_id,
            execution.operation.arguments.clone(),
            arguments_hash,
        );
        let mut credential_canary_scanner = CredentialCanaryOutputScannerV1::new(
            secret.as_ref().map(|frame| frame.secret().expose()),
        );
        let mut events = self.subscribe_events();
        let execute = self.execute_authorized_tool(host_permit, execution, secret);
        tokio::pin!(execute);
        let mut protocol_error = None;
        let mut cancellation_requested = false;
        let mut runtime_started_committed = false;

        let outcome = loop {
            tokio::select! {
                result = &mut execute => break result?,
                event = events.recv() => {
                    let result = match event {
                        Ok(value) => accept_stdio_host_notification(
                                self,
                                tool_execution_attempt_id,
                                &mut tracker,
                                &mut credential_canary_scanner,
                                value,
                            ).await.and_then(|notification| {
                                if notification.execution_started && !runtime_started_committed {
                                    lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::RuntimeStarted).map_err(
                                        StdioDispatchStreamErrorV1::RuntimeStartedCommit,
                                    )?;
                                    runtime_started_committed = true;
                                }
                                for progress in notification.progress {
                                    lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::Progress(progress))
                                        .map_err(StdioDispatchStreamErrorV1::ProgressCommit)?;
                                }
                                Ok(())
                            }),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            Err(StdioDispatchStreamErrorV1::EventStreamLagged)
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            Err(StdioDispatchStreamErrorV1::EventStreamClosed)
                        }
                    };
                    if let Err(error) = result
                        && protocol_error.is_none()
                    {
                        protocol_error = Some(error);
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
                Ok(value) if protocol_error.is_none() => {
                    if let Err(error) = accept_stdio_host_notification(
                        self,
                        tool_execution_attempt_id,
                        &mut tracker,
                        &mut credential_canary_scanner,
                        value,
                    )
                    .await
                    .and_then(|notification| {
                        if notification.execution_started && !runtime_started_committed {
                            lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::RuntimeStarted)
                                .map_err(StdioDispatchStreamErrorV1::RuntimeStartedCommit)?;
                            runtime_started_committed = true;
                        }
                        for progress in notification.progress {
                            lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::Progress(
                                progress,
                            ))
                            .map_err(StdioDispatchStreamErrorV1::ProgressCommit)?;
                        }
                        Ok(())
                    }) {
                        protocol_error = Some(error);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    protocol_error.get_or_insert(StdioDispatchStreamErrorV1::EventStreamLagged);
                    break;
                }
            }
        }

        if !runtime_started_committed && protocol_error.is_none() {
            protocol_error = Some(StdioDispatchStreamErrorV1::RuntimeStartedEvidenceMissing);
        }

        let mut candidate = reconcile_stdio_dispatch_candidate(
            &outcome,
            expected_network_enforcement,
            &tracker,
            protocol_error.as_ref(),
        );
        let mut stderr = tracker.stderr().to_vec();
        let mut internal_protocol_error = protocol_error.map(|error| error.to_string());
        let credential_canary_detected = credential_canary_scanner.detected()
            || credential_canary_scanner.canary().is_some_and(|canary| {
                stdio_candidate_contains_bytes(&candidate, canary)
                    || contains_bytes(&stderr, canary)
            });
        if credential_canary_detected {
            candidate = ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
            stderr.zeroize();
            stderr.clear();
            internal_protocol_error = Some(
                "Sandbox output contained the attempt Credential canary and was quarantined"
                    .to_owned(),
            );
        }
        let artifact_declarations = tracker.accepted_artifacts().to_vec();
        Ok(ToolStdioDispatchEvidenceV1 {
            candidate,
            stderr,
            internal_protocol_error,
            credential_canary_detected,
            artifact_declarations,
            verified_artifacts: Vec::new(),
        })
    }

    pub(crate) async fn execute_authorized_conformance_stdio_tool<F>(
        &self,
        host_permit: ToolSandboxHostPermit,
        execution: AuthorizedToolConformanceSandboxExecutionV1,
        secret: Option<ToolConformanceRuntimeSecretFrameV1>,
        mut lifecycle_update: F,
    ) -> std::result::Result<ToolStdioDispatchEvidenceV1, ToolSandboxExecutionError>
    where
        F: FnMut(
            ToolRuntimeDispatchLifecycleUpdateV1,
        ) -> std::result::Result<(), ToolSandboxExecutionError>,
    {
        let allow = &execution.binding.allow_once;
        let execution_id = allow.sandbox_execution_id;
        let tool_call_id = allow.tool_call_id;
        let tool_execution_attempt_id = allow.tool_execution_attempt_id;
        let cancellation_id = execution.plan.cancellation_id;
        let expected_network_enforcement = execution.plan.network_enforcement;
        let arguments_hash = ContentHash::new(
            aura_domain_common::ContentHashAlgorithm::Sha256,
            execution.runtime.tool_arguments_hash.hex(),
        )
        .map_err(|error| ToolSandboxExecutionError::Unavailable(anyhow::Error::new(error)))?;
        let mut tracker = StdioSandboxHostTrackerV1::new(
            execution_id,
            tool_call_id,
            tool_execution_attempt_id,
            execution.runtime.tool_arguments.clone(),
            arguments_hash,
        );
        let mut credential_canary_scanner = CredentialCanaryOutputScannerV1::new(
            secret.as_ref().map(|frame| frame.secret().expose()),
        );
        let mut events = self.subscribe_events();
        let execute = self.execute_authorized_tool_conformance(host_permit, execution, secret);
        tokio::pin!(execute);
        let mut protocol_error = None;
        let mut cancellation_requested = false;
        let mut runtime_started_committed = false;

        let outcome = loop {
            tokio::select! {
                result = &mut execute => break result?,
                event = events.recv() => {
                    let result = match event {
                        Ok(value) => accept_stdio_host_notification(
                                self,
                                tool_execution_attempt_id,
                                &mut tracker,
                                &mut credential_canary_scanner,
                                value,
                            ).await.and_then(|notification| {
                                if notification.execution_started && !runtime_started_committed {
                                    lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::RuntimeStarted).map_err(
                                        StdioDispatchStreamErrorV1::RuntimeStartedCommit,
                                    )?;
                                    runtime_started_committed = true;
                                }
                                for progress in notification.progress {
                                    lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::Progress(progress))
                                        .map_err(StdioDispatchStreamErrorV1::ProgressCommit)?;
                                }
                                Ok(())
                            }),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            Err(StdioDispatchStreamErrorV1::EventStreamLagged)
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            Err(StdioDispatchStreamErrorV1::EventStreamClosed)
                        }
                    };
                    if let Err(error) = result
                        && protocol_error.is_none()
                    {
                        protocol_error = Some(error);
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
                Ok(value) if protocol_error.is_none() => {
                    if let Err(error) = accept_stdio_host_notification(
                        self,
                        tool_execution_attempt_id,
                        &mut tracker,
                        &mut credential_canary_scanner,
                        value,
                    )
                    .await
                    .and_then(|notification| {
                        if notification.execution_started && !runtime_started_committed {
                            lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::RuntimeStarted)
                                .map_err(StdioDispatchStreamErrorV1::RuntimeStartedCommit)?;
                            runtime_started_committed = true;
                        }
                        for progress in notification.progress {
                            lifecycle_update(ToolRuntimeDispatchLifecycleUpdateV1::Progress(
                                progress,
                            ))
                            .map_err(StdioDispatchStreamErrorV1::ProgressCommit)?;
                        }
                        Ok(())
                    }) {
                        protocol_error = Some(error);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    protocol_error.get_or_insert(StdioDispatchStreamErrorV1::EventStreamLagged);
                    break;
                }
            }
        }

        if !runtime_started_committed && protocol_error.is_none() {
            protocol_error = Some(StdioDispatchStreamErrorV1::RuntimeStartedEvidenceMissing);
        }
        let mut candidate = reconcile_stdio_dispatch_candidate(
            &outcome,
            expected_network_enforcement,
            &tracker,
            protocol_error.as_ref(),
        );
        let mut stderr = tracker.stderr().to_vec();
        let mut internal_protocol_error = protocol_error.map(|error| error.to_string());
        let credential_canary_detected = credential_canary_scanner.detected()
            || credential_canary_scanner.canary().is_some_and(|canary| {
                stdio_candidate_contains_bytes(&candidate, canary)
                    || contains_bytes(&stderr, canary)
            });
        if credential_canary_detected {
            candidate = ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid;
            stderr.zeroize();
            stderr.clear();
            internal_protocol_error = Some(
                "Sandbox output contained the Conformance Credential canary and was quarantined"
                    .to_owned(),
            );
        }
        let artifact_declarations = tracker.accepted_artifacts().to_vec();
        Ok(ToolStdioDispatchEvidenceV1 {
            candidate,
            stderr,
            internal_protocol_error,
            credential_canary_detected,
            artifact_declarations,
            verified_artifacts: Vec::new(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum StdioDispatchStreamErrorV1 {
    #[error("Sandbox Host event stream lagged and lost execution evidence")]
    EventStreamLagged,
    #[error("Sandbox Host event stream closed before execution reconciliation")]
    EventStreamClosed,
    #[error("Sandbox Host event is not a valid typed event")]
    EventContract(#[source] serde_json::Error),
    #[error("Sandbox output contained the Credential canary")]
    CredentialCanaryDetected,
    #[error("Sandbox Host did not provide its execution Started boundary")]
    RuntimeStartedEvidenceMissing,
    #[error("Core could not persist the RuntimeStarted boundary")]
    RuntimeStartedCommit(#[source] ToolSandboxExecutionError),
    #[error("Core could not persist a Runtime progress update")]
    ProgressCommit(#[source] ToolSandboxExecutionError),
    #[error(transparent)]
    HostEvent(#[from] StdioHostEventErrorV1),
    #[error(transparent)]
    HostInput(#[from] ToolSandboxExecutionError),
}

pub(super) async fn accept_stdio_host_notification(
    service: &ToolSandboxExecutionService,
    tool_execution_attempt_id: aura_domain_common::ToolExecutionAttemptId,
    tracker: &mut StdioSandboxHostTrackerV1,
    credential_canary_scanner: &mut CredentialCanaryOutputScannerV1,
    value: Value,
) -> Result<StdioHostNotificationOutcomeV1, StdioDispatchStreamErrorV1> {
    let event = serde_json::from_value::<SandboxHostEvent>(value)
        .map_err(StdioDispatchStreamErrorV1::EventContract)?;
    if credential_canary_scanner.observe(&event) {
        return Err(StdioDispatchStreamErrorV1::CredentialCanaryDetected);
    }
    let execution_started = matches!(event, SandboxHostEvent::Started { .. });
    for frame in tracker.accept_host_event(event)? {
        service
            .send_stdio_input(&ToolStdioHostInputV1 {
                sandbox_execution_id: frame_sandbox_execution_id(tracker),
                tool_execution_attempt_id,
                frame,
            })
            .await?;
    }
    Ok(StdioHostNotificationOutcomeV1 { execution_started, progress: tracker.take_progress() })
}

pub(super) struct StdioHostNotificationOutcomeV1 {
    pub(super) execution_started: bool,
    pub(super) progress: Vec<StdioObservedProgressV1>,
}

pub(super) fn frame_sandbox_execution_id(
    tracker: &StdioSandboxHostTrackerV1,
) -> aura_domain_common::SandboxExecutionId {
    tracker.sandbox_execution_id()
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
