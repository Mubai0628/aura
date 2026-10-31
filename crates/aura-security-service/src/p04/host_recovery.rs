use super::*;
use aura_core_base::append_core_log;

impl P04Services {
    pub(super) fn recover_security_state(&self) -> Result<()> {
        self.persistence
            .reconcile_security_after_core_restart(CorrelationId::new())
            .context("failed to reconcile security state after Core restart")?;
        SandboxExecutor::new(
            self.persistence.as_ref().clone(),
            CancellationRegistry::default(),
            SandboxExecutorOptions::default(),
        )
        .recover_acl_journal(|_| {})
        .context("failed to recover Sandbox ACL journal")?;
        for pending in self
            .persistence
            .pending_host_preparation_operations()
            .context("failed to read Host Preparation journal")?
        {
            let recovery = match pending.operation.state {
                HostPreparationOperationState::Applying
                | HostPreparationOperationState::RollbackPending => {
                    classify_interrupted_host_preparation(&pending.broker_plan)
                        .unwrap_or(HostPreparationRecoveryDisposition::Unknown)
                }
                _ => HostPreparationRecoveryDisposition::PreState,
            };
            let (next_state, ownership_after, error_code) = match recovery {
                HostPreparationRecoveryDisposition::Applied(ownership) => {
                    (HostPreparationOperationState::Completed, Some(ownership), None)
                }
                HostPreparationRecoveryDisposition::PreState => (
                    if pending.operation.state == HostPreparationOperationState::RollbackPending {
                        HostPreparationOperationState::RolledBack
                    } else {
                        HostPreparationOperationState::Failed
                    },
                    None,
                    Some("sandbox.core_restarted".to_owned()),
                ),
                HostPreparationRecoveryDisposition::Unknown => {
                    if pending.operation.state == HostPreparationOperationState::RollbackPending {
                        continue;
                    }
                    (
                        HostPreparationOperationState::RollbackPending,
                        None,
                        Some("sandbox.elevated_state_unknown".to_owned()),
                    )
                }
            };
            let mut runtime =
                build_runtime_projection(&self.persistence, RuntimeComponentState::Unavailable)?;
            runtime.host_preparation.last_error_code = error_code.clone();
            let expected_host_preparation_revision = self
                .persistence
                .host_preparation_state()
                .context("failed to read Host Preparation revision during recovery")?
                .context("Host Preparation state disappeared during recovery")?
                .status
                .revision;
            self.persistence
                .transition_host_preparation_operation(HostPreparationOperationTransition {
                    operation_id: pending.operation.operation_id,
                    expected_state: pending.operation.state,
                    expected_operation_revision: pending.operation.operation_revision,
                    expected_host_preparation_revision,
                    next_state,
                    ownership_after,
                    error_code,
                    runtime,
                    correlation_id: CorrelationId::new(),
                })
                .context("failed to close interrupted Host Preparation operation")?;
        }
        self.probe_and_persist(CorrelationId::new())?;
        Ok(())
    }
}

/// Records one bounded Host Preparation sub-stage in the ordinary Core log.
///
/// Durations, a stage name and identifiers only: no host path, no file content and
/// never a credential. A log that cannot be written loses the measurement; it never
/// changes what the route answers.
pub(super) fn record_host_preparation_stage(
    report: crate::host_preparation::HostPreparationStageReport,
) {
    append_core_log(
        "security.host_preparation.stage",
        serde_json::json!({
            "stage": report.stage.as_str(),
            "correlation_id": report.correlation_id,
            "operation_id": report.operation_id,
            "ms": report.ms,
        }),
    );
}

/// The host facts an interrupted-plan decision needs, gathered once.
pub(super) struct InterruptedHostFacts {
    /// Whether the probed system drive root is the one the plan froze.
    pub(super) system_drive_root_matches: bool,
    pub(super) all_application_packages_ready: bool,
    pub(super) all_restricted_application_packages_ready: bool,
    /// `None` when the NUL question could not be answered at all.
    pub(super) null_ready: Option<bool>,
    /// Whether NUL is still exactly the descriptor the plan froze, or `None`
    /// when the plan carries no frozen original.
    pub(super) frozen_null_descriptor_matches: Option<bool>,
}

