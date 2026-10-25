use aura_domain_common::{SessionId, WorkspaceId};
use aura_persistence_core::import_report::ImportDisposition as D;
use aura_persistence_v2::V2Store;
use aura_v2_importer::{ImportFault, ImportInput, apply, preview};
use rusqlite::{Connection, params};
use std::path::Path;

fn legacy(parent: &Path) -> (ImportInput, WorkspaceId, SessionId) {
    let root = parent.join("workspace");
    std::fs::create_dir(&root).expect("legacy");
    std::fs::write(root.join("keep.txt"), b"untouched user memory").expect("legacy");
    let db = parent.join("legacy.db");
    let c = Connection::open(&db).expect("new legacy snapshot");
    c.execute_batch(include_str!("../../../fixtures/persistence/schema-42.sql"))
        .expect("frozen, complete Schema 42 definition");
    let wid = WorkspaceId::new();
    let sid = SessionId::new();
    let at = "2026-09-07T00:00:00.000000000Z";
    c.execute(
        "INSERT INTO workspaces(
            workspace_id,display_name,selected_root,canonical_root,state,
            created_at,updated_at,revision,catalog_freshness,authorization_epoch
         ) VALUES(?1,'Imported workspace',?2,?2,'online',?3,?3,1,'unavailable',1)",
        params![wid.to_string(), root.to_string_lossy(), at],
    )
    .expect("Schema 42 workspace");
    c.execute(
        "INSERT INTO sessions(session_id,workspace_id,title,state,created_at,updated_at)
         VALUES(?1,?2,'Metadata only','running',?3,?3)",
        params![sid.to_string(), wid.to_string(), at],
    )
    .expect("Schema 42 session");
    let mut registry =
        aura_contract_schema::canonical_fixture().expect("legacy").models.registry.expect("legacy");
    for endpoint in registry.endpoints.values_mut() {
        let fingerprint = aura_model_config::endpoint_fingerprint(endpoint).expect("legacy");
        if let Some(compatibility) = &mut endpoint.compatibility {
            compatibility.endpoint_fingerprint = fingerprint;
        }
    }
    aura_model_config::validate_model_registry(&registry)
        .expect("semantic legacy configuration fixture");
    let profile = registry.models.keys().next().expect("legacy");
    c.execute("INSERT INTO session_model_bindings(session_id,model_profile_id,binding_revision,bound_at,readable_ref_at_binding) VALUES(?1,?2,1,'2026-09-07T00:00:00Z','fixture')",params![sid.to_string(),profile.to_string()]).expect("legacy");
    // Arbitrary old tables are reported, not silently copied into new authority.
    c.execute_batch("CREATE TABLE old_secret_fixture(value TEXT); INSERT INTO old_secret_fixture VALUES('secret-canary-NEVER-COPY'); PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").expect("legacy");
    drop(c);
    let models = parent.join("models.json");
    let doc = aura_config::ModuleConfigDocument {
        schema_version: 1,
        revision: 1,
        values: serde_json::from_value(serde_json::to_value(registry).expect("legacy"))
            .expect("legacy"),
    };
    std::fs::write(
        &models,
        aura_config::encode_document(aura_config::ConfigModule::Models, &doc).expect("legacy"),
    )
    .expect("legacy");
    let settings = parent.join("aura.toml");
    std::fs::write(&settings, b"schema_version = 1\nrevision = 1\nlocale = 'zh-CN'\n")
        .expect("legacy");
    (
        ImportInput {
            database: db,
            include_sessions: true,
            models: Some(models),
            settings: Some(settings),
            memory_vault: Some(root),
            config_root: None,
        },
        wid,
        sid,
    )
}

#[test]
fn unsupported_snapshot_versions_are_rejected_without_changing_the_source() {
    for version in [41_i64, 43, 47] {
        let directory = tempfile::tempdir().expect("isolated snapshot");
        let (input, _, _) = legacy(directory.path());
        let connection = Connection::open(&input.database).expect("declared version fixture");
        // Deliberately invalid version metadata tests the exact version gate. It
        // is not presented as a genuine historical schema of another version.
        connection.execute("DELETE FROM schema_migrations", []).expect("replace declared version");
        connection
            .execute(
                "INSERT INTO schema_migrations(version,name,checksum,applied_at)
                 VALUES(?1,'unsupported-fixture','unsupported','2026-09-07T00:00:00Z')",
                [version],
            )
            .expect("unsupported version metadata");
        drop(connection);
        let original = std::fs::read(&input.database).expect("snapshot bytes");
        let error = match preview(input.clone()) {
            Ok(_) => panic!("unexpectedly accepted declared schema version {version}"),
            Err(error) => error,
        };
        assert!(
            matches!(
                error,
                aura_persistence_core::PersistenceCoreError::Invalid(
                    "only explicit Schema 42 snapshots can be imported"
                )
            ),
            "wrong rejection for declared schema {version}: {error:?}"
        );
        assert_eq!(std::fs::read(&input.database).expect("snapshot bytes"), original);
    }
}

