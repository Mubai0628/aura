use super::*;
use crate::recovery::ConfigRecoveryAction::ResetToDefault;
fn paths(root: &Path) -> AuraPaths {
    AuraPaths::from_roots(root.join("roaming/Aura"), root.join("local/Aura"))
}

#[test]
fn crash_child() {
    let Some(root) = std::env::var_os("AURA_S05_RECOVERY_CRASH_ROOT") else {
        return;
    };
    let paths = paths(Path::new(&root));
    let facade =
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact");
    if std::env::var_os("AURA_S05_RESET_CRASH").is_some() {
        let directory = config_bundle_transaction_dir(&paths);
        fs::create_dir_all(&directory).expect("obtain the actual isolated configuration fact");
        fs::write(directory.join("fault.json"), b"unreadable-bundle-original")
            .expect("obtain the actual isolated configuration fact");
        drop(facade);
        let facade =
            SettingsFacade::open(paths).expect("obtain the actual isolated configuration fact");
        let plan = facade
            .plan_bundle_recovery(&ConfigModule::ALL)
            .expect("obtain the actual isolated configuration fact");
        facade.apply_bound_bundle_for_crash_test(&plan, ResetToDefault);
    }
    fs::write(paths.config_path(ConfigModule::Aura), b"damaged-first-original")
        .expect("obtain the actual isolated configuration fact");
    fs::write(paths.config_path(ConfigModule::Models), b"damaged-second-original")
        .expect("obtain the actual isolated configuration fact");
    let plan = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("obtain the actual isolated configuration fact");
    facade.apply_bound_bundle_for_crash_test(&plan, ResetToDefault);
}

fn interrupted(root: &Path) -> AuraPaths {
    let status = std::process::Command::new(
        std::env::current_exe().expect("obtain the actual isolated configuration fact"),
    )
    .args(["--exact", "service::recovery_journal::tests::crash_child", "--nocapture"])
    .env("AURA_S05_RECOVERY_CRASH_ROOT", root)
    .status()
    .expect("obtain the actual isolated configuration fact");
    assert_eq!(status.code(), Some(77), "the real process must end inside publication");
    paths(root)
}

#[test]
fn real_process_exit_keeps_responsibility_and_completes_the_confirmed_set() {
    let root = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
    let paths = interrupted(root.path());
    assert_eq!(
        scan_bundle_journals(&config_bundle_transaction_dir(&paths))
            .expect("obtain the actual isolated configuration fact")
            .len(),
        1
    );
    assert_eq!(
        fs::read(paths.config_path(ConfigModule::Models))
            .expect("obtain the actual isolated configuration fact"),
        b"damaged-second-original"
    );
    let facade =
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact");
    for module in [ConfigModule::Aura, ConfigModule::Models] {
        assert!(facade.health.health(module).is_ready());
        assert_eq!(
            facade
                .snapshot(module)
                .expect("obtain the actual isolated configuration fact")
                .document
                .revision,
            2
        );
    }
    assert!(
        scan_bundle_journals(&config_bundle_transaction_dir(&paths))
            .expect("obtain the actual isolated configuration fact")
            .is_empty()
    );
    assert!(
        facade.latest_joint_commit_record(&[ConfigModule::Aura, ConfigModule::Models]).is_some()
    );
}

#[test]
fn a_competing_edit_after_process_exit_holds_the_whole_set_without_overwrite() {
    let root = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
    let paths = interrupted(root.path());
    let first = fs::read(paths.config_path(ConfigModule::Aura))
        .expect("obtain the actual isolated configuration fact");
    let competitor = b"a-later-models-edit";
    fs::write(paths.config_path(ConfigModule::Models), competitor)
        .expect("obtain the actual isolated configuration fact");
    let facade =
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact");
    assert!(facade.snapshot(ConfigModule::Aura).is_err());
    assert!(facade.snapshot(ConfigModule::Models).is_err());
    assert!(facade.snapshot(ConfigModule::Ui).is_ok());
    assert_eq!(
        fs::read(paths.config_path(ConfigModule::Aura))
            .expect("obtain the actual isolated configuration fact"),
        first
    );
    assert_eq!(
        fs::read(paths.config_path(ConfigModule::Models))
            .expect("obtain the actual isolated configuration fact"),
        competitor
    );
    assert_eq!(
        scan_bundle_journals(&config_bundle_transaction_dir(&paths))
            .expect("obtain the actual isolated configuration fact")
            .len(),
        1
    );
}

