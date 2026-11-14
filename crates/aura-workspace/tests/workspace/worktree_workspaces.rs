//! A worktree is registered as a workspace of its own, listed under the workspace it was made
//! from. Its resolver opens only the worktree: a path is judged by its final path after links,
//! through the handle that was opened, and against the identity the worktree's folder had.
use crate::runtime_support;

use std::{sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, WorkspacePath};
use aura_workspace::{CatalogOptions, WorkspaceError, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{WorkspaceCreateCommand, WorkspaceWorktreeLink};
use runtime_support::persistence;

fn runtime(persistence: &Arc<aura_persistence::PersistenceCoordinator>) -> Arc<WorkspaceRuntime> {
    WorkspaceRuntime::open(
        Arc::clone(persistence),
        WorkspaceRuntimeOptions {
            catalog: CatalogOptions::with_index_file_limit(1024 * 1024),
            watcher_coalesce: Duration::from_millis(20),
            watcher_poll_interval: Duration::from_secs(1),
            watcher_rescan_attempts: 1,
            ..WorkspaceRuntimeOptions::default()
        },
    )
    .expect("workspace runtime")
}

/// A project and a worktree folder beside it: a `.git` file names the project's Git directory, as
/// `git worktree add` writes it.
fn folders() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let base = tempfile::tempdir().expect("folders");
    let project = base.path().join("project");
    let worktree = base.path().join("worktrees").join("0a1b2c3d");
    std::fs::create_dir_all(project.join(".git/worktrees/0a1b2c3d")).expect("project");
    std::fs::write(project.join("secret.txt"), "the project's own file").expect("project file");
    std::fs::create_dir_all(&worktree).expect("worktree");
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", project.join(".git/worktrees/0a1b2c3d").display()),
    )
    .expect("worktree link");
    std::fs::write(worktree.join("own.txt"), "the worktree's file").expect("worktree file");
    (base, project, worktree)
}

fn path(text: &str) -> WorkspacePath {
    WorkspacePath::parse(text).expect("workspace path")
}

#[test]
fn a_worktree_is_a_workspace_listed_under_the_one_it_was_made_from() {
    let data = tempfile::tempdir().expect("data");
    let (_base, project, worktree) = folders();
    let persistence = persistence(&data);
    let runtime = runtime(&persistence);
    let parent = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: project.display().to_string(),
                display_name: "project".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("project")
        .workspace;
    let own = runtime
        .create_worktree(
            WorkspaceCreateCommand {
                selected_root: worktree.display().to_string(),
                display_name: "project · A1".to_owned(),
            },
            parent.workspace_id,
            CorrelationId::new(),
        )
        .expect("worktree workspace");
    assert_ne!(own.workspace_id, parent.workspace_id);
    let projection = persistence.workspace_session_projection().expect("projection");
    assert_eq!(
        projection.worktrees,
        vec![WorkspaceWorktreeLink {
            workspace_id: own.workspace_id,
            parent_workspace_id: parent.workspace_id,
        }]
    );
    // The same folder again answers with the same workspace.
    let again = runtime
        .create_worktree(
            WorkspaceCreateCommand {
                selected_root: worktree.display().to_string(),
                display_name: "project · A1".to_owned(),
            },
            parent.workspace_id,
            CorrelationId::new(),
        )
        .expect("again");
    assert_eq!(again.workspace_id, own.workspace_id);
    let resolver = runtime.resolver(own.workspace_id).expect("resolver");
    assert_eq!(
        std::fs::canonicalize(resolver.canonical_root()).expect("root"),
        std::fs::canonicalize(&worktree).expect("worktree")
    );
}

#[test]
fn a_worktrees_paths_never_reach_the_project_or_a_replaced_folder() {
    let data = tempfile::tempdir().expect("data");
    let (_base, project, worktree) = folders();
    // A link inside the worktree that points back into the project.
    junction::create(&project, worktree.join("escape")).expect("junction");
    let persistence = persistence(&data);
    let runtime = runtime(&persistence);
    let parent = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: project.display().to_string(),
                display_name: "project".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("project")
        .workspace;
    let own = runtime
        .create_worktree(
            WorkspaceCreateCommand {
                selected_root: worktree.display().to_string(),
                display_name: "project · A1".to_owned(),
            },
            parent.workspace_id,
            CorrelationId::new(),
        )
        .expect("worktree workspace");
    let resolver = runtime.resolver(own.workspace_id).expect("resolver");
    let inside = resolver.resolve_existing(&path("own.txt")).expect("its own file");
    assert!(
        inside.absolute_path.starts_with(resolver.canonical_root()),
        "{}",
        inside.absolute_path.display()
    );
    // The final path decides: the project's file through the link is outside the worktree.
    assert!(matches!(
        resolver.resolve_existing(&path("escape/secret.txt")),
        Err(WorkspaceError::ScopeUnavailable { .. })
    ));
    assert!(matches!(
        resolver.validate_parent(&path("escape/new.txt")),
        Err(WorkspaceError::ScopeUnavailable { .. })
    ));
    // A folder put at the worktree's path after it was registered is not the worktree.
    std::fs::rename(&worktree, worktree.with_extension("moved")).expect("move the worktree away");
    std::fs::create_dir_all(&worktree).expect("another folder at the same path");
    std::fs::write(worktree.join("own.txt"), "not the worktree").expect("decoy");
    assert!(matches!(
        resolver.resolve_existing(&path("own.txt")),
        Err(WorkspaceError::RootIdentityChanged)
    ));
}
