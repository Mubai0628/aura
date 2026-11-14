//! What one incremental catalog update is allowed to change, and what it must leave
//! exactly as it found it.
//!
//! The incident this suite comes from: an access-control change touches every object
//! in the workspace, the Windows watcher reports a modification for each one, and the
//! index answered by re-reading and rewriting whole directories that had not changed.
//! Per-path text-index deletes had no index to use, so each rewrite scanned the whole
//! text index; the resulting write held the single persistence writer long enough to
//! stall the status route for nearly thirteen minutes.
//!
//! So these tests are written in pairs. A notification that describes nothing new must
//! neither rewrite the text index nor advance the revision — and a real content change
//! at the very same path must still be indexed. An optimisation that satisfies only
//! the first half is the original bug with the numbers reversed.
//!
//! Every test drives the production catalog, the production persistence layer and the
//! real database file. The workspace's own watcher is stopped so the only change
//! applied is the one the test names.
use std::{collections::BTreeMap, path::Path, str::FromStr, sync::Arc};

use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, WorkspaceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{
    CatalogChange, CatalogEntry, CatalogOptions, CatalogUpdate, PathCatalog, SecurePathResolver,
    WorkspaceError, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{
    CatalogIndexMode, CatalogPartitionSummary, TextEncoding, WorkspaceCreateCommand,
};
use rusqlite::{Connection, OpenFlags};

struct Harness {
    root: tempfile::TempDir,
    persistence: Arc<PersistenceCoordinator>,
    catalog: Arc<PathCatalog>,
    resolver: Arc<SecurePathResolver>,
    database: std::path::PathBuf,
}

impl Harness {
    fn new(options: CatalogOptions, setup: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        setup(root.path());
        let data = tempfile::tempdir().expect("data root");
        let database = data.path().join("aura.sqlite3");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            database.clone(),
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
            WorkspaceRuntimeOptions { catalog: options, ..WorkspaceRuntimeOptions::default() },
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
        let catalog = workspace.catalog(created.workspace.workspace_id).expect("catalog");
        catalog.wait_until_built(std::time::Duration::from_secs(60)).expect("the first walk");
        let resolver = workspace.resolver(created.workspace.workspace_id).expect("resolver");
        // A live watcher would deliver the test's own filesystem change a second time
        // and hide which path produced the result.
        drop(workspace);
        Self { root, persistence, catalog, resolver, database }
    }

    fn with_notifications(setup: impl FnOnce(&Path)) -> Self {
        Self::new(CatalogOptions::with_index_file_limit(64 * 1024 * 1024), setup)
    }

    fn apply(&self, changes: Vec<(&str, CatalogChange)>) -> Result<Revision, WorkspaceError> {
        let changes = changes
            .into_iter()
            .map(|(path, change)| (WorkspacePath::parse(path).expect("path"), change))
            .collect::<Vec<_>>();
        self.catalog.apply_changes(
            &self.resolver,
            CatalogUpdate {
                inputs: changes.len(),
                ..CatalogUpdate::new(changes, "incremental catalog review")
            },
            &self.catalog.options(),
            CorrelationId::new(),
        )
    }

    fn entry(&self, path: &str) -> Option<CatalogEntry> {
        self.catalog.entry(&WorkspacePath::parse(path).expect("path")).expect("entry")
    }

    fn read_only(&self) -> Connection {
        Connection::open_with_flags(&self.database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("read-only database")
    }

    /// The text-index rows exactly as they are stored, keyed by path.
    ///
    /// A path marked for a rebuild has a map row but no text row, so it is absent here
    /// rather than reported as a row with no address.
    fn text_rows(&self) -> BTreeMap<String, i64> {
        let database = self.read_only();
        let mut statement = database
            .prepare(
                "SELECT relative_path, fts_rowid FROM workspace_content_index
                 WHERE fts_rowid IS NOT NULL
                 ORDER BY relative_path",
            )
            .expect("statement");
        statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .expect("rows")
            .collect::<Result<BTreeMap<_, _>, _>>()
            .expect("rows")
    }

    /// The indexed text of one path, read through the rowid map the index writes.
    fn indexed_text(&self, path: &str) -> Option<String> {
        let rowid = self.text_rows().get(path).copied()?;
        self.read_only()
            .query_row("SELECT content FROM workspace_content_fts WHERE rowid=?1", [rowid], |row| {
                row.get::<_, String>(0)
            })
            .ok()
    }

    fn text_row_count(&self) -> u64 {
        let count = self
            .read_only()
            .query_row(
                "SELECT COUNT(*) FROM workspace_content_index WHERE fts_rowid IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count");
        u64::try_from(count).expect("count")
    }

    fn revision(&self) -> Revision {
        self.catalog.revision().expect("revision")
    }

    fn workspace_id(&self) -> WorkspaceId {
        self.persistence
            .workspace_session_projection()
            .expect("projection")
            .workspaces
            .first()
            .expect("one workspace")
            .workspace_id
    }

    fn reopen(&self) -> Result<PathCatalog, WorkspaceError> {
        PathCatalog::open(
            self.workspace_id(),
            Arc::clone(&self.persistence),
            self.catalog.options(),
        )
    }

    /// Stands in for what the migration leaves behind: the path's text row is
    /// unprovable, so its map row becomes a bare mark with no text address.
    fn mark(&self, path: &str) {
        let workspace_id = self.workspace_id().to_string();
        let database = Connection::open(&self.database).expect("database");
        database
            .execute("DELETE FROM workspace_content_fts WHERE relative_path=?1", [path])
            .expect("drop text");
        let updated = database
            .execute(
                "UPDATE workspace_content_index
                 SET fts_rowid=NULL, content_hash=NULL, needs_reindex=1
                 WHERE workspace_id=?1 AND relative_path=?2",
                rusqlite::params![workspace_id, path],
            )
            .expect("mark");
        if updated == 0 {
            database
                .execute(
                    "INSERT INTO workspace_content_index(
                        workspace_id, relative_path, fts_rowid, content_hash, needs_reindex
                     ) VALUES (?1, ?2, NULL, NULL, 1)",
                    rusqlite::params![workspace_id, path],
                )
                .expect("insert mark");
        }
    }

    /// The paths whose re-derivation mark the durable index still holds.
    fn marked_paths(&self) -> Vec<String> {
        let database = self.read_only();
        let mut statement = database
            .prepare(
                "SELECT relative_path FROM workspace_content_index
                 WHERE needs_reindex=1 ORDER BY relative_path",
            )
            .expect("statement");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows")
    }

    /// The catalog revision after exactly one more update. A revision advances once per
    /// persisted delta, so an exact value proves how many writes happened rather than
    /// only that something changed.
    fn one_update_after(before: Revision) -> Revision {
        Revision::new(before.value() + 1)
    }

    /// The version token one directory page would be labelled with.
    fn directory_version(&self, path: &str) -> Revision {
        let (_, _, version) = self
            .catalog
            .children_page(&WorkspacePath::parse(path).expect("path"), None, 8)
            .expect("children page");
        version
    }

    fn partitions(&self) -> Vec<CatalogPartitionSummary> {
        self.catalog.partitions().expect("partitions")
    }

    fn write(&self, path: &str, content: &str) {
        std::fs::write(self.root.path().join(path), content).expect("write");
    }

    fn remove(&self, path: &str) {
        std::fs::remove_file(self.root.path().join(path)).expect("remove");
    }
}

/// The incident's own counterexample: the same file, described again, unchanged.
///
/// This is what an access-control change produces for every object in the tree. The
/// old implementation compared content facts that a fresh listing never fills in, so
/// the comparison always said "different", deleted the text row and inserted a new
/// one — a new rowid, a new revision, and a cursor that no longer described anything.
#[test]
fn an_unchanged_notification_neither_rewrites_the_text_index_nor_advances_the_revision() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("a.txt"), "synthetic unchanged content").expect("file");
        std::fs::write(root.join("b.txt"), "another unchanged body").expect("file");
    });
    let before = harness.text_rows();
    let revision = harness.revision();
    let version = harness.directory_version(".");

    harness.apply(vec![("a.txt", CatalogChange::Changed)]).expect("unchanged notification");
    let first = harness.catalog.last_update().expect("first update");
    let after_first = harness.text_rows();
    harness.apply(vec![("a.txt", CatalogChange::Changed)]).expect("unchanged notification again");
    let report = harness.catalog.last_update().expect("last update");
    assert_eq!(
        after_first,
        before,
        "the first notification already rewrote an unchanged text row (report: {first:?}, \
         recorded: {:?}, now: {:?})",
        harness.entry("a.txt"),
        harness.catalog.entry(&WorkspacePath::parse("a.txt").expect("path"))
    );

    assert_eq!(
        harness.text_rows(),
        before,
        "an unchanged file must keep the text row it already had (report: {:?})",
        report
    );
    assert_eq!(
        harness.revision(),
        revision,
        "a notification that describes nothing new must not advance the catalog revision"
    );
    assert_eq!(
        harness.directory_version("."),
        version,
        "an unchanged file must not invalidate the directory cursor"
    );
    assert_eq!(
        harness.indexed_text("a.txt").as_deref(),
        Some("synthetic unchanged content"),
        "the indexed text must still be the file's content"
    );
    // The saving is in the write, not in the read: a described file is read, and the
    // read is what proves there is nothing to write.
    assert!(report.read >= 1, "a described file is read before it is called unchanged");
    assert_eq!(report.no_op, 1, "the read settled it as unchanged: {report:?}");
    assert_eq!(report.upserted, 0, "and nothing was written: {report:?}");
}

