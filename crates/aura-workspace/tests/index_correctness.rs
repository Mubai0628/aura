//! Index correctness for local refresh work.
//!
//! Each test drives the real catalog and the real persistent database through the
//! production entry point, with the workspace's own watcher stopped so the only
//! change applied is the one the test names. They are the review's counter-
//! examples, moved into the repository so the behaviour they describe cannot come
//! back silently:
//!
//! - a local rescan must keep the ignore rules that were in effect above it;
//! - rescanning a directory must drop the descendants that are gone from disk;
//! - a change to one file must not re-read unrelated subtrees, which is the
//!   difference between an incremental update and a full one;
//! - a failed persistent write must not publish changed memory under the old
//!   revision.
use std::{path::Path, sync::Arc};

use aura_domain_common::{
    CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{
    CatalogChange, CatalogEntry, CatalogOptions, CatalogUpdate, PathCatalog, SecurePathResolver,
    WorkspaceError, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{SessionSummary, WorkspaceCreateCommand};

struct Harness {
    root: tempfile::TempDir,
    data: tempfile::TempDir,
    persistence: Arc<PersistenceCoordinator>,
    catalog: Arc<PathCatalog>,
    resolver: Arc<SecurePathResolver>,
}

impl Harness {
    fn new(setup: impl FnOnce(&Path)) -> Self {
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
                catalog: CatalogOptions::with_index_file_limit(64 * 1024 * 1024),
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let created = workspace
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "index review".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        let workspace_id: WorkspaceId = created.workspace.workspace_id;
        let session_id = SessionId::new();
        let current = persistence.workspace(workspace_id).expect("current workspace");
        persistence
            .upsert_workspace_session(
                current,
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: "index session".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create session");
        let catalog = workspace.catalog(workspace_id).expect("catalog");
        catalog.wait_until_built(std::time::Duration::from_secs(60)).expect("the first walk");
        let resolver = workspace.resolver(workspace_id).expect("resolver");
        // The tests apply exactly one change of their own; a live watcher would
        // deliver the same filesystem change a second time and hide which path
        // produced the result.
        drop(workspace);
        Self { root, data, persistence, catalog, resolver }
    }

    fn apply(&self, path: &str, change: CatalogChange) -> Result<Revision, WorkspaceError> {
        self.catalog.apply_changes(
            &self.resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes: vec![(WorkspacePath::parse(path).expect("path"), change)],
                reason: "index review".into(),
            },
            &self.catalog.options(),
            CorrelationId::new(),
        )
    }

    fn entry(&self, path: &str) -> Option<CatalogEntry> {
        self.catalog.entry(&WorkspacePath::parse(path).expect("path")).expect("entry")
    }

    /// Every path the index currently holds, in path order.
    fn indexed(&self) -> Vec<String> {
        self.catalog
            .entries()
            .expect("entries")
            .into_iter()
            .map(|entry| entry.path.as_str().to_owned())
            .collect()
    }
}

/// What an out-of-order change description really does.
///
/// Change descriptions arrive from two directions — the write path and the watcher —
/// so a description can be applied after the state it describes is gone. The index has
/// to converge on the tree without waiting for another event, and without a rebuild:
///
/// - a removal that names a path which really is gone drops that path and its subtree;
/// - a removal that names a path which is back is an observation that has been
///   overtaken, so the path stays indexed exactly as it is on disk (this is the gap the
///   review found: the entry used to disappear until some later listing restored it);
/// - a change for a path that no longer exists must not resurrect it.
#[test]
fn an_out_of_order_change_description_still_converges_on_the_tree() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/late.txt"), "late body").expect("late file");
        std::fs::write(root.join("src/gone.txt"), "gone body").expect("gone file");
    });
    // The index starts from what is really there.
    harness.apply("src", CatalogChange::DirectoryRescanned).expect("rescan");
    assert!(harness.entry("src/late.txt").is_some());
    assert!(harness.entry("src/gone.txt").is_some());

    // A removal description that arrives after the file was created again: the index
    // must keep showing what is on disk, immediately.
    harness.apply("src/late.txt", CatalogChange::Removed).expect("stale removal");
    assert!(
        harness.entry("src/late.txt").is_some(),
        "a removal for a path that is back must not delete the live entry"
    );

    // A removal description for a path that is really gone does drop it.
    std::fs::remove_file(harness.root.path().join("src/gone.txt")).expect("remove on disk");
    harness.apply("src/gone.txt", CatalogChange::Removed).expect("real removal");
    assert!(
        harness.entry("src/gone.txt").is_none(),
        "a removal for a path that is gone must drop it"
    );

    // A "changed" description that arrives after the file was deleted must not
    // resurrect it either.
    let _ = harness.apply("src/gone.txt", CatalogChange::Changed);
    assert!(
        harness.entry("src/gone.txt").is_none(),
        "a description for a deleted file must not resurrect it"
    );

    let mut indexed = harness.indexed();
    indexed.retain(|path| path.starts_with("src/"));
    let mut on_disk = std::fs::read_dir(harness.root.path().join("src"))
        .expect("src directory")
        .map(|entry| format!("src/{}", entry.expect("entry").file_name().to_string_lossy()))
        .collect::<Vec<_>>();
    on_disk.sort();
    indexed.sort();
    assert_eq!(
        indexed, on_disk,
        "the index must hold exactly the files that exist after out-of-order \
         descriptions, with no later event required"
    );
}

