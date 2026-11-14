use std::{sync::Arc, thread, time::Duration};

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOwnerRef, ArtifactScope, ArtifactSystemOwner,
};
use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId};
use aura_persistence::{
    ARTIFACT_CHUNK_SIZE, ArtifactReadRequest, ArtifactWriteRequest, PersistenceCoordinator,
    PersistenceError, PersistenceFaultPoint, PersistenceOptions, PersistenceStartup,
};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use rusqlite::Connection;
use tempfile::TempDir;

fn start(directory: &TempDir, runtime: RuntimeInstanceId) -> PersistenceCoordinator {
    let options = PersistenceOptions::new(
        directory.path().join("data/aura.db"),
        directory.path().join("data/artifact-objects"),
        runtime,
    );
    match PersistenceCoordinator::start(options).expect("start persistence") {
        PersistenceStartup::Ready(coordinator) => coordinator,
        PersistenceStartup::RecoveryRequired(recovery) => {
            panic!("unexpected recovery requirement: {}", recovery.reason)
        }
    }
}

fn workspace_session(index: usize) -> (WorkspaceSummary, SessionSummary) {
    let workspace_id = WorkspaceId::new();
    let root = format!(r"C:\Aura Tests\Workspace {index}");
    (
        WorkspaceSummary {
            workspace_id,
            display_name: format!("Workspace {index}"),
            selected_root: root.clone(),
            canonical_root: root,
            root_identity: None,
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        },
        SessionSummary {
            session_id: SessionId::new(),
            workspace_id,
            title: format!("Session {index}"),
            state: "idle".to_owned(),
        },
    )
}

#[test]
fn state_event_and_outbox_are_atomic_and_publish_only_after_commit() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let pragmas = coordinator.pragmas().expect("database pragmas");
    assert!(pragmas.foreign_keys);
    assert!(pragmas.journal_mode_wal);
    assert_eq!(pragmas.busy_timeout_milliseconds, 5_000);

    let receiver = coordinator.subscribe_workspace_sessions().expect("projection subscription");
    let (workspace, session) = workspace_session(1);
    let failed = coordinator.upsert_workspace_session_with_fault(
        workspace.clone(),
        session.clone(),
        CorrelationId::new(),
        Some(PersistenceFaultPoint::BeforeCommit),
    );
    assert!(failed.is_err());
    assert_eq!(coordinator.event_count().expect("event count"), 0);
    assert!(
        coordinator
            .workspace_session_projection()
            .expect("projection after rollback")
            .workspaces
            .is_empty()
    );
    assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());

    let committed = coordinator
        .upsert_workspace_session(workspace, session, CorrelationId::new())
        .expect("commit workspace/session");
    let published = receiver.recv_timeout(Duration::from_secs(1)).expect("post-commit projection");
    assert_eq!(published, committed);
    assert_eq!(coordinator.event_count().expect("event count"), 1);
    assert_eq!(coordinator.pending_outbox_count().expect("outbox count"), 0);
    let rebuilt = coordinator.rebuild_workspace_session_projection().expect("rebuild projection");
    let rebuilt_published =
        receiver.recv_timeout(Duration::from_secs(1)).expect("rebuilt projection published");
    assert_eq!(rebuilt_published, rebuilt);
    assert_eq!(coordinator.pending_outbox_count().expect("rebuilt outbox count"), 0);
}

#[test]
fn concurrent_clients_share_one_writer_and_multiple_readers() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = Arc::new(start(&directory, RuntimeInstanceId::new()));
    let mut writers = Vec::new();
    for index in 0..8 {
        let coordinator = Arc::clone(&coordinator);
        writers.push(thread::spawn(move || {
            let (workspace, session) = workspace_session(index);
            coordinator
                .upsert_workspace_session(workspace, session, CorrelationId::new())
                .expect("serialized writer");
        }));
    }
    for writer in writers {
        writer.join().expect("writer thread");
    }
    let mut readers = Vec::new();
    for _ in 0..8 {
        let coordinator = Arc::clone(&coordinator);
        readers.push(thread::spawn(move || {
            let projection = coordinator.workspace_session_projection().expect("concurrent read");
            assert_eq!(projection.workspaces.len(), 8);
            assert_eq!(projection.sessions.len(), 8);
        }));
    }
    for reader in readers {
        reader.join().expect("reader thread");
    }
    assert_eq!(coordinator.event_count().expect("event count"), 8);
}

#[test]
fn artifacts_deduplicate_range_read_and_gc_with_session_isolation() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace, session) = workspace_session(1);
    coordinator
        .upsert_workspace_session(workspace, session.clone(), CorrelationId::new())
        .expect("seed session");

    let bytes = vec![b'a'; ARTIFACT_CHUNK_SIZE + 100_000];
    let access = ArtifactAccess::USER_READ.union(ArtifactAccess::EXPORT);
    let first = coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "text/plain".to_owned(),
            access,
            owner: ArtifactOwnerRef::Session { session_id: session.session_id },
            bytes: bytes.clone(),
        })
        .expect("first artifact");
    let second = coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "text/plain".to_owned(),
            access,
            owner: ArtifactOwnerRef::Session { session_id: session.session_id },
            bytes: bytes.clone(),
        })
        .expect("second artifact");
    assert_eq!(coordinator.artifact_count().expect("artifact count"), 2);
    assert_eq!(
        coordinator.artifact_object_count().expect("object count"),
        2,
        "the two physical chunks must be shared by both artifact records"
    );

    let offset = u64::try_from(ARTIFACT_CHUNK_SIZE - 32_768).expect("offset");
    let range = coordinator
        .read_artifact_range(ArtifactReadRequest {
            artifact_id: first.artifact_id,
            session_id: Some(session.session_id),
            required_access: ArtifactAccess::USER_READ,
            offset,
            length: 65_536,
            allow_quarantine: false,
        })
        .expect("range crossing a chunk boundary");
    assert_eq!(range, bytes[ARTIFACT_CHUNK_SIZE - 32_768..ARTIFACT_CHUNK_SIZE + 32_768]);

    assert!(
        coordinator
            .release_session_artifact(first.artifact_id, session.session_id)
            .expect("unlink first")
    );
    assert!(
        coordinator
            .release_session_artifact(second.artifact_id, session.session_id)
            .expect("unlink second")
    );
    let report = coordinator.run_artifact_gc(10).expect("artifact gc");
    assert_eq!(report.artifacts_removed, 2);
    assert_eq!(report.objects_removed, 2);
    assert_eq!(report.orphan_files_remaining, 0);
    assert_eq!(coordinator.artifact_count().expect("artifact count"), 0);
    assert_eq!(coordinator.artifact_object_count().expect("object count"), 0);
}

