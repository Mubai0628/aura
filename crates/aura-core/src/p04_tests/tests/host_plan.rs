use super::*;

#[tokio::test]
async fn the_plan_generated_on_this_machine_never_replaces_the_nul_descriptor() {
    // The real planner, the real host probes and the real read-only
    // installation verification: whatever plan Core would hand to the elevated
    // broker on this machine is written out and checked here. The assertion is
    // a consistency rule, not a hard-coded expectation, so the test is honest
    // on a machine without an installation too — and on this one it must show
    // that the verified capability ACE keeps the NUL descriptor untouched.
    use aura_windows_platform::installation_identity::{
        InstallationIdentity, NullDeviceVerdict, published_installations_base,
        verify_installation_identity,
    };

    let fixture = crate::r03_tests::Fixture::with_active_release_layout();
    let state = fixture.start().await;
    let probed = crate::r03_tests::success(
        &state,
        method::SECURITY_HOST_PREPARATION_PROBE,
        serde_json::json!({}),
    )
    .await;
    let revision = probed["revision"].as_u64().expect("a probed revision");
    let began = crate::r03_tests::success(
        &state,
        method::SECURITY_HOST_PREPARATION_PREPARE,
        serde_json::json!({ "expected_revision": revision }),
    )
    .await;

    let readiness = probe_null_device_readiness();
    let identity = published_installations_base()
        .map(|base| verify_installation_identity(&base))
        .unwrap_or_else(|error| InstallationIdentity::Unverified(error.to_string()));
    let current_descriptor = capture_null_device_security_descriptor()
        .expect("this machine's NUL descriptor is readable read-only");
    let capability_present =
        identity.descriptor_carries_capability(&current_descriptor).unwrap_or(false);
    let probe = probe_host_compatibility().expect("host compatibility probe");

    let Some(operation) = began["status"]["active_operation"].as_object() else {
        // Nothing to change: no plan exists, so no NUL replacement can exist.
        assert!(
            readiness.is_ready(),
            "a plan is only skipped when NUL already satisfies the rule: {readiness:?}"
        );
        return;
    };
    let operation_id: aura_domain_common::HostPreparationOperationId =
        serde_json::from_value(operation["operation_id"].clone()).expect("operation id");
    let crate::StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be ready");
    };
    let journal = services
        .persistence
        .host_preparation_operation(operation_id)
        .expect("the generated plan is persisted");
    let plan = &journal.broker_plan;

    assert_eq!(
        plan.update_null_device_security,
        match readiness {
            NullDeviceVerdict::Ready => false,
            // A candidate that did not verify refuses the plan instead of
            // guessing, so reaching here means the answer was definite.
            NullDeviceVerdict::NotReady => true,
            NullDeviceVerdict::Unanswerable(reason) =>
                panic!("the planner must refuse an unanswerable host: {reason}"),
        },
        "the plan's NUL decision must be the readiness rule the gates use"
    );
    assert_eq!(
        plan.add_all_application_packages_ace, !probe.all_application_packages_ready,
        "the plan must add exactly the missing system-drive ACEs"
    );
    assert_eq!(
        plan.add_all_restricted_application_packages_ace,
        !probe.all_restricted_application_packages_ready
    );
    if capability_present {
        assert!(
            !plan.update_null_device_security,
            "a verified installation's capability ACE is already the pre-state: the plan must not replace the descriptor"
        );
        assert!(
            plan.ownership_before.original_null_device_sddl.is_none(),
            "nothing froze a NUL original, because nothing will write one"
        );
    }

    // Evidence for the batch: the facts and the exact plan, side by side.
    // Written under the workspace `target/` (the test's working directory is
    // the crate directory), so the batch's evidence sits with the rest.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join(format!("w7-host-plan-{stamp}"));
    std::fs::create_dir_all(&directory).expect("evidence directory");
    let evidence = serde_json::json!({
        "read_only": true,
        "process_elevated": current_process_is_elevated().unwrap_or(false),
        "identity_state": match &identity {
            InstallationIdentity::Absent => "absent",
            InstallationIdentity::Verified(_) => "verified",
            InstallationIdentity::Unverified(_) => "unverified",
        },
        "capability_ace_present": capability_present,
        "null_device_verdict": match &readiness {
            NullDeviceVerdict::Ready => "ready".to_owned(),
            NullDeviceVerdict::NotReady => "not_ready".to_owned(),
            NullDeviceVerdict::Unanswerable(reason) => format!("unanswerable: {reason}"),
        },
        "system_drive_ready": probe.system_drive_ready,
        "plan": {
            "operation_id": operation_id.to_string(),
            "action": plan.action,
            "add_all_application_packages_ace": plan.add_all_application_packages_ace,
            "add_all_restricted_application_packages_ace":
                plan.add_all_restricted_application_packages_ace,
            "update_null_device_security": plan.update_null_device_security,
            "frozen_null_original_present":
                plan.ownership_before.original_null_device_sddl.is_some(),
            "plan_hash": plan.plan_hash.to_string(),
            "requires_elevation": operation["requires_elevation"],
        },
        "current_null_device_sddl_sha256": format!(
            "{:x}",
            Sha256::digest(current_descriptor.as_bytes())
        ),
    });
    std::fs::write(
        directory.join("host-preparation-plan.json"),
        serde_json::to_vec_pretty(&evidence).expect("evidence json"),
    )
    .expect("write evidence");
}