pub(super) fn classify_interrupted_host_preparation(
    plan: &HostPreparationBrokerPlan,
) -> Result<HostPreparationRecoveryDisposition> {
    let probe =
        probe_host_compatibility().context("failed to probe interrupted Host Preparation")?;
    let null_ready = match probe_null_device_readiness() {
        NullDeviceVerdict::Ready => Some(true),
        NullDeviceVerdict::NotReady => Some(false),
        // Unanswerable is not a verdict: the caller treats it as "unknown", never
        // as "the change was applied" or "the change was not needed".
        NullDeviceVerdict::Unanswerable(_) => None,
    };
    let frozen_null_descriptor_matches =
        match plan.ownership_before.original_null_device_sddl.as_deref() {
            Some(original) => Some(
                null_device_security_descriptor_matches(original)
                    .context("failed to compare the frozen NUL security descriptor")?,
            ),
            None => None,
        };
    Ok(classify_interrupted_from_facts(
        plan,
        &InterruptedHostFacts {
            system_drive_root_matches: canonical_path(probe.system_drive_root)
                == plan.system_drive_root,
            all_application_packages_ready: probe.all_application_packages_ready,
            all_restricted_application_packages_ready: probe
                .all_restricted_application_packages_ready,
            null_ready,
            frozen_null_descriptor_matches,
        },
    ))
}

/// The decision itself, over facts the caller gathered.
///
/// Readiness is deliberately not the whole answer: with a coexistence-aware rule
/// the NUL device can look ready both before and after the change, so only the
/// frozen original can say whether the change happened.
pub(super) fn classify_interrupted_from_facts(
    plan: &HostPreparationBrokerPlan,
    facts: &InterruptedHostFacts,
) -> HostPreparationRecoveryDisposition {
    if !facts.system_drive_root_matches {
        return HostPreparationRecoveryDisposition::Unknown;
    }
    let null_ready = facts.null_ready;
    let mut changed_states = Vec::new();
    let mut unknown = false;

    match plan.action {
        HostPreparationAction::Prepare | HostPreparationAction::Repair => {
            classify_prepare_ace(
                plan.add_all_application_packages_ace,
                facts.all_application_packages_ready,
                &mut changed_states,
                &mut unknown,
            );
            classify_prepare_ace(
                plan.add_all_restricted_application_packages_ace,
                facts.all_restricted_application_packages_ready,
                &mut changed_states,
                &mut unknown,
            );
            if plan.update_null_device_security {
                // The frozen original decides this, not readiness. With a
                // coexistence-aware readiness rule both "before" and "after" can
                // look ready, so "ready now" must never be read as "this plan
                // ran": a plan that never executed would be recorded as applied.
                let Some(matches) = facts.frozen_null_descriptor_matches else {
                    // A plan that updates NUL always freezes the original first;
                    // without it nothing here can prove what happened.
                    return HostPreparationRecoveryDisposition::Unknown;
                };
                if matches {
                    // Still exactly the pre-state: this change was not applied.
                    changed_states.push(false);
                } else if null_ready == Some(true) {
                    changed_states.push(true);
                } else {
                    unknown = true;
                }
            } else if null_ready != Some(true) {
                unknown = true;
            }
        }
        HostPreparationAction::Remove => {
            classify_remove_ace(
                plan.add_all_application_packages_ace,
                facts.all_application_packages_ready,
                &mut changed_states,
            );
            classify_remove_ace(
                plan.add_all_restricted_application_packages_ace,
                facts.all_restricted_application_packages_ready,
                &mut changed_states,
            );
            if plan.update_null_device_security {
                let Some(matches) = facts.frozen_null_descriptor_matches else {
                    return HostPreparationRecoveryDisposition::Unknown;
                };
                if matches {
                    changed_states.push(true);
                } else if null_ready == Some(true) {
                    changed_states.push(false);
                } else {
                    unknown = true;
                }
            }
        }
    }

    if unknown || changed_states.is_empty() {
        return HostPreparationRecoveryDisposition::Unknown;
    }
    if changed_states.iter().all(|applied| *applied) {
        let mut ownership = plan.ownership_before.clone();
        match plan.action {
            HostPreparationAction::Prepare | HostPreparationAction::Repair => {
                ownership.system_drive_root = Some(plan.system_drive_root.clone());
                ownership.all_application_packages_ace_owned |=
                    plan.add_all_application_packages_ace;
                ownership.all_restricted_application_packages_ace_owned |=
                    plan.add_all_restricted_application_packages_ace;
            }
            HostPreparationAction::Remove => {
                ownership.system_drive_root = None;
                ownership.all_application_packages_ace_owned = false;
                ownership.all_restricted_application_packages_ace_owned = false;
                ownership.original_null_device_sddl = None;
            }
        }
        return HostPreparationRecoveryDisposition::Applied(ownership);
    }
    if changed_states.iter().all(|applied| !*applied) {
        return HostPreparationRecoveryDisposition::PreState;
    }
    HostPreparationRecoveryDisposition::Unknown
}

