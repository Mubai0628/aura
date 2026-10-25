//! Transport spelling differences must not turn a named watcher change into a full rescan.
use super::*;
use aura_domain_common::{CorrelationId, RuntimeInstanceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_windows_platform::{FileWatchEvent, FileWatchEventKind};
use aura_workspace_domain::WorkspaceCreateCommand;
use std::sync::Arc;

#[test]
fn a_named_watcher_change_without_a_verbatim_prefix_stays_scoped() {
    let root = tempfile::tempdir().expect("create a workspace fixture");
    std::fs::create_dir(root.path().join("alpha")).expect("create an indexed directory");
    let data = tempfile::tempdir().expect("create an isolated catalog store");
    let PersistenceStartup::Ready(persistence) =
        PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("store.db"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("open the isolated persistence owner")
    else {
        panic!("the fixture store must be ready")
    };
    let runtime = crate::WorkspaceRuntime::open(Arc::new(persistence), Default::default())
        .expect("open the workspace runtime");
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "watch scope".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create and index the real workspace");
    runtime.stop_watchers().expect("inspect an event without a concurrent watcher");
    let resolver = runtime.resolver(created.workspace.workspace_id).expect("the verified resolver");
    let catalog = runtime.catalog(created.workspace.workspace_id).expect("the real catalog");
    let canonical = resolver.canonical_root().join("alpha/once.txt");
    let text = canonical.display().to_string();
    let ordinary = std::path::PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text));
    let event =
        FileWatchEvent { kind: FileWatchEventKind::Modify, paths: vec![ordinary], detail: None };
    let WatchDescription::Described(changes) =
        changes_for_watch_events(&resolver, &catalog, &[event])
    else {
        panic!("a watcher spelling of a known workspace path must remain a bounded description")
    };
    assert_eq!(
        changes,
        vec![(
            WorkspacePath::parse("alpha/once.txt").expect("the changed relative path"),
            CatalogChange::Changed
        )]
    );
}