/// The other half of the pair: the same shape of notification, but the content really
/// changed. Skipping the read because the length and the time looked familiar would
/// leave the old text indexed.
#[test]
fn a_real_content_change_at_the_same_path_is_still_indexed() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("a.txt"), "first body").expect("file");
    });
    let revision = harness.revision();

    harness.write("a.txt", "a completely different body");
    harness.apply(vec![("a.txt", CatalogChange::Changed)]).expect("changed notification");

    assert_eq!(
        harness.revision(),
        Harness::one_update_after(revision),
        "the rewrite is one update, not several"
    );
    assert_eq!(
        harness.indexed_text("a.txt").as_deref(),
        Some("a completely different body"),
        "the text index must hold the file's new content"
    );
    assert_eq!(harness.text_row_count(), 1, "one file must have exactly one text row");
}

/// Rewriting a file with content of exactly the same length. The listing facts alone
/// cannot tell this apart from an untouched file, so the decision has to come from the
/// content the update actually read.
#[test]
fn a_same_length_rewrite_is_detected_by_content_not_by_length() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("value.txt"), "value=1111\n").expect("file");
    });
    let revision = harness.revision();

    harness.write("value.txt", "value=2222\n");
    let fresh = std::fs::metadata(harness.root.path().join("value.txt")).expect("metadata");
    let recorded = harness.entry("value.txt").expect("entry");
    assert_eq!(
        recorded.byte_length,
        Some(fresh.len()),
        "this test is only meaningful while the two lengths really are equal"
    );

    harness.apply(vec![("value.txt", CatalogChange::Changed)]).expect("changed notification");

    assert_eq!(
        harness.revision(),
        Harness::one_update_after(revision),
        "a same-length rewrite must still be re-indexed"
    );
    assert_eq!(harness.indexed_text("value.txt").as_deref(), Some("value=2222\n"));
}

