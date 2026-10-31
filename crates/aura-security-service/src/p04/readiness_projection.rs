use super::*;

impl P04Services {
    pub fn security_runtime_projection(&self) -> Result<SecurityRuntimeProjection> {
        build_runtime_projection(&self.persistence, RuntimeComponentState::NotStarted)
    }
}

/// The runtime projection the Host Preparation worker builds.
///
/// Public to the worker because the probe and the settlement both need it, and neither
/// may build it inside a request handler.
pub(crate) fn build_runtime_projection_at_start(
    persistence: &PersistenceCoordinator,
) -> Result<SecurityRuntimeProjection> {
    build_runtime_projection(persistence, RuntimeComponentState::NotStarted)
}

pub(super) fn build_runtime_projection(
    persistence: &PersistenceCoordinator,
    elevated_helper_state: RuntimeComponentState,
) -> Result<SecurityRuntimeProjection> {
    let probe = probe_windows_sandbox();
    let residual_acl_count = u32::try_from(
        persistence
            .pending_sandbox_acl_cleanup()
            .context("failed to inspect residual Sandbox ACLs")?
            .len(),
    )
    .unwrap_or(u32::MAX);
    let quarantine_count = u32::try_from(
        persistence.list_quarantine().context("failed to count quarantined executables")?.len(),
    )
    .unwrap_or(u32::MAX);
    let sandbox_available = probe.appcontainer_api_available
        && probe.nested_jobs_available
        && probe.standard_user_token_available
        && probe.system_drive_compatibility_ready
        && probe.nul_compatibility_ready;
    Ok(SecurityRuntimeProjection {
        sandbox_available,
        host_preparation: aura_permission_domain::HostPreparationSummary {
            appcontainer_available: probe.appcontainer_api_available,
            nested_jobs_available: probe.nested_jobs_available,
            standard_user_token_available: probe.standard_user_token_available,
            system_drive_compatibility_ready: probe.system_drive_compatibility_ready,
            nul_compatibility_ready: probe.nul_compatibility_ready,
            residual_acl_count,
            residual_firewall_rule_count: 0,
            residual_appcontainer_profile_count: 0,
            restart_required: false,
            last_error_code: None,
        },
        sandbox_host_state: if sandbox_available {
            RuntimeComponentState::NotStarted
        } else {
            RuntimeComponentState::Unavailable
        },
        egress_proxy_state: RuntimeComponentState::NotStarted,
        elevated_helper_state,
        quarantine_count,
    })
}