#[test]
fn real_schema_42_import_is_readonly_idempotent_and_does_not_copy_credentials_or_running_state() {
    let t = tempfile::tempdir().expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    let (input, wid, sid) = legacy(t.path());
    let original = std::fs::read(&input.database).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    let plan = preview(input.clone()).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    assert!(
        plan.report().items.iter().any(|i| i.kind == "workspace" && i.disposition == D::Imported)
    );
    assert!(
        plan.report()
            .items
            .iter()
            .any(|i| i.kind == "endpoint_credential" && i.disposition == D::RequiresUserAction)
    );
    let target = t.path().join("v2");
    let report = apply(&plan, &target, None).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    assert!(report.applied && !report.already_applied);
    assert!(apply(&plan, &target, None).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state").already_applied);
    let s = V2Store::open(&target).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    assert!(aura_persistence_v2::workspace::get(s.database.connection(), wid).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state").is_some());
    assert!(
        aura_persistence_v2::workspace::session(s.database.connection(), sid).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state").is_some()
    );
    let state: String = s
        .database
        .connection()
        .query_row("SELECT state FROM sessions WHERE id=?1", [sid.to_string()], |r| r.get(0))
        .expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    assert_eq!(state, "idle");
    let credentials: i64 = s
        .database
        .connection()
        .query_row("SELECT count(*) FROM credential_records", [], |r| r.get(0))
        .expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    assert_eq!(credentials, 0);
    let bindings: i64 = s
        .database
        .connection()
        .query_row("SELECT count(*) FROM session_model_bindings", [], |r| r.get(0))
        .expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    assert_eq!(bindings, 1);
    drop(s);
    assert_eq!(std::fs::read(&input.database).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state"), original);
    assert!(!Path::new(&format!("{}-wal", input.database.display())).exists());
    assert!(!Path::new(&format!("{}-shm", input.database.display())).exists());
    assert_eq!(
        std::fs::read(input.memory_vault.as_ref().expect("real schema 42 import is readonly idempotent and does not copy credentials or running state").join("keep.txt")).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state"),
        b"untouched user memory"
    );
    for name in ["aura-v2.db", "models.json", "aura.toml", "import-report.json"] {
        assert!(
            !String::from_utf8_lossy(&std::fs::read(target.join(name)).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state"))
                .contains("secret-canary-NEVER-COPY")
        );
    }
    let mut other = input;
    other.include_sessions = false;
    other.memory_vault = None;
    let conflict = apply(&preview(other).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state"), &target, None).expect("real schema 42 import is readonly idempotent and does not copy credentials or running state");
    assert!(!conflict.applied);
    assert_eq!(conflict.items.last().expect("real schema 42 import is readonly idempotent and does not copy credentials or running state").disposition, D::Conflict);
}

#[test]
fn import_faults_do_not_publish_partial_data_and_source_changes_are_rejected() {
    let t = tempfile::tempdir()
        .expect("import faults do not publish partial data and source changes are rejected");
    let (input, _, _) = legacy(t.path());
    let plan = preview(input.clone())
        .expect("import faults do not publish partial data and source changes are rejected");
    let original = std::fs::read(&input.database)
        .expect("import faults do not publish partial data and source changes are rejected");
    for fault in [ImportFault::BeforeCommit, ImportFault::BeforePublish] {
        let target = t.path().join("failed");
        assert!(apply(&plan, &target, Some(fault)).is_err());
        assert!(!target.exists());
    }
    assert_eq!(
        std::fs::read(&input.database)
            .expect("import faults do not publish partial data and source changes are rejected"),
        original
    );
    std::fs::write(
        input
            .settings
            .as_ref()
            .expect("import faults do not publish partial data and source changes are rejected"),
        b"schema_version = 1\nrevision = 2\n",
    )
    .expect("import faults do not publish partial data and source changes are rejected");
    assert!(apply(&plan, &t.path().join("changed"), None).is_err());
}

#[test]
fn invalid_rows_and_secret_config_are_reported_without_reflection() {
    let t = tempfile::tempdir()
        .expect("invalid rows and secret config are reported without reflection");
    let (mut input, _, _) = legacy(t.path());
    input.include_sessions = false;
    let c = Connection::open(&input.database)
        .expect("invalid rows and secret config are reported without reflection");
    c.execute("UPDATE workspaces SET selected_root='C:/missing-r02-root'", [])
        .expect("invalid rows and secret config are reported without reflection");
    drop(c);
    std::fs::write(
        input
            .settings
            .as_ref()
            .expect("invalid rows and secret config are reported without reflection"),
        b"schema_version = 1\nrevision = 1\napi_key = 'config-secret-canary'\n",
    )
    .expect("invalid rows and secret config are reported without reflection");
    let plan =
        preview(input).expect("invalid rows and secret config are reported without reflection");
    assert!(
        plan.report()
            .items
            .iter()
            .any(|i| i.disposition == D::RequiresUserAction && i.kind == "workspace")
    );
    assert!(
        plan.report().items.iter().any(|i| i.disposition == D::Invalid && i.kind == "settings")
    );
    let target = t.path().join("v2");
    let report = apply(&plan, &target, None)
        .expect("invalid rows and secret config are reported without reflection");
    assert!(
        !serde_json::to_string(&report)
            .expect("invalid rows and secret config are reported without reflection")
            .contains("config-secret-canary")
    );
    assert!(
        !std::fs::read_to_string(target.join("aura.toml"))
            .expect("invalid rows and secret config are reported without reflection")
            .contains("config-secret-canary")
    );
}

#[test]
fn staged_publication_resume_checks_database_and_file_manifest_and_backup_is_new_only() {
    let t = tempfile::tempdir().expect(
        "staged publication resume checks database and file manifest and backup is new only",
    );
    let (input, _, _) = legacy(t.path());
    let plan = preview(input.clone()).expect(
        "staged publication resume checks database and file manifest and backup is new only",
    );
    let target = t.path().join("v2");
    apply(&plan, &target, None).expect(
        "staged publication resume checks database and file manifest and backup is new only",
    );
    let stage = t.path().join("committed-stage");
    std::fs::rename(&target, &stage).expect(
        "staged publication resume checks database and file manifest and backup is new only",
    );
    std::fs::write(stage.join("aura.toml"), b"tampered").expect(
        "staged publication resume checks database and file manifest and backup is new only",
    );
    assert!(aura_v2_importer::resume(&stage, &target).is_err());
    assert!(!target.exists());
    let original = aura_config::encode_document(
        aura_config::ConfigModule::Aura,
        &aura_config::parse_document(
            aura_config::ConfigModule::Aura,
            &std::fs::read(input.settings.as_ref().expect("staged publication resume checks database and file manifest and backup is new only")).expect("staged publication resume checks database and file manifest and backup is new only"),
        )
        .expect("staged publication resume checks database and file manifest and backup is new only"),
    )
    .expect("staged publication resume checks database and file manifest and backup is new only");
    std::fs::write(stage.join("aura.toml"), original).expect(
        "staged publication resume checks database and file manifest and backup is new only",
    );
    assert!(
        aura_v2_importer::resume(&stage, &target)
            .expect(
                "staged publication resume checks database and file manifest and backup is new only"
            )
            .applied
    );
    let backup = t.path().join("backup.db");
    aura_v2_importer::backup_legacy(&input.database, &backup).expect(
        "staged publication resume checks database and file manifest and backup is new only",
    );
    assert!(aura_v2_importer::backup_legacy(&input.database, &backup).is_err());
    let mut backed = input;
    backed.database = backup;
    assert!(preview(backed).is_ok());
}

#[test]
fn settings_directory_and_cli_use_the_isolated_production_store() {
    let t =
        tempfile::tempdir().expect("settings directory and cli use the isolated production store");
    let (mut input, _, _) = legacy(t.path());
    let configs = t.path().join("configs");
    std::fs::create_dir_all(configs.join("settings"))
        .expect("settings directory and cli use the isolated production store");
    std::fs::write(
        configs.join("settings/ui.toml"),
        b"schema_version=1\nrevision=5\ntheme='dark'\n",
    )
    .expect("settings directory and cli use the isolated production store");
    std::fs::write(configs.join("settings/security.toml"), b"legacy-authority-not-imported")
        .expect("settings directory and cli use the isolated production store");
    input.config_root = Some(configs);
    let plan =
        preview(input).expect("settings directory and cli use the isolated production store");
    let target = t.path().join("v2");
    apply(&plan, &target, None)
        .expect("settings directory and cli use the isolated production store");
    assert!(
        std::fs::read_to_string(target.join("settings/ui.toml"))
            .expect("settings directory and cli use the isolated production store")
            .contains("dark")
    );
    assert!(!target.join("settings/security.toml").exists());
    assert!(
        plan.report()
            .items
            .iter()
            .any(|i| i.reason_code == "extension_or_security_state_requires_later_stage")
    );
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_aura-v2-importer"))
        .arg("verify")
        .arg(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("settings directory and cli use the isolated production store");
    assert!(status.success());
    let new = t.path().join("cli-v2");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_aura-v2-importer"))
        .arg("init")
        .arg(&new)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("settings directory and cli use the isolated production store");
    assert!(status.success());
    V2Store::open(&new).expect("settings directory and cli use the isolated production store");
}

#[test]
fn active_wal_and_replaced_workspace_identity_fail_closed() {
    let t = tempfile::tempdir().expect("active wal and replaced workspace identity fail closed");
    let (input, _, _) = legacy(t.path());
    let plan =
        preview(input.clone()).expect("active wal and replaced workspace identity fail closed");
    let wal = format!("{}-wal", input.database.display());
    std::fs::write(&wal, b"active")
        .expect("active wal and replaced workspace identity fail closed");
    assert!(preview(input.clone()).is_err());
    std::fs::remove_file(&wal).expect("active wal and replaced workspace identity fail closed");
    let root = input
        .memory_vault
        .as_ref()
        .expect("active wal and replaced workspace identity fail closed");
    std::fs::rename(root, t.path().join("previous-root"))
        .expect("active wal and replaced workspace identity fail closed");
    std::fs::create_dir(root).expect("active wal and replaced workspace identity fail closed");
    assert!(apply(&plan, &t.path().join("changed-root"), None).is_err());
}

#[test]
fn recovery_manifest_cannot_authorize_a_path_outside_the_config_whitelist() {
    let t = tempfile::tempdir()
        .expect("recovery manifest cannot authorize a path outside the config whitelist");
    let (input, _, _) = legacy(t.path());
    let target = t.path().join("v2");
    apply(
        &preview(input)
            .expect("recovery manifest cannot authorize a path outside the config whitelist"),
        &target,
        None,
    )
    .expect("recovery manifest cannot authorize a path outside the config whitelist");
    let stage = t.path().join("stage");
    std::fs::rename(&target, &stage)
        .expect("recovery manifest cannot authorize a path outside the config whitelist");
    let outside = t.path().join("outside.txt");
    std::fs::write(&outside, b"outside-canary")
        .expect("recovery manifest cannot authorize a path outside the config whitelist");
    let mut files: std::collections::BTreeMap<String, String> = serde_json::from_slice(
        &std::fs::read(stage.join("import-files.json"))
            .expect("recovery manifest cannot authorize a path outside the config whitelist"),
    )
    .expect("recovery manifest cannot authorize a path outside the config whitelist");
    files.insert("../outside.txt".into(), blake3::hash(b"outside-canary").to_hex().to_string());
    let encoded = aura_persistence_core::record::encode(&files)
        .expect("recovery manifest cannot authorize a path outside the config whitelist");
    std::fs::write(stage.join("import-files.json"), &encoded)
        .expect("recovery manifest cannot authorize a path outside the config whitelist");
    // Even a manifest made internally consistent is not allowed to select paths.
    let store = V2Store::open(&stage)
        .expect("recovery manifest cannot authorize a path outside the config whitelist");
    store
        .database
        .connection()
        .execute("UPDATE import_batches SET file_manifest=?1", [&encoded])
        .expect("recovery manifest cannot authorize a path outside the config whitelist");
    drop(store);
    assert!(aura_v2_importer::resume(&stage, &target).is_err());
    assert!(!target.exists());
    assert_eq!(
        std::fs::read(outside)
            .expect("recovery manifest cannot authorize a path outside the config whitelist"),
        b"outside-canary"
    );
}
