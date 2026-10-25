use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, mpsc::Receiver},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aura_config::{
    AuraPaths, CUSTOM_SYSTEM_ADDENDUM_FIELD, ConfigBundleFaultPoint, ConfigEvent, ConfigEventKind,
    ConfigFaultPoint, ConfigModule, ConfigModuleBundleEntry, ConfigSource, ConfigValidationError,
    ModuleConfigDocument, ModuleConfigValidator, SETTINGS_FILE_MAX_BYTES,
    SETTINGS_FILE_TOO_LARGE_CODE, SettingsFacade, SettingsFacadeError, encode_document,
};
use aura_domain_common::CredentialRef;
use aura_windows_platform::{SecretBytes, WindowsCredentialStore};
use serde_json::json;
use tempfile::TempDir;

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

#[test]
fn creates_full_layout_enforces_revision_and_retains_five_valid_versions() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    for module in ConfigModule::ALL {
        assert!(paths.config_path(module).is_file(), "{module:?} config");
    }
    for directory in [
        paths.data_dir.as_path(),
        paths.artifact_objects_dir.as_path(),
        paths.config_history_dir.as_path(),
        paths.config_transaction_dir.as_path(),
        paths.local_root.join("tools").as_path(),
        paths.local_root.join("skills").as_path(),
        paths.local_root.join("plugins").as_path(),
        paths.local_root.join("web").as_path(),
        paths.local_root.join("worktrees").as_path(),
        paths.local_root.join("e2e-runs").as_path(),
    ] {
        assert!(directory.is_dir(), "missing {}", directory.display());
    }

    let updated = facade
        .update(ConfigModule::Models, 1, |values| {
            values.insert(
                "endpoint".to_owned(),
                json!({ "credential_ref": "wincred://aura/model/main" }),
            );
        })
        .expect("valid update");
    assert_eq!(updated.document.revision, 2);
    assert!(
        facade.update(ConfigModule::Models, 1, |_| {}).is_err(),
        "stale expected_revision must be rejected"
    );

    let rejected_marker = "aura-secret-must-never-reach-history";
    assert!(
        facade
            .update(ConfigModule::Models, 2, |values| {
                values.insert("unsafe".to_owned(), json!({ "api_key": rejected_marker }));
            })
            .is_err()
    );
    assert_eq!(facade.snapshot(ConfigModule::Models).expect("snapshot").document.revision, 2);

    for expected in 2..9 {
        facade
            .update(ConfigModule::Models, expected, |values| {
                values.insert("counter".to_owned(), json!(expected));
            })
            .expect("revision update");
    }
    let history = paths.history_path(ConfigModule::Models);
    let valid_versions = fs::read_dir(&history)
        .expect("history")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("version-"))
        .count();
    assert_eq!(valid_versions, 5);
    assert!(!tree_contains(&paths.config_root, rejected_marker.as_bytes()));
    assert!(!tree_contains(&paths.config_history_dir, rejected_marker.as_bytes()));
}

#[test]
fn prompt_settings_snapshot_normalizes_manual_edits_and_tracks_revision() {
    let root = tempfile::tempdir().expect("temp root");
    let paths = paths(&root);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");

    let initial = facade.prompt_snapshot().expect("initial prompt snapshot");
    assert_eq!(initial.revision, 1);
    assert_eq!(initial.custom_system_addendum, None);

    let prompt_path = paths.config_path(ConfigModule::Prompt);
    fs::write(
        &prompt_path,
        "schema_version = 1\nrevision = 2\ncustom_system_addendum = \"\"\"  alpha\r\nbeta\rgamma  \"\"\"\n",
    )
    .expect("write prompt settings");
    assert!(facade.reload(ConfigModule::Prompt).expect("reload prompt settings"));

    let updated = facade.prompt_snapshot().expect("updated prompt snapshot");
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.custom_system_addendum.as_deref(), Some("  alpha\nbeta\ngamma  "));

    facade
        .update(ConfigModule::Prompt, 2, |values| {
            values.insert(CUSTOM_SYSTEM_ADDENDUM_FIELD.to_owned(), json!(" \t\n"));
        })
        .expect("write whitespace prompt settings");
    let whitespace = facade.prompt_snapshot().expect("whitespace prompt snapshot");
    assert_eq!(whitespace.revision, 3);
    assert_eq!(whitespace.custom_system_addendum, None);
}

