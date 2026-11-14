use aura_config::{AuraPaths, ConfigModule, HookRegistration, SettingsFacade, SettingsFacadeError};

fn facade() -> (tempfile::TempDir, AuraPaths, std::sync::Arc<SettingsFacade>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    (temp, paths, facade)
}

fn entry(path: &str, enabled: bool, trusted: Option<&str>) -> HookRegistration {
    HookRegistration { path: path.into(), enabled, trusted_fingerprint: trusted.map(str::to_owned) }
}

#[test]
fn the_registry_keeps_order_trust_and_enablement_at_each_revision() {
    let (_temp, paths, facade) = facade();
    let empty = facade.hook_registry();
    assert!(empty.entries.is_empty() && empty.issue.is_none());
    let fingerprint = "a".repeat(64);
    let entries = vec![
        entry("C:/hooks/second", false, None),
        entry("C:/hooks/first", true, Some(&fingerprint)),
    ];
    let revision = facade.replace_hook_registry(empty.revision, &entries).expect("written");
    assert_eq!(revision, empty.revision + 1);
    let reopened = SettingsFacade::open(paths).expect("reopened").hook_registry();
    assert_eq!(reopened.entries, entries, "order is execution order and survives a restart");
    assert_eq!(reopened.revision, revision);
}

#[test]
fn a_stale_revision_is_a_conflict_not_an_overwrite() {
    let (_temp, _paths, facade) = facade();
    let start = facade.hook_registry().revision;
    facade.replace_hook_registry(start, &[entry("C:/hooks/a", true, None)]).expect("first");
    let error =
        facade.replace_hook_registry(start, &[entry("C:/hooks/b", true, None)]).expect_err("stale");
    assert!(matches!(error, SettingsFacadeError::RevisionConflict { .. }), "{error:?}");
    assert_eq!(facade.hook_registry().entries, [entry("C:/hooks/a", true, None)]);
}

#[test]
fn a_hand_edited_registry_that_cannot_be_read_runs_no_hook() {
    let (_temp, paths, facade) = facade();
    drop(facade);
    for bad in [
        "schema_version = 1\nrevision = 3\n[[hooks]]\npath = \"C:/a\"\nallow = true\n",
        "schema_version = 1\nrevision = 3\n[[hooks]]\npath = \"C:/a\"\ntrusted_fingerprint = \"short\"\n",
        "schema_version = 1\nrevision = 3\n[values]\nextra = 1\n",
        "schema_version = 1\nrevision = 3\n[[hooks]]\npath = \"C:/a\"\n[[hooks]]\npath = \"c:/A\"\n",
    ] {
        std::fs::write(paths.config_path(ConfigModule::Hooks), bad).expect("hand edit");
        let reopened = SettingsFacade::open(paths.clone()).expect("other domains stay available");
        let registry = reopened.hook_registry();
        assert!(registry.entries.is_empty(), "{bad}");
        assert!(registry.issue.as_deref().is_some_and(|i| i.contains("都不运行")), "{bad}");
        assert_eq!(
            std::fs::read_to_string(paths.config_path(ConfigModule::Hooks)).expect("kept"),
            bad
        );
    }
}
