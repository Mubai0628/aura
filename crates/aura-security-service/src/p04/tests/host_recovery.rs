use super::*;

/// A frozen plan whose NUL change and two system-drive ACEs are all planned.
fn interrupted_plan() -> HostPreparationBrokerPlan {
    let mut plan = HostPreparationBrokerPlan {
        operation_id: aura_domain_common::HostPreparationOperationId::new(),
        action: HostPreparationAction::Prepare,
        host_preparation_revision: 3,
        runtime_instance_id: aura_domain_common::RuntimeInstanceId::new(),
        release_descriptor: aura_runtime_contract::ReleaseDescriptor {
            bundle_namespace: "aura.desktop.test.p04".parse().expect("namespace"),
            identity: aura_runtime_contract::ReleaseIdentity {
                release_bundle_id: aura_domain_common::ReleaseBundleId::new(),
                component_manifest_hash: ContentHash::new(
                    ContentHashAlgorithm::Sha256,
                    "22".repeat(32),
                )
                .expect("manifest hash"),
            },
        },
        plan_nonce_hash: ContentHash::blake3(b"nonce"),
        expires_at: UtcTimestamp::now(),
        system_drive_root: canonical_path(PathBuf::from(r"C:\")),
        ownership_before: HostPreparationOwnership {
            original_null_device_sddl: Some("O:BAG:SYD:(A;;FA;;;SY)S:(ML;;NW;;;LW)".to_owned()),
            ..HostPreparationOwnership::default()
        },
        add_all_application_packages_ace: true,
        add_all_restricted_application_packages_ace: true,
        update_null_device_security: true,
        plan_hash: ContentHash::blake3(&[]),
    };
    plan.plan_hash = host_preparation_plan_hash(&plan).expect("plan hash");
    plan
}

/// Facts for a run that changed nothing yet.
fn untouched_facts() -> InterruptedHostFacts {
    InterruptedHostFacts {
        system_drive_root_matches: true,
        all_application_packages_ready: false,
        all_restricted_application_packages_ready: false,
        null_ready: Some(false),
        frozen_null_descriptor_matches: Some(true),
    }
}

#[test]
fn an_interrupted_plan_that_never_ran_is_the_pre_state() {
    let plan = interrupted_plan();
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &untouched_facts()),
        HostPreparationRecoveryDisposition::PreState
    ));
    // The same plan with the frozen evidence missing cannot be called "not
    // applied": nothing proves it.
    let mut without_frozen = untouched_facts();
    without_frozen.frozen_null_descriptor_matches = None;
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &without_frozen),
        HostPreparationRecoveryDisposition::Unknown
    ));
}

#[test]
fn an_interrupted_plan_that_ran_completely_is_applied_with_exact_ownership() {
    let plan = interrupted_plan();
    let facts = InterruptedHostFacts {
        system_drive_root_matches: true,
        all_application_packages_ready: true,
        all_restricted_application_packages_ready: true,
        null_ready: Some(true),
        frozen_null_descriptor_matches: Some(false),
    };
    match classify_interrupted_from_facts(&plan, &facts) {
        HostPreparationRecoveryDisposition::Applied(ownership) => {
            assert_eq!(ownership.system_drive_root, Some(plan.system_drive_root.clone()));
            assert!(ownership.all_application_packages_ace_owned);
            assert!(ownership.all_restricted_application_packages_ace_owned);
            assert!(ownership.original_null_device_sddl.is_some());
        }
        other => panic!("a completed plan must be applied with its ownership, got {other:?}"),
    }
}

#[test]
fn readiness_alone_cannot_decide_a_plan_that_looks_compatible_before_and_after() {
    // The trap: with the coexistence-aware rule the NUL device is ready both
    // before the change (frozen constant plus the verified capability) and
    // after it (the frozen constant alone). If readiness decided, a plan that
    // never ran would be recorded as applied — so the frozen original decides,
    // and readiness only breaks the tie when it differs.
    let plan = interrupted_plan();
    let never_ran = InterruptedHostFacts {
        all_application_packages_ready: false,
        all_restricted_application_packages_ready: false,
        null_ready: Some(true),
        frozen_null_descriptor_matches: Some(true),
        ..untouched_facts()
    };
    // NUL looks ready here (the frozen constant plus the verified capability
    // coexists), so a readiness-based decision would read "ready" as "already
    // applied". The frozen original says it never ran, and the two drive ACEs
    // agree with it.
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &never_ran),
        HostPreparationRecoveryDisposition::PreState
    ));
    let did_run = InterruptedHostFacts {
        all_application_packages_ready: true,
        all_restricted_application_packages_ready: true,
        frozen_null_descriptor_matches: Some(false),
        ..never_ran
    };
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &did_run),
        HostPreparationRecoveryDisposition::Applied(_)
    ));
}

