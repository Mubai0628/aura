//! Adding a workspace returns before its index is built: listings read the disk meanwhile,
//! queries wait for the build a bounded time, writes' index updates wait for it, a cap leaves
//! folders unindexed and readable from disk, and a closing workspace stops the walk.
use super::*;
use crate::WorkspaceError;
use crate::catalog::build::{self, CatalogBuildPhase};
use aura_domain_common::{CorrelationId, RuntimeInstanceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace_domain::WorkspaceCreateCommand;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, Instant};

struct Fixture {
    _root: tempfile::TempDir,
    _data: tempfile::TempDir,
    runtime: Arc<crate::WorkspaceRuntime>,
    workspace_id: aura_domain_common::WorkspaceId,
    /// How long adding the workspace took.
    added_in: Duration,
}

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("a workspace path")
}

/// A workspace of `folders` folders holding `files` files each.
fn fixture(folders: usize, files: usize) -> Fixture {
    let root = tempfile::tempdir().expect("workspace folder");
    for folder in 0..folders {
        let directory = root.path().join(format!("folder-{folder:03}"));
        std::fs::create_dir(&directory).expect("folder");
        for file in 0..files {
            std::fs::write(directory.join(format!("file-{file:03}.txt")), "some text\n")
                .expect("file");
        }
    }
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
    let started = Instant::now();
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: root.path().display().to_string(),
                display_name: "background build".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("add the workspace");
    let added_in = started.elapsed();
    Fixture {
        _root: root,
        _data: data,
        runtime,
        workspace_id: created.workspace.workspace_id,
        added_in,
    }
}

impl Fixture {
    fn catalog(&self) -> Arc<PathCatalog> {
        self.runtime.catalog(self.workspace_id).expect("the catalog")
    }

    fn resolver(&self) -> Arc<crate::SecurePathResolver> {
        self.runtime.resolver(self.workspace_id).expect("the resolver")
    }
}

#[test]
fn adding_returns_at_once_and_the_index_is_built_afterwards() {
    let f = fixture(100, 60);
    let started = Instant::now();
    f.runtime.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
    let walked_in = started.elapsed();
    assert!(f.added_in < Duration::from_secs(1), "adding took {:?}", f.added_in);
    assert!(f.added_in < walked_in, "added in {:?}, walked in {walked_in:?}", f.added_in);
    let status = f.catalog().build_status().expect("status");
    assert_eq!(status.phase, CatalogBuildPhase::Ready);
    assert!(status.listed_entries >= 100 * 61, "{status:?}");
    assert!(f.catalog().entry(&path("folder-099/file-059.txt")).expect("read").is_some());
}

#[test]
fn while_building_listings_read_the_disk_and_queries_answer_how_far_it_got() {
    let f = fixture(2, 3);
    f.runtime.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
    let catalog = f.catalog();
    // A first build again, as a fresh workspace has: the index answers nothing yet.
    catalog.build.begin(true).expect("begin");
    let (root, _) = crate::children_beyond_index(&catalog, &f.resolver(), &path("."), None, 10)
        .expect("list the root")
        .expect("the root is read from disk while building");
    let names = root.iter().map(|entry| entry.path.as_str().to_owned()).collect::<Vec<_>>();
    assert_eq!(names, ["folder-000", "folder-001"]);
    let waited = catalog.build.wait(Duration::from_millis(50));
    let Err(error @ WorkspaceError::IndexBuilding { .. }) = waited else {
        panic!("a query past its wait is told the index is still being built: {waited:?}")
    };
    assert!(error.to_string().contains("索引还在建立"));
    catalog.build.finish(None).expect("finish");
    assert!(catalog.find_paths("file-002", 10).expect("find").matches.len() >= 2);
}

