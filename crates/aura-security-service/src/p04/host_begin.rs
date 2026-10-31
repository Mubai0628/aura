use super::*;

impl P04Services {
    pub(super) fn begin_host_preparation(
        &self,
        action: HostPreparationAction,
        input: HostPreparationMutationInput,
        correlation_id: CorrelationId,
    ) -> Result<HostPreparationBeginResult, IpcErrorEnvelope> {
        self.reconcile_expired_host_preparation(correlation_id)
            .map_err(|error| internal_error(error, correlation_id))?;
        let current = self
            .persistence
            .host_preparation_state()
            .map_err(|error| persistence_error(error, correlation_id))?
            .ok_or_else(|| {
                envelope(
                    "sandbox.host_preparation_unavailable",
                    ErrorCategory::Unavailable,
                    "Host Preparation has not been probed.",
                    true,
                    correlation_id,
                )
            })?;
        if current.status.revision != input.expected_revision {
            return Err(envelope(
                "sandbox.host_preparation_revision_conflict",
                ErrorCategory::Conflict,
                "Host Preparation state changed; refresh before creating a new plan.",
                true,
                correlation_id,
            ));
        }
        if current.status.active_operation.is_some() {
            return Err(envelope(
                "sandbox.host_preparation_operation_active",
                ErrorCategory::Conflict,
                "Another Host Preparation operation is already active.",
                false,
                correlation_id,
            ));
        }
        let probe = probe_host_compatibility()
            .map_err(|error| host_preparation_platform_error(error, correlation_id))?;
        // NUL readiness is decided by the read-only installation identity check as
        // well: with a verified installation its capability ACE is an allowed
        // coexistence, and with an unverifiable one no answer exists at all.
        let null_device = probe_null_device_readiness();
        let system_drive_root = canonical_path(probe.system_drive_root);
        if let Some(owned_root) = &current.ownership.system_drive_root
            && owned_root != &system_drive_root
        {
            return Err(envelope(
                "sandbox.host_preparation_ownership_conflict",
                ErrorCategory::Conflict,
                "Aura-owned Host Preparation changes refer to a different system drive root.",
                false,
                correlation_id,
            ));
        }
        let mut ownership_before = current.ownership.clone();
        let (
            add_all_application_packages_ace,
            add_all_restricted_application_packages_ace,
            update_null_device_security,
        ) = match action {
            HostPreparationAction::Prepare | HostPreparationAction::Repair => {
                let update_null_device_security = match null_device {
                    NullDeviceVerdict::Ready => false,
                    NullDeviceVerdict::NotReady => true,
                    NullDeviceVerdict::Unanswerable(reason) => {
                        // A published candidate exists but did not verify. Whether
                        // replacing the descriptor would delete a capability is
                        // unknown, and an unknown must never become a plan.
                        return Err(envelope(
                            "sandbox.host_preparation_identity_unverified",
                            ErrorCategory::Unavailable,
                            format!(
                                "The published Aura installation could not be verified, so the NUL device change cannot be decided: {reason}"
                            ),
                            false,
                            correlation_id,
                        ));
                    }
                };
                if update_null_device_security
                    && ownership_before.original_null_device_sddl.is_none()
                {
                    ownership_before.original_null_device_sddl =
                        Some(capture_null_device_security_descriptor().map_err(|error| {
                            host_preparation_platform_error(error, correlation_id)
                        })?);
                }
                (
                    !probe.all_application_packages_ready,
                    !probe.all_restricted_application_packages_ready,
                    update_null_device_security,
                )
            }
            HostPreparationAction::Remove => (
                ownership_before.all_application_packages_ace_owned,
                ownership_before.all_restricted_application_packages_ace_owned,
                ownership_before.original_null_device_sddl.is_some(),
            ),
        };
        if !add_all_application_packages_ace
            && !add_all_restricted_application_packages_ace
            && !update_null_device_security
        {
            return Ok(HostPreparationBeginResult { status: current.status, approval: None });
        }

        let mut nonce_bytes = [0_u8; 32];
        getrandom::fill(&mut nonce_bytes).map_err(|error| internal_error(error, correlation_id))?;
        let plan_nonce = SecretNonce::from_bytes(nonce_bytes);
        let plan_nonce_hash = ContentHash::blake3(plan_nonce.expose().as_bytes());
        let now = UtcTimestamp::now();
        let expires_at = UtcTimestamp::from_value(now.value() + HOST_PREPARATION_PLAN_TTL);
        let operation_id = HostPreparationOperationId::new();
        let requires_elevation = !current_process_is_elevated()
            .map_err(|error| host_preparation_platform_error(error, correlation_id))?;
        let mut broker_plan = HostPreparationBrokerPlan {
            operation_id,
            action,
            host_preparation_revision: input.expected_revision,
            runtime_instance_id: self.broker.runtime_instance_id,
            release_descriptor: self.broker.release_descriptor.clone(),
            plan_nonce_hash,
            expires_at,
            system_drive_root,
            ownership_before,
            add_all_application_packages_ace,
            add_all_restricted_application_packages_ace,
            update_null_device_security,
            plan_hash: ContentHash::blake3(&[]),
        };
        broker_plan.plan_hash = host_preparation_plan_hash(&broker_plan)
            .map_err(|error| internal_error(error, correlation_id))?;
        validate_active_release_identity(&self.broker, &broker_plan)
            .map_err(|error| host_preparation_release_error(error, correlation_id))?;
        let approval =
            host_preparation_approval_view(action, now, broker_plan.release_descriptor.clone());
        let operation = HostPreparationOperationView {
            operation_id,
            operation_revision: 1,
            action,
            state: HostPreparationOperationState::Prepared,
            host_preparation_revision: input.expected_revision,
            runtime_instance_id: broker_plan.runtime_instance_id,
            release_descriptor: broker_plan.release_descriptor.clone(),
            plan_hash: broker_plan.plan_hash.clone(),
            expires_at,
            requires_elevation,
            add_all_application_packages_ace,
            add_all_restricted_application_packages_ace,
            update_null_device_security,
            created_at: now,
            updated_at: now,
            error_code: None,
        };
        let runtime =
            build_runtime_projection(&self.persistence, RuntimeComponentState::NotStarted)
                .map_err(|error| internal_error(error, correlation_id))?;
        let mut nonces = self.host_preparation_nonces.lock().map_err(|_| {
            internal_error("Host Preparation nonce state is unavailable", correlation_id)
        })?;
        let stored = self
            .persistence
            .begin_host_preparation_operation(
                input.expected_revision,
                operation,
                broker_plan,
                approval.clone(),
                runtime,
                correlation_id,
            )
            .map_err(|error| persistence_error(error, correlation_id))?;
        nonces.insert(operation_id, plan_nonce);
        Ok(HostPreparationBeginResult { status: stored.status, approval: Some(approval) })
    }
}

