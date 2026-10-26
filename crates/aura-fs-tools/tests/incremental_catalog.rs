//! The write path must update the catalog incrementally, through the real
//! `FsToolService` mutation entry point, and must stay consistent with a full
//! rebuild while doing it.
//!
//! The failure this guards against is a write that re-indexes the whole
//! workspace: it makes a single file edit cost a full rescan, and it advances the
//! catalog revision for every directory at once, which invalidates pagination in
//! directories the write never touched.
use std::{path::Path, sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, PathCatalog, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsDeleteRequest, FsExpectedState, FsWriteFileRequest, LineEnding, SessionSummary, TextEncoding,
    WorkspaceCreateCommand, WorkspaceFileTarget,
};

const CATALOG_UPDATED: &str = "workspace.catalog.updated";
const CATALOG_REBUILT: &str = "workspace.catalog.rebuilt";

struct Harness {
    workspace: Arc<WorkspaceRuntime>,
    persistence: Arc<PersistenceCoordinator>,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session_id: SessionId,
}

impl Harness {
    fn new(setup: impl FnOnce(&Path)) -> Self {
        Self::with_partition_limit(setup, CatalogOptions::DEFAULT_MAX_PARTITION_FILES)
    }

    /// The same workspace with a small partition limit, so a test can cross it
    /// without creating twenty thousand files.
    fn with_partition_limit(setup: impl FnOnce(&Path), max_partition_files: u64) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        setup(root.path());
        let data = tempfile::tempdir().expect("data root");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("aura.sqlite3"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected recovery: {}", value.reason)
            }
        };
        let workspace = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions {
                    max_partition_files,
                    ..CatalogOptions::with_index_file_limit(64 * 1024 * 1024)
                },
                watcher_coalesce: Duration::from_millis(20),
                watcher_poll_interval: Duration::from_secs(1),
                watcher_rescan_attempts: 1,
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let created = workspace
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "incremental catalog".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        // The first walk runs after the create returns; the cases start from a complete index.
        workspace.wait_until_indexed(Duration::from_secs(60)).expect("the first walk");
        let workspace_id = created.workspace.workspace_id;
        let session_id = SessionId::new();
        let current = persistence.workspace(workspace_id).expect("current workspace");
        persistence
            .upsert_workspace_session(
                current,
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: "incremental session".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create session");
        Self { workspace, persistence, root, _data: data, workspace_id, session_id }
    }

    fn service(&self) -> Arc<FsToolService> {
        FsToolService::new(
            Arc::clone(&self.workspace),
            Arc::clone(&self.persistence),
            FsToolPolicy::default(),
        )
        .expect("fs service")
    }

    fn write(&self, service: &FsToolService, path: &str, text: &str, expected: FsExpectedState) {
        // A new file is created as UTF-8 with LF; an overwrite names neither, so
        // the mutation layer keeps the file's own encoding and line endings. This
        // is the same choice the model-facing `fs.write` tool makes.
        let creating = expected == FsExpectedState::Absent;
        service
            .write_file(
                FsWriteFileRequest {
                    workspace_id: self.workspace_id,
                    session_id: self.session_id,
                    target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                    content: text.to_owned(),
                    expected,
                    encoding: creating.then_some(TextEncoding::Utf8),
                    line_ending: creating.then_some(LineEnding::Lf),
                },
                CorrelationId::new(),
            )
            .expect("write file");
    }

    fn catalog(&self) -> Arc<PathCatalog> {
        self.workspace.catalog(self.workspace_id).expect("catalog")
    }

    fn event_count(&self, event_type: &str) -> u64 {
        self.persistence
            .workspace_catalog_event_count(self.workspace_id, event_type)
            .expect("catalog event count")
    }

    /// The observable index facts for one directory.
    fn listing(&self, directory: &str) -> Vec<(String, Option<u64>)> {
        let path = WorkspacePath::parse(directory).expect("directory");
        let mut listing = self
            .catalog()
            .entries()
            .expect("entries")
            .into_iter()
            .filter(|entry| entry.parent_path == path)
            .map(|entry| (entry.path.as_str().to_owned(), entry.byte_length))
            .collect::<Vec<_>>();
        listing.sort();
        listing
    }

    /// Every index fact, so an incremental result can be compared with a rebuild.
    fn facts(&self) -> Vec<(String, String, Option<u64>)> {
        let mut facts = self
            .catalog()
            .entries()
            .expect("entries")
            .into_iter()
            .map(|entry| {
                (entry.path.as_str().to_owned(), format!("{:?}", entry.kind), entry.byte_length)
            })
            .collect::<Vec<_>>();
        facts.sort();
        facts
    }

    /// The page version of one directory, which is derived from its own contents.
    fn directory_version(&self, directory: &str) -> u64 {
        let path = WorkspacePath::parse(directory).expect("directory");
        self.catalog().children_page(&path, None, 100).expect("children page").2.value()
    }

    fn delete_recursive(&self, service: &FsToolService, path: &str) {
        service
            .delete(
                FsDeleteRequest {
                    workspace_id: self.workspace_id,
                    session_id: self.session_id,
                    target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                    expected: FsExpectedState::Directory,
                    recursive: true,
                },
                CorrelationId::new(),
            )
            .expect("delete subtree");
    }

    fn root_path(&self, path: &str) -> std::path::PathBuf {
        self.root.path().join(path)
    }
}

