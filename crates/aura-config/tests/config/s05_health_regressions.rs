use std::{collections::BTreeMap, fs};

use aura_config::{
    AuraPaths, ConfigHealthRegistry, ConfigModule, ConfigModuleBundleEntry, ConfigSource,
    ConfigValidationError, DomainHealth, DomainHealthState, SettingsFacade, SettingsFacadeError,
};
use serde_json::json;

fn registry() -> ConfigHealthRegistry {
    ConfigHealthRegistry::new(
        ConfigModule::ALL
            .into_iter()
            .map(|module| DomainHealth::ready(module, ConfigSource::File, 1))
            .collect(),
    )
}

fn paths(directory: &tempfile::TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

#[test]
fn a_second_failure_after_recovery_does_not_report_ready() {
    let module = ConfigModule::Browser;
    let registry = registry();
    let first = SettingsFacadeError::BundleRecoveryConflict { module };
    registry.mark_degraded(module, &first);
    registry.record_recovery(module, ConfigSource::File, 5);
    assert!(registry.health(module).is_ready());
    assert_eq!(registry.health(module).degraded_since, None);

    let second = SettingsFacadeError::Validation {
        module,
        source: ConfigValidationError::Semantic("second failure".to_owned()),
    };
    registry.mark_degraded(module, &second);
    let health = registry.health(module);
    assert_eq!(health.state, DomainHealthState::Degraded);
    assert_eq!(health.revision, None);
    assert_eq!(health.source, None);
    assert!(health.degraded_since.is_some());
    assert_eq!(health.original_message, Some(first.to_string()));
    assert_eq!(
        health.recovery_attempts.last().expect("second failure is recorded").message,
        second.to_string()
    );
    assert!(registry.health(ConfigModule::Memory).is_ready());
}

#[test]
fn retry_history_is_bounded_without_losing_first_cause_or_attempt_number() {
    let module = ConfigModule::Browser;
    let registry = registry();
    let first = SettingsFacadeError::BundleRecoveryConflict { module };
    registry.mark_degraded(module, &first);
    let later = SettingsFacadeError::Validation {
        module,
        source: ConfigValidationError::Semantic("later failure".to_owned()),
    };
    for _ in 1..1024 {
        registry.record_failure(module, &later);
    }
    let health = registry.health(module);
    assert!(health.recovery_attempts.len() <= 64);
    assert_eq!(health.recovery_attempts.last().expect("retry history is present").attempt, 1024);
    assert_eq!(health.original_message, Some(first.to_string()));
    assert!(!health.is_ready());
}

#[test]
fn health_reports_the_snapshot_actually_saved() {
    let directory = tempfile::tempdir().expect("isolated config root");
    let facade = SettingsFacade::open(paths(&directory)).expect("initial settings");
    let saved = facade
        .update(ConfigModule::Memory, 1, |values| {
            // Gave Memory real fields; a valid one keeps this test about health.
            values.insert("automatic_maintenance".to_owned(), json!(false));
        })
        .expect("save a newer Memory snapshot");
    let health = facade.health(ConfigModule::Memory);
    assert_eq!(health.source, Some(saved.source));
    assert_eq!(health.revision, Some(saved.document.revision));
    assert!(health.is_ready());
    assert!(health.recovery_attempts.is_empty());
}

#[test]
fn health_reports_an_accepted_external_reload() {
    let directory = tempfile::tempdir().expect("isolated config root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("initial settings");
    fs::write(
        paths.config_path(ConfigModule::Memory),
        b"schema_version = 1\nrevision = 4\nautomatic_maintenance = false\n",
    )
    .expect("write an external valid revision");
    assert!(facade.reload(ConfigModule::Memory).expect("reload the newer revision"));
    let snapshot = facade.snapshot(ConfigModule::Memory).expect("accepted Memory snapshot");
    let health = facade.health(ConfigModule::Memory);
    assert_eq!(health.revision, Some(snapshot.document.revision));
    assert_eq!(health.revision, Some(4));
    assert_eq!(health.source, Some(ConfigSource::File));
    assert!(health.recovery_attempts.is_empty());
}

#[test]
fn health_reports_both_activated_bundle_snapshots() {
    let directory = tempfile::tempdir().expect("isolated config root");
    let facade = SettingsFacade::open(paths(&directory)).expect("initial settings");
    facade
        .commit_module_bundle(vec![
            ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: 1,
                replacement_values: Some(BTreeMap::from([("theme".to_owned(), json!("system"))])),
            },
            ConfigModuleBundleEntry {
                module: ConfigModule::Browser,
                expected_revision: 1,
                replacement_values: Some(BTreeMap::from([("changed".to_owned(), json!(true))])),
            },
        ])
        .expect("activate the two-domain bundle");
    for module in [ConfigModule::Aura, ConfigModule::Browser] {
        let snapshot = facade.snapshot(module).expect("activated bundle snapshot");
        let health = facade.health(module);
        assert_eq!(health.revision, Some(snapshot.document.revision));
        assert_eq!(health.revision, Some(2));
        assert_eq!(health.source, Some(ConfigSource::File));
        assert!(health.recovery_attempts.is_empty());
    }
}