pub(super) fn host_preparation_approval_view(
    action: HostPreparationAction,
    created_at: UtcTimestamp,
    release_descriptor: aura_runtime_contract::ReleaseDescriptor,
) -> ApprovalView {
    let (title, summary) = match action {
        HostPreparationAction::Prepare => {
            ("配置 Host Preparation", "应用 Core 已冻结的 Aura 沙箱兼容系统配置。")
        }
        HostPreparationAction::Repair => {
            ("修复 Host Preparation", "修复 Core 探测到的 Aura 沙箱兼容系统配置。")
        }
        HostPreparationAction::Remove => {
            ("移除 Host Preparation", "仅移除 Aura Journal 记录为自身创建的系统配置。")
        }
    };
    ApprovalView {
        id: ApprovalRequestId::new(),
        title: title.to_owned(),
        session_id: None,
        model_purpose: None,
        actual_operation_summary: summary.to_owned(),
        source_summary: "Aura Core Host Preparation".to_owned(),
        risk_flags: vec![
            OperationRiskFlag::SystemConfigurationMutation,
            OperationRiskFlag::Elevation,
        ],
        allowed_decisions: vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
        request_revision: 1,
        created_at,
        release_descriptor: Some(release_descriptor),
    }
}

pub(super) fn issue_elevation_launch_ticket(
    approval_id: ApprovalRequestId,
    plan: &HostPreparationBrokerPlan,
    correlation_id: CorrelationId,
) -> Result<ElevationLaunchTicket, IpcErrorEnvelope> {
    let mut nonce_bytes = [0_u8; 32];
    getrandom::fill(&mut nonce_bytes).map_err(|error| internal_error(error, correlation_id))?;
    let issued_at = UtcTimestamp::now();
    Ok(ElevationLaunchTicket {
        ticket_id: ElevationLaunchTicketId::new(),
        approval_id,
        operation_id: plan.operation_id,
        plan_hash: plan.plan_hash.clone(),
        runtime_instance_id: plan.runtime_instance_id,
        release_descriptor: plan.release_descriptor.clone(),
        issued_at,
        expires_at: UtcTimestamp::from_value(issued_at.value() + ELEVATION_LAUNCH_TICKET_TTL),
        ticket_nonce: SecretNonce::from_bytes(nonce_bytes),
    })
}