#[test]
fn a_local_rescan_keeps_the_ancestor_ignore_rules() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join(".gitignore"), "*.log\n").expect("gitignore");
        std::fs::write(root.join("src/trace.log"), "entry").expect("ignored file");
    });
    assert!(harness.entry("src/trace.log").expect("indexed entry").ignored);
    harness.apply("src", CatalogChange::DirectoryRescanned).expect("rescan");
    assert!(
        harness.entry("src/trace.log").expect("indexed entry").ignored,
        "a local rescan dropped the rule the root .gitignore still states"
    );
}

#[test]
fn rescanning_a_directory_removes_deleted_descendants() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src/nested")).expect("nested");
        std::fs::write(root.join("src/nested/gone.txt"), "gone").expect("file");
    });
    std::fs::remove_file(harness.root.path().join("src/nested/gone.txt")).expect("remove");
    harness.apply("src", CatalogChange::DirectoryRescanned).expect("rescan");
    assert!(
        harness.entry("src/nested/gone.txt").is_none(),
        "a file that is gone from disk is still reported as existing"
    );
}

#[test]
fn a_root_level_file_change_reads_the_file_and_its_listing_only() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("unrelated/nested")).expect("nested");
        std::fs::write(root.join("unrelated/nested/large.txt"), "before").expect("file");
        std::fs::write(root.join("touched.txt"), "one").expect("touched");
    });
    let untouched = harness.entry("unrelated/nested/large.txt").expect("entry");
    // Both files change on disk. Only the one the update names is read, so the
    // unrelated subtree keeps the facts a full scan would have refreshed.
    std::fs::write(
        harness.root.path().join("unrelated/nested/large.txt"),
        "changed under the index",
    )
    .expect("rewrite unrelated");
    std::fs::write(harness.root.path().join("touched.txt"), "two much longer").expect("rewrite");
    // What the write path describes for one root file: the root listing (so the
    // directory's own metadata follows) and the file itself.
    harness.apply(".", CatalogChange::DirectoryListed).expect("list the root");
    harness.apply("touched.txt", CatalogChange::Changed).expect("re-read the file");

    let afterwards = harness.entry("unrelated/nested/large.txt").expect("entry");
    assert_eq!(
        afterwards.byte_length, untouched.byte_length,
        "a root-level refresh re-read a descendant in an unrelated subtree"
    );
    assert_eq!(
        afterwards.content_hash, untouched.content_hash,
        "the unrelated file was read and hashed again"
    );
    assert_eq!(
        harness.entry("touched.txt").expect("entry").byte_length,
        Some("two much longer".len() as u64),
        "the file the update named is indexed with its new content"
    );
}

#[test]
fn a_failed_persistence_write_does_not_publish_changed_memory() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("one.txt"), "old").expect("file");
    });
    let previous = harness.entry("one.txt").expect("entry").byte_length;
    let revision = harness.catalog.revision().expect("revision");
    let database =
        rusqlite::Connection::open(harness.data.path().join("aura.sqlite3")).expect("database");
    // The next delta cannot be written, so the update has to fail as a whole.
    database
        .execute_batch(
            "CREATE TRIGGER review_reject_delta BEFORE INSERT ON workspace_catalog_entries \
             BEGIN SELECT RAISE(FAIL, 'review_reject_delta'); END;",
        )
        .expect("trigger");
    std::fs::write(harness.root.path().join("one.txt"), "new-long-content").expect("rewrite");
    assert!(harness.apply("one.txt", CatalogChange::Changed).is_err(), "the write must fail");
    assert_eq!(harness.catalog.revision().expect("revision"), revision);
    assert_eq!(
        harness.entry("one.txt").expect("entry").byte_length,
        previous,
        "a failed transaction already changed the published memory view"
    );
    drop(harness.persistence);
}