/// A write confined to one directory takes the incremental path, records no full
/// rebuild, leaves an unrelated directory's pagination valid, and produces exactly
/// the facts a rebuild would.
#[test]
fn a_single_file_write_updates_the_catalog_incrementally_and_matches_a_rebuild() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("alpha")).expect("alpha");
        std::fs::create_dir_all(root.join("beta/nested")).expect("beta");
        for index in 0..40 {
            std::fs::write(root.join(format!("alpha/file{index:03}.txt")), "alpha body")
                .expect("alpha file");
            std::fs::write(root.join(format!("beta/nested/file{index:03}.txt")), "beta body")
                .expect("beta file");
        }
    });
    let service = harness.service();
    let updates_before = harness.event_count(CATALOG_UPDATED);
    let rebuilds_before = harness.event_count(CATALOG_REBUILT);
    let beta_before = harness.directory_version("beta");

    // A real overwrite through the tool service, then a create in the same
    // directory: both are scoped to `alpha`. The overwrite has to name the current
    // content, which is what the tool contract requires.
    let existing =
        std::fs::read_to_string(harness.root_path("alpha/file000.txt")).expect("existing content");
    harness.write(
        &service,
        "alpha/file000.txt",
        "edited body",
        FsExpectedState::FileHash {
            hash: aura_domain_common::ContentHash::blake3(existing.as_bytes()),
        },
    );
    harness.write(&service, "alpha/new.txt", "new file", FsExpectedState::Absent);

    assert_eq!(
        harness.event_count(CATALOG_REBUILT),
        rebuilds_before,
        "the write itself must not trigger a full catalog rebuild"
    );
    assert!(
        harness.event_count(CATALOG_UPDATED) > updates_before,
        "the write must record a bounded catalog update"
    );
    assert_eq!(
        harness.directory_version("beta"),
        beta_before,
        "a write in alpha must leave beta's pagination valid"
    );
    assert!(
        harness
            .listing("alpha")
            .iter()
            .any(|(path, length)| path == "alpha/new.txt" && *length == Some(8)),
        "the created file is indexed with its size: {:?}",
        harness.listing("alpha")
    );

    // The scoped update must equal what a full rebuild of the same tree produces.
    let incremental = harness.facts();
    let resolver = harness.workspace.resolver(harness.workspace_id).expect("resolver");
    harness.catalog().rebuild(&resolver, CorrelationId::new()).expect("full rebuild");
    assert_eq!(
        incremental,
        harness.facts(),
        "the incrementally updated index must equal a full rebuild"
    );
}

/// The same change observed twice must land once.
///
/// A watcher and the write path both describe changes, so the same path can be
/// reported more than once — which is the shape a duplicate event has. Applying that
/// description again must not index the file twice, drop it, or fall back to a full
/// rebuild, and the result still has to equal a rebuild of the same tree.
#[test]
fn applying_the_same_change_twice_indexes_it_once() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("alpha")).expect("alpha");
        std::fs::write(root.join("alpha/keep.txt"), "keep body").expect("keep file");
    });
    let service = harness.service();
    let rebuilds_before = harness.event_count(CATALOG_REBUILT);

    harness.write(&service, "alpha/once.txt", "one body", FsExpectedState::Absent);
    let after_first = harness.facts();
    // The same path with the same content, seen again: nothing about the tree changed.
    harness.write(
        &service,
        "alpha/once.txt",
        "one body",
        FsExpectedState::FileHash { hash: aura_domain_common::ContentHash::blake3(b"one body") },
    );

    assert_eq!(
        harness.event_count(CATALOG_REBUILT),
        rebuilds_before,
        "re-applying a change must not force a full rebuild"
    );
    let listed = harness.listing("alpha");
    assert_eq!(
        listed.iter().filter(|(path, _)| path == "alpha/once.txt").count(),
        1,
        "a repeated change must not index the same path twice: {listed:?}"
    );
    let incremental = harness.facts();
    let resolver = harness.workspace.resolver(harness.workspace_id).expect("resolver");
    harness.catalog().rebuild(&resolver, CorrelationId::new()).expect("full rebuild");
    assert_eq!(
        incremental,
        harness.facts(),
        "the index must equal a full rebuild after a repeated change"
    );
    assert_eq!(
        after_first.iter().map(|fact| fact.0.clone()).collect::<Vec<_>>(),
        incremental.iter().map(|fact| fact.0.clone()).collect::<Vec<_>>(),
        "re-applying an unchanged file must not change which paths are indexed"
    );
}