/// A file rewritten with the same length, with its modification time put back exactly
/// where it was.
///
/// This is the case a length-and-time shortcut gets wrong, and it was observed doing
/// so: the disk held `value=2222`, the index still held `value=1111`, and the update
/// reported itself as a no-op. A listing cannot prove that the bytes are unchanged, so
/// a notification that describes a text file has to be settled by reading it. The
/// assertion below is therefore about the *content the index holds*, not about how
/// little work the update did.
#[test]
fn a_real_change_is_indexed_even_when_the_length_and_time_are_restored() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("pinned.txt"), "value=1111\n").expect("file");
    });
    let recorded = harness.entry("pinned.txt").expect("entry");
    let revision = harness.revision();

    let pinned = filetime_from(recorded.modified_at.as_deref().expect("recorded time"));
    harness.write("pinned.txt", "value=2222\n");
    std::fs::OpenOptions::new()
        .write(true)
        .open(harness.root.path().join("pinned.txt"))
        .expect("open")
        .set_modified(pinned)
        .expect("restore modification time");
    let facts = std::fs::metadata(harness.root.path().join("pinned.txt")).expect("metadata");
    assert_eq!(
        facts.len(),
        recorded.byte_length.expect("recorded length"),
        "the rewrite kept the length, which is what makes the listing facts look unchanged"
    );
    assert_eq!(
        facts.modified().expect("modification time"),
        pinned,
        "the test needs the modification time to be back where the index recorded it"
    );

    harness.apply(vec![("pinned.txt", CatalogChange::Changed)]).expect("notification");
    let report = harness.catalog.last_update().expect("last update");
    assert!(
        report.read >= 1,
        "a described text file must be read before the index is called correct: {report:?}"
    );

    assert_eq!(
        harness.indexed_text("pinned.txt").as_deref(),
        Some("value=2222\n"),
        "the index must hold what is on disk, not what the listing facts look like"
    );
    assert!(
        harness.revision() > revision,
        "a real content change must advance the revision ({report:?})"
    );
    assert_eq!(
        harness.entry("pinned.txt").expect("entry").byte_length,
        Some("value=2222\n".len() as u64)
    );

    // The other half: a described file that really is unchanged is read and still costs
    // no write, no text-index rewrite and no revision.
    let settled = harness.revision();
    harness.apply(vec![("pinned.txt", CatalogChange::Changed)]).expect("notification");
    let report = harness.catalog.last_update().expect("last update");
    assert!(report.read >= 1, "an unproven file is read again: {report:?}");
    assert_eq!(report.no_op, 1, "and the read settles it as unchanged: {report:?}");
    assert_eq!(
        harness.revision(),
        settled,
        "verifying an unchanged file must not rewrite the text index: {report:?}"
    );
}

