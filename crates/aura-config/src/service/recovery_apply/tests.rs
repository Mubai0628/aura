use super::*;
use crate::recovery::ConfigRecoveryAction::{ResetToDefault, RestorePreviousValid};
use serde_json::json;
fn fixture() -> (tempfile::TempDir, Arc<SettingsFacade>) {
    let root = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
    let paths =
        AuraPaths::from_roots(root.path().join("roaming/Aura"), root.path().join("local/Aura"));
    let facade =
        SettingsFacade::open(paths).expect("obtain the actual isolated configuration fact");
    (root, facade)
}
fn edit(path: &Path, value: &str) -> Vec<u8> {
    let mut document = ModuleConfigDocument::empty();
    document.values.insert("theme".into(), json!(value));
    let bytes =
        encode(ConfigModule::Ui, &document).expect("obtain the actual isolated configuration fact");
    fs::write(path, &bytes).expect("obtain the actual isolated configuration fact");
    bytes
}
#[test]
fn same_revision_history_replacement_invalidates_the_confirmed_restore() {
    let (_root, facade) = fixture();
    facade
        .update(ConfigModule::Ui, 1, |v| {
            v.insert("theme".into(), json!("approved"));
        })
        .expect("obtain the actual isolated configuration fact");
    let plan = facade
        .plan_domain_recovery(ConfigModule::Ui)
        .expect("obtain the actual isolated configuration fact");
    let current = fs::read(facade.paths.config_path(ConfigModule::Ui))
        .expect("obtain the actual isolated configuration fact");
    let version = crate::recovery::recorded_versions(&facade.paths, ConfigModule::Ui)
        .expect("obtain the actual isolated configuration fact")
        .remove(0);
    let mut document = parse_document(
        ConfigModule::Ui,
        &fs::read(&version.path).expect("obtain the actual isolated configuration fact"),
    )
    .expect("obtain the actual isolated configuration fact");
    document.values.insert("theme".into(), json!("unconfirmed"));
    fs::write(
        &version.path,
        encode(ConfigModule::Ui, &document).expect("obtain the actual isolated configuration fact"),
    )
    .expect("obtain the actual isolated configuration fact");
    let error = facade
        .apply_domain_recovery(&plan, RestorePreviousValid)
        .expect_err("the configuration fault must be reported");
    assert!(error.to_string().contains(crate::recovery::codes::PREVIEW_STALE), "{error}");
    assert_eq!(
        fs::read(facade.paths.config_path(ConfigModule::Ui))
            .expect("obtain the actual isolated configuration fact"),
        current
    );
}
#[test]
fn an_in_place_edit_after_the_last_preview_check_survives_the_write() {
    let (_root, facade) = fixture();
    let path = facade.paths.config_path(ConfigModule::Ui);
    let plan = facade
        .plan_domain_recovery(ConfigModule::Ui)
        .expect("obtain the actual isolated configuration fact");
    let mut competitor = Vec::new();
    let result = facade.apply_domain_observed(&plan, ResetToDefault, || {
        competitor = edit(&path, "later-save");
    });
    assert!(result.is_err(), "the late edit must be refused");
    assert_eq!(fs::read(path).expect("obtain the actual isolated configuration fact"), competitor);
}
#[test]
fn a_new_file_after_an_absent_preview_is_never_replaced() {
    let (_root, facade) = fixture();
    let path = facade.paths.config_path(ConfigModule::Ui);
    fs::remove_file(&path).expect("obtain the actual isolated configuration fact");
    let plan = facade
        .plan_domain_recovery(ConfigModule::Ui)
        .expect("obtain the actual isolated configuration fact");
    let mut competitor = Vec::new();
    let result = facade.apply_domain_observed(&plan, ResetToDefault, || {
        competitor = edit(&path, "new-file");
    });
    assert!(result.is_err());
    assert_eq!(fs::read(path).expect("obtain the actual isolated configuration fact"), competitor);
}
#[test]
fn late_second_member_edit_aborts_the_owned_first_publication() {
    let (_root, facade) = fixture();
    let plan = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("obtain the actual isolated configuration fact");
    let first = facade.paths.config_path(ConfigModule::Aura);
    let second = facade.paths.config_path(ConfigModule::Models);
    let original = fs::read(&first).expect("obtain the actual isolated configuration fact");
    let competitor = b"external-late-models-input";
    let result = facade.apply_bound_bundle(&plan, ResetToDefault, |index| {
        if index == 1 {
            fs::write(&second, competitor).expect("obtain the actual isolated configuration fact");
        }
    });
    assert!(result.is_err());
    assert_eq!(
        fs::read(first).expect("obtain the actual isolated configuration fact"),
        original,
        "native abort restores the exact first object"
    );
    assert_eq!(
        fs::read(second).expect("obtain the actual isolated configuration fact"),
        competitor,
        "the conflicting second member is untouched"
    );
}
#[test]
fn bundle_restore_consumes_the_joint_record_not_each_domains_newer_history() {
    let (_root, facade) = fixture();
    facade
        .commit_module_bundle(vec![
            ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: 1,
                replacement_values: Some(BTreeMap::from([("marker".into(), json!("joint"))])),
            },
            ConfigModuleBundleEntry {
                module: ConfigModule::Models,
                expected_revision: 1,
                replacement_values: Some(BTreeMap::new()),
            },
        ])
        .expect("obtain the actual isolated configuration fact");
    facade
        .update(ConfigModule::Aura, 2, |values| {
            values.insert("marker".into(), json!("unrelated-later"));
        })
        .expect("obtain the actual isolated configuration fact");
    let plan = facade
        .plan_bundle_recovery(&[ConfigModule::Aura, ConfigModule::Models])
        .expect("obtain the actual isolated configuration fact");
    assert_eq!(
        plan.consistent_set
            .iter()
            .find(|e| e.0 == ConfigModule::Aura)
            .expect("obtain the actual isolated configuration fact")
            .1,
        2
    );
    facade
        .apply_bundle_recovery(&plan, RestorePreviousValid)
        .expect("obtain the actual isolated configuration fact");
    assert_eq!(
        facade
            .snapshot(ConfigModule::Aura)
            .expect("obtain the actual isolated configuration fact")
            .document
            .values
            .get("marker"),
        Some(&json!("joint"))
    );
    assert_eq!(
        facade
            .snapshot(ConfigModule::Aura)
            .expect("obtain the actual isolated configuration fact")
            .document
            .revision,
        4
    );
}
#[test]
fn removal_of_an_unreadable_original_requires_a_new_preview() {
    let (_root, facade) = fixture();
    let path = facade.paths.config_path(ConfigModule::Ui);
    fs::remove_file(&path).expect("obtain the actual isolated configuration fact");
    fs::create_dir(&path).expect("obtain the actual isolated configuration fact");
    let blocked = facade
        .plan_domain_recovery(ConfigModule::Ui)
        .expect("obtain the actual isolated configuration fact");
    fs::remove_dir(&path).expect("obtain the actual isolated configuration fact");
    let error = facade
        .apply_domain_recovery(&blocked, ResetToDefault)
        .expect_err("the configuration fault must be reported");
    assert!(error.to_string().contains(crate::recovery::codes::PREVIEW_STALE));
    let fresh = facade
        .plan_domain_recovery(ConfigModule::Ui)
        .expect("obtain the actual isolated configuration fact");
    assert!(
        facade
            .apply_domain_recovery(&fresh, ResetToDefault)
            .expect("obtain the actual isolated configuration fact")
            .applied
    );
}

