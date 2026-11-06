//! A prepared bundle that was never published can be ended by its transaction identity alone,
//! for a caller whose preparation failed after the reservation; a published one cannot.

use std::collections::BTreeMap;

use aura_config::{
    AuraPaths, ConfigModule, ConfigModuleBundleEntry, PreparedConfigBundle, SettingsFacade,
    SettingsFacadeError,
};
use serde_json::json;
use tempfile::TempDir;

fn facade(directory: &TempDir) -> std::sync::Arc<SettingsFacade> {
    SettingsFacade::open(AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    ))
    .expect("facade")
}

fn prepare_theme(
    facade: &SettingsFacade,
    transaction_id: &str,
    theme: &str,
) -> Result<PreparedConfigBundle, SettingsFacadeError> {
    let revision = facade.snapshot(ConfigModule::Aura).expect("aura").document.revision;
    facade.prepare_module_bundle(
        transaction_id.to_owned(),
        vec![ConfigModuleBundleEntry {
            module: ConfigModule::Aura,
            expected_revision: revision,
            replacement_values: Some(BTreeMap::from([("theme".to_owned(), json!(theme))])),
        }],
    )
}

#[test]
fn an_unpublished_bundle_ended_by_its_identity_releases_its_domains() {
    let directory = tempfile::tempdir().expect("temp root");
    let facade = facade(&directory);
    prepare_theme(&facade, "txn_unpublished", "dark").expect("prepare");
    let busy = prepare_theme(&facade, "txn_next", "light").expect_err("the domain is reserved");
    assert!(matches!(busy, SettingsFacadeError::DomainBusy { .. }), "{busy:?}");

    assert!(facade.abort_unpublished_module_bundle("txn_unpublished").expect("end it"));
    assert!(!facade.abort_unpublished_module_bundle("txn_unpublished").expect("already ended"));
    let next = prepare_theme(&facade, "txn_next", "light").expect("the domain is free again");
    facade.publish_prepared_module_bundle(&next).expect("publish");
    facade.activate_published_module_bundle(&next).expect("activate");
    assert_eq!(
        facade.snapshot(ConfigModule::Aura).expect("aura").document.values["theme"],
        json!("light")
    );
}

#[test]
fn a_published_bundle_is_left_to_its_owner() {
    let directory = tempfile::tempdir().expect("temp root");
    let facade = facade(&directory);
    let prepared = prepare_theme(&facade, "txn_published", "dark").expect("prepare");
    facade.publish_prepared_module_bundle(&prepared).expect("publish");

    let refused =
        facade.abort_unpublished_module_bundle("txn_published").expect_err("it was published");
    assert!(
        matches!(
            &refused,
            SettingsFacadeError::BundlePending { transaction_id }
                if transaction_id == "txn_published"
        ),
        "{refused:?}"
    );
    facade.activate_published_module_bundle(&prepared).expect("the owner still finishes it");
    assert_eq!(
        facade.snapshot(ConfigModule::Aura).expect("aura").document.values["theme"],
        json!("dark")
    );
}
