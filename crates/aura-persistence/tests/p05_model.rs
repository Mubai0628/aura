use aura_domain_common::{
    CorrelationId, ExpectedRevision, ModelProfileId, Revision, RuntimeInstanceId, SessionId,
    UtcTimestamp, WorkspaceId,
};
use aura_model_domain::{ModelExecutionReadinessView, SessionModelSelectorProjection};
use aura_persistence::{
    CommitSessionModelBinding, PersistenceCoordinator, PersistenceError, PersistenceOptions,
    PersistenceStartup,
};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};

#[test]
fn session_model_binding_uses_cas_survives_restart_and_keeps_deleted_profile_identity() {
    let directory = tempfile::tempdir().expect("temp root");
    let database_path = directory.path().join("data/aura.db");
    let artifact_root = directory.path().join("data/artifacts");
    let persistence = start(database_path.clone(), artifact_root.clone());
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: "P05 Workspace".to_owned(),
                selected_root: r"C:\Aura Tests\P05".to_owned(),
                canonical_root: r"C:\Aura Tests\P05".to_owned(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id,
                workspace_id,
                title: "P05 Session".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("seed session");
    let first_profile = ModelProfileId::new();
    let mut old_selector = empty_selector(session_id);
    old_selector.binding_revision = Revision::INITIAL;
    persistence
        .commit_model_projection_snapshots(vec![
            aura_persistence::ModelProjectionDraft::SessionModelSelector(old_selector),
        ])
        .expect("legacy incorrect unbound selector");
    let initialized = persistence
        .ensure_session_model_selector(empty_selector(session_id))
        .expect("repair selector without a configuration edit");
    assert!(
        matches!(initialized, aura_projection_contract::ProjectionChange::SessionModelSelector { payload, .. }
        if payload.binding_revision == Revision::new(0))
    );
    let first = persistence
        .commit_session_model_binding(CommitSessionModelBinding {
            session_id,
            expected_revision: ExpectedRevision::new(0),
            model_profile_id: Some(first_profile),
            readable_ref_at_binding: Some("provider/endpoint/model-a".to_owned()),
            bound_at: UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
            atomic_group_id: None,
            selector_projection: empty_selector(session_id),
        })
        .expect("initial binding");
    assert_eq!(first.revision, Revision::new(1));
    assert_eq!(first.binding.as_ref().expect("binding").binding_revision, Revision::INITIAL);
    assert_eq!(first.projection_changes.len(), 1);
    let raced = persistence
        .ensure_session_model_selector(empty_selector(session_id))
        .expect("a late initializer cannot overwrite a committed binding");
    assert!(
        matches!(raced, aura_projection_contract::ProjectionChange::SessionModelSelector { payload, .. }
        if payload.binding_revision == Revision::INITIAL && payload.selected_model_profile_id == Some(first_profile))
    );
    let second_profile = ModelProfileId::new();
    let second = persistence
        .commit_session_model_binding(CommitSessionModelBinding {
            session_id,
            expected_revision: ExpectedRevision::new(1),
            model_profile_id: Some(second_profile),
            readable_ref_at_binding: Some("provider/endpoint/model-b".to_owned()),
            bound_at: UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
            atomic_group_id: None,
            selector_projection: empty_selector(session_id),
        })
        .expect("explicit model switch");
    assert_eq!(second.revision, Revision::new(2));
    assert!(matches!(
        persistence.commit_session_model_binding(CommitSessionModelBinding {
            session_id,
            expected_revision: ExpectedRevision::new(1),
            model_profile_id: Some(first_profile),
            readable_ref_at_binding: Some("provider/endpoint/model-a".to_owned()),
            bound_at: UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
            atomic_group_id: None,
            selector_projection: empty_selector(session_id),
        }),
        Err(PersistenceError::SessionModelBindingRevisionConflict)
    ));
    let cleared = persistence
        .commit_session_model_binding(CommitSessionModelBinding {
            session_id,
            expected_revision: ExpectedRevision::new(2),
            model_profile_id: None,
            readable_ref_at_binding: None,
            bound_at: UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
            atomic_group_id: None,
            selector_projection: empty_selector(session_id),
        })
        .expect("clear binding");
    assert_eq!(cleared.revision, Revision::new(3));
    assert_eq!(cleared.binding, None);
    drop(persistence);

    let reopened = start(database_path, artifact_root);
    let restored = reopened.session_model_binding_state(session_id).expect("load binding state");
    assert_eq!(restored.revision, Revision::new(3));
    assert_eq!(restored.binding, None);
}

fn empty_selector(session_id: SessionId) -> SessionModelSelectorProjection {
    SessionModelSelectorProjection {
        session_id,
        binding_revision: Revision::new(0),
        selected_model_profile_id: None,
        default_model_profile_id: None,
        providers: Vec::new(),
        endpoints: Vec::new(),
        models: Vec::new(),
        selected_model_execution_readiness: ModelExecutionReadinessView {
            is_executable: false,
            issue_code: None,
        },
        projection_revision: Revision::INITIAL,
    }
}

fn start(
    database_path: std::path::PathBuf,
    artifact_root: std::path::PathBuf,
) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        database_path,
        artifact_root,
        RuntimeInstanceId::new(),
    ))
    .expect("start persistence")
    {
        PersistenceStartup::Ready(coordinator) => coordinator,
        PersistenceStartup::RecoveryRequired(recovery) => {
            panic!("unexpected recovery requirement: {}", recovery.reason)
        }
    }
}