#[test]
fn prompt_settings_reject_control_characters_and_preserve_lkg() {
    let root = tempfile::tempdir().expect("temp root");
    let paths = paths(&root);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    facade
        .update(ConfigModule::Prompt, 1, |values| {
            values.insert(CUSTOM_SYSTEM_ADDENDUM_FIELD.to_owned(), json!("safe"));
        })
        .expect("write valid prompt settings");

    fs::write(
        paths.config_path(ConfigModule::Prompt),
        b"schema_version = 1\nrevision = 3\ncustom_system_addendum = \"bad\\u007fvalue\"\n",
    )
    .expect("write invalid prompt settings");
    assert!(facade.reload(ConfigModule::Prompt).is_err());
    let retained = facade.prompt_snapshot().expect("retained prompt snapshot");
    assert_eq!(retained.revision, 2);
    assert_eq!(retained.custom_system_addendum.as_deref(), Some("safe"));
}

#[test]
fn runtime_invalid_edit_keeps_last_known_good_and_valid_edit_hot_refreshes() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    let config_path = paths.config_path(ConfigModule::Models);
    fs::write(&config_path, br#"{"schema_version":1,"revision":2,"api_key":"forbidden"}"#)
        .expect("invalid external edit");
    assert!(facade.reload(ConfigModule::Models).is_err());
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("last known good").document.revision,
        1
    );
    assert!(
        String::from_utf8(fs::read(&config_path).expect("invalid file retained"))
            .expect("utf8")
            .contains("forbidden")
    );

    let mut candidate = ModuleConfigDocument::empty();
    candidate.revision = 2;
    candidate.values.insert("enabled".to_owned(), json!(true));
    fs::write(&config_path, encode_document(ConfigModule::Models, &candidate).expect("encode"))
        .expect("valid external edit");
    assert!(facade.reload(ConfigModule::Models).expect("hot reload"));
    assert_eq!(facade.snapshot(ConfigModule::Models).expect("new snapshot").document.revision, 2);
}

#[test]
fn settings_file_gate_accepts_exact_limit_and_rejects_one_byte_more_with_lkg() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    let events = facade.subscribe().expect("config events");
    let config_path = paths.config_path(ConfigModule::Models);

    fs::write(
        &config_path,
        padded_config(ConfigModule::Models, 2, SETTINGS_FILE_MAX_BYTES as usize),
    )
    .expect("write exact-limit config");
    assert!(facade.reload(ConfigModule::Models).expect("accept exact-limit config"));
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("exact-limit snapshot").document.revision,
        2
    );

    fs::write(
        &config_path,
        padded_config(ConfigModule::Models, 3, SETTINGS_FILE_MAX_BYTES as usize + 1),
    )
    .expect("write oversized config");
    let error = facade.reload(ConfigModule::Models).expect_err("reject oversized config");
    assert!(matches!(
        error,
        SettingsFacadeError::FileTooLarge {
            module: ConfigModule::Models,
            limit_bytes: SETTINGS_FILE_MAX_BYTES,
        }
    ));
    assert_eq!(error.stable_code(), Some(SETTINGS_FILE_TOO_LARGE_CODE));
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("last known good").document.revision,
        2
    );
    let rejected = wait_for(&events, ConfigModule::Models, ConfigEventKind::Rejected);
    assert_eq!(rejected.revision, 2);
    assert_eq!(
        fs::metadata(config_path).expect("oversized file retained").len(),
        SETTINGS_FILE_MAX_BYTES + 1
    );
}

#[test]
fn facade_owned_write_rejects_oversized_encoding_before_replacing_config() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    let config_path = paths.config_path(ConfigModule::Models);
    let before = fs::read(&config_path).expect("initial config");

    let error = facade
        .update(ConfigModule::Models, 1, |values| {
            values.insert(
                "large_value".to_owned(),
                json!("x".repeat(SETTINGS_FILE_MAX_BYTES as usize)),
            );
        })
        .expect_err("oversized encoded update must fail");
    assert!(matches!(
        error,
        SettingsFacadeError::FileTooLarge {
            module: ConfigModule::Models,
            limit_bytes: SETTINGS_FILE_MAX_BYTES,
        }
    ));
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("unchanged snapshot").document.revision,
        1
    );
    assert_eq!(fs::read(config_path).expect("unchanged file"), before);
}

