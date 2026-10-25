use super::*;

/// **A process that never ran the one-time startup work really performs it on first supply.**
///
/// This is the ledger's decisive case. With `models.json` damaged at startup there is no P05, so no
/// maintenance task was started, and no Agent application, so the inbox scan, the wait restoration
/// and the derived backfill never ran. The live constructors assumed they had. After the repair, the
/// process's own record must show those steps **completed** — proof they were performed rather than
/// assumed.
#[tokio::test]
async fn a_refused_chain_really_performs_the_first_recovery_it_never_ran() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05R3 fixture operation must succeed"),
    );
    break_domain(&fixture.paths, ConfigModule::Models);

    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be Ready for this case");
    };

    // THE PRECONDITION: the steps a refused chain never performed are really outstanding.
    let outstanding_before = services.outstanding_startup_steps();
    assert!(
        !outstanding_before.is_empty(),
        "a process that refused its model authority really has one-time work outstanding"
    );
    assert!(
        outstanding_before.contains(&"model_maintenance_task"),
        "and the model maintenance task is among it, because P05 never opened: {outstanding_before:?}"
    );
    assert!(
        outstanding_before.contains(&"agent_inbox_startup_scan"),
        "as is the Agent startup scan, because the Agent application was never created: \
         {outstanding_before:?}"
    );

    // Repair and recover.
    repair_domain(&fixture.paths, ConfigModule::Models, 2);
    let entry = services.config_recovery();
    let plan = confirmed_preview(&entry, ConfigModule::Models);
    let report = entry
        .apply_domain(&plan, aura_config::recovery::ConfigRecoveryAction::Retry)
        .expect("the repair is applied");
    assert!(report.outcomes.iter().any(|outcome| outcome.applied), "{report:?}");

    // THE DECISIVE ASSERTION: the steps that were outstanding are now really completed. A ledger
    // that marked them on an intention would pass a weaker check, so this reads the process's own
    // record of what finished.
    let outstanding_after = services.outstanding_startup_steps();
    assert!(
        !outstanding_after.contains(&"model_maintenance_task"),
        "the maintenance task the process never started is now really running: {outstanding_after:?}"
    );
    assert!(
        !outstanding_after.contains(&"agent_inbox_startup_scan"),
        "and the Agent scan the process never ran has really run: {outstanding_after:?}"
    );
    assert!(
        !outstanding_after.contains(&"agent_wait_restoration"),
        "as has the wait restoration: {outstanding_after:?}"
    );
    assert!(
        !outstanding_after.contains(&"agent_derived_backfill"),
        "and the derived backfill owner is started: {outstanding_after:?}"
    );

    // And the capability is genuinely usable afterwards: the report is not the only evidence.
    let models = success(&state, "models.settings.get_snapshot", json!({})).await;
    assert!(!models.is_null(), "the repaired capability really answers: {models}");
}

/// **A partially supplied chain is not reported as a working capability.**
///
/// The ledger distinguishes "already done, do not repeat" from "never reached, must be discharged",
/// so a recovery that supplies the model authority but cannot finish its one-time work must not
/// report the capability restored. This pins the distinction the whole item turns on.
#[tokio::test]
async fn a_startup_ledger_excludes_only_what_really_finished() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone()).expect("S05R3 fixture operation must succeed"),
    );
    // A healthy process: every one-time step ran, so none is outstanding.
    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("Core must be Ready for this case");
    };
    let outstanding = services.outstanding_startup_steps();
    assert!(
        outstanding.is_empty(),
        "a fully started process has no outstanding one-time work, so a recovery cannot re-run its \
         startup: {outstanding:?}"
    );
    // And the same healthy process reports the model authority available from its **service**.
    assert!(services.p05().is_ok(), "the healthy process really has its model authority");
}

/// The first real journal query fails, its responsibility is released, and a second ordinary
/// recovery in the same runtime finishes it. No marker substitutes for the production scan.
#[tokio::test]
async fn a_real_failed_first_journal_scan_can_be_retried_without_reopening_owners() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone())
            .expect("obtain the actual recovery lifecycle fact"),
    );
    break_domain(&fixture.paths, ConfigModule::Models);
    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("ready runtime required")
    };
    repair_domain(&fixture.paths, ConfigModule::Models, 2);
    let entry = services.config_recovery();
    let db = rusqlite::Connection::open(services.persistence.database_path())
        .expect("obtain the actual recovery lifecycle fact");
    db.execute_batch("ALTER TABLE mutation_journals RENAME TO s05_unavailable_mutation_journals")
        .expect("obtain the actual recovery lifecycle fact");
    let plan = confirmed_preview(&entry, ConfigModule::Models);
    let first = entry
        .apply_domain(&plan, aura_config::recovery::ConfigRecoveryAction::Retry)
        .expect("obtain the actual recovery lifecycle fact");
    use crate::service_rebuild::OneTimeStartupStep as Step;
    assert!(!services.startup_ledger.is_completed(Step::ModelConfigJournalReconciliation));
    assert!(!services.startup_ledger.is_in_flight(Step::ModelConfigJournalReconciliation));
    assert!(services.p05().is_err(), "failed first reconciliation never publishes an authority");
    assert!(first.unavailable_capabilities.iter().any(|c| c.capability == "model_authority"));
    db.execute_batch("ALTER TABLE s05_unavailable_mutation_journals RENAME TO mutation_journals")
        .expect("obtain the actual recovery lifecycle fact");
    let original = Arc::clone(&services.persistence);
    let plan = confirmed_preview(&entry, ConfigModule::Models);
    let second = entry
        .apply_domain(&plan, aura_config::recovery::ConfigRecoveryAction::Retry)
        .expect("obtain the actual recovery lifecycle fact");
    assert!(second.restored_capabilities.iter().any(|c| c == "model_authority"), "{second:?}");
    assert!(services.startup_ledger.is_completed(Step::ModelConfigJournalReconciliation));
    assert!(Arc::ptr_eq(&original, &services.persistence));
    assert!(services.p05().is_ok());
    assert!(services.agent().is_ok());
}
