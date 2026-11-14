//! A worktree's workspace is listed under the workspace it was made from. The link is named from
//! the first publication that lists the worktree, so the sidebar never shows it on its own, and the
//! projection outbox keeps the shape earlier builds read.
use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, WorkspaceId};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace_domain::{
    CatalogFreshness, WorkspaceState, WorkspaceSummary, WorkspaceWorktreeLink,
};
use std::time::Duration;

fn start(data: &std::path::Path) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        data.join("aura.sqlite3"),
        data.join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

fn workspace(root: &std::path::Path, name: &str) -> WorkspaceSummary {
    WorkspaceSummary {
        workspace_id: WorkspaceId::new(),
        display_name: name.to_owned(),
        selected_root: root.display().to_string(),
        canonical_root: root.display().to_string(),
        root_identity: None,
        state: WorkspaceState::Online,
        revision: Revision::INITIAL,
        catalog_freshness: CatalogFreshness::Unavailable,
        authorization_epoch: Revision::INITIAL,
    }
}

#[test]
fn a_worktree_is_listed_under_its_workspace_from_its_first_publication() {
    let data = tempfile::tempdir().expect("data");
    let roots = tempfile::tempdir().expect("roots");
    let persistence = start(data.path());
    let project = workspace(&roots.path().join("project"), "project");
    persistence.create_workspace(project.clone(), CorrelationId::new()).expect("project");
    let published = persistence.subscribe_workspace_sessions().expect("subscribe");
    let worktree = workspace(&roots.path().join("worktree"), "project · A1");
    persistence.link_worktree(worktree.workspace_id, project.workspace_id).expect("link");
    persistence.create_workspace(worktree.clone(), CorrelationId::new()).expect("worktree");
    // Read without the store: the Skills a sub-agent in the worktree has are the project's.
    assert_eq!(persistence.worktree_parent(worktree.workspace_id), Some(project.workspace_id));
    assert_eq!(persistence.worktree_parent(project.workspace_id), None);
    let link = WorkspaceWorktreeLink {
        workspace_id: worktree.workspace_id,
        parent_workspace_id: project.workspace_id,
    };
    let first = loop {
        let change = published.recv_timeout(Duration::from_secs(5)).expect("a publication");
        if change.payload.workspaces.iter().any(|item| item.workspace_id == worktree.workspace_id) {
            break change;
        }
    };
    assert_eq!(first.payload.worktrees, vec![link.clone()]);
    assert_eq!(persistence.workspace_session_projection().expect("read").worktrees, vec![link]);
    // The outbox keeps what the store itself holds.
    let outbox: String = rusqlite::Connection::open(data.path().join("aura.sqlite3"))
        .expect("database")
        .query_row(
            "SELECT payload_json FROM projection_outbox WHERE projection_kind='workspace_session' \
             ORDER BY outbox_id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("outbox row");
    assert!(outbox.contains(&worktree.workspace_id.to_string()), "{outbox}");
    assert!(!outbox.contains("worktrees"), "{outbox}");
}

#[test]
fn a_projection_without_worktrees_reads_as_before() {
    let data = tempfile::tempdir().expect("data");
    let roots = tempfile::tempdir().expect("roots");
    let persistence = start(data.path());
    persistence
        .create_workspace(workspace(&roots.path().join("project"), "project"), CorrelationId::new())
        .expect("project");
    let projection = persistence.workspace_session_projection().expect("read");
    assert!(projection.worktrees.is_empty());
    let text = serde_json::to_string(&projection).expect("json");
    assert!(!text.contains("worktrees"), "{text}");
}