#[test]
fn an_index_update_asked_for_while_walking_is_applied_after_the_walk() {
    let f = fixture(1, 1);
    f.runtime.stop_watchers().expect("drive the catalog without its watcher");
    let catalog = f.catalog();
    std::fs::write(f.resolver().canonical_root().join("folder-000/new.txt"), "new").expect("write");
    catalog.build.begin(false).expect("a walk starts");
    let update =
        CatalogUpdate::new(vec![(path("folder-000/new.txt"), CatalogChange::Changed)], "test");
    catalog
        .apply_changes(&f.resolver(), update, &catalog.options(), CorrelationId::new())
        .expect("kept for after the walk");
    assert!(catalog.entry(&path("folder-000/new.txt")).expect("read").is_none());
    catalog.build.finish(None).expect("the walk ends");
    let kept = catalog.build.take_deferred().expect("kept updates");
    assert_eq!(kept.len(), 1);
    catalog
        .apply_changes(
            &f.resolver(),
            CatalogUpdate::new(kept, "after the walk"),
            &catalog.options(),
            CorrelationId::new(),
        )
        .expect("applied");
    assert!(catalog.entry(&path("folder-000/new.txt")).expect("read").is_some());
}

#[test]
fn past_the_cap_a_folder_is_recorded_unindexed_and_read_from_disk() {
    let f = fixture(4, 5);
    f.runtime.stop_watchers().expect("drive the catalog without its watcher");
    let catalog = f.catalog();
    let progress = catalog.build.progress();
    progress.cap_at(4);
    catalog.build.begin(false).expect("a walk starts");
    let walked = build::with_walk(
        progress,
        Arc::new(AtomicBool::new(false)),
        || {},
        || catalog.rebuild(&f.resolver(), CorrelationId::new()),
    );
    catalog.build.finish(walked.err().map(|error| error.to_string())).expect("finish");
    let status = catalog.build_status().expect("status");
    assert_eq!(status.phase, CatalogBuildPhase::Incomplete);
    assert_eq!(status.unindexed_directories, 4, "{status:?}");
    assert!(catalog.entry(&path("folder-002")).expect("read").is_some());
    assert!(catalog.entry(&path("folder-002/file-000.txt")).expect("read").is_none());
    let (listed, _) =
        crate::children_beyond_index(&catalog, &f.resolver(), &path("folder-002"), None, 10)
            .expect("list")
            .expect("an unindexed folder is read from disk");
    assert_eq!(listed.len(), 5);
    // A lookup names the folders it could not look into.
    let root = WorkspacePath::parse(WorkspacePath::ROOT).expect("root");
    assert_eq!(catalog.unindexed_within(&root, true).expect("named").len(), 4);
    assert_eq!(
        catalog.unindexed_within(&path("folder-002"), true).expect("named"),
        vec![path("folder-002")],
        "a lookup inside an unindexed folder names that folder"
    );
}

#[test]
fn a_closing_workspace_stops_the_walk_and_a_first_build_reports_the_failure() {
    let f = fixture(2, 2);
    f.runtime.stop_watchers().expect("drive the catalog without its watcher");
    let catalog = f.catalog();
    catalog.build.begin(true).expect("a first build starts");
    let walked = build::with_walk(
        catalog.build.progress(),
        Arc::new(AtomicBool::new(true)),
        || {},
        || catalog.rebuild(&f.resolver(), CorrelationId::new()),
    );
    assert!(matches!(walked, Err(WorkspaceError::Busy { .. })), "{walked:?}");
    catalog.build.finish(walked.err().map(|error| error.to_string())).expect("finish");
    assert!(matches!(
        catalog.build_status().expect("status").phase,
        CatalogBuildPhase::Failed { .. }
    ));
}

#[test]
fn a_requested_walk_runs_on_the_watcher_thread_and_a_second_request_starts_nothing() {
    let f = fixture(10, 10);
    f.runtime.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
    assert!(f.runtime.rebuild_index(f.workspace_id).expect("asked"), "a walk starts");
    assert_eq!(f.catalog().build_status().expect("status").phase, CatalogBuildPhase::Building);
    assert!(!f.runtime.rebuild_index(f.workspace_id).expect("asked again"), "one walk at a time");
    f.runtime.wait_until_indexed(Duration::from_secs(60)).expect("the requested walk");
    assert_eq!(f.catalog().build_status().expect("status").phase, CatalogBuildPhase::Ready);
    assert!(f.catalog().entry(&path("folder-009/file-009.txt")).expect("read").is_some());
}
