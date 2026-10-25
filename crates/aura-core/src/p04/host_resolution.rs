use super::*;

impl P04Services {
    pub(super) async fn resolve_host_preparation(
        &self,
        input: ResolveHostPreparationInput,
        correlation_id: CorrelationId,
    ) -> Result<HostPreparationStatus, IpcErrorEnvelope> {
        let state = self
            .persistence
            .host_preparation_state()
            .map_err(|error| persistence_error(error, correlation_id))?
            .ok_or_else(|| {
                envelope(
                    "sandbox.host_preparation_unavailable",
                    ErrorCategory::Unavailable,
                    "Host Preparation state is unavailable.",
                    false,
                    correlation_id,
                )
            })?;
        let active_operation = state.status.active_operation.clone().ok_or_else(|| {
            envelope(
                "sandbox.host_preparation_operation_conflict",
                ErrorCategory::Conflict,
                "No Host Preparation plan is awaiting a trusted Main decision.",
                false,
                correlation_id,
            )
        })?;
        let journal = self
            .persistence
            .host_preparation_operation(active_operation.operation_id)
            .map_err(|error| persistence_error(error, correlation_id))?;
        let approval = journal.approval.as_ref().ok_or_else(|| {
            envelope(
                "sandbox.host_preparation_approval_unavailable",
                ErrorCategory::Conflict,
                "The Host Preparation plan has no current native-dialog approval.",
                false,
                correlation_id,
            )
        })?;
        if journal.operation.state != HostPreparationOperationState::Prepared
            || journal.operation.operation_revision != active_operation.operation_revision
            || approval.id != input.approval_id
            || approval.request_revision != 1
            || approval.release_descriptor.as_ref() != Some(&journal.broker_plan.release_descriptor)
            || journal.operation.release_descriptor != journal.broker_plan.release_descriptor
            || journal.broker_plan.release_descriptor != self.broker.release_descriptor
        {
            return Err(envelope(
                "sandbox.host_preparation_operation_conflict",
                ErrorCategory::Conflict,
                "The Host Preparation plan is no longer awaiting this decision.",
                false,
                correlation_id,
            ));
        }

        if !matches!(input.decision, ApprovalDecisionKind::AllowOnce | ApprovalDecisionKind::Deny) {
            return Err(envelope(
                "sandbox.host_preparation_decision_not_allowed",
                ErrorCategory::Permission,
                "Host Preparation accepts only AllowOnce or Deny from trusted Electron Main.",
                false,
                correlation_id,
            ));
        }

        if input.decision == ApprovalDecisionKind::Deny {
            let runtime =
                build_runtime_projection(&self.persistence, RuntimeComponentState::NotStarted)
                    .map_err(|error| internal_error(error, correlation_id))?;
            let stored = self
                .persistence
                .transition_host_preparation_operation(HostPreparationOperationTransition {
                    operation_id: active_operation.operation_id,
                    expected_state: HostPreparationOperationState::Prepared,
                    expected_operation_revision: active_operation.operation_revision,
                    expected_host_preparation_revision: state.status.revision,
                    next_state: HostPreparationOperationState::Denied,
                    ownership_after: None,
                    error_code: Some("sandbox.host_preparation_denied".to_owned()),
                    runtime,
                    correlation_id,
                })
                .map_err(|error| persistence_error(error, correlation_id))?;
            self.remove_host_preparation_nonce(active_operation.operation_id, correlation_id)?;
            return Ok(stored.status);
        }

        let expected_prepared_status_revision =
            journal.broker_plan.host_preparation_revision.checked_add(1).ok_or_else(|| {
                internal_error("Host Preparation plan revision exhausted", correlation_id)
            })?;
        if state.status.revision != expected_prepared_status_revision {
            self.fail_prepared_host_preparation(
                &journal.operation,
                state.status.revision,
                "sandbox.host_preparation_plan_stale",
                correlation_id,
            )?;
            self.remove_host_preparation_nonce(active_operation.operation_id, correlation_id)?;
            return Err(envelope(
                "sandbox.host_preparation_plan_stale",
                ErrorCategory::Conflict,
                "Host Preparation state changed after planning; create a new plan.",
                false,
                correlation_id,
            ));
        }

        let plan_nonce = self
            .host_preparation_nonces
            .lock()
            .map_err(|_| {
                internal_error("Host Preparation nonce state is unavailable", correlation_id)
            })?
            .get(&active_operation.operation_id)
            .cloned()
            .ok_or_else(|| {
                envelope(
                    "sandbox.host_preparation_nonce_unavailable",
                    ErrorCategory::Conflict,
                    "The one-time Host Preparation plan nonce is no longer available.",
                    false,
                    correlation_id,
                )
            })?;
        if UtcTimestamp::now() > journal.broker_plan.expires_at {
            self.fail_prepared_host_preparation(
                &journal.operation,
                state.status.revision,
                "sandbox.host_preparation_plan_expired",
                correlation_id,
            )?;
            self.remove_host_preparation_nonce(active_operation.operation_id, correlation_id)?;
            return Err(envelope(
                "sandbox.host_preparation_plan_expired",
                ErrorCategory::Conflict,
                "The Host Preparation plan expired; create a new plan.",
                false,
                correlation_id,
            ));
        }
        if let Err(error) = validate_host_preparation_plan_nonce(&journal.broker_plan, &plan_nonce)
        {
            self.fail_prepared_host_preparation(
                &journal.operation,
                state.status.revision,
                "sandbox.host_preparation_nonce_invalid",
                correlation_id,
            )?;
            self.remove_host_preparation_nonce(active_operation.operation_id, correlation_id)?;
            return Err(host_preparation_release_error(error, correlation_id));
        }
        if let Err(error) = validate_active_release_identity(&self.broker, &journal.broker_plan) {
            self.fail_prepared_host_preparation(
                &journal.operation,
                state.status.revision,
                "sandbox.host_preparation_release_changed",
                correlation_id,
            )?;
            self.remove_host_preparation_nonce(active_operation.operation_id, correlation_id)?;
            return Err(host_preparation_release_error(error, correlation_id));
        }

        let elevation_launch_ticket =
            issue_elevation_launch_ticket(approval.id, &journal.broker_plan, correlation_id)?;

        let elevated = current_process_is_elevated()
            .map_err(|error| host_preparation_platform_error(error, correlation_id))?;
        let initial_next_state = if elevated {
            HostPreparationOperationState::Applying
        } else {
            HostPreparationOperationState::AwaitingElevation
        };
        let runtime = build_runtime_projection(
            &self.persistence,
            if elevated { RuntimeComponentState::Ready } else { RuntimeComponentState::Starting },
        )
        .map_err(|error| internal_error(error, correlation_id))?;
        let consumed = self
            .persistence
            .transition_host_preparation_operation(HostPreparationOperationTransition {
                operation_id: active_operation.operation_id,
                expected_state: HostPreparationOperationState::Prepared,
                expected_operation_revision: active_operation.operation_revision,
                expected_host_preparation_revision: state.status.revision,
                next_state: initial_next_state,
                ownership_after: None,
                error_code: None,
                runtime,
                correlation_id,
            })
            .map_err(|error| persistence_error(error, correlation_id))?;
        self.remove_host_preparation_nonce(active_operation.operation_id, correlation_id)?;

        // The apply is the one step that can block inside the platform call, so its
        // start and completion are traced: an operation that stops between these
        // two lines is stuck in the write, not in the state machine.
        aura_ipc::trace(
            "core.host_preparation.apply.start",
            &format!(
                "operation={} elevated={elevated} action={:?} update_null={} add_app={} add_restricted={}",
                active_operation.operation_id,
                journal.broker_plan.action,
                journal.broker_plan.update_null_device_security,
                journal.broker_plan.add_all_application_packages_ace,
                journal.broker_plan.add_all_restricted_application_packages_ace,
            ),
        );
        let result = if elevated {
            execute_approved_host_preparation_broker_plan(
                &journal.broker_plan,
                &elevation_launch_ticket,
            )
            .map_err(anyhow::Error::from)
        } else {
            let persistence = Arc::clone(&self.persistence);
            let operation = consumed
                .status
                .active_operation
                .clone()
                .context("consumed Host Preparation operation disappeared")
                .map_err(|error| internal_error(error, correlation_id))?;
            let expected_host_preparation_revision = consumed.status.revision;
            execute_elevated_host_preparation(
                &self.broker,
                journal.broker_plan.clone(),
                plan_nonce,
                elevation_launch_ticket,
                move || {
                    let runtime =
                        build_runtime_projection(&persistence, RuntimeComponentState::Ready)?;
                    persistence.transition_host_preparation_operation(
                        HostPreparationOperationTransition {
                            operation_id: operation.operation_id,
                            expected_state: HostPreparationOperationState::AwaitingElevation,
                            expected_operation_revision: operation.operation_revision,
                            expected_host_preparation_revision,
                            next_state: HostPreparationOperationState::Applying,
                            ownership_after: None,
                            error_code: None,
                            runtime,
                            correlation_id,
                        },
                    )?;
                    Ok(())
                },
            )
            .await
        };
        aura_ipc::trace(
            "core.host_preparation.apply.done",
            &format!(
                "operation={} ok={} detail={}",
                active_operation.operation_id,
                result.is_ok(),
                match &result {
                    Ok(value) => format!(
                        "written app={} restricted={}",
                        value.ownership_after.all_application_packages_ace_owned,
                        value.ownership_after.all_restricted_application_packages_ace_owned,
                    ),
                    Err(error) => format!("{error:#}"),
                },
            ),
        );

        match result {
            Ok(result) => {
                // Both execution paths record the verified write before attempting
                // persistence. The event does not assert that Completed was committed.
                record_host_preparation_write_evidence(&result, crate::append_core_log);
                let current = self.current_host_preparation_operation(
                    active_operation.operation_id,
                    correlation_id,
                )?;
                let runtime =
                    build_runtime_projection(&self.persistence, RuntimeComponentState::NotStarted)
                        .map_err(|error| internal_error(error, correlation_id))?;
                self.persistence
                    .transition_host_preparation_operation(HostPreparationOperationTransition {
                        operation_id: active_operation.operation_id,
                        expected_state: HostPreparationOperationState::Applying,
                        expected_operation_revision: current.operation.operation_revision,
                        expected_host_preparation_revision: current.status_revision,
                        next_state: HostPreparationOperationState::Completed,
                        ownership_after: Some(result.ownership_after),
                        error_code: None,
                        runtime,
                        correlation_id,
                    })
                    .map(|stored| stored.status)
                    .map_err(|error| persistence_error(error, correlation_id))
            }
            Err(error) => {
                let current = self.current_host_preparation_operation(
                    active_operation.operation_id,
                    correlation_id,
                )?;
                let error_code =
                    host_preparation_execution_error_code(current.operation.state, &error);
                let runtime =
                    build_runtime_projection(&self.persistence, RuntimeComponentState::Unavailable)
                        .map_err(|projection_error| {
                            internal_error(projection_error, correlation_id)
                        })?;
                self.persistence
                    .transition_host_preparation_operation(HostPreparationOperationTransition {
                        operation_id: active_operation.operation_id,
                        expected_state: current.operation.state,
                        expected_operation_revision: current.operation.operation_revision,
                        expected_host_preparation_revision: current.status_revision,
                        next_state: HostPreparationOperationState::Failed,
                        ownership_after: None,
                        error_code: Some(error_code.to_owned()),
                        runtime,
                        correlation_id,
                    })
                    .map_err(|transition_error| {
                        persistence_error(transition_error, correlation_id)
                    })?;
                Err(envelope(
                    error_code,
                    host_preparation_failure_category(error_code),
                    format!("Host Preparation did not complete. A new plan is required: {error:#}"),
                    false,
                    correlation_id,
                ))
            }
        }
    }

