use aura_artifact_contract::{ArtifactAccess, ArtifactOwnerRef, ArtifactScope};
use aura_domain_common::{
    CorrelationId, MutationJournalId, RendererInstanceId, Revision, RuntimeInstanceId, SessionId,
    WorkspaceId, WorkspaceLifecycleOperationId,
};
use aura_persistence::{
    ArtifactWriteRequest, PersistenceCoordinator, PersistenceOptions, PersistenceStartup,
    WorkspaceLifecycleRecord,
};
use aura_workspace_domain::{
    CatalogFreshness, SessionSummary, WorkspaceLifecycleOperation, WorkspaceLifecycleOperationKind,
    WorkspaceLifecycleState, WorkspaceLifecycleSummary, WorkspaceState, WorkspaceSummary,
};

#[test]
fn permanent_delete_removes_only_aura_state_and_queues_artifact_objects_for_gc() {
    let user_root = tempfile::tempdir().expect("user workspace");
    for path in [
        "AGENTS.md",
        "docs/design.md",
        ".memories/rules.md",
        ".agents/skills/example/SKILL.md",
        ".aura/commands/build.md",
        ".git/config",
        "src/main.rs",
    ] {
        let absolute = user_root.path().join(path);
        std::fs::create_dir_all(absolute.parent().expect("parent")).expect("directory");
        std::fs::write(&absolute, path).expect("user file");
    }
    let data = tempfile::tempdir().expect("data root");
    let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
        data.path().join("aura.sqlite3"),
        data.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    };
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let mut workspace = WorkspaceSummary {
        workspace_id,
        display_name: "Delete test".to_owned(),
        selected_root: user_root.path().display().to_string(),
        canonical_root: user_root.path().display().to_string(),
        root_identity: None,
        state: WorkspaceState::Online,
        revision: Revision::INITIAL,
        catalog_freshness: CatalogFreshness::Unavailable,
        authorization_epoch: Revision::INITIAL,
    };
    persistence.create_workspace(workspace.clone(), CorrelationId::new()).expect("workspace");
    persistence
        .upsert_workspace_session(
            workspace.clone(),
            SessionSummary {
                session_id,
                workspace_id,
                title: "Delete test".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("session");
    let artifact = persistence
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "application/octet-stream".to_owned(),
            access: ArtifactAccess::USER_READ,
            owner: ArtifactOwnerRef::Session { session_id },
            bytes: vec![0x5a; 20 * 1024],
        })
        .expect("artifact");
    assert!(!artifact.inline);
    assert_eq!(persistence.artifact_object_count().expect("objects"), 1);
    let projection_updates = persistence.subscribe_workspace_sessions().expect("subscription");

    workspace.state = WorkspaceState::Archived;
    workspace.revision = Revision::new(2);
    workspace.authorization_epoch = Revision::new(2);
    persistence
        .update_workspace(workspace, "workspace.archived", CorrelationId::new(), false)
        .expect("archive state");
    let operation_id = WorkspaceLifecycleOperationId::new();
    let committing = WorkspaceLifecycleRecord {
        summary: WorkspaceLifecycleSummary {
            approval_follow_up: Vec::new(),
            operation_id,
            workspace_id,
            renderer_instance_id: RendererInstanceId::new(),
            operation: WorkspaceLifecycleOperationKind::PermanentDelete,
            lifecycle_revision: Revision::INITIAL,
            state: WorkspaceLifecycleState::Committing,
            ack_deadline_at: None,
            preflight: None,
            error_code: None,
        },
        operation: WorkspaceLifecycleOperation::PermanentDelete {
            delete_confirmation_id: aura_domain_common::DeleteConfirmationId::new(),
        },
    };
    persistence
        .create_workspace_lifecycle_operation(committing.clone())
        .expect("lifecycle journal");
    let mut completed = committing;
    completed.summary.lifecycle_revision = Revision::new(2);
    completed.summary.state = WorkspaceLifecycleState::Completed;
    let report = persistence
        .permanently_delete_workspace(workspace_id, completed)
        .expect("permanent delete");
    assert_eq!(report.ended_sessions, vec![session_id]);
    assert_eq!(report.artifacts_queued_for_gc, 1);
    assert!(persistence.workspace_session_projection().expect("projection").workspaces.is_empty());
    let latest = projection_updates.try_iter().last().expect("delete projection update");
    assert!(latest.payload.workspaces.is_empty());
    assert!(latest.payload.sessions.is_empty());
    assert_eq!(persistence.event_count().expect("events"), 0);
    assert_eq!(persistence.event_search_entry_count().expect("event search"), 0);
    assert_eq!(
        persistence
            .cleanup_notification_count("session.ended", &session_id.to_string())
            .expect("cleanup notification"),
        1
    );
    assert_eq!(persistence.artifact_count().expect("artifact count"), 1);

    for path in [
        "AGENTS.md",
        "docs/design.md",
        ".memories/rules.md",
        ".agents/skills/example/SKILL.md",
        ".aura/commands/build.md",
        ".git/config",
        "src/main.rs",
    ] {
        assert!(user_root.path().join(path).is_file(), "user file was removed: {path}");
    }

    let gc = persistence.run_artifact_gc(10).expect("artifact gc");
    assert_eq!(gc.artifacts_removed, 1);
    assert_eq!(gc.objects_removed, 1);
    assert_eq!(persistence.artifact_count().expect("artifact count"), 0);
    assert_eq!(persistence.artifact_object_count().expect("object count"), 0);
}