#[test]
fn activation_failure_retains_durable_responsibility_and_recovers_on_next_open() {
    for bundle in [false, true] {
        let (_root, facade) = fixture();
        let domain = facade
            .plan_domain_recovery(ConfigModule::Ui)
            .expect("obtain the actual isolated configuration fact");
        let set = facade
            .plan_bundle_recovery(&[ConfigModule::Ui, ConfigModule::Tools])
            .expect("obtain the actual isolated configuration fact");
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held =
                facade.subscribers.lock().expect("obtain the actual isolated configuration fact");
            panic!("controlled activation failure");
        }));
        let error = if bundle {
            facade.apply_bundle_recovery(&set, ResetToDefault).map(|_| ())
        } else {
            facade.apply_domain_recovery(&domain, ResetToDefault).map(|_| ())
        }
        .expect_err("the configuration fault must be reported");
        assert!(error.to_string().contains("config event subscribers"), "{error}");
        assert!(!facade.health.health(ConfigModule::Ui).is_ready());
        let journal_dir = facade.paths.config_transaction_dir.join("module-bundles");
        assert_eq!(
            scan_bundle_journals(&journal_dir)
                .expect("obtain the actual isolated configuration fact")
                .len(),
            1
        );
        let recovered = SettingsFacade::open(facade.paths.clone())
            .expect("obtain the actual isolated configuration fact");
        assert!(recovered.health.health(ConfigModule::Ui).is_ready());
        assert_eq!(
            recovered
                .snapshot(ConfigModule::Ui)
                .expect("obtain the actual isolated configuration fact")
                .document
                .revision,
            domain.next_revision
        );
        assert!(
            scan_bundle_journals(&journal_dir)
                .expect("obtain the actual isolated configuration fact")
                .is_empty()
        );
    }
}