/// A directory's own metadata changing — the shape an access-control notification has
/// for a directory — must not re-list or rewrite its children.
#[test]
fn a_directory_metadata_change_does_not_rewrite_its_children() {
    let harness = Harness::with_notifications(|root| {
        std::fs::create_dir_all(root.join("src/nested")).expect("directory");
        std::fs::write(root.join("src/one.txt"), "one body").expect("file");
        std::fs::write(root.join("src/nested/two.txt"), "two body").expect("file");
    });
    let before = harness.text_rows();
    let revision = harness.revision();

    harness.apply(vec![("src", CatalogChange::Changed)]).expect("directory notification");

    assert_eq!(harness.text_rows(), before, "a directory's own change must not touch its children");
    assert_eq!(harness.revision(), revision);
    assert!(harness.entry("src").is_some(), "the directory itself stays indexed");
}

/// A batch that repeats one path — the write path and the watcher describing the same
/// file, plus the parent listing that comes with it — must count that path once.
#[test]
fn a_batch_that_repeats_a_path_describes_it_once() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("dup.txt"), "duplicate body").expect("file");
    });
    let before = harness.text_rows();
    let revision = harness.revision();

    harness.apply(vec![("dup.txt", CatalogChange::Changed)]).expect("repeated notification");

    assert_eq!(harness.text_rows(), before, "a repeated unchanged path must not be rewritten");
    assert_eq!(harness.revision(), revision);
    assert_eq!(harness.text_row_count(), 1, "a repeated path must not gain a second text row");
}