#[test]
fn artifact_reads_require_capability_and_verify_inline_integrity() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let artifact = coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::SystemCache,
            media_type: "text/plain".to_owned(),
            access: ArtifactAccess::USER_READ,
            owner: ArtifactOwnerRef::System { owner: ArtifactSystemOwner::ProviderCatalogCache },
            bytes: b"inline-integrity".to_vec(),
        })
        .expect("inline artifact");
    let mut request = ArtifactReadRequest {
        artifact_id: artifact.artifact_id,
        session_id: None,
        required_access: ArtifactAccess::NONE,
        offset: 0,
        length: 16,
        allow_quarantine: false,
    };
    assert!(matches!(
        coordinator.read_artifact_range(request),
        Err(PersistenceError::InvalidRequest(_))
    ));

    Connection::open(coordinator.database_path())
        .expect("corruption connection")
        .execute(
            "UPDATE artifacts SET inline_bytes=?1 WHERE artifact_id=?2",
            (b"tampered-content".as_slice(), artifact.artifact_id.to_string()),
        )
        .expect("corrupt inline bytes");
    request.required_access = ArtifactAccess::USER_READ;
    assert!(matches!(
        coordinator.read_artifact_range(request),
        Err(PersistenceError::ArtifactIntegrity)
    ));
}

#[test]
fn artifact_gc_rejects_persisted_object_path_escape() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace, session) = workspace_session(2);
    coordinator
        .upsert_workspace_session(workspace, session.clone(), CorrelationId::new())
        .expect("seed session");
    let artifact = coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "application/octet-stream".to_owned(),
            access: ArtifactAccess::USER_READ,
            owner: ArtifactOwnerRef::Session { session_id: session.session_id },
            bytes: vec![7; ARTIFACT_CHUNK_SIZE + 1],
        })
        .expect("chunked artifact");
    let victim = directory.path().join("data/victim.txt");
    std::fs::write(&victim, b"must-survive").expect("victim");
    Connection::open(coordinator.database_path())
        .expect("tamper connection")
        .execute(
            "UPDATE artifact_object_refs
             SET relative_path='../victim.txt'
             WHERE object_hash=(
                 SELECT object_hash FROM artifact_object_refs ORDER BY object_hash LIMIT 1
             )",
            [],
        )
        .expect("tamper object path");
    assert!(
        coordinator
            .release_session_artifact(artifact.artifact_id, session.session_id)
            .expect("queue gc")
    );
    assert!(matches!(coordinator.run_artifact_gc(10), Err(PersistenceError::ArtifactIntegrity)));
    assert_eq!(std::fs::read(victim).expect("victim survives"), b"must-survive");
    assert_eq!(coordinator.artifact_count().expect("rollback retained artifact"), 1);
}

#[test]
fn quarantine_scope_requires_explicit_access() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace, session) = workspace_session(3);
    coordinator
        .upsert_workspace_session(workspace, session.clone(), CorrelationId::new())
        .expect("seed session");
    let artifact = coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Quarantine,
            media_type: "application/octet-stream".to_owned(),
            access: ArtifactAccess::USER_READ,
            owner: ArtifactOwnerRef::Session { session_id: session.session_id },
            bytes: b"quarantined".to_vec(),
        })
        .expect("quarantined artifact");
    let mut request = ArtifactReadRequest {
        artifact_id: artifact.artifact_id,
        session_id: Some(session.session_id),
        required_access: ArtifactAccess::USER_READ,
        offset: 0,
        length: 11,
        allow_quarantine: false,
    };
    assert!(coordinator.read_artifact_range(request).is_err());
    request.allow_quarantine = true;
    assert_eq!(
        coordinator.read_artifact_range(request).expect("authorized quarantine read"),
        b"quarantined"
    );
}

#[test]
fn old_runtime_leases_expire_on_restart_and_fts5_exists() {
    let directory = tempfile::tempdir().expect("temp root");
    let first_runtime = RuntimeInstanceId::new();
    let first = start(&directory, first_runtime);
    let lease = first.acquire_runtime_lease("tool", "example").expect("runtime lease");
    drop(first);

    let second = start(&directory, RuntimeInstanceId::new());
    let connection = Connection::open(second.database_path()).expect("diagnostic reader");
    let state = connection
        .query_row("SELECT state FROM runtime_leases WHERE lease_id=?1", [&lease], |row| {
            row.get::<_, String>(0)
        })
        .expect("lease state");
    assert_eq!(state, "expired");
    let fts = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type='table' AND name='event_search_fts'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("fts schema");
    assert_eq!(fts, 1);
}
