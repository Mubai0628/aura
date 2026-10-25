use aura_domain_common::{
    CommandInvocationId, CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId,
};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_security_contract::PermissionMode;
use aura_workspace_domain::{
    CatalogFreshness, SessionCreateRequest, SessionRenameRequest, SessionSummary, WorkspaceState,
    WorkspaceSummary,
};

#[test]
fn unnamed_session_and_cas_title_survive_restart_without_changing_message_history() {
    let directory = tempfile::tempdir().expect("temporary database");
    let open = || match PersistenceCoordinator::start(PersistenceOptions::new(
        directory.path().join("aura.db"),
        directory.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("open")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(_) => panic!("unexpected recovery"),
    };
    let persistence = open();
    let workspace_id = WorkspaceId::new();
    persistence
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: "synthetic".into(),
                selected_root: "C:/synthetic".into(),
                canonical_root: "C:/synthetic".into(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                authorization_epoch: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
            },
            SessionSummary {
                session_id: SessionId::new(),
                workspace_id,
                title: "existing".into(),
                state: "idle".into(),
            },
            CorrelationId::new(),
        )
        .expect("workspace");
    let command = CommandInvocationId::new();
    let request = SessionCreateRequest { workspace_id, title: String::new() };
    let session = persistence
        .create_session(command, request.clone(), PermissionMode::ReadOnly, CorrelationId::new())
        .expect("unnamed session");
    assert!(session.title.is_empty());
    assert_eq!(
        persistence
            .create_session(command, request, PermissionMode::ReadOnly, CorrelationId::new())
            .expect("idempotent create"),
        session
    );
    let rename = SessionRenameRequest {
        session_id: session.session_id,
        expected_title: String::new(),
        title: "请帮我完成".into(),
    };
    let command = CommandInvocationId::new();
    let renamed = persistence
        .rename_session(command, rename.clone(), CorrelationId::new())
        .expect("first accepted title");
    assert_eq!(
        persistence
            .rename_session(command, rename.clone(), CorrelationId::new())
            .expect("idempotent title"),
        renamed
    );
    persistence
        .rename_session(
            CommandInvocationId::new(),
            SessionRenameRequest {
                expected_title: renamed.title,
                title: "我的名称".into(),
                ..rename.clone()
            },
            CorrelationId::new(),
        )
        .expect("manual rename");
    assert!(
        persistence
            .rename_session(CommandInvocationId::new(), rename, CorrelationId::new())
            .is_err()
    );
    drop(persistence);
    let reopened = open();
    let saved = reopened
        .workspace_session_projection()
        .expect("reopen")
        .sessions
        .into_iter()
        .find(|value| value.session_id == session.session_id)
        .expect("session retained");
    assert_eq!(saved.title, "我的名称");
}
