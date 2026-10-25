//! Production Core routes with real, isolated domain stores/files. Providers and
//! process backends are not needed; no Contract Driver advances execution state.
use crate::r03_tests::{Fixture, request, success};
use crate::*;
use aura_domain_common::{ContentHash, ExpectedRevision, SessionId};
use aura_execution_contract::{ExecutionResult, SideEffectStateV1};
use aura_workspace_domain::SessionSummary;
mod l05_desktop;
mod l06_commands;
mod safety_tests;
mod services_tests;

async fn workspace(
    fixture: &Fixture,
    state: &RuntimeState,
) -> (PathBuf, WorkspaceSummary, SessionId) {
    let root = fixture.root.path().join("workspace");
    // create_dir_all: a test may seed the workspace directory before creating
    // the workspace itself, so the import step must accept an existing one.
    fs::create_dir_all(&root).expect("workspace");
    let value = success(
        state,
        method::WORKSPACE_CREATE,
        json!({"selected_root":root,"display_name":"Broker"}),
    )
    .await;
    let workspace: WorkspaceSummary =
        serde_json::from_value(value["workspace"].clone()).expect("workspace result");
    let session = SessionId::new();
    services(state)
        .persistence
        .upsert_workspace_session(
            workspace.clone(),
            SessionSummary {
                session_id: session,
                workspace_id: workspace.workspace_id,
                title: "R05".into(),
                state: "idle".into(),
            },
            CorrelationId::new(),
        )
        .expect("actual session");
    (root, workspace, session)
}
fn services(state: &RuntimeState) -> &RuntimeServices {
    let StartupMode::Ready(s) = &state.startup_mode else { panic!("Core must be ready") };
    s
}
fn results(state: &RuntimeState) -> Vec<ExecutionResult> {
    let store = services(state).broker.lock().expect("store");
    let mut query = store
        .database
        .connection()
        .prepare("SELECT payload FROM execution_results ORDER BY rowid")
        .expect("results");
    query
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query")
        .map(|r| serde_json::from_str(&r.expect("row")).expect("result"))
        .collect()
}
fn write_input(w: &WorkspaceSummary, s: SessionId, path: &str, text: &str) -> Value {
    json!({"workspace_id":w.workspace_id,"session_id":s,"target":{"path":path},"content":text,"expected":{"expected_kind":"absent"},"encoding":"utf8","line_ending":"lf"})
}

