use super::*;

impl CommittedAuthorizedToolAttemptV1 {
    /// Crosses the one-shot Host boundary and persists Running only after the
    /// Host emits the matching execution `Started` event. Failure to persist
    /// that boundary requests cancellation inside the Host adapter and leaves
    /// the durable DispatchPrepared attempt for fail-closed recovery.
    pub async fn dispatch(
        self,
        persistence: &PersistenceCoordinator,
        sync: &crate::sync_boundary::SyncBoundary,
        sandbox: &ToolSandboxExecutionService,
        secret: Option<ToolRuntimeSecretFrameV1>,
    ) -> Result<DispatchedAuthorizedToolAttemptV1, ToolAttemptDispatchErrorV1> {
        let Self {
            tool_batch_id,
            mut attempt,
            execution,
            mut projection_state,
            persistence: pre_dispatch_persistence,
            projection_publish_error,
            scheduler_permit,
            host_permit,
            result_contract,
            execution_semantics,
            timeout_ms,
            max_retries,
            dispatch_prepared_event_id,
            correlation_id,
            result_context,
            attempt_output_root,
        } = self;
        let NonSecretToolRuntimeFactsV1::Sandbox { facts } = &execution.operation.runtime else {
            return Err(ToolAttemptDispatchErrorV1::RuntimeMismatch);
        };
        let runtime_kind = facts.runtime_kind;
        // Credential I/O shares the runtime's bounded owner. Scheduler/Host permits alone do
        // not bound blocking threads. A refused or unknown read must not dispatch the backend.
        let secret = if secret.is_none() && facts.credential.is_some() {
            let credential_persistence = persistence.clone();
            let credential_execution = execution.clone();
            match sync
                .run(
                    crate::sync_boundary::SyncSection::CredentialSecret,
                    crate::sync_boundary::SyncSubject::new()
                        .method("tool.sandbox.credential")
                        .request(attempt.tool_execution_attempt_id),
                    move || {
                        crate::tool_credential_service::sandbox::secret_frame(
                            &credential_persistence,
                            &credential_execution,
                        )
                    },
                )
                .await
            {
                crate::blocking_pool::BlockingAdmission::Completed(Ok(secret)) => secret,
                _ => return Err(ToolAttemptDispatchErrorV1::CredentialUnavailable),
            }
        } else {
            secret
        };
        let attempt_id = attempt.tool_execution_attempt_id;
        let success_exit_codes = facts.success_exit_codes.clone();
        let credential_canary =
            secret.as_ref().map(|frame| Zeroizing::new(frame.secret().expose().to_vec()));
        let output_authorization = execution.binding.runtime_output_authorization.clone();
        if runtime_kind == ToolPreparedRuntimeKindV1::AuraStdio {
            let output_root = attempt_output_root
                .as_ref()
                .ok_or(ToolAttemptDispatchErrorV1::OutputBindingMissing)?;
            std::fs::create_dir(output_root)
                .map_err(|_| ToolAttemptDispatchErrorV1::OutputRootPreparationFailed)?;
        }
        let mut runtime_started_persistence = None;
        let mut progress_persistence = Vec::new();
        let mut runtime_started_publish_error = None;
        let mut progress_publish_errors = Vec::new();
        let mut runtime_started_error = None;
        let mut progress_error = None;
        let evidence = {
            let mut record_runtime_lifecycle = |update| match update {
                ToolRuntimeDispatchLifecycleUpdateV1::RuntimeStarted => {
                    if runtime_started_persistence.is_some() || runtime_started_error.is_some() {
                        return Err(ToolSandboxExecutionError::Unavailable(anyhow::anyhow!(
                            "the RuntimeStarted boundary was already resolved"
                        )));
                    }
                    let updated_at = UtcTimestamp::now();
                    let mut running_attempt = attempt.clone();
                    running_attempt.status = ToolExecutionAttemptStatusV1::Running;
                    running_attempt.side_effect_state = SideEffectStateV1::Unknown;
                    let mut running_projection_state = projection_state.clone();
                    let attempt_projection =
                        match running_projection_state.runtime_started(&running_attempt) {
                            Ok(projection) => projection,
                            Err(error) => {
                                runtime_started_error = Some(error.to_string());
                                return Err(ToolSandboxExecutionError::Unavailable(
                                    anyhow::Error::new(error),
                                ));
                            }
                        };
                    let committed = match persistence.commit_tool_execution_attempt_runtime_started(
                        ToolExecutionAttemptRuntimeStartedRequest {
                            attempt: running_attempt.clone(),
                            attempt_projection,
                            updated_at,
                        },
                    ) {
                        Ok(commit) => commit,
                        Err(error) => {
                            runtime_started_error = Some(error.to_string());
                            return Err(ToolSandboxExecutionError::Unavailable(
                                anyhow::Error::new(error),
                            ));
                        }
                    };
                    runtime_started_publish_error = persistence
                        .publish_tool_projection_changes(&committed.projection_changes)
                        .err();
                    attempt = running_attempt;
                    projection_state = running_projection_state;
                    runtime_started_persistence = Some(committed);
                    Ok(())
                }
                ToolRuntimeDispatchLifecycleUpdateV1::Progress(progress) => {
                    if runtime_started_persistence.is_none() || progress_error.is_some() {
                        return Err(ToolSandboxExecutionError::Unavailable(anyhow::anyhow!(
                            "Runtime progress arrived outside the committed Running boundary"
                        )));
                    }
                    let mut progressed_attempt = attempt.clone();
                    let mut progressed_projection_state = projection_state.clone();
                    let attempt_projection = match progressed_projection_state.record_progress(
                        &mut progressed_attempt,
                        ToolAttemptProgressView {
                            sequence: progress.sequence,
                            percentage: progress.payload.percentage,
                            message: progress.payload.message,
                            updated_at: UtcTimestamp::now(),
                        },
                    ) {
                        Ok(projection) => projection,
                        Err(error) => {
                            progress_error = Some(error.to_string());
                            return Err(ToolSandboxExecutionError::Unavailable(
                                anyhow::Error::new(error),
                            ));
                        }
                    };
                    let committed = match persistence.commit_tool_execution_attempt_progress(
                        ToolExecutionAttemptProgressRequest { attempt_projection },
                    ) {
                        Ok(commit) => commit,
                        Err(error) => {
                            progress_error = Some(error.to_string());
                            return Err(ToolSandboxExecutionError::Unavailable(
                                anyhow::Error::new(error),
                            ));
                        }
                    };
                    if let Err(error) =
                        persistence.publish_tool_projection_changes(&committed.projection_changes)
                    {
                        progress_publish_errors.push(error);
                    }
                    attempt = progressed_attempt;
                    projection_state = progressed_projection_state;
                    progress_persistence.push(committed);
                    Ok(())
                }
            };

            match runtime_kind {
                ToolPreparedRuntimeKindV1::DeclarativeCommandArgv
                | ToolPreparedRuntimeKindV1::DeclarativeCommandPowershellScript => {
                    ToolRuntimeDispatchEvidenceV1::Command(
                        sandbox
                            .execute_authorized_command_tool(
                                host_permit,
                                execution,
                                secret,
                                success_exit_codes,
                                || {
                                    record_runtime_lifecycle(
                                        ToolRuntimeDispatchLifecycleUpdateV1::RuntimeStarted,
                                    )
                                },
                            )
                            .await?,
                    )
                }
                ToolPreparedRuntimeKindV1::AuraStdio => {
                    let stdio_result = sandbox
                        .execute_authorized_stdio_tool(
                            host_permit,
                            execution,
                            secret,
                            &mut record_runtime_lifecycle,
                        )
                        .await;
                    let mut stdio = match stdio_result {
                        Ok(value) => value,
                        Err(error) => {
                            if let Some(root) = &attempt_output_root {
                                let _ = cleanup_output_root(root);
                            }
                            return Err(error.into());
                        }
                    };
                    let output_root = attempt_output_root
                        .as_ref()
                        .ok_or(ToolAttemptDispatchErrorV1::OutputBindingMissing)?;
                    let authorization = output_authorization
                        .as_ref()
                        .ok_or(ToolAttemptDispatchErrorV1::OutputBindingMissing)?;
                    match import_stdio_attempt_outputs_v1(
                        persistence,
                        attempt_id,
                        output_root,
                        authorization,
                        &stdio.artifact_declarations,
                        credential_canary.as_deref().map(|value| value.as_slice()),
                    ) {
                        Ok(artifacts) => stdio.verified_artifacts = artifacts,
                        Err(error) => {
                            stdio.candidate = match error.class() {
                            ToolOutputFailureClassV1::ProtocolViolation
                            | ToolOutputFailureClassV1::CredentialCanaryDetected => {
                                aura_tool_runtime::ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid
                            }
                            ToolOutputFailureClassV1::IoLimitExceeded => {
                                aura_tool_runtime::ToolRuntimeCandidateV1::DeterministicFailure {
                                    code: ToolRuntimeErrorCode::IoLimitExceeded,
                                }
                            }
                            ToolOutputFailureClassV1::ArtifactInvalid => {
                                aura_tool_runtime::ToolRuntimeCandidateV1::DeterministicFailure {
                                    code: ToolRuntimeErrorCode::ArtifactInvalid,
                                }
                            }
                        };
                            stdio.internal_protocol_error = Some(match error.class() {
                            ToolOutputFailureClassV1::CredentialCanaryDetected => {
                                "Attempt output contained the credential canary and was quarantined"
                            }
                            ToolOutputFailureClassV1::ProtocolViolation => {
                                "Attempt output declarations violated the frozen protocol"
                            }
                            ToolOutputFailureClassV1::IoLimitExceeded => {
                                "Attempt output exceeded a frozen I/O limit"
                            }
                            ToolOutputFailureClassV1::ArtifactInvalid => {
                                "Attempt output failed handle-bound verification"
                            }
                        }
                        .to_owned());
                        }
                    }
                    stdio.artifact_declarations.clear();
                    ToolRuntimeDispatchEvidenceV1::AuraStdio(stdio)
                }
                ToolPreparedRuntimeKindV1::DeclarativeHttp => {
                    return Err(ToolAttemptDispatchErrorV1::RuntimeMismatch);
                }
            }
        };
        let mut projection_publish_errors =
            projection_publish_error.into_iter().collect::<Vec<_>>();
        projection_publish_errors.extend(runtime_started_publish_error);
        projection_publish_errors.extend(progress_publish_errors);
        Ok(DispatchedAuthorizedToolAttemptV1 {
            tool_batch_id,
            attempt,
            projection_state,
            evidence,
            pre_dispatch_persistence,
            runtime_started_persistence,
            progress_persistence,
            projection_publish_errors,
            runtime_started_error,
            scheduler_permit,
            result_contract,
            execution_semantics,
            timeout_ms,
            max_retries,
            dispatch_prepared_event_id,
            correlation_id,
            result_context,
        })
    }
}