pub(super) fn classify_prepare_ace(
    planned_change: bool,
    currently_ready: bool,
    changed_states: &mut Vec<bool>,
    unknown: &mut bool,
) {
    if planned_change {
        changed_states.push(currently_ready);
    } else if !currently_ready {
        *unknown = true;
    }
}

pub(super) fn classify_remove_ace(
    planned_change: bool,
    currently_ready: bool,
    changed_states: &mut Vec<bool>,
) {
    if planned_change {
        changed_states.push(!currently_ready);
    }
}

pub(super) fn host_preparation_platform_error(
    error: impl std::fmt::Display,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    envelope(
        "sandbox.host_preparation_probe_failed",
        ErrorCategory::Unavailable,
        error.to_string(),
        true,
        correlation_id,
    )
}

pub(super) fn host_preparation_execution_error_code(
    state: HostPreparationOperationState,
    error: &anyhow::Error,
) -> &'static str {
    // A refusal that proved it wrote nothing is not an unknown state: checking it
    // first keeps the order "what is known" before "what is merely possible".
    if let Some(refusal) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<crate::p04_broker::ElevatedHelperRefusal>())
    {
        return refusal.code();
    }
    let release_trust_failed =
        error.chain().any(|cause| cause.downcast_ref::<ActiveReleaseTrustError>().is_some());
    let launch_state_unknown = error.chain().any(|cause| {
        cause
            .downcast_ref::<ElevatedHelperLaunchError>()
            .is_some_and(ElevatedHelperLaunchError::is_state_unknown)
    });
    if state == HostPreparationOperationState::Applying || launch_state_unknown {
        "sandbox.elevated_state_unknown"
    } else if release_trust_failed {
        "sandbox.host_preparation_release_trust_failed"
    } else {
        "sandbox.host_preparation_uac_failed"
    }
}

/// The category of a Host Preparation apply failure, by its frozen code.
///
/// A refusal or an inconsistent plan is a conflict with the current host state,
/// not an availability problem; an unproven state stays `Unavailable`.
pub(super) fn host_preparation_failure_category(error_code: &str) -> ErrorCategory {
    match error_code {
        "sandbox.host_preparation_capability_conflict"
        | "sandbox.host_preparation_plan_invalid"
        | "sandbox.host_preparation_release_changed" => ErrorCategory::Conflict,
        _ => ErrorCategory::Unavailable,
    }
}

pub(super) fn host_preparation_release_error(
    error: impl std::fmt::Display,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    envelope(
        "sandbox.host_preparation_release_changed",
        ErrorCategory::Conflict,
        format!(
            "The active Release Bundle no longer matches the frozen Host Preparation plan: {error}"
        ),
        false,
        correlation_id,
    )
}
