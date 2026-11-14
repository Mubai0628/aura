//! A normal exit writes the log back into the file and empties it, and notes a run owed before the
//! next start when enough of the file is free, or in use beyond what the last run kept.
use aura_domain_common::{CorrelationId, Revision, SessionId, WorkspaceId};
use aura_persistence::slimming;
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};

use super::*;

fn conversation(coordinator: &PersistenceCoordinator, index: usize) {
    let workspace_id = WorkspaceId::new();
    let root = format!(r"C:\Aura Tests\Workspace {index}");
    let workspace = WorkspaceSummary {
        workspace_id,
        display_name: format!("Workspace {index}"),
        selected_root: root.clone(),
        canonical_root: root,
        root_identity: None,
        state: WorkspaceState::Online,
        revision: Revision::INITIAL,
        catalog_freshness: CatalogFreshness::Unavailable,
        authorization_epoch: Revision::INITIAL,
    };
    let session = SessionSummary {
        session_id: SessionId::new(),
        workspace_id,
        title: format!("Session {index}"),
        state: "idle".to_owned(),
    };
    coordinator.upsert_workspace_session(workspace, session, CorrelationId::new()).unwrap();
}

fn write_marker(store: &Store, kept_bytes: u64, owed_runs: Option<u32>) {
    let marker = serde_json::json!({
        "schema_version": 1, "kept_bytes": kept_bytes, "owed_runs": owed_runs,
    });
    std::fs::write(store.database.with_file_name(slimming::MARKER), marker.to_string()).unwrap();
}

#[test]
fn a_normal_exit_writes_the_log_back_and_empties_it_and_the_store_still_serves() {
    let store = Store::new();
    let coordinator = store.start();
    for index in 0..20 {
        conversation(&coordinator, index);
    }
    assert!(store.wal_bytes() > 0, "the writes are in the log");
    let exit = coordinator.prepare_exit().unwrap();
    assert!(exit.log_emptied, "{exit:?}");
    assert_eq!(store.wal_bytes(), 0);
    // Core serves until Main stops it: a write after the exit is kept as before.
    conversation(&coordinator, 20);
    drop(coordinator);
    let reopened = store.start();
    assert_eq!(reopened.workspace_session_projection().unwrap().workspaces.len(), 21);
}

#[test]
fn a_normal_exit_notes_a_run_owed_when_much_of_the_file_is_free() {
    let store = Store::new();
    let coordinator = store.start();
    let exit = coordinator.prepare_exit().unwrap();
    assert!(!exit.slimming_owed, "a new store owes nothing");
    assert_eq!(slimming::maintenance(&store.database), None);
    drop(coordinator);

    store.leave_free(70);
    let coordinator = store.start();
    assert!(coordinator.prepare_exit().unwrap().slimming_owed);
    assert_eq!(slimming::maintenance(&store.database).unwrap().owed_runs, Some(0));
    // A run already owed keeps the runs that started on it.
    write_marker(&store, 0, Some(2));
    assert!(coordinator.prepare_exit().unwrap().slimming_owed);
    assert_eq!(slimming::maintenance(&store.database).unwrap().owed_runs, Some(2));
}

#[test]
fn pages_in_use_past_what_the_last_run_kept_note_a_run_owed_too() {
    let store = Store::new();
    store.slim(u64::MAX / 4);
    let kept = slimming::maintenance(&store.database).unwrap().kept_bytes;
    assert!(kept > 0);
    // Growth within the allowance owes nothing.
    let db = store.raw();
    let grow = |mib: u64| {
        for _ in 0..mib {
            db.execute(
                "INSERT INTO artifacts(artifact_id, scope, media_type, access_flags, byte_length,
                    content_hash_algorithm, content_hash_hex, inline_bytes, created_at)
                 VALUES (lower(hex(randomblob(16))), 'system_cache', 'application/octet-stream',
                    0, 1048576, 'blake3', lower(hex(randomblob(32))), zeroblob(1048576),
                    '2026-11-06T00:00:00Z')",
                [],
            )
            .unwrap();
        }
    };
    grow(20);
    let coordinator = store.start();
    assert!(!coordinator.prepare_exit().unwrap().slimming_owed);
    drop(coordinator);
    grow(50);
    drop(db);
    let coordinator = store.start();
    assert!(coordinator.prepare_exit().unwrap().slimming_owed);
    let marker = slimming::maintenance(&store.database).unwrap();
    assert_eq!((marker.kept_bytes, marker.owed_runs), (kept, Some(0)));
}