#[test]
fn oversized_startup_file_is_left_untouched_and_recovers_from_bounded_lkg() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    facade
        .update(ConfigModule::Models, 1, |values| {
            values.insert("valid".to_owned(), json!("last-known-good"));
        })
        .expect("seed last known good");
    drop(facade);

    let config_path = paths.config_path(ConfigModule::Models);
    let oversized = padded_config(ConfigModule::Models, 3, SETTINGS_FILE_MAX_BYTES as usize + 1);
    fs::write(&config_path, &oversized).expect("write oversized startup config");

    let recovered = SettingsFacade::open(paths.clone()).expect("recover settings");
    let snapshot = recovered.snapshot(ConfigModule::Models).expect("recovered snapshot");
    assert_eq!(snapshot.document.revision, 2);
    assert_eq!(snapshot.source, ConfigSource::LastKnownGood);
    assert_eq!(fs::read(&config_path).expect("oversized file retained"), oversized);
    assert!(recovered.startup_events().iter().any(|event| {
        event.module == ConfigModule::Models
            && event.kind == ConfigEventKind::Recovered
            && event.message.contains("Oversized startup configuration was left untouched")
    }));
    let invalid_dir = paths.history_path(ConfigModule::Models).join("invalid");
    assert!(
        !invalid_dir.exists()
            || fs::read_dir(invalid_dir).expect("invalid history directory").next().is_none(),
        "oversized input must not be copied into invalid history"
    );
}

#[test]
fn prepared_bundle_is_not_runtime_visible_until_explicit_activation() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    let events = facade.subscribe().expect("config events");
    let prepared = facade
        .prepare_module_bundle(
            "jnl_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            vec![ConfigModuleBundleEntry {
                module: ConfigModule::Models,
                expected_revision: 1,
                replacement_values: Some(BTreeMap::from([(
                    "visibility".to_owned(),
                    json!("durable-but-hidden"),
                )])),
            }],
        )
        .expect("prepare bundle");

    facade.publish_prepared_module_bundle(&prepared).expect("durably publish bundle");
    let on_disk: ModuleConfigDocument = serde_json::from_slice(
        &fs::read(paths.config_path(ConfigModule::Models)).expect("models config"),
    )
    .expect("decode models config");
    assert_eq!(on_disk.revision, 2);
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("runtime snapshot").document.revision,
        1
    );
    // The domain is mid-bundle, so a reload genuinely cannot
    // decide which image is current. This used to return `Ok(false)` — a silent
    // "nothing changed" — which hid the real state from the caller. It is now an
    // explicit refusal naming the transaction that owns the domain.
    //
    // The property this case exists for is unchanged and is re-asserted just
    // below: a durable publish is NOT runtime-visible until activation.
    let reload = facade
        .reload(ConfigModule::Models)
        .expect_err("a mid-bundle reload is refused rather than silently skipped");
    assert!(
        matches!(reload, aura_config::SettingsFacadeError::BundlePending { .. }),
        "the refusal names the transaction that owns the domain; got {reload:?}"
    );
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("runtime snapshot").document.revision,
        1,
        "a refused reload must not make the durable publish visible"
    );
    assert!(events.try_recv().is_err(), "durable publish must emit no runtime event");

    let activated =
        facade.activate_published_module_bundle(&prepared).expect("activate committed metadata");
    assert_eq!(
        activated.get(&ConfigModule::Models).expect("models activation").document.revision,
        2
    );
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("runtime snapshot").document.revision,
        2
    );
    assert_eq!(events.recv().expect("runtime event").revision, 2);
}

#[test]
fn invalid_startup_file_is_backed_up_and_restored_from_last_known_good() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    facade
        .update(ConfigModule::Models, 1, |values| {
            values.insert("valid".to_owned(), json!("snapshot"));
        })
        .expect("last known good");
    drop(facade);

    fs::write(paths.config_path(ConfigModule::Models), b"{not-json")
        .expect("corrupt startup config");
    let recovered = SettingsFacade::open(paths.clone()).expect("recovered settings");
    let snapshot = recovered.snapshot(ConfigModule::Models).expect("recovered snapshot");
    assert_eq!(snapshot.document.revision, 2);
    assert_eq!(snapshot.source, ConfigSource::LastKnownGood);
    assert!(
        recovered.startup_events().iter().any(|event| event.module == ConfigModule::Models
            && event.kind == ConfigEventKind::Recovered)
    );
    let invalid_dir = paths.history_path(ConfigModule::Models).join("invalid");
    assert!(fs::read_dir(invalid_dir).expect("invalid backups").next().is_some());
}

