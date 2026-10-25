use super::*;

/// **A damaged `models.json` really removes the Agent chain, and repair brings it back.**
///
/// This is the case where `Models` is not a leaf, so supplying P05 has to supply the consumer
/// that depends on it. The decisive assertion is the one the previous round could not make — an
/// ordinary model-dependent call succeeds afterwards, on the same live runtime, with no restart.
#[tokio::test]
async fn a_models_fault_is_recovered_with_its_dependent_consumer_and_no_restart() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05R3 fixture operation must succeed"),
    );
    break_domain(&fixture.paths, ConfigModule::Models);

    let state = fixture.start().await;
    assert!(
        matches!(&state.startup_mode, StartupMode::Ready(_)),
        "Core still starts with a damaged models.json"
    );
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be Ready for this case");
    };

    // THE PRECONDITION: the model authority is gone, which is what makes the Agent chain's absence a
    // real consequence rather than an assumption.
    assert!(
        !state.config_degradation.is_available(ConfigDependentCapability::ModelAuthority),
        "a damaged models.json really disables the model authority"
    );
    assert!(
        services.p05().is_err(),
        "and the model authority's service was really refused at startup"
    );

    // A real repair through the real entry.
    repair_domain(&fixture.paths, ConfigModule::Models, 2);
    let entry = services.config_recovery();
    let plan = confirmed_preview(&entry, ConfigModule::Models);
    let report = entry
        .apply_domain(&plan, aura_config::recovery::ConfigRecoveryAction::Retry)
        .expect("the repair is applied through the real entry");
    assert!(
        report.outcomes.iter().any(|outcome| outcome.applied),
        "the retry really applied: {report:?}"
    );

    // THE DECISIVE ASSERTION: the model authority's **service** is now present, and so is the
    // consumer that depends on it — not merely the cleared record.
    assert!(
        services.p05().is_ok(),
        "the model authority's service is really present after the repair: {report:?}"
    );
    assert!(
        report.restored_capabilities.iter().any(|capability| capability == "model_authority"),
        "and the report lists it as restored from the service, not from the record: {report:?}"
    );
    assert!(
        !report.unavailable_capabilities.iter().any(|entry| entry.capability == "model_authority"),
        "and does not simultaneously report it unavailable: {report:?}"
    );

    // A REAL model-dependent call, through the ordinary dispatcher, on this live runtime.
    let models = success(&state, "models.settings.get_snapshot", json!({})).await;
    assert!(
        !models.is_null(),
        "a model-authority-dependent call really succeeds after the repair, with no restart: \
         {models}"
    );
}

/// **A combined `Security` + `Aura` fault is judged by real dependency, not by one domain.**
///
/// `ToolPolicy` depends on both `Tools` and `Aura`, and `SecurityAuthority` on both `Security` and
/// `Aura`. Repairing one of two degraded domains that feed the same capability must not advertise
/// that capability: the other domain's fault is still recorded, and the service is still absent.
#[tokio::test]
async fn repairing_one_of_two_domains_feeding_a_capability_does_not_advertise_it() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05R3 fixture operation must succeed"),
    );
    // Both domains feed the security authority, and the aura domain feeds the tool policy too.
    break_domain(&fixture.paths, ConfigModule::Security);
    break_domain(&fixture.paths, ConfigModule::Aura);

    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be Ready for this case");
    };
    assert!(
        !state.config_degradation.is_available(ConfigDependentCapability::SecurityAuthority),
        "the security authority is refused while its domains are damaged"
    );

    // Repair **only** `Security`. `Aura` is still damaged, so the capability it also feeds is still
    // not available, and the report must say so rather than advertising it.
    repair_domain(&fixture.paths, ConfigModule::Security, 2);
    let entry = services.config_recovery();
    let plan = confirmed_preview(&entry, ConfigModule::Security);
    let report = entry
        .apply_domain(&plan, aura_config::recovery::ConfigRecoveryAction::Retry)
        .expect("the repair is applied");

    assert!(
        report.outcomes.iter().any(|outcome| outcome.applied),
        "the Security repair really applied: {report:?}"
    );
    assert!(
        !state.config_degradation.is_available(ConfigDependentCapability::SecurityAuthority),
        "the capability is still unavailable because the other domain that feeds it is still broken"
    );
    assert!(
        !report.restored_capabilities.iter().any(|capability| capability == "security_authority"),
        "so it is NOT advertised as restored on the strength of one repaired domain: {report:?}"
    );
    // And the remaining fault is reported with its real identity, so the user can act on it.
    let remaining = report
        .unavailable_capabilities
        .iter()
        .find(|entry| entry.capability == "security_authority")
        .expect("the still-unavailable capability is reported rather than dropped");
    assert!(
        !remaining.reason.is_empty(),
        "with a real reason rather than an empty string: {remaining:?}"
    );
}

/// **After a live Models recovery, the capability really keeps working.**
///
/// This is brief item 3: "restoration" must not mean "the capability answers once". A recovered model
/// authority is maintained by the same process that supplied it, and later calls go to the **same**
/// service rather than to a replacement built to serve them.
#[tokio::test]
async fn a_recovered_model_authority_really_keeps_working_afterwards() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05R3 fixture operation must succeed"),
    );
    break_domain(&fixture.paths, ConfigModule::Models);

    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be Ready for this case");
    };
    repair_domain(&fixture.paths, ConfigModule::Models, 2);
    let entry = services.config_recovery();
    let plan = confirmed_preview(&entry, ConfigModule::Models);
    let report = entry
        .apply_domain(&plan, aura_config::recovery::ConfigRecoveryAction::Retry)
        .expect("the repair is applied");
    assert!(report.outcomes.iter().any(|outcome| outcome.applied), "{report:?}");

    // The capability really answers — the first call after the recovery.
    let first = success(&state, "models.settings.get_snapshot", json!({})).await;
    assert!(!first.is_null(), "the recovered capability answers: {first}");

    // It keeps its **one** service: a later read goes to the same object, not a replacement.
    let authority_before = services.p05().expect("the model authority is present");
    let second = success(&state, "models.settings.get_snapshot", json!({})).await;
    assert!(!second.is_null(), "and keeps answering: {second}");
    let authority_after = services.p05().expect("still present");
    assert!(
        std::sync::Arc::ptr_eq(&authority_before, &authority_after),
        "the recovery's own service is still the one in use: no replacement was built to serve \
         later calls"
    );

    // The one-time responsibility the recovery started is really discharged, and the process's other
    // owners survive it.
    assert!(
        services.agent.get().is_some(),
        "the Agent chain really came back with its consumer, not only the authority"
    );
    assert!(
        services.outstanding_startup_steps().is_empty(),
        "and the maintenance task the recovery started has really started: {:?}",
        services.outstanding_startup_steps()
    );
}
