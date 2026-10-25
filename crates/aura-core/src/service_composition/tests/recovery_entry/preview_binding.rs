//! Observing a repaired configuration must not mutate the version being confirmed.
use super::*;

#[tokio::test]
async fn s06_r3_recovery_preview_observes_one_stable_configuration_version() {
    let fixture = Fixture::new();
    drop(SettingsFacade::open(fixture.paths.clone()).expect("seed valid configuration"));
    break_domain(&fixture.paths, ConfigModule::Tools);
    let state = fixture.start().await;
    let path = fixture.paths.config_path(ConfigModule::Tools);
    fs::remove_dir_all(&path).expect("clear this fixture's damaged domain");
    fs::write(&path, json!({"schema_version":1,"revision":5,"values":{}}).to_string())
        .expect("supply recovery input before planning");
    let before = fs::read(&path).expect("read the provided recovery input");
    let first = success(&state, "config.recovery.plan", json!({"domains":["tools"]})).await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let second = success(&state, "config.recovery.plan", json!({"domains":["tools"]})).await;
    let after = fs::read(&path).expect("read the same input after the watcher observed it");
    assert_eq!(before, after, "a read-only recovery preview must not rewrite its input");
    assert_eq!(
        first["domains"][0]["preview_id"], second["domains"][0]["preview_id"],
        "watcher observation must not invalidate unchanged recovery material: first={first}; second={second}"
    );
}