#[test]
fn an_unknown_journal_can_only_be_reset_as_the_explicit_whole_configuration() {
    let root = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
    let paths = paths(root.path());
    drop(
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact"),
    );
    let directory = config_bundle_transaction_dir(&paths);
    fs::write(directory.join("fault.json"), b"unreadable-bundle-original")
        .expect("obtain the actual isolated configuration fact");
    let facade =
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact");
    assert!(!facade.recovery_scope_known());
    let narrow = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("obtain the actual isolated configuration fact");
    assert!(!narrow.allows(ResetToDefault));
    let all = facade
        .plan_bundle_recovery(&ConfigModule::ALL)
        .expect("obtain the actual isolated configuration fact");
    assert!(all.allows(ResetToDefault));
    assert_eq!(all.options.len(), 1);
    facade
        .apply_bundle_recovery(&all, ResetToDefault)
        .expect("obtain the actual isolated configuration fact");
    assert!(facade.recovery_scope_known());
    for module in ConfigModule::ALL {
        assert!(facade.snapshot(module).is_ok());
    }
    assert_eq!(
        fs::read(paths.config_transaction_dir.join("recovery-originals/fault.json"))
            .expect("obtain the actual isolated configuration fact"),
        b"unreadable-bundle-original"
    );
    assert!(
        scan_bundle_journals(&directory)
            .expect("obtain the actual isolated configuration fact")
            .is_empty()
    );
}

#[test]
fn an_edited_journal_invalidates_the_all_domain_reset_preview() {
    let root = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
    let paths = paths(root.path());
    drop(
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact"),
    );
    let path = config_bundle_transaction_dir(&paths).join("fault.json");
    fs::write(&path, b"unreadable-bundle-original")
        .expect("obtain the actual isolated configuration fact");
    let facade =
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact");
    let plan = facade
        .plan_bundle_recovery(&ConfigModule::ALL)
        .expect("obtain the actual isolated configuration fact");
    let before = fs::read(paths.config_path(ConfigModule::Aura))
        .expect("obtain the actual isolated configuration fact");
    fs::write(&path, b"a-new-journal-input")
        .expect("obtain the actual isolated configuration fact");
    let error = facade
        .apply_bundle_recovery(&plan, ResetToDefault)
        .expect_err("the configuration fault must be reported");
    assert!(error.to_string().contains(crate::recovery::codes::PREVIEW_STALE));
    assert_eq!(
        fs::read(paths.config_path(ConfigModule::Aura))
            .expect("obtain the actual isolated configuration fact"),
        before
    );
    assert_eq!(
        fs::read(path).expect("obtain the actual isolated configuration fact"),
        b"a-new-journal-input"
    );
}

#[test]
fn an_all_domain_reset_survives_a_real_process_exit_before_its_second_member() {
    let root = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
    let status = std::process::Command::new(
        std::env::current_exe().expect("obtain the actual isolated configuration fact"),
    )
    .args(["--exact", "service::recovery_journal::tests::crash_child", "--nocapture"])
    .env("AURA_S05_RECOVERY_CRASH_ROOT", root.path())
    .env("AURA_S05_RESET_CRASH", "1")
    .status()
    .expect("obtain the actual isolated configuration fact");
    assert_eq!(status.code(), Some(77));
    let paths = paths(root.path());
    assert_eq!(
        scan_bundle_journals(&config_bundle_transaction_dir(&paths))
            .expect("obtain the actual isolated configuration fact")
            .len(),
        2
    );
    let facade =
        SettingsFacade::open(paths.clone()).expect("obtain the actual isolated configuration fact");
    assert!(facade.recovery_scope_known());
    for module in ConfigModule::ALL {
        assert!(facade.snapshot(module).is_ok());
    }
    assert_eq!(
        fs::read(paths.config_transaction_dir.join("recovery-originals/fault.json"))
            .expect("obtain the actual isolated configuration fact"),
        b"unreadable-bundle-original"
    );
    assert!(
        scan_bundle_journals(&config_bundle_transaction_dir(&paths))
            .expect("obtain the actual isolated configuration fact")
            .is_empty()
    );
}