#[test]
fn a_third_drift_state_is_unknown_not_applied() {
    let plan = interrupted_plan();
    // Neither the frozen original nor the frozen expectation: some other
    // change happened, so nothing here may claim success.
    let drifted = InterruptedHostFacts {
        all_application_packages_ready: true,
        all_restricted_application_packages_ready: true,
        null_ready: Some(false),
        frozen_null_descriptor_matches: Some(false),
        ..untouched_facts()
    };
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &drifted),
        HostPreparationRecoveryDisposition::Unknown
    ));
    // An unanswerable NUL question is a drift state too, never a success.
    let unanswerable = InterruptedHostFacts {
        all_application_packages_ready: true,
        all_restricted_application_packages_ready: true,
        null_ready: None,
        frozen_null_descriptor_matches: Some(false),
        ..untouched_facts()
    };
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &unanswerable),
        HostPreparationRecoveryDisposition::Unknown
    ));
    // A different system drive root is unknown before anything else is read.
    let other_root = InterruptedHostFacts { system_drive_root_matches: false, ..unanswerable };
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &other_root),
        HostPreparationRecoveryDisposition::Unknown
    ));
}

#[test]
fn a_plan_that_needed_no_nul_change_still_reports_unknown_when_nul_is_not_ready() {
    // The plan claims NUL needs nothing, but the device does not satisfy the
    // rule: Aura cannot tell an interrupted fix from a new problem.
    let mut plan = interrupted_plan();
    plan.update_null_device_security = false;
    plan.ownership_before.original_null_device_sddl = None;
    plan.plan_hash = host_preparation_plan_hash(&plan).expect("plan hash");
    let not_ready = InterruptedHostFacts {
        all_application_packages_ready: true,
        all_restricted_application_packages_ready: true,
        null_ready: Some(false),
        frozen_null_descriptor_matches: None,
        ..untouched_facts()
    };
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &not_ready),
        HostPreparationRecoveryDisposition::Unknown
    ));
    // With the device ready, the two ACE changes alone decide.
    let ready = InterruptedHostFacts { null_ready: Some(true), ..not_ready };
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &ready),
        HostPreparationRecoveryDisposition::Applied(_)
    ));
}

#[test]
fn an_interrupted_rollback_that_restored_the_frozen_descriptor_is_applied() {
    let mut plan = interrupted_plan();
    plan.action = HostPreparationAction::Remove;
    plan.add_all_application_packages_ace = false;
    plan.add_all_restricted_application_packages_ace = false;
    plan.ownership_before.all_application_packages_ace_owned = false;
    plan.ownership_before.all_restricted_application_packages_ace_owned = false;
    plan.plan_hash = host_preparation_plan_hash(&plan).expect("plan hash");
    // The frozen original is present again: the rollback wrote it.
    let restored = InterruptedHostFacts {
        all_application_packages_ready: false,
        all_restricted_application_packages_ready: false,
        null_ready: Some(false),
        frozen_null_descriptor_matches: Some(true),
        ..untouched_facts()
    };
    match classify_interrupted_from_facts(&plan, &restored) {
        HostPreparationRecoveryDisposition::Applied(ownership) => {
            assert!(ownership.original_null_device_sddl.is_none());
            assert!(ownership.system_drive_root.is_none());
        }
        other => panic!("a completed rollback must be applied, got {other:?}"),
    }
    // Still the post-change descriptor: the rollback did not happen.
    let not_restored = InterruptedHostFacts {
        frozen_null_descriptor_matches: Some(false),
        null_ready: Some(true),
        ..restored
    };
    assert!(matches!(
        classify_interrupted_from_facts(&plan, &not_restored),
        HostPreparationRecoveryDisposition::PreState
    ));
    // A rollback plan that owned a drive ACE: the descriptor is restored but
    // the ACE is still there, so the plan half-ran. Both halves are part of
    // this plan, and one of them says "not applied".
    let mut owning_plan = plan.clone();
    owning_plan.add_all_application_packages_ace = true;
    owning_plan.ownership_before.all_application_packages_ace_owned = true;
    owning_plan.plan_hash = host_preparation_plan_hash(&owning_plan).expect("plan hash");
    let mixed = InterruptedHostFacts { all_application_packages_ready: true, ..restored };
    assert!(matches!(
        classify_interrupted_from_facts(&owning_plan, &mixed),
        HostPreparationRecoveryDisposition::Unknown
    ));
}