/// Crossing the full-text partition threshold. The text a partition stores has to
/// follow the new index mode even though every file is byte-for-byte unchanged.
#[test]
fn crossing_the_partition_threshold_rebuilds_the_stored_text() {
    // 512 bytes of indexable text is the whole budget, so the second file crosses it.
    let harness = Harness::new(
        CatalogOptions {
            max_partition_files: 20_000,
            max_partition_text_bytes: 512,
            max_index_file_bytes: 64 * 1024 * 1024,
        },
        |root| {
            std::fs::create_dir_all(root.join("src")).expect("directory");
            std::fs::write(root.join("src/package.json"), "{}").expect("manifest");
            std::fs::write(root.join("src/first.txt"), "a".repeat(400)).expect("file");
        },
    );
    assert_eq!(
        harness.partitions().iter().map(|partition| partition.index_mode).collect::<Vec<_>>(),
        vec![CatalogIndexMode::FullText],
        "the workspace must start small enough to keep its text"
    );
    assert!(harness.indexed_text("src/first.txt").is_some());

    // The same file is rewritten, so the file itself is not what crosses the limit:
    // the second file's bytes are.
    harness.write("src/second.txt", &"b".repeat(400));
    harness.apply(vec![("src/second.txt", CatalogChange::Changed)]).expect("changed notification");

    assert_eq!(
        harness.partitions().iter().map(|partition| partition.index_mode).collect::<Vec<_>>(),
        vec![CatalogIndexMode::MetadataOnly],
        "the partition must switch to metadata-only once it exceeds its text budget"
    );

    // Crossing back re-reads the unchanged files: the text they lost has to come back.
    harness.write("src/second.txt", "small");
    harness.apply(vec![("src/second.txt", CatalogChange::Changed)]).expect("changed notification");
    assert_eq!(
        harness.partitions().iter().map(|partition| partition.index_mode).collect::<Vec<_>>(),
        vec![CatalogIndexMode::FullText],
    );
    assert_eq!(
        harness.indexed_text("src/first.txt").as_deref(),
        Some("a".repeat(400).as_str()),
        "an unchanged file must get its text back when the partition stores text again"
    );
}

/// An encoding change is a content fact change even when the decoded text is identical.
#[test]
fn an_encoding_change_re_indexes_the_same_text() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("encoded.txt"), "same text").expect("file");
    });
    assert_eq!(
        harness.entry("encoded.txt").expect("entry").text_encoding,
        Some(TextEncoding::Utf8)
    );
    let revision = harness.revision();

    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(b"same text");
    std::fs::write(harness.root.path().join("encoded.txt"), bytes).expect("rewrite");
    harness.apply(vec![("encoded.txt", CatalogChange::Changed)]).expect("changed notification");

    assert_eq!(
        harness.revision(),
        Harness::one_update_after(revision),
        "a byte-order-mark change must rewrite the text row"
    );
    assert_eq!(
        harness.entry("encoded.txt").expect("entry").text_encoding,
        Some(TextEncoding::Utf8Bom)
    );
    assert_eq!(
        harness.indexed_text("encoded.txt").as_deref(),
        Some("same text"),
        "the decoded text is unchanged, and so is what the index holds"
    );
}

/// Deleting a file and creating a new one at the same path must leave exactly one text
/// row, and the new content must be the one that is indexed.
#[test]
fn a_path_deleted_and_recreated_holds_one_text_row() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("recreated.txt"), "first generation").expect("file");
    });
    let first = harness.catalog.last_update();
    harness.remove("recreated.txt");
    harness.apply(vec![("recreated.txt", CatalogChange::Removed)]).expect("removal notification");
    let removal = harness.catalog.last_update();
    let removal_entry = harness.entry("recreated.txt");
    let removal_rows = harness.text_rows();
    assert_eq!(
        harness.text_row_count(),
        0,
        "a removed file must not keep a text row ({first:?} -> {removal:?}, \
         entry: {removal_entry:?}, rows: {removal_rows:?})"
    );
    assert!(
        harness.entry("recreated.txt").is_none(),
        "the removed path must leave the published index too"
    );

    harness.write("recreated.txt", "second generation");
    harness
        .apply(vec![("recreated.txt", CatalogChange::Changed)])
        .expect("recreation notification");

    assert_eq!(
        harness.text_row_count(),
        1,
        "the recreated path must have exactly one text row (entry: {:?}, revision: {:?}, \
         rows: {:?}, report: {:?}, partitions: {:?})",
        harness.entry("recreated.txt"),
        harness.revision(),
        harness.text_rows(),
        harness.catalog.last_update(),
        harness.partitions()
    );
    assert_eq!(harness.indexed_text("recreated.txt").as_deref(), Some("second generation"));
}

