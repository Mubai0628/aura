//! Git's own folder is indexed, but a name lookup of the workspace does not answer with its
//! files: a broad pattern such as `*` would otherwise list Git's internals among the user's
//! files. A lookup inside `.git` still finds them.
use super::*;
use aura_domain_common::{CorrelationId, RuntimeInstanceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace_domain::WorkspaceCreateCommand;
use std::sync::Arc;

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("a workspace path")
}

/// A workspace with a source file, a README and a `.git` folder, its index built.
fn catalog() -> (tempfile::TempDir, tempfile::TempDir, Arc<PathCatalog>) {
    let root = tempfile::tempdir().expect("workspace folder");
    std::fs::create_dir_all(root.path().join(".git/hooks")).expect("git folder");
    std::fs::write(root.path().join(".git/config"), "[core]").expect("git config");
    std::fs::write(root.path().join(".git/HEAD"), "ref: refs/heads/main").expect("git head");
    std::fs::write(root.path().join(".git/hooks/pre-commit.sample"), "#!/bin/sh").expect("hook");
    std::fs::create_dir(root.path().join("src")).expect("source folder");
    std::fs::write(root.path().join("src/main.rs"), "fn main() {}").expect("source file");
    std::fs::write(root.path().join("README.md"), "# readme").expect("readme");
    let data = tempfile::tempdir().expect("store folder");
    let PersistenceStartup::Ready(persistence) =
        PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("store.db"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("open the store")
    else {
        panic!("the fixture store must be ready")
    };
    let runtime = crate::WorkspaceRuntime::open(Arc::new(persistence), Default::default())
        .expect("open the workspace runtime");
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "git metadata".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create the workspace");
    runtime.stop_watchers().expect("drive the catalog without a concurrent watcher");
    let catalog = runtime.catalog(created.workspace.workspace_id).expect("the catalog");
    (root, data, catalog)
}

fn found(page: &scan::NameFindPage) -> Vec<&str> {
    page.matches.iter().map(|entry| entry.path.as_str()).collect()
}

#[test]
fn a_name_lookup_of_the_workspace_leaves_out_git_metadata() {
    let (_root, _data, catalog) = catalog();
    let every = ["*".to_owned()];
    let page = catalog
        .find_names(
            &path(WorkspacePath::ROOT),
            &every,
            true,
            NameMatchMode::Glob,
            false,
            None,
            100,
            16_384,
        )
        .expect("lookup");
    assert_eq!(found(&page), ["README.md", "src/main.rs"]);
    assert!(page.complete);
    // A name only Git's folder holds is not found from the root either.
    let config = ["config".to_owned()];
    let page = catalog
        .find_names(
            &path(WorkspacePath::ROOT),
            &config,
            true,
            NameMatchMode::Exact,
            false,
            None,
            10,
            16_384,
        )
        .expect("lookup");
    assert!(found(&page).is_empty(), "{:?}", found(&page));
}

#[test]
fn a_name_lookup_inside_git_finds_its_files() {
    let (_root, _data, catalog) = catalog();
    let config = ["config".to_owned()];
    let page = catalog
        .find_names(&path(".git"), &config, false, NameMatchMode::Exact, false, None, 10, 16_384)
        .expect("lookup");
    assert_eq!(found(&page), [".git/config"]);
    let samples = ["*.sample".to_owned()];
    let page = catalog
        .find_names(&path(".git"), &samples, true, NameMatchMode::Glob, false, None, 10, 16_384)
        .expect("lookup");
    assert_eq!(found(&page), [".git/hooks/pre-commit.sample"]);
}