    pub(super) fn fail_prepared_host_preparation(
        &self,
        operation: &HostPreparationOperationView,
        expected_host_preparation_revision: u64,
        error_code: &str,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        let runtime =
            build_runtime_projection(&self.persistence, RuntimeComponentState::Unavailable)
                .map_err(|error| internal_error(error, correlation_id))?;
        self.persistence
            .transition_host_preparation_operation(HostPreparationOperationTransition {
                operation_id: operation.operation_id,
                expected_state: HostPreparationOperationState::Prepared,
                expected_operation_revision: operation.operation_revision,
                expected_host_preparation_revision,
                next_state: HostPreparationOperationState::Failed,
                ownership_after: None,
                error_code: Some(error_code.to_owned()),
                runtime,
                correlation_id,
            })
            .map_err(|error| persistence_error(error, correlation_id))?;
        Ok(())
    }

    pub(super) fn remove_host_preparation_nonce(
        &self,
        operation_id: HostPreparationOperationId,
        correlation_id: CorrelationId,
    ) -> Result<(), IpcErrorEnvelope> {
        self.host_preparation_nonces
            .lock()
            .map_err(|_| {
                internal_error("Host Preparation nonce state is unavailable", correlation_id)
            })?
            .remove(&operation_id);
        Ok(())
    }

    pub(super) fn current_host_preparation_operation(
        &self,
        operation_id: HostPreparationOperationId,
        correlation_id: CorrelationId,
    ) -> Result<CurrentHostPreparationOperation, IpcErrorEnvelope> {
        let operation = self
            .persistence
            .host_preparation_operation(operation_id)
            .map_err(|error| persistence_error(error, correlation_id))?
            .operation;
        let status_revision = self
            .persistence
            .host_preparation_state()
            .map_err(|error| persistence_error(error, correlation_id))?
            .ok_or_else(|| {
                envelope(
                    "sandbox.host_preparation_unavailable",
                    ErrorCategory::Unavailable,
                    "Host Preparation state is unavailable.",
                    false,
                    correlation_id,
                )
            })?
            .status
            .revision;
        Ok(CurrentHostPreparationOperation { operation, status_revision })
    }
}