/// Two workspaces in one database, holding a file of the same name. The rowid map is
/// keyed by workspace as well as by path, so neither workspace's row can address the
/// other's text.
#[test]
fn two_workspaces_can_index_the_same_relative_path() {
    let first_root = tempfile::tempdir().expect("first root");
    let second_root = tempfile::tempdir().expect("second root");
    std::fs::write(first_root.path().join("shared.txt"), "first workspace body").expect("file");
    std::fs::write(second_root.path().join("shared.txt"), "second workspace body").expect("file");
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
    let workspace =
        WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
            .expect("workspace runtime");
    let mut catalogs = Vec::new();
    for (root, name) in [(&first_root, "first"), (&second_root, "second")] {
        let created = workspace
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: name.to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        workspace.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
        catalogs.push((
            workspace.catalog(created.workspace.workspace_id).expect("catalog"),
            workspace.resolver(created.workspace.workspace_id).expect("resolver"),
        ));
    }
    drop(workspace);

    let path = WorkspacePath::parse("shared.txt").expect("path");
    for (catalog, resolver) in &catalogs {
        assert!(catalog.entry(&path).expect("entry").is_some());
        catalog
            .apply_changes(
                resolver,
                CatalogUpdate::new(
                    vec![(path.clone(), CatalogChange::Changed)],
                    "cross-workspace review",
                ),
                &catalog.options(),
                CorrelationId::new(),
            )
            .expect("notification");
    }

    let database = Connection::open_with_flags(
        data.path().join("aura.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read-only database");
    let rows = database
        .query_row("SELECT COUNT(*) FROM workspace_content_index", [], |row| row.get::<_, i64>(0))
        .expect("count");
    assert_eq!(rows, 2, "each workspace holds its own row for the same relative path");
    let distinct = database
        .query_row("SELECT COUNT(DISTINCT fts_rowid) FROM workspace_content_index", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("count");
    assert_eq!(distinct, 2, "the two workspaces must not share one text row");
}

/// A text update that fails after the text row was already written must roll back
/// whole: no text row, no map row, and the published revision stays where it was.
#[test]
fn a_failed_text_update_rolls_back_the_index_and_the_revision() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("guarded.txt"), "original body").expect("file");
    });
    let before = harness.text_rows();
    let revision = harness.revision();
    Connection::open(&harness.database)
        .expect("database")
        .execute_batch(
            "CREATE TRIGGER review_reject_map BEFORE INSERT ON workspace_content_index
             BEGIN SELECT RAISE(FAIL, 'review_reject_map'); END;",
        )
        .expect("trigger");

    harness.write("guarded.txt", "a different body");
    assert!(
        harness.apply(vec![("guarded.txt", CatalogChange::Changed)]).is_err(),
        "the write must fail"
    );

    assert_eq!(harness.text_rows(), before, "a rolled-back update must not leave a map row");
    assert_eq!(harness.revision(), revision, "a rolled-back update must not publish a revision");
    assert_eq!(
        harness.indexed_text("guarded.txt").as_deref(),
        Some("original body"),
        "the published text must not disagree with the durable one"
    );
    drop(harness.persistence);
}

