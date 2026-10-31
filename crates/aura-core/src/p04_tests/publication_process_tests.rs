//! Real OS termination of a service fixture, followed by the production startup reconciler.
//! This proves process death; authenticated Main/Core transport is covered separately.
use super::*;
use crate::AuraPaths;
use aura_domain_common::RuntimeInstanceId;
use serde_json::json;
use std::{
    cell::RefCell,
    process::{self, Command},
    time::{Duration, Instant},
};

thread_local! {
    static PROCESS_POINT: RefCell<Option<(String, PathBuf, Value)>> = const { RefCell::new(None) };
}

pub(super) fn hold_for_process_controller(stage: &str) {
    PROCESS_POINT.with(|point| {
        let point = point.borrow();
        let Some((selected, output, mut witness)) = point.as_ref().cloned() else { return };
        if selected != stage {
            return;
        }
        let database = PathBuf::from(witness["database"].as_str().unwrap());
        let connection = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let (id, state, binding): (String, String, String) = connection
            .query_row(
                "SELECT security_operation_id,state,binding_json FROM artifact_overwrite_journal",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        witness["pid"] = json!(process::id());
        witness["operation"] = json!(id);
        witness["state"] = json!(state);
        witness["binding"] = serde_json::from_str(&binding).unwrap();
        let temporary = output.with_extension("pending");
        std::fs::write(&temporary, serde_json::to_vec(&witness).unwrap()).unwrap();
        std::fs::rename(temporary, output).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("controller did not terminate the process at the actual publication boundary");
    });
}

#[tokio::test]
#[ignore = "only invoked as an isolated subprocess by the process controller"]
async fn os_publication_child() {
    let output = PathBuf::from(std::env::var_os("AURA_S045_PUBLICATION_WITNESS").unwrap());
    let selected = std::env::var("AURA_S045_PUBLICATION_STAGE").unwrap();
    assert!(matches!(selected.as_str(), "applying" | "published"));
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let target = seed(&services(&state).p04_required(), fixture.root.path());
    PROCESS_POINT.with(|point| {
        *point.borrow_mut() = Some((
            selected,
            output,
            json!({
                "root": fixture.root.path(), "database": fixture.paths.database_path,
                "target": target.path, "runtime": state.options.runtime_instance_id,
            }),
        ));
    });
    publish(&services(&state).p04_required(), &target).unwrap();
    panic!("publication missed the selected process hold");
}

#[tokio::test]
async fn os_kill_after_publication_keeps_original_binding_and_never_replays() {
    for stage in ["applying", "published"] {
        let control = tempfile::tempdir().unwrap();
        let output = control.path().join("arrival.json");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "p04_tests::publication_recovery_tests::publication_process_tests::os_publication_child",
                "--ignored",
                "--nocapture",
            ])
            .env("AURA_S045_PUBLICATION_WITNESS", &output)
            .env("AURA_S045_PUBLICATION_STAGE", stage)
            .spawn()
            .expect("start isolated publication process");
        let deadline = Instant::now() + Duration::from_secs(45);
        while !output.exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("publication child ended before its real hold: {status}");
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("publication child did not reach its real hold");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let witness: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(witness["pid"], child.id());
        assert_eq!(witness["state"], stage);
        let target = PathBuf::from(witness["target"].as_str().unwrap());
        let effect_time = std::fs::metadata(&target).unwrap().modified().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        child.kill().expect("real OS process termination");
        assert!(!child.wait().unwrap().success());
        let root = PathBuf::from(witness["root"].as_str().unwrap());
        let options = crate::CoreOptions {
            pipe_name: "s045-process-recovery".into(),
            bootstrap_stdin: true,
            parent_process_id: process::id(),
            runtime_instance_id: RuntimeInstanceId::new(),
            component_manifest_path: root.join("manifest.json"),
            allow_dev_component_mismatch: false,
            safe_mode: false,
            test_mode: crate::CoreTestMode::Normal,
            continue_after: None,
        };
        let release = crate::load_release_identity(&options).unwrap();
        let paths = AuraPaths::from_roots(root.join("config"), root.join("local"));
        let restarted = crate::initialize_services_at(&options, &release, paths).await.unwrap();
        let id: SecurityOperationId = witness["operation"].as_str().unwrap().parse().unwrap();
        let record = restarted.persistence.artifact_overwrite_journal(id).unwrap();
        assert_eq!(record.state, ArtifactOverwriteJournalState::NeedsReconcile);
        assert_eq!(serde_json::to_value(&record.binding).unwrap(), witness["binding"]);
        assert_eq!(record.error_code.as_deref(), Some("core_restarted_external_state_ambiguous"));
        assert_ne!(json!(options.runtime_instance_id), witness["runtime"]);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&target).unwrap().modified().unwrap(), effect_time);
        println!(
            "S045 OS publication recovery: stage={stage} killed_pid={} old_runtime={} new_runtime={} operation={id} state=needs_reconcile bytes=new mtime_unchanged=true",
            witness["pid"], witness["runtime"], options.runtime_instance_id
        );
    }
}
