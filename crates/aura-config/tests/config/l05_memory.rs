use aura_config::{AuraPaths, ConfigModule, SettingsFacade};
use serde_json::json;

#[test]
fn invalid_memory_at_startup_is_preserved_instead_of_restoring_an_enabled_lkg() {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    facade.update_memory_settings(1, &aura_config::MemorySettings::default()).expect("enabled LKG");
    drop(facade);
    let bad = b"schema_version = 1\nrevision = 3\nautomatic_maintenance = 'bad'\n";
    std::fs::write(paths.config_path(ConfigModule::Memory), bad).expect("bad input");
    let reopened = SettingsFacade::open(paths.clone()).expect("other domains stay available");
    assert!(!reopened.memory_settings().settings.automatic_maintenance);
    assert!(reopened.memory_settings().issue.is_some());
    assert_eq!(std::fs::read(paths.config_path(ConfigModule::Memory)).expect("preserved"), bad);
    assert!(reopened.snapshot(ConfigModule::Tools).is_ok());
}

#[test]
fn defaults_are_enabled_but_invalid_external_config_disables_without_overwriting_and_can_be_recovered()
 {
    use aura_config::{MemorySettings, recovery::ConfigRecoveryAction};
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    assert_eq!(facade.memory_settings().settings, MemorySettings::default());
    facade
        .update_memory_settings(
            1,
            &MemorySettings { automatic_maintenance: false, event_retention_days: 7 },
        )
        .expect("save");
    let damaged=b"schema_version = 1\nrevision = 3\nautomatic_maintenance = 'not a bool'\nevent_retention_days = 7\n";
    std::fs::write(paths.config_path(ConfigModule::Memory), damaged)
        .expect("external invalid edit");
    assert!(facade.reload(ConfigModule::Memory).is_err());
    assert!(!facade.memory_settings().settings.automatic_maintenance);
    assert!(facade.memory_settings().issue.is_some());
    assert_eq!(
        std::fs::read(paths.config_path(ConfigModule::Memory)).expect("raw preserved"),
        damaged
    );
    let preview = facade.plan_domain_recovery(ConfigModule::Memory).expect("preview");
    assert!(preview.allows(ConfigRecoveryAction::RestorePreviousValid));
    facade
        .apply_domain_recovery(&preview, ConfigRecoveryAction::RestorePreviousValid)
        .expect("explicit restore");
    assert!(!facade.memory_settings().settings.automatic_maintenance, "restores valid user choice");
    assert!(facade.snapshot(ConfigModule::Memory).expect("valid").document.revision > 2);
}

#[test]
fn memory_configuration_rejects_bad_values_and_does_not_publish_them() {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths).expect("settings");
    for (field, value) in [
        ("automatic_maintenance", json!("yes")),
        ("event_retention_days", json!(0)),
        ("global_root", json!("C:/forbidden")),
    ] {
        assert!(
            facade
                .update(ConfigModule::Memory, 1, |values| {
                    values.insert(field.into(), value);
                })
                .is_err(),
            "{field} must be rejected before publication"
        );
        assert_eq!(facade.snapshot(ConfigModule::Memory).expect("unchanged").document.revision, 1);
    }
}