/// A Main-side **copy** really produces both files, with the source untouched.
///
/// Copy is the operation where an implementation can most easily be wrong in a way a
/// classification test cannot see: a copy that moved, or that wrote a second file but lost the
/// original, would still leave the ordered lane correctly. So this asserts the two ends on the
/// real disk and that both really ran.
#[tokio::test]
async fn r05_main_copy_leaves_the_source_and_really_writes_the_destination() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    success(&state, method::FS_WRITE_FILE, write_input(&w, s, "source.txt", "copied bytes")).await;
    success(&state,method::FS_COPY,json!({"workspace_id":w.workspace_id,"session_id":s,"source":{"path":"source.txt"},"destination":{"path":"destination.txt"},"source_expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"copied bytes")},"destination_expected":{"expected_kind":"absent"}})).await;
    // Both ends, read from the real workspace rather than from the response.
    assert_eq!(fs::read(root.join("source.txt")).expect("source survives"), b"copied bytes");
    assert_eq!(fs::read(root.join("destination.txt")).expect("destination"), b"copied bytes");
    let last = results(&state).last().expect("copy receipt").clone();
    assert_eq!(last.side_effect, SideEffectStateV1::FullyApplied);
    assert!(last.receipt.postcondition_fingerprint.is_some());
}

/// A changeset's **complete** operation set is applied as one admitted unit.
///
/// The confirmed rule is that a changeset is admitted as its whole set, not operation by
/// operation: a set the caller is not allowed to apply must not be partially applied because
/// only some of its members were evaluated. This asserts every declared operation really landed
/// — and, because a set that silently dropped a member would also "succeed", that the count of
/// real results matches the count the caller sent.
#[tokio::test]
async fn r05_a_changeset_admits_and_applies_its_complete_operation_set() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    success(&state, method::FS_WRITE_FILE, write_input(&w, s, "one.txt", "first")).await;
    success(&state, method::FS_APPLY_CHANGESET, json!({
        "workspace_id": w.workspace_id,
        "session_id": s,
        "operations": [
            {"operation":"write_file","target":{"path":"two.txt"},"content":"second","expected":{"expected_kind":"absent"},"encoding":"utf8","line_ending":"lf"},
            {"operation":"write_file","target":{"path":"three.txt"},"content":"third","expected":{"expected_kind":"absent"},"encoding":"utf8","line_ending":"lf"},
            {"operation":"copy","source":{"path":"one.txt"},"destination":{"path":"four.txt"},"source_expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"first")},"destination_expected":{"expected_kind":"absent"}}
        ]
    }))
    .await;
    // Every member of the set took effect, each read from the disk.
    assert_eq!(fs::read(root.join("two.txt")).expect("second"), b"second");
    assert_eq!(fs::read(root.join("three.txt")).expect("third"), b"third");
    assert_eq!(fs::read(root.join("four.txt")).expect("copy"), b"first");
    assert_eq!(fs::read(root.join("one.txt")).expect("source survives"), b"first");
    // One admitted unit produced one result, and it really completed: a partially applied set
    // would be `Compensated`, and a dropped member would show up here as a smaller set.
    let after = results(&state);
    let applied = after.last().expect("changeset result");
    assert_eq!(applied.side_effect, SideEffectStateV1::FullyApplied);
    assert!(applied.receipt.postcondition_fingerprint.is_some());
}

#[tokio::test]
async fn r05_real_file_write_move_delete_have_receipts_without_process_backend() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    success(&state, method::FS_WRITE_FILE, write_input(&w, s, "a.txt", "real broker")).await;
    assert_eq!(fs::read(root.join("a.txt")).expect("read"), b"real broker");
    success(&state,method::FS_MOVE,json!({"workspace_id":w.workspace_id,"session_id":s,"source":{"path":"a.txt"},"destination":{"path":"b.txt"},"source_expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"real broker")},"destination_expected":{"expected_kind":"absent"}})).await;
    assert!(!root.join("a.txt").exists());
    assert!(root.join("b.txt").exists());
    success(&state,method::FS_DELETE,json!({"workspace_id":w.workspace_id,"session_id":s,"target":{"path":"b.txt"},"expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"real broker")},"recursive":false})).await;
    assert!(!root.join("b.txt").exists());
    let completed = results(&state);
    assert_eq!(completed.len(), 3);
    for result in completed {
        assert_eq!(result.side_effect, SideEffectStateV1::FullyApplied);
        assert!(result.receipt.postcondition_fingerprint.is_some());
        let store = services(&state).broker.lock().expect("store");
        let bytes = aura_persistence_core::artifact::read(
            store.database.connection(),
            result.receipt.evidence_ref,
            &result.receipt.attempt.operation_id.to_string(),
        )
        .expect("owned receipt");
        assert!(String::from_utf8(bytes).expect("JSON").contains("completed"));
    }
    let denied =
        request(&state, method::FS_WRITE_FILE, write_input(&w, s, "../escape.txt", "denied")).await;
    assert!(denied.get("error").is_some());
    assert!(!fixture.root.path().join("escape.txt").exists());
    crate::r03_tests::assert_core_broker_ready_without_requiring_process_backends(&state);
}

#[tokio::test]
async fn a_denied_file_access_is_refused_locally_and_never_rewrites_host_acl_state() {
    // Boundary counter-example for a *refused* operation, on a controlled temporary
    // subtree only: a real Windows access denial must be reported by the file tool,
    // and must not start Host Preparation, launch the elevation entry, reach the host
    // ACL write boundary, or repair/widen the DACL by itself.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    let readable = root.join("readable.txt");
    fs::write(&readable, "readable contents").expect("write the readable file");
    let locked = root.join("locked.txt");
    fs::write(&locked, "locked contents").expect("write the locked file");
    let read = |path: &str| {
        json!({
            "workspace_id": w.workspace_id,
            "session_id": s,
            "target": { "path": path },
            "range": { "range_kind": "full" },
            "explicit_encoding": null,
        })
    };
    let allowed = success(&state, method::FS_READ_TEXT, read("readable.txt")).await;
    assert_eq!(allowed["text"], "readable contents", "the tool has to work before the denial");

    let probed = success(&state, method::SECURITY_HOST_PREPARATION_PROBE, json!({})).await;
    assert!(probed["active_operation"].is_null(), "{probed}");
    let root_before =
        aura_windows_platform::capture_file_dacl(&root).expect("capture the root DACL");
    let original = aura_windows_platform::capture_file_dacl(&locked).expect("capture the DACL");

    // A real access denial: an explicit deny ACE for Everyone on this one object,
    // which the access check evaluates before the allows inherited from the workspace
    // root. It is installed before the locked file is ever read, so the refusal cannot
    // be masked by an already-open handle.
    aura_windows_platform::restore_file_dacl(&locked, "D:(D;;FA;;;WD)(A;;FA;;;SY)")
        .expect("install the denying DACL");
    let denying = aura_windows_platform::capture_file_dacl(&locked).expect("capture the DACL");
    assert!(denying.contains("(D;;FA;;;WD)"), "the denial has to be on the object: {denying}");

    let denied = request(&state, method::FS_READ_TEXT, read("locked.txt")).await;
    assert!(denied.get("error").is_some(), "a denied read must be reported: {denied}");
    let denied_again = request(&state, method::FS_READ_TEXT, read("locked.txt")).await;
    assert!(
        denied_again.get("error").is_some(),
        "a refusal must not repair itself on the next attempt: {denied_again}"
    );

    // Nothing on the host side moved: the Host Preparation state machine never entered
    // an operation (that is the only path that can launch the elevated helper or the
    // ACL write boundary), and the controlled subtree is byte-identical.
    let after = success(&state, method::SECURITY_HOST_PREPARATION_STATUS, json!({})).await;
    assert_eq!(after, probed, "a denied file access must not change Host Preparation state");
    assert!(after["active_operation"].is_null(), "{after}");
    assert_eq!(
        aura_windows_platform::capture_file_dacl(&root).expect("capture the root DACL"),
        root_before,
        "the workspace root DACL must be untouched"
    );
    assert_eq!(
        aura_windows_platform::capture_file_dacl(&locked).expect("capture the DACL"),
        denying,
        "Aura must not restore, widen or otherwise rewrite the denied object's DACL"
    );

    // Operator-side recovery is what restores access.
    aura_windows_platform::restore_file_dacl(&locked, &original).expect("restore the DACL");
    let recovered = success(&state, method::FS_READ_TEXT, read("locked.txt")).await;
    assert_eq!(recovered["text"], "locked contents");
}

#[tokio::test]
async fn ordinary_file_changes_do_not_begin_or_revise_host_preparation() {
    // Boundary counter-example: ordinary file work is a Core Broker action, not a
    // host fact. Editing a file and creating/deleting directories must not begin,
    // revise or repair a Host Preparation operation — that is what would raise the
    // elevation prompt and re-run the ACL work.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    let probed = success(&state, method::SECURITY_HOST_PREPARATION_PROBE, json!({})).await;
    assert!(probed["active_operation"].is_null(), "{probed}");
    let baseline_revision = probed["revision"].clone();

    success(&state, method::FS_WRITE_FILE, write_input(&w, s, "note.txt", "ordinary edit")).await;
    success(
        &state,
        method::FS_CREATE_DIRECTORY,
        json!({"workspace_id":w.workspace_id,"session_id":s,"target":{"path":"ordinary"}}),
    )
    .await;
    success(
        &state,
        method::FS_WRITE_FILE,
        write_input(&w, s, "ordinary/inner.txt", "nested ordinary edit"),
    )
    .await;
    success(
        &state,
        method::FS_DELETE,
        json!({"workspace_id":w.workspace_id,"session_id":s,"target":{"path":"ordinary/inner.txt"},"expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"nested ordinary edit")},"recursive":false}),
    )
    .await;
    success(
        &state,
        method::FS_DELETE,
        json!({"workspace_id":w.workspace_id,"session_id":s,"target":{"path":"ordinary"},"expected":{"expected_kind":"directory"},"recursive":true}),
    )
    .await;
    assert!(root.join("note.txt").is_file(), "the ordinary edit must really have happened");
    assert!(!root.join("ordinary").exists(), "the ordinary directory must really be gone");

    let after = success(&state, method::SECURITY_HOST_PREPARATION_STATUS, json!({})).await;
    assert_eq!(
        after, probed,
        "ordinary file changes must not change the host preparation state or revision"
    );
    assert_eq!(after["revision"], baseline_revision);
    assert!(after["active_operation"].is_null(), "{after}");
}

#[tokio::test]
async fn r05_model_config_commit_noop_and_unvalidated_secret_canary() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let created=success(&state,"models.provider.create",json!({"expected_revision":ExpectedRevision::new(1),"input":{"display_name":"R05 provider","source":{"type":"custom","catalog_provider_id":null}}})).await;
    let provider = created["value"]["id"].clone();
    let revision = services(&state)
        ._settings
        .snapshot(aura_config::ConfigModule::Models)
        .expect("snapshot")
        .document
        .revision;
    success(&state,"models.provider.rename",json!({"expected_revision":ExpectedRevision::new(revision),"provider_id":provider,"display_name":"R05 provider"})).await;
    let canary = "R05-SECRET-CANARY-MUST-NOT-BE-IN-BROKER-DB";
    let rejected = request(
        &state,
        "models.provider.create",
        json!({"expected_revision":ExpectedRevision::new(revision),"bad_secret_field":canary}),
    )
    .await;
    assert!(rejected.get("error").is_some());
    let completed = results(&state);
    assert_eq!(completed[0].side_effect, SideEffectStateV1::FullyApplied);
    assert_eq!(completed[1].side_effect, SideEffectStateV1::NoEffect);
    let store = services(&state).broker.lock().expect("store");
    for table in ["operations", "execution_attempts", "execution_results"] {
        let count: i64 = store
            .database
            .connection()
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE payload LIKE ?1"),
                [format!("%{canary}%")],
                |r| r.get(0),
            )
            .expect("canary");
        assert_eq!(count, 0);
    }
    let count: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM artifact_objects WHERE CAST(bytes AS TEXT) LIKE ?1",
            [format!("%{canary}%")],
            |r| r.get(0),
        )
        .expect("evidence canary");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn r05_commit_failure_after_effect_is_unknown_and_cannot_replay_after_restart() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    services(&state).broker.lock().expect("store").database.connection().execute_batch("CREATE TRIGGER r05_fault BEFORE INSERT ON execution_results BEGIN SELECT RAISE(ABORT,'receipt fault'); END;").expect("inject result transaction failure");
    let command = CommandInvocationId::new();
    let call = JsonRpcRequest {
        jsonrpc: JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: Some(command),
        method: method::FS_WRITE_FILE.into(),
        params: write_input(&w, s, "committed.txt", "only once"),
    };
    let output =
        broker::dispatch_files(&state, services(&state), &call).await.expect_err("persist fault");
    assert_eq!(output.code.as_str(), "command.outcome_unknown");
    assert_eq!(fs::read(root.join("committed.txt")).expect("real effect"), b"only once");
    assert!(results(&state).is_empty());
    services(&state)
        .broker
        .lock()
        .expect("store")
        .database
        .connection()
        .execute_batch("DROP TRIGGER r05_fault;")
        .expect("remove own fault");
    drop(state);
    let restarted = fixture.start().await;
    assert!(broker::dispatch_files(&restarted, services(&restarted), &call).await.is_err());
    let store = services(&restarted).broker.lock().expect("store");
    assert_eq!(
        aura_persistence_core::operation::recoverable(store.database.connection())
            .expect("unknown claim")
            .len(),
        1
    );
    assert_eq!(
        store
            .database
            .connection()
            .query_row("SELECT COUNT(*) FROM execution_attempts", [], |r| r.get::<_, i64>(0))
            .expect("attempt count"),
        1
    );
}

#[tokio::test]
#[cfg(feature = "p08-contract-driver")]
async fn r05_git_reads_do_not_write_index_or_execute_hooks() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    // CLI prepares only this temporary repo; Core's actual native Git additionally
    // receives the explicit compile-time test environment-block profile.
    let isolated_config = fixture.root.path().join("git-config");
    fs::write(&isolated_config, "").expect("empty Git configuration");
    let git = |args: &[&str]| {
        let output = std::process::Command::new(
            std::env::var_os("AURA_GIT_TEST_BINARY").unwrap_or_else(|| "git".into()),
        )
        .arg("-C")
        .arg(&root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", &isolated_config)
        .env("GIT_CONFIG_SYSTEM", &isolated_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("HOME", fixture.root.path())
        .output()
        .expect("fixture Git");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).expect("Git UTF-8")
    };
    git(&["init"]);
    fs::write(root.join("tracked.txt"), "before\n").expect("file");
    git(&["add", "tracked.txt"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "baseline",
    ]);
    let commit = git(&["rev-parse", "HEAD"]).trim().to_owned();
    fs::write(root.join("tracked.txt"), "after\n").expect("change");
    let hook_canary = root.join("hook-ran.txt");
    fs::write(
        root.join(".git/hooks/post-index-change"),
        format!("#!/bin/sh\necho ran > '{}'\n", hook_canary.display()),
    )
    .expect("hook canary");
    git(&["config", "filter.forbidden.clean", "echo R05_EXTERNAL_FILTER_CANARY"]);
    fs::rename(root.join(".git/config"), root.join(".git/CONFIG")).expect("case variant");
    fs::write(root.join(".gitattributes"), "tracked.txt filter=forbidden\n").expect("attributes");
    let index_before = fs::read(root.join(".git/index")).expect("index bytes");
    for query in [
        json!({"kind":"status","input":{}}),
        json!({"kind":"diff","input":{}}),
        json!({"kind":"log","input":{"limit":5}}),
        json!({"kind":"log","input":{"commit":commit.to_string()}}),
        json!({"kind":"status","input":{"include_branches":true}}),
    ] {
        let output = success(
            &state,
            "git.workspace.read",
            json!({"workspace_id":w.workspace_id,"session_id":s,"query":query}),
        )
        .await;
        assert!(output["result"].is_object(), "structured native Git result");
    }
    assert_eq!(fs::read(root.join(".git/index")).expect("index unchanged"), index_before);
    assert!(!hook_canary.exists());
    let bad=request(&state,"git.workspace.read",json!({"workspace_id":w.workspace_id,"session_id":s,"query":{"kind":"log","input":{"commit":"HEAD; command"}}})).await;
    assert!(bad.get("error").is_some());
    // Native config semantics: an external malformed include is
    // an honest configuration error, never silently replaced with a private config.
    fs::write(fixture.root.path().join("poison-config"), "[broken\n").expect("include canary");
    git(&["config", "include.path", "../../poison-config"]);
    assert!(
        request(
            &state,
            "git.workspace.read",
            json!({"workspace_id":w.workspace_id,"session_id":s,"query":{"kind":"status","input":{}}})
        )
        .await
        .get("error")
        .is_some()
    );
}

#[tokio::test]
async fn r05_artifact_owner_and_export_postcondition_are_real() {
    use aura_artifact_contract::*;
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    let artifact = services(&state)
        .persistence
        .write_artifact(aura_persistence::ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "text/plain".into(),
            access: ArtifactAccess::USER_READ.union(ArtifactAccess::EXPORT),
            owner: ArtifactOwnerRef::Session { session_id: s },
            bytes: b"artifact bytes".to_vec(),
        })
        .expect("owned artifact");
    success(&state,method::ARTIFACT_EXPORT,json!({"workspace_id":w.workspace_id,"session_id":s,"artifact_id":artifact.artifact_id,"target":{"target_kind":"workspace","path":"artifact.txt"},"expected":{"expected_kind":"absent"}})).await;
    assert_eq!(fs::read(root.join("artifact.txt")).expect("export"), b"artifact bytes");
    assert_eq!(
        results(&state).last().expect("receipt").side_effect,
        SideEffectStateV1::FullyApplied
    );
    let rejected=request(&state,method::ARTIFACT_EXPORT,json!({"workspace_id":w.workspace_id,"session_id":SessionId::new(),"artifact_id":artifact.artifact_id,"target":{"target_kind":"workspace","path":"forbidden.txt"},"expected":{"expected_kind":"absent"}})).await;
    assert!(rejected.get("error").is_some());
    assert!(!root.join("forbidden.txt").exists());
    let memory = success(&state, "broker.context", json!({"kind":"memory_settings"})).await;
    assert!(memory["value"].is_object());
}

#[tokio::test]
async fn r05_fs_search_text_hits_real_files_without_process_backend() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    // Seed the file before the workspace is created: the search answers from the
    // catalog snapshot, so a file that appears after it must not decide whether
    // this test sees it (that made the test flaky under load, not stricter).
    let seeded = fixture.root.path().join("workspace");
    fs::create_dir(&seeded).expect("workspace");
    fs::write(seeded.join("needle.txt"), "unique_r05_search_token\n").expect("search file");
    let (root, w, s) = workspace(&fixture, &state).await;
    assert!(root.join("needle.txt").exists(), "the seeded file must be in the workspace");
    let found = success(
        &state,
        method::FS_SEARCH_TEXT,
        json!({
            "workspace_id": w.workspace_id,
            "session_id": s,
            "pattern": {"kind":"literal","text":"unique_r05_search_token","case_sensitive":true},
            "root": ".",
            "path_glob": null,
            "include_ignored": false,
            "include_generated": false,
            "limit": 50
        }),
    )
    .await;
    assert_eq!(found["matches"].as_array().expect("matches").len(), 1);
    assert_eq!(found["matches"][0]["excerpt"], "unique_r05_search_token");
    assert_eq!(found["completion"]["status"], "complete");
    crate::r03_tests::assert_core_broker_ready_without_requiring_process_backends(&state);
}