#[test]
fn secret_bearing_startup_config_is_recovered_without_copying_secret_to_history() {
    struct PermissiveValidator;

    impl ModuleConfigValidator for PermissiveValidator {
        fn validate(
            &self,
            _module: ConfigModule,
            _document: &ModuleConfigDocument,
        ) -> Result<(), ConfigValidationError> {
            Ok(())
        }
    }

    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    paths.ensure_layout().expect("layout");
    // Recovery requires a legitimate backup. An invalid existing document with
    // no such backup must be preserved and unavailable, never reset to defaults.
    drop(SettingsFacade::open(paths.clone()).expect("create legitimate initial snapshot"));
    let secret = b"aura-p02-invalid-startup-secret";
    fs::write(
        paths.config_path(ConfigModule::Models),
        format!(
            "{{\"schema_version\":1,\"revision\":2,\"Client.Secret\":\"{}\"}}",
            String::from_utf8_lossy(secret)
        ),
    )
    .expect("secret-bearing startup config");
    let mut validators: BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>> = BTreeMap::new();
    validators.insert(ConfigModule::Models, Arc::new(PermissiveValidator));

    let facade =
        SettingsFacade::open_with_validators(paths.clone(), validators).expect("safe recovery");
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("last-known-good snapshot").document.revision,
        1
    );
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("legitimate recovered snapshot").source,
        ConfigSource::LastKnownGood
    );
    assert!(!tree_contains(&paths.config_root, secret));
    assert!(!tree_contains(&paths.config_history_dir, secret));
    assert!(
        facade.startup_events().iter().filter(|event| event.module == ConfigModule::Models).all(
            |event| !event.message.as_bytes().windows(secret.len()).any(|part| part == secret)
        )
    );
}

#[test]
fn fault_boundaries_never_leave_a_half_committed_config() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    assert!(
        facade
            .update_with_fault(
                ConfigModule::Models,
                1,
                Some(ConfigFaultPoint::AfterTempSync),
                |values| {
                    values.insert("candidate".to_owned(), json!("not-published"));
                }
            )
            .is_err()
    );
    let on_disk = fs::read(paths.config_path(ConfigModule::Models)).expect("config");
    let on_disk: ModuleConfigDocument = serde_json::from_slice(&on_disk).expect("unchanged config");
    assert_eq!(on_disk.revision, 1);
    drop(facade);

    let facade = SettingsFacade::open(paths.clone()).expect("restart cleanup");
    assert!(
        facade
            .update_with_fault(
                ConfigModule::Models,
                1,
                Some(ConfigFaultPoint::AfterReplace),
                |values| {
                    values.insert("candidate".to_owned(), json!("fully-on-disk"));
                }
            )
            .is_err()
    );
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("old in-memory snapshot").document.revision,
        1
    );
    assert!(facade.reload(ConfigModule::Models).expect("adopt committed file"));
    assert_eq!(
        facade.snapshot(ConfigModule::Models).expect("adopted snapshot").document.revision,
        2
    );
}