/// A path an upgrade could not settle is re-derived from the file, not guessed at.
///
/// An older database can hold two generations of one file's text with no digest that
/// says which is current. The migration drops both and marks the path; this is the other
/// half — the catalog reads the file, so the text in the index is the text on disk.
#[test]
fn a_marked_path_is_re_derived_from_the_file_when_the_catalog_opens() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("ambiguous.txt"), "the text that is really on disk")
            .expect("file");
    });
    // Stand in for what the migration leaves behind: no text row, a mark, and no digest
    // borrowed from the catalog.
    let database = Connection::open(&harness.database).expect("database");
    let workspace_id: String = database
        .query_row("SELECT workspace_id FROM workspaces LIMIT 1", [], |row| row.get(0))
        .expect("workspace");
    database
        .execute("DELETE FROM workspace_content_fts WHERE relative_path='ambiguous.txt'", [])
        .expect("drop text");
    database
        .execute(
            "UPDATE workspace_content_index
             SET fts_rowid=NULL, content_hash=NULL, needs_reindex=1
             WHERE workspace_id=?1 AND relative_path='ambiguous.txt'",
            [&workspace_id],
        )
        .expect("mark");
    assert!(
        !harness.text_rows().contains_key("ambiguous.txt"),
        "the marked path starts with no text row"
    );

    let reopened = PathCatalog::open(
        WorkspaceId::from_str(&workspace_id).expect("workspace id"),
        Arc::clone(&harness.persistence),
        harness.catalog.options(),
    )
    .expect("reopen");

    let remaining_marks = harness
        .persistence
        .workspace_content_rebuild_paths(WorkspaceId::from_str(&workspace_id).expect("id"))
        .expect("remaining marks");
    assert!(remaining_marks.is_empty(), "the repair must clear the mark it acted on");
    let report = reopened.last_update().expect("repair report");
    let rows = harness.text_rows();
    assert!(
        rows.contains_key("ambiguous.txt"),
        "the repair must produce a text row (report: {report:?}, rows: {rows:?})"
    );
    let text = Connection::open_with_flags(&harness.database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read-only database")
        .query_row(
            "SELECT content FROM workspace_content_fts AS text
             JOIN workspace_content_index AS map ON map.fts_rowid = text.rowid
             WHERE map.relative_path='ambiguous.txt'",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("re-derived text");
    assert_eq!(
        text, "the text that is really on disk",
        "the rebuilt text must come from the file, not from a stored copy"
    );
}

/// A repair whose durable write fails must not spend the mark it was settling.
///
/// The independent counterexample this pins down: a database trigger aborted the
/// repair's insert, the open reported the failure, and the mark was gone anyway —
/// so the path never got its text back and nothing ever retried. The mark settles
/// in the same transaction as the conclusion it belongs to now, so a failed write
/// leaves it exactly where it was.
#[test]
fn a_failed_repair_commit_keeps_the_mark_and_reports_the_failure() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("value.txt"), "current file text").expect("file");
    });
    harness.mark("value.txt");
    assert_eq!(harness.marked_paths(), vec!["value.txt"]);
    Connection::open(&harness.database)
        .expect("database")
        .execute_batch(
            "CREATE TRIGGER review_reject_repair BEFORE INSERT ON workspace_catalog_entries
             BEGIN SELECT RAISE(ABORT, 'independent repair commit failure'); END;",
        )
        .expect("trigger");

    let error = match harness.reopen() {
        Ok(_) => panic!("a repair whose write is aborted must fail the open"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("independent repair commit failure"),
        "the open must report the persistence failure, not swallow it: {error}"
    );
    assert_eq!(
        harness.marked_paths(),
        vec!["value.txt"],
        "a failed repair must leave its mark exactly as it was"
    );
    assert_eq!(harness.text_row_count(), 0, "the aborted write must not leave text behind");

    Connection::open(&harness.database)
        .expect("database")
        .execute_batch("DROP TRIGGER review_reject_repair;")
        .expect("drop trigger");

    let repaired = harness.reopen().expect("reopen without the fault");
    assert!(harness.marked_paths().is_empty(), "the committed repair clears the mark it settled");
    assert_eq!(harness.text_row_count(), 1);
    assert_eq!(harness.indexed_text("value.txt").as_deref(), Some("current file text"));

    // A repair that already settled is not repeated: another open describes nothing
    // to repair, writes nothing and does not advance the revision.
    let revision = repaired.revision().expect("revision");
    let reopened = harness.reopen().expect("second reopen");
    assert_eq!(
        reopened.revision().expect("second revision"),
        revision,
        "a settled repair must not advance the revision on every open"
    );
    assert_eq!(harness.text_row_count(), 1, "no duplicate text rows");
    assert_eq!(harness.indexed_text("value.txt").as_deref(), Some("current file text"));
}