/// A directory that grows past the partition limit must still be updated in place.
///
/// Partitions are how the text index is split, so crossing the limit changes how the
/// directory's entries are stored. The scoped update therefore has to keep producing
/// exactly what a full scan of the same tree produces, including for the directory
/// whose partition just changed shape — that is the case the review listed as
/// uncovered, and it is checked the same way the scoped update is: by comparing the
/// increment against a real rebuild of the same files.
#[test]
fn crossing_the_partition_limit_still_matches_a_full_rebuild() {
    let harness = Harness::with_partition_limit(
        |root| {
            // Exactly at the limit, so the create below crosses it.
            std::fs::write(root.join("a.txt"), "alpha body").expect("first file");
            std::fs::write(root.join("b.txt"), "beta body").expect("second file");
        },
        2,
    );
    let service = harness.service();
    let rebuilds_before = harness.event_count(CATALOG_REBUILT);

    harness.write(&service, "c.txt", "gamma body", FsExpectedState::Absent);

    assert_eq!(
        harness.event_count(CATALOG_REBUILT),
        rebuilds_before,
        "crossing a partition limit must not force a full rebuild"
    );

    // The scoped update must equal what a full rebuild of the same tree produces.
    let incremental = harness.facts();
    let resolver = harness.workspace.resolver(harness.workspace_id).expect("resolver");
    harness.catalog().rebuild(&resolver, CorrelationId::new()).expect("full rebuild");
    assert_eq!(
        incremental,
        harness.facts(),
        "the incrementally updated index must equal a full rebuild after crossing the \
         partition limit"
    );
}

/// The watcher must not re-index the whole workspace when it observes a change.
///
/// The write path and the watcher now share one bounded change description, so a
/// single-file write costs a re-read of its own directory whether Aura or another
/// program made the change. An unclassified backend event still names its paths
/// and is therefore describable too; only an event that names nothing escalates to
/// a full rebuild.
#[test]
fn the_watcher_does_not_rebuild_the_whole_catalog_for_one_write() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("alpha")).expect("alpha");
        std::fs::create_dir_all(root.join("beta")).expect("beta");
        std::fs::write(root.join("alpha/one.txt"), "one").expect("alpha file");
        std::fs::write(root.join("beta/two.txt"), "two").expect("beta file");
    });
    let service = harness.service();
    let rebuilds_before = harness.event_count(CATALOG_REBUILT);
    let beta_before = harness.directory_version("beta");

    harness.write(&service, "alpha/three.txt", "three", FsExpectedState::Absent);
    std::thread::sleep(Duration::from_millis(500));

    assert_eq!(
        harness.event_count(CATALOG_REBUILT),
        rebuilds_before,
        "one write must not cause a full rebuild when the watcher observes it"
    );
    assert_eq!(
        harness.directory_version("beta"),
        beta_before,
        "a write in alpha must leave beta's pagination valid"
    );
}

/// A deleted subtree leaves the index entirely, and the delete stays scoped.
///
/// This is the review's ignored case, now a real regression. Three things had to
/// change before it could pass, and each of them was a real defect:
///
/// - deleting a directory stages it by moving it aside, and a directory the workspace
///   watcher holds open cannot be moved (`Access is denied`, os error 5), so the
///   delete never reached the index at all;
/// - after staging, the delete described its target as a *change*, which re-lists only
///   the directory that holds it — so the deleted subtree's own entries stayed in the
///   index;
/// - the watcher then raced its own deletion: a directory that had already gone was
///   still in the watch list, the whole watch group failed to start, and the workspace
///   answered that with a full rebuild.
///
/// The assertions below are what the review asked for: the delete is scoped (no
/// rebuild), an unrelated directory's pagination stays valid, the subtree is gone from
/// the index, and the increment equals a rebuild of what is left.
#[test]
fn a_deleted_subtree_leaves_the_index_and_stays_scoped() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("keep")).expect("keep");
        std::fs::create_dir_all(root.join("gone/nested")).expect("gone");
        std::fs::write(root.join("keep/one.txt"), "one").expect("keep file");
        std::fs::write(root.join("gone/nested/deep.txt"), "deep").expect("deep file");
        std::fs::write(root.join("gone/shallow.txt"), "shallow").expect("shallow file");
    });
    let service = harness.service();
    let keep_before = harness.directory_version("keep");
    let rebuilds_before = harness.event_count(CATALOG_REBUILT);

    harness.delete_recursive(&service, "gone");

    assert_eq!(
        harness.event_count(CATALOG_REBUILT),
        rebuilds_before,
        "a scoped delete must not force a full rebuild"
    );
    assert_eq!(
        harness.directory_version("keep"),
        keep_before,
        "deleting gone must leave keep's pagination valid"
    );
    let paths = harness
        .catalog()
        .entries()
        .expect("entries")
        .into_iter()
        .map(|entry| entry.path.as_str().to_owned())
        .collect::<Vec<_>>();
    assert!(
        paths.iter().all(|path| !path.starts_with("gone")),
        "a deleted subtree must leave the index: {paths:?}"
    );
    assert!(paths.iter().any(|path| path == "keep/one.txt"));

    // The scoped delete must equal a rebuild of what is left.
    let after_delete = harness.facts();
    let resolver = harness.workspace.resolver(harness.workspace_id).expect("resolver");
    harness.catalog().rebuild(&resolver, CorrelationId::new()).expect("full rebuild");
    assert_eq!(after_delete, harness.facts(), "a scoped delete must equal a rebuild");
}