#[test]
fn module_bundle_recovers_after_the_first_replace_and_preserves_unowned_aura_fields() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    facade
        .update(ConfigModule::Aura, 1, |values| {
            values.insert("default_model_profile_id".to_owned(), json!("mdl_old"));
            values.insert("default_permission_mode".to_owned(), json!("read_only"));
        })
        .expect("seed aura config");
    facade
        .update(ConfigModule::Models, 1, |values| {
            values.insert("profile".to_owned(), json!("mdl_old"));
        })
        .expect("seed models config");
    let mut aura_values =
        facade.snapshot(ConfigModule::Aura).expect("aura snapshot").document.values.clone();
    aura_values.remove("default_model_profile_id");
    let mut model_values =
        facade.snapshot(ConfigModule::Models).expect("models snapshot").document.values.clone();
    model_values.remove("profile");
    let result = facade.commit_module_bundle_with_fault(
        vec![
            ConfigModuleBundleEntry {
                module: ConfigModule::Aura,
                expected_revision: 2,
                replacement_values: Some(aura_values),
            },
            ConfigModuleBundleEntry {
                module: ConfigModule::Models,
                expected_revision: 2,
                replacement_values: Some(model_values),
            },
        ],
        Some(ConfigBundleFaultPoint::AfterFirstReplace),
    );
    assert!(result.is_err());
    drop(facade);

    let recovered = SettingsFacade::open(paths.clone()).expect("recover module bundle");
    let aura = recovered.snapshot(ConfigModule::Aura).expect("recovered aura");
    let models = recovered.snapshot(ConfigModule::Models).expect("recovered models");
    assert_eq!(aura.document.revision, 3);
    assert_eq!(models.document.revision, 3);
    assert!(!aura.document.values.contains_key("default_model_profile_id"));
    assert_eq!(aura.document.values.get("default_permission_mode"), Some(&json!("read_only")));
    assert!(!models.document.values.contains_key("profile"));
    assert!(
        fs::read_dir(paths.config_transaction_dir.join("module-bundles"))
            .expect("bundle journal directory")
            .next()
            .is_none()
    );
}

#[test]
fn watcher_accepts_valid_edits_and_reports_invalid_edits() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    let events = facade.subscribe().expect("events");
    let _watcher = facade.start_watching().expect("watcher");

    let mut valid = ModuleConfigDocument::empty();
    valid.revision = 2;
    valid.values.insert("ui_scale".to_owned(), json!(1.0));
    fs::write(
        paths.config_path(ConfigModule::Ui),
        encode_document(ConfigModule::Ui, &valid).expect("encode"),
    )
    .expect("valid edit");
    let changed = wait_for(&events, ConfigModule::Ui, ConfigEventKind::Changed);
    assert_eq!(changed.revision, 2);

    fs::write(
        paths.config_path(ConfigModule::Ui),
        "schema_version = 1\nrevision = 3\npassword = \"forbidden\"\n",
    )
    .expect("invalid edit");
    let rejected = wait_for(&events, ConfigModule::Ui, ConfigEventKind::Rejected);
    assert_eq!(rejected.revision, 2);
    assert_eq!(facade.snapshot(ConfigModule::Ui).expect("last known good").document.revision, 2);
}

#[test]
fn credential_values_never_enter_config_or_history() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let facade = SettingsFacade::open(paths.clone()).expect("settings facade");
    let run = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
    let reference =
        CredentialRef::for_test(&format!("{run:x}"), "config-history").expect("test reference");
    let cleanup = CredentialCleanup(reference.clone());
    let secret = b"aura-p02-secret-value-never-persist";
    WindowsCredentialStore
        .write(&reference, &SecretBytes::new(secret.to_vec()))
        .expect("store test credential");
    facade
        .update(ConfigModule::Models, 1, |values| {
            values.insert("endpoint".to_owned(), json!({ "credential_ref": reference }));
        })
        .expect("persist only CredentialRef");
    assert!(!tree_contains(&paths.config_root, secret));
    assert!(!tree_contains(&paths.config_history_dir, secret));
    drop(cleanup);
}

struct CredentialCleanup(CredentialRef);

impl Drop for CredentialCleanup {
    fn drop(&mut self) {
        let _ = WindowsCredentialStore.delete(&self.0);
    }
}

fn wait_for(
    receiver: &Receiver<ConfigEvent>,
    module: ConfigModule,
    kind: ConfigEventKind,
) -> ConfigEvent {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let event = receiver
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("timed out waiting for {module:?} {kind:?}"));
        if event.module == module && event.kind == kind {
            return event;
        }
    }
}

fn tree_contains(root: &Path, needle: &[u8]) -> bool {
    if root.is_file() {
        return fs::read(root)
            .is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle));
    }
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| tree_contains(&entry.path(), needle))
}

fn padded_config(module: ConfigModule, revision: u64, target_size: usize) -> Vec<u8> {
    let mut document = ModuleConfigDocument::empty();
    document.revision = revision;
    document.values.insert("accepted".to_owned(), json!(true));
    let mut bytes = encode_document(module, &document).expect("encode padded config");
    assert!(bytes.len() <= target_size, "fixture must fit requested size");
    bytes.resize(target_size, b' ');
    bytes
}
