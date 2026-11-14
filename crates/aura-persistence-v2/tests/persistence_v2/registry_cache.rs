//! A stored tool registry snapshot is decoded once
//! and shared, because decoding one took tens of milliseconds on every read of a tool call. What
//! is kept must never answer for another store, nor for a row that is gone.
use aura_domain_common::{Revision, ToolRegistrySnapshotId, UtcTimestamp, WorkspaceId};
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{V2Store, tool};
use aura_tool_contract::{ToolRegistryScopeV1, ToolRegistrySnapshotV1};
use std::sync::Arc;

fn snapshot(id: ToolRegistrySnapshotId, eligibility: &[u8]) -> ToolRegistrySnapshotV1 {
    let at: UtcTimestamp = "2026-10-20T01:00:00Z".parse().expect("time");
    ToolRegistrySnapshotV1::new(
        id,
        Revision::new(1),
        ToolRegistryScopeV1::Workspace { workspace_id: WorkspaceId::new() },
        None,
        None,
        aura_domain_common::jcs::CanonicalSha256HashV1::sha256(eligibility),
        vec![],
        vec![],
        vec![],
        at,
    )
    .expect("registry snapshot")
}

fn store(root: &std::path::Path, snapshot: &ToolRegistrySnapshotV1) -> V2Store {
    let mut store = V2Store::create(root).expect("store");
    store
        .database
        .unit_of_work(|tx| {
            tool::insert_registry(tx, snapshot)?;
            Ok::<_, Error>(())
        })
        .expect("stored snapshot");
    store
}

#[test]
fn a_snapshot_read_again_is_the_one_already_decoded() {
    let dir = tempfile::tempdir().expect("temp");
    let id = ToolRegistrySnapshotId::new();
    let stored = snapshot(id, b"shared");
    let store = store(&dir.path().join("v2"), &stored);
    let first = tool::registry(store.database.connection(), id).expect("read").expect("stored");
    let again = tool::registry(store.database.connection(), id).expect("read").expect("stored");
    assert_eq!(*first, stored);
    assert!(Arc::ptr_eq(&first, &again), "the second read shares the first decoded value");
}

#[test]
fn what_is_kept_never_answers_for_another_store() {
    let dir = tempfile::tempdir().expect("temp");
    let id = ToolRegistrySnapshotId::new();
    let (one, other) = (snapshot(id, b"one store"), snapshot(id, b"another store"));
    let first = store(&dir.path().join("one"), &one);
    let second = store(&dir.path().join("other"), &other);
    for _ in 0..2 {
        let read = |s: &V2Store| {
            tool::registry(s.database.connection(), id).expect("read").expect("stored")
        };
        assert_eq!(*read(&first), one);
        assert_eq!(*read(&second), other);
    }
}

#[test]
fn a_snapshot_whose_row_is_gone_is_not_answered_from_what_was_kept() {
    let dir = tempfile::tempdir().expect("temp");
    let id = ToolRegistrySnapshotId::new();
    let mut store = store(&dir.path().join("v2"), &snapshot(id, b"purged"));
    assert!(tool::registry(store.database.connection(), id).expect("read").is_some());
    // A deleted conversation's purge removes snapshots nothing else uses.
    store
        .database
        .unit_of_work(|tx| {
            tx.execute("DELETE FROM tool_registry_snapshots WHERE id=?1", [id.to_string()])?;
            Ok::<_, Error>(())
        })
        .expect("purged");
    assert!(tool::registry(store.database.connection(), id).expect("read").is_none());
}
