use super::*;

#[test]
fn security_defaults_are_low_privilege_and_bounded() {
    let config = SecurityConfig::default();
    assert_eq!(config.native_sandbox.default_mode, "compatibility_appcontainer");
    assert!(!config.native_sandbox.allow_unrestricted_inline_scripts_in_full_access);
    assert!(config.network.private_network_requires_grant);
    assert!(config.network.quarantine_executables);
}

#[test]
fn host_preparation_plan_default_expiry_is_three_minutes() {
    assert_eq!(HOST_PREPARATION_PLAN_TTL.whole_seconds(), 180);
    assert_eq!(ELEVATION_LAUNCH_TICKET_TTL.whole_seconds(), 60);
}

#[test]
fn host_preparation_native_approval_is_allow_once_or_deny_only() {
    let approval = host_preparation_approval_view(
        HostPreparationAction::Prepare,
        UtcTimestamp::now(),
        aura_runtime_contract::ReleaseDescriptor {
            bundle_namespace: "aura.desktop.test.p04-core".parse().expect("test namespace"),
            identity: aura_runtime_contract::ReleaseIdentity {
                release_bundle_id: aura_domain_common::ReleaseBundleId::new(),
                component_manifest_hash: ContentHash::new(
                    ContentHashAlgorithm::Sha256,
                    "11".repeat(32),
                )
                .expect("manifest hash"),
            },
        },
    );
    assert_eq!(
        approval.allowed_decisions,
        [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    );
    assert_eq!(approval.request_revision, 1);
    assert!(approval.release_descriptor.is_some());
}