#[test]
fn a_new_runtime_marks_only_ended_runtime_journals_for_reconciliation() {
    let root = tempfile::tempdir().expect("workspace root");
    let current_root = tempfile::tempdir().expect("current workspace root");
    let data = tempfile::tempdir().expect("data root");
    let database = data.path().join("aura.sqlite3");
    let artifacts = data.path().join("artifacts");
    let first_runtime = RuntimeInstanceId::new();
    let first = match PersistenceCoordinator::start(PersistenceOptions::new(
        database.clone(),
        artifacts.clone(),
        first_runtime,
    ))
    .expect("first startup")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    };
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let workspace = WorkspaceSummary {
        workspace_id,
        display_name: "Interrupted".to_owned(),
        selected_root: root.path().display().to_string(),
        canonical_root: root.path().display().to_string(),
        root_identity: None,
        state: WorkspaceState::Online,
        revision: Revision::INITIAL,
        catalog_freshness: CatalogFreshness::Fresh,
        authorization_epoch: Revision::INITIAL,
    };
    first
        .upsert_workspace_session(
            workspace,
            SessionSummary {
                session_id,
                workspace_id,
                title: "Interrupted".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("workspace session");
    first.acquire_mutation_ownership(workspace_id, session_id).expect("old ownership");
    first
        .begin_workspace_journal(
            MutationJournalId::new(),
            workspace_id,
            session_id,
            "fs.apply_changeset",
            serde_json::json!({ "applied_steps": 1 }),
        )
        .expect("interrupted journal");
    drop(first);

    let second_runtime = RuntimeInstanceId::new();
    let second = match PersistenceCoordinator::start(PersistenceOptions::new(
        database,
        artifacts,
        second_runtime,
    ))
    .expect("second startup")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    };
    let affected = second
        .recover_interrupted_workspace_mutations(CorrelationId::new())
        .expect("targeted recovery");
    assert_eq!(affected, vec![workspace_id]);
    let recovered = second.workspace(workspace_id).expect("recovered workspace");
    assert_eq!(recovered.state, WorkspaceState::ReconciliationRequired);
    assert_eq!(recovered.catalog_freshness, CatalogFreshness::Stale);
    assert_eq!(recovered.revision, Revision::new(2));
    assert_eq!(recovered.authorization_epoch, Revision::new(2));
    let blockers = second.workspace_blockers(workspace_id).expect("blockers");
    assert_eq!(blockers.mutation_ownership, 0);
    assert_eq!(blockers.unfinished_journals, 1);
    assert!(
        second
            .recover_interrupted_workspace_mutations(CorrelationId::new())
            .expect("idempotent recovery")
            .is_empty()
    );
    assert_eq!(
        second.workspace(workspace_id).expect("stable workspace").revision,
        Revision::new(2)
    );

    let current_workspace_id = WorkspaceId::new();
    let current_session_id = SessionId::new();
    second
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id: current_workspace_id,
                display_name: "Current".to_owned(),
                selected_root: current_root.path().display().to_string(),
                canonical_root: current_root.path().display().to_string(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Fresh,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id: current_session_id,
                workspace_id: current_workspace_id,
                title: "Current".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("current workspace session");
    second
        .acquire_mutation_ownership(current_workspace_id, current_session_id)
        .expect("current ownership");
    assert!(
        second
            .recover_interrupted_workspace_mutations(CorrelationId::new())
            .expect("safe repeated recovery")
            .is_empty()
    );
    assert_eq!(
        second
            .workspace_blockers(current_workspace_id)
            .expect("current blockers")
            .mutation_ownership,
        1
    );
}