/// One repair batch settles one mark at a time. A path whose text is written gives
/// its mark up; a path whose text still cannot be established keeps it, and a
/// later open re-derives it once reading succeeds.
#[test]
fn a_repair_clears_only_the_marks_of_the_paths_it_settled() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("good.txt"), "readable body").expect("file");
        // Not decodable as any supported text encoding: the index records the file
        // but can never hold its text.
        std::fs::write(root.join("binary.txt"), [0x80_u8, 0x81, 0x82]).expect("file");
    });
    assert_eq!(harness.indexed_text("good.txt").as_deref(), Some("readable body"));
    assert!(harness.indexed_text("binary.txt").is_none());
    harness.mark("good.txt");
    harness.mark("binary.txt");
    assert_eq!(harness.marked_paths(), vec!["binary.txt", "good.txt"]);

    harness.reopen().expect("repair open");

    assert_eq!(
        harness.marked_paths(),
        vec!["binary.txt"],
        "the path whose text was written gives up its mark; the path whose text could \
         not be established keeps it"
    );
    assert_eq!(harness.text_row_count(), 1, "exactly one text row exists after the repair");
    assert_eq!(harness.indexed_text("good.txt").as_deref(), Some("readable body"));

    // The surviving mark is a recoverable fact, not a permanent stall: once the
    // file's text can be read, the next repair settles it too.
    harness.write("binary.txt", "now readable");
    let recovered = harness.reopen().expect("recovery open");
    assert!(harness.marked_paths().is_empty(), "the recovery settles the remaining mark");
    assert_eq!(harness.indexed_text("binary.txt").as_deref(), Some("now readable"));
    let revision = recovered.revision().expect("revision");
    let again = harness.reopen().expect("settled open");
    assert_eq!(
        again.revision().expect("settled revision"),
        revision,
        "a fully settled repair is not repeated"
    );
    assert_eq!(harness.text_row_count(), 2);
}

/// A repair whose file is gone settles the mark with the removal: the committed
/// conclusion is the deletion, so the mark must not survive it.
#[test]
fn a_repair_mark_is_cleared_by_a_committed_removal() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("doomed.txt"), "last generation").expect("file");
    });
    harness.mark("doomed.txt");
    harness.remove("doomed.txt");

    let reopened = harness.reopen().expect("repair open");

    assert!(harness.marked_paths().is_empty(), "the removal settles the mark");
    assert!(
        reopened
            .entry(&WorkspacePath::parse("doomed.txt").expect("path"))
            .expect("entry")
            .is_none(),
        "the gone file leaves the index"
    );
    assert_eq!(harness.text_row_count(), 0);
}

/// The version a directory page is labelled with must survive a restart of the index.
#[test]
fn a_directory_version_survives_reopening_the_catalog() {
    let harness = Harness::with_notifications(|root| {
        std::fs::write(root.join("stable.txt"), "stable body").expect("file");
    });
    let version = harness.directory_version(".");
    let revision = harness.revision();
    let workspace_id = harness
        .persistence
        .workspace_session_projection()
        .expect("projection")
        .workspaces
        .first()
        .expect("one workspace")
        .workspace_id;
    let reopened = PathCatalog::open(
        workspace_id,
        Arc::clone(&harness.persistence),
        harness.catalog.options(),
    )
    .expect("reopen");
    let root = WorkspacePath::parse(WorkspacePath::ROOT).expect("root path");
    let (_, after_revision, after) = reopened.children_page(&root, None, 8).expect("children page");
    assert_eq!(after_revision, revision, "a reopened catalog keeps its revision");
    assert_eq!(after, version, "the same contents must produce the same directory version");
}

fn filetime_from(recorded: &str) -> std::time::SystemTime {
    let millis: u64 = recorded.parse().expect("recorded millisecond timestamp");
    std::time::UNIX_EPOCH + std::time::Duration::from_millis(millis)
}
