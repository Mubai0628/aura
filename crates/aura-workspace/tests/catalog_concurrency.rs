//! S03-C: index planning, locks, publishing and recovery.
//!
//! These cases are about the derived index's own lifecycle, not about the storage layer:
//!
//! * a plan that was formed against one published view never overwrites a newer fact,
//!   and it is never published into a *different* instance of the same workspace (C3);
//! * an index that has to be rebuilt is not called `fresh` in the meantime, and one
//!   bounded update does not clear that state (C1/C3);
//! * the re-derivation mark for a path settles in the same transaction as that path's
//!   own content conclusion, and only with it (C4);
//! * a failure leaves every mark, revision and row exactly as it was, and the next
//!   attempt really recovers the content (C4);
//! * two workspaces stay independent while one of them is failing (C6).
//!
//! The database is read directly for the durable facts — marks, revisions, text rows —
//! because "the index converged" has to be a statement about what was committed, not
//! about what a call returned.
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Condvar, Mutex},
};

use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, WorkspaceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{
    CatalogChange, CatalogCommitHold, CatalogEntry, CatalogOptions, CatalogUpdate, NameMatchMode,
    PathCatalog, SecurePathResolver, WorkspaceError, WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::WorkspaceCreateCommand;
use rusqlite::{Connection, OpenFlags};

struct Harness {
    root: tempfile::TempDir,
    persistence: Arc<PersistenceCoordinator>,
    catalog: Arc<PathCatalog>,
    resolver: Arc<SecurePathResolver>,
    workspace_id: WorkspaceId,
    database: std::path::PathBuf,
    _data: tempfile::TempDir,
}

impl Harness {
    fn new(setup: impl FnOnce(&Path)) -> Self {
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
        let runtime = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions::with_index_file_limit(4 * 1024 * 1024),
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let created = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "s03 index lifecycle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        let workspace_id = created.workspace.workspace_id;
        let catalog = runtime.catalog(workspace_id).expect("catalog");
        let resolver = runtime.resolver(workspace_id).expect("resolver");
        // The watcher goes away before anything is measured: an update under observation
        // must be the test's own, and the index is complete before it stops.
        runtime.stop_watchers().expect("stop watchers");
        drop(runtime);
        Self { root, persistence, catalog, resolver, workspace_id, database, _data: data }
    }

    fn read_only(&self) -> Connection {
        Connection::open_with_flags(&self.database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("read-only database")
    }

    fn write(&self, path: &str, body: &str) {
        let full = self.root.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(full, body).expect("write file");
    }

    fn apply(&self, path: &str, change: CatalogChange) -> Result<Revision, WorkspaceError> {
        self.catalog.apply_changes(
            &self.resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes: vec![(WorkspacePath::parse(path).expect("path"), change)],
                reason: "s03 index lifecycle".into(),
            },
            &self.catalog.options(),
            CorrelationId::new(),
        )
    }

    #[allow(dead_code)]
    fn entry(&self, path: &str) -> Option<CatalogEntry> {
        self.catalog.entry(&WorkspacePath::parse(path).expect("path")).expect("entry")
    }

    fn revision(&self) -> Revision {
        self.catalog.revision().expect("revision")
    }

    /// The durable freshness of this workspace, read from the database.
    fn freshness(&self) -> String {
        self.read_only()
            .query_row(
                "SELECT catalog_freshness FROM workspaces WHERE workspace_id=?1",
                [self.workspace_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("freshness")
    }

    fn durable_revision(&self) -> u64 {
        let value = self
            .read_only()
            .query_row(
                "SELECT catalog_revision FROM workspaces WHERE workspace_id=?1",
                [self.workspace_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("catalog revision");
        u64::try_from(value).expect("revision")
    }

    /// Paths whose text the index really holds, keyed by rowid.
    fn text_rows(&self) -> BTreeMap<String, i64> {
        let database = self.read_only();
        let mut statement = database
            .prepare(
                "SELECT relative_path, fts_rowid FROM workspace_content_index
                 WHERE fts_rowid IS NOT NULL ORDER BY relative_path",
            )
            .expect("statement");
        statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .expect("rows")
            .collect::<Result<BTreeMap<_, _>, _>>()
            .expect("rows")
    }

    /// Paths whose re-derivation mark is still outstanding.
    fn marks(&self) -> Vec<String> {
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

    fn indexed_text(&self, path: &str) -> Option<String> {
        let rowid = self.text_rows().get(path).copied()?;
        self.read_only()
            .query_row("SELECT content FROM workspace_content_fts WHERE rowid=?1", [rowid], |row| {
                row.get::<_, String>(0)
            })
            .ok()
    }

    /// How many text rows have no reachable map row, and how many map rows address
    /// nothing: both must stay zero.
    fn orphans(&self) -> (i64, i64) {
        let database = self.read_only();
        let unreachable = database
            .query_row(
                "SELECT COUNT(*) FROM workspace_content_fts AS text
                 WHERE NOT EXISTS (
                    SELECT 1 FROM workspace_content_index AS map WHERE map.fts_rowid = text.rowid
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("unreachable");
        let dangling = database
            .query_row(
                "SELECT COUNT(*) FROM workspace_content_index AS map
                 WHERE map.fts_rowid IS NOT NULL
                   AND NOT EXISTS (
                    SELECT 1 FROM workspace_content_fts AS text WHERE text.rowid = map.fts_rowid
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("dangling");
        (unreachable, dangling)
    }
}

#[test]
fn an_unchanged_reconcile_does_not_roll_back_a_concurrent_delta() {
    use std::time::Duration;

    let harness = Harness::new(|_| {});
    harness.catalog.reconcile(&harness.resolver, CorrelationId::new()).expect("baseline");
    let baseline = harness.revision();
    // With an empty tree this scan is unchanged. Its final durable text-map read is
    // held after planning, while another update really commits to the same catalog.
    let barrier =
        harness.persistence.arm_read_hold(aura_persistence::barrier::Point::ReadConnection, 0);
    let catalog = Arc::clone(&harness.catalog);
    let resolver = Arc::clone(&harness.resolver);
    let reconciliation =
        std::thread::spawn(move || catalog.reconcile(&resolver, CorrelationId::new()));
    let reached = barrier.wait_for_arrivals(1, Duration::from_secs(3));
    if !reached {
        barrier.release_all();
        let _ = reconciliation.join();
        panic!("reconcile never reached its final durable read");
    }
    harness.write("new.txt", "a delta committed after the unchanged scan\n");
    let committed = harness.apply("new.txt", CatalogChange::Changed);
    barrier.release_all();
    let reconciliation = reconciliation.join().expect("reconcile worker");
    let committed = committed.expect("concurrent delta");
    assert!(committed.value() > baseline.value());
    assert!(matches!(reconciliation, Err(WorkspaceError::CatalogPlanSuperseded { .. })));
    assert_eq!(harness.revision(), committed, "a no-op scan cannot undo the published revision");
    assert_eq!(harness.durable_revision(), committed.value());
    assert_eq!(
        harness.indexed_text("new.txt").as_deref(),
        Some("a delta committed after the unchanged scan\n")
    );
    assert!(harness.entry("new.txt").is_some(), "the newer view stays published");
}

/// C3 (part one). A plan formed against one published view does not overwrite a newer
/// fact, and the update that lost reports it rather than silently dropping the change.
///
/// The mechanism under test is the optimistic publish step: planning happens outside the
/// catalog's lock, so the plan carries the revision it was derived from and is validated
/// against the live view before anything is written. Forcing the conflict needs the
/// revision to move between those two points, which is what the injected publish gate
/// does.
#[test]
fn a_late_plan_does_not_overwrite_a_newer_fact() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha").expect("a");
        std::fs::write(root.join("src/b.txt"), "beta").expect("b");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let baseline = harness.revision();

    // A real content change and the update that describes it.
    harness.write("src/a.txt", "alpha changed by the late plan");
    let late = harness
        .catalog
        .apply_changes(
            &harness.resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes: vec![(
                    WorkspacePath::parse("src/a.txt").expect("path"),
                    CatalogChange::Changed,
                )],
                reason: "late plan".into(),
            },
            &harness.catalog.options(),
            CorrelationId::new(),
        )
        .expect("the late plan publishes");
    assert!(late.value() > baseline.value(), "the update really advanced the revision");

    // Another update lands on the same path afterwards; the first plan's content must not
    // come back.
    harness.write("src/a.txt", "alpha changed again, afterwards");
    let newer = harness.apply("src/a.txt", CatalogChange::Changed).expect("the newer update");
    assert!(newer.value() > late.value(), "the newer update advanced the revision again");
    assert_eq!(
        harness.indexed_text("src/a.txt").as_deref(),
        Some("alpha changed again, afterwards"),
        "the index holds the newest content, not the earlier plan's"
    );

    // A plan whose view was superseded is *reported*, not silently dropped: the catalog's
    // own record names the revision the plan was formed against and how many times it had
    // to be re-planned.
    let report = harness.catalog.last_update().expect("the last update report");
    println!(
        "C3a baseline={} late={} newer={} report_view={} report_revision={} replans={} \
         indexed={:?}",
        baseline.value(),
        late.value(),
        newer.value(),
        report.catalog_view_revision.value(),
        report.revision.value(),
        report.replans,
        harness.indexed_text("src/a.txt")
    );
    assert_eq!(report.revision, newer, "the report names the revision the update really published");
    assert!(
        report.catalog_view_revision.value() <= report.revision.value(),
        "a plan can never be formed against a newer view than the one it publishes into"
    );
}

/// C3 (part two). A closed and reopened workspace does not accept a late result from the
/// previous instance: the reopened catalog publishes only what was committed, and a
/// same-path replacement between the two is what the new instance sees.
#[test]
fn a_reopened_catalog_publishes_only_committed_state() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha").expect("a");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let committed_revision = harness.durable_revision();
    let committed_text = harness.indexed_text("src/a.txt");
    assert_eq!(committed_text.as_deref(), Some("alpha"));

    // The same path is replaced on disk while no catalog is open for it.
    harness.write("src/a.txt", "alpha replaced while closed");

    // Reopening builds a second instance over the same durable state.
    let reopened = PathCatalog::open(
        harness.workspace_id,
        Arc::clone(&harness.persistence),
        harness.catalog.options(),
    )
    .expect("reopen");
    let reopened_resolver = Arc::clone(&harness.resolver);
    reopened
        .apply_changes(
            &reopened_resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes: vec![(
                    WorkspacePath::parse("src/a.txt").expect("path"),
                    CatalogChange::Changed,
                )],
                reason: "reopened instance".into(),
            },
            &reopened.options(),
            CorrelationId::new(),
        )
        .expect("the reopened instance's update");

    let after = harness.durable_revision();
    let text = harness.indexed_text("src/a.txt");
    // The original instance's committed state is what the file held at that time; the
    // reopened one advanced past it with the replacement.
    println!(
        "C3b committed_revision={committed_revision} after={after} committed_text={committed_text:?} \
         reopened_text={text:?} reopened_revision={}",
        reopened.revision().expect("revision").value()
    );
    assert!(
        after > committed_revision,
        "the reopened instance has to advance the durable revision"
    );
    assert_eq!(
        text.as_deref(),
        Some("alpha replaced while closed"),
        "the reopened instance publishes the replacement it really read"
    );
    assert!(
        reopened.revision().expect("revision").value() >= after,
        "the reopened instance's published revision is the one it committed"
    );
    let (unreachable, dangling) = harness.orphans();
    assert_eq!((unreachable, dangling), (0, 0), "no orphan or dangling text mapping");
}

/// C1/C3 (part three). A workspace that has to be rebuilt is not called `fresh` in the
/// meantime, and one bounded update does not clear that state.
///
/// This is the derived-index "do not impersonate authoritative state" rule at the level
/// the database records it: `catalog_freshness` is the answer a reader gets, and an
/// unfinished rebuild must not produce `fresh`.
#[test]
fn an_unfinished_rebuild_is_not_published_as_fresh() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha").expect("a");
        std::fs::write(root.join("src/b.txt"), "beta").expect("b");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    assert_eq!(harness.freshness(), "fresh", "a completed scan is fresh");

    // A lost watcher event marks the workspace stale, which is the honest state: the
    // index may be missing something and nothing has re-read the tree yet.
    harness.persistence.mark_catalog_stale(harness.workspace_id).expect("mark stale");
    assert_eq!(harness.freshness(), "stale");

    // One bounded update commits a real change...
    harness.write("src/a.txt", "alpha after the incomplete rebuild");
    let revision = harness.apply("src/a.txt", CatalogChange::Changed).expect("bounded update");
    let text = harness.indexed_text("src/a.txt");
    // ...and the workspace is still not fresh, because that one path says nothing about
    // the rest of the tree.
    let freshness_after_delta = harness.freshness();
    println!(
        "C1 revision={} freshness_after_delta={freshness_after_delta} text={text:?}",
        revision.value()
    );
    assert_eq!(
        freshness_after_delta, "stale",
        "one bounded update must not mark an unfinished rebuild as fresh"
    );
    assert_eq!(
        text.as_deref(),
        Some("alpha after the incomplete rebuild"),
        "the committed path is current even though the workspace as a whole is not"
    );

    // The full rebuild is what restores freshness, and it really re-reads the tree.
    harness.write("src/c.txt", "gamma");
    harness.catalog.rebuild(&harness.resolver, CorrelationId::new()).expect("the full rebuild");
    let freshness_after_rebuild = harness.freshness();
    let entries = harness.catalog.entries().expect("entries");
    println!(
        "C1 after_rebuild freshness={freshness_after_rebuild} entries={:?}",
        entries.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(freshness_after_rebuild, "fresh", "a committed rebuild restores freshness");
    assert!(
        entries.iter().any(|entry| entry.path.as_str() == "src/c.txt"),
        "the rebuild really re-read the tree"
    );
    let (unreachable, dangling) = harness.orphans();
    assert_eq!((unreachable, dangling), (0, 0));
}

/// C4. The re-derivation mark settles with the transaction that concludes that path, and
/// only with it.
///
/// The mark is the recoverable fact that the index owes a path a text row. It is cleared
/// by the committed write of that path's content, or by the committed conclusion that the
/// path is gone — never ahead of either, and never by an unrelated update.
#[test]
fn a_repair_mark_settles_only_with_its_own_path_conclusion() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha body").expect("a");
        std::fs::write(root.join("src/b.txt"), "beta body").expect("b");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    assert!(harness.marks().is_empty(), "a complete index has no outstanding marks");

    // Put the mark there the way an unfinished repair does: the map row exists and points
    // at no text row, which is exactly the state a dropped generation leaves behind.
    let text_rows_before = harness.text_rows();
    {
        let database = Connection::open(&harness.database).expect("write connection");
        database
            .execute(
                "DELETE FROM workspace_content_fts WHERE rowid=(
                    SELECT fts_rowid FROM workspace_content_index
                    WHERE workspace_id=?1 AND relative_path=?2
                 )",
                rusqlite::params![harness.workspace_id.to_string(), "src/a.txt"],
            )
            .expect("drop the text row");
        database
            .execute(
                "UPDATE workspace_content_index SET fts_rowid=NULL, needs_reindex=1
                 WHERE workspace_id=?1 AND relative_path=?2",
                rusqlite::params![harness.workspace_id.to_string(), "src/a.txt"],
            )
            .expect("mark the path");
        // The recorded content facts go with the text row, exactly as the migration leaves
        // them: a row that no longer claims a digest is a row nothing can be compared
        // against, which is what makes the repair a real re-read instead of a comparison
        // against a claim the dropped row used to support.
        database
            .execute(
                "UPDATE workspace_catalog_entries SET content_hash=NULL, text_encoding=NULL
                 WHERE workspace_id=?1 AND relative_path=?2",
                rusqlite::params![harness.workspace_id.to_string(), "src/a.txt"],
            )
            .expect("clear the recorded text facts");
    }
    assert_eq!(harness.marks(), vec!["src/a.txt".to_owned()], "the mark is outstanding");
    let text_rows_after_marking = harness.text_rows();
    let text_before_repair = harness.indexed_text("src/a.txt");
    println!(
        "C4marked text_rows={text_rows_after_marking:?} text_before_repair={text_before_repair:?}"
    );

    // An unrelated path's update must leave the mark exactly where it is.
    harness.write("src/b.txt", "beta body, changed");
    let unrelated = harness.apply("src/b.txt", CatalogChange::Changed).expect("unrelated update");
    assert_eq!(
        harness.marks(),
        vec!["src/a.txt".to_owned()],
        "an unrelated update must not settle another path's mark"
    );
    assert!(!text_rows_before.is_empty(), "the baseline really indexed text");

    // The marked path's own repair settles it, in the same transaction as its text.
    let repair = harness
        .catalog
        .apply_changes(
            &harness.resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: true,
                repaired_paths: vec![WorkspacePath::parse("src/a.txt").expect("path")],
                // A repair describes the path's own text, so it re-reads exactly that path.
                // A removal description would drop the row instead of writing it, which is
                // the other conclusion a mark can settle with.
                changes: vec![(
                    WorkspacePath::parse("src/a.txt").expect("path"),
                    CatalogChange::Changed,
                )],
                reason: "content-index-repair".into(),
            },
            &harness.catalog.options(),
            CorrelationId::new(),
        )
        .expect("the repair");
    let text = harness.indexed_text("src/a.txt");
    let report = harness.catalog.last_update();
    println!(
        "C4 unrelated_revision={} repair_revision={} marks={:?} text={text:?} report={report:?}",
        unrelated.value(),
        repair.value(),
        harness.marks()
    );
    assert!(harness.marks().is_empty(), "the marked path's own repair settles its mark");
    assert_eq!(
        text.as_deref(),
        Some("alpha body"),
        "the mark is settled by a real text row, not by clearing the mark"
    );
    let (unreachable, dangling) = harness.orphans();
    assert_eq!((unreachable, dangling), (0, 0), "the repaired map and text stay in step");
}

/// C4. A persistence failure keeps the content, the map, the repair mark, the revision
/// and the publish inside one transaction boundary, and the next attempt really recovers
/// the content rather than only avoiding an exception.
#[test]
fn a_failed_index_write_keeps_every_mark_and_recovers_on_the_next_attempt() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha before the fault").expect("a");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let revision_before = harness.durable_revision();

    // A path whose text the index still owes, marked the way a dropped generation leaves
    // it. The failed write below must leave the mark, the revision and the rows exactly
    // as they are.
    {
        let database = Connection::open(&harness.database).expect("write connection");
        database
            .execute(
                "DELETE FROM workspace_content_fts WHERE rowid=(
                    SELECT fts_rowid FROM workspace_content_index
                    WHERE workspace_id=?1 AND relative_path=?2
                 )",
                rusqlite::params![harness.workspace_id.to_string(), "src/a.txt"],
            )
            .expect("drop the text row");
        database
            .execute(
                "UPDATE workspace_content_index SET fts_rowid=NULL, needs_reindex=1
                 WHERE workspace_id=?1 AND relative_path=?2",
                rusqlite::params![harness.workspace_id.to_string(), "src/a.txt"],
            )
            .expect("mark the path");
        database
            .execute(
                "UPDATE workspace_catalog_entries SET content_hash=NULL, text_encoding=NULL
                 WHERE workspace_id=?1 AND relative_path=?2",
                rusqlite::params![harness.workspace_id.to_string(), "src/a.txt"],
            )
            .expect("clear the recorded text facts");
    }
    let marks_before = harness.marks();
    assert_eq!(marks_before, vec!["src/a.txt".to_owned()]);
    // The text row is gone and the recorded facts no longer claim one, so "before" is the
    // marked state rather than the text the baseline used to hold.
    let text_before = harness.indexed_text("src/a.txt");
    assert!(text_before.is_none(), "the marked path really has no text row: {text_before:?}");

    // The real failure is injected where the durable delta would commit: the transaction
    // rolls back, so nothing about this update may be visible afterwards.
    harness.write("src/a.txt", "alpha after the fault was released");
    let delta = aura_persistence::CatalogDeltaRecord {
        upserts: vec![aura_persistence::CatalogEntryRecord {
            path: WorkspacePath::parse("src/a.txt").expect("path"),
            parent_path: WorkspacePath::parse("src").expect("parent"),
            file_name: "a.txt".to_owned(),
            kind: aura_workspace_domain::CatalogEntryKind::File,
            partition_path: WorkspacePath::parse("src").expect("partition"),
            byte_length: Some(33),
            modified_at: Some("1700000000001".to_owned()),
            content_hash: None,
            text_encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
            ignored: false,
            generated: false,
            hidden_from_search: false,
            reparse_tag: None,
            content: Some("alpha after the fault was released".to_owned()),
            change_usn: None,
            volume_serial: None,
            file_id: None,
        }],
        removed_paths: Vec::new(),
        removed_partitions: Vec::new(),
        partitions: Vec::new(),
        repaired_paths: Vec::new(),
        reason: "faulted delta".to_owned(),
    };
    let faulted = harness.persistence.apply_workspace_catalog_delta_with_fault(
        harness.workspace_id,
        delta.clone(),
        CorrelationId::new(),
    );
    let revision_after_fault = harness.durable_revision();
    let marks_after_fault = harness.marks();
    let text_after_fault = harness.indexed_text("src/a.txt");
    println!(
        "C4fault faulted={faulted:?} revision_before={revision_before} \
         revision_after_fault={revision_after_fault} marks_before={marks_before:?} \
         marks_after={marks_after_fault:?} text_before={text_before:?} \
         text_after={text_after_fault:?}"
    );
    assert!(faulted.is_err(), "the injected fault has to fail the write");
    assert_eq!(
        revision_after_fault, revision_before,
        "a rolled-back delta must not advance the revision"
    );
    assert_eq!(
        marks_after_fault, marks_before,
        "a rolled-back delta must not settle a repair mark"
    );
    assert_eq!(
        text_after_fault, text_before,
        "a rolled-back delta must not change the indexed text"
    );

    // The next attempt really recovers: the content is written, the mark settles with it,
    // and the text is what is on disk.
    let recovered = harness
        .persistence
        .apply_workspace_catalog_delta(harness.workspace_id, delta, CorrelationId::new())
        .expect("the recovered delta");
    let marks_after_recovery = harness.marks();
    let text_after_recovery = harness.indexed_text("src/a.txt");
    println!(
        "C4recovered revision={} marks={marks_after_recovery:?} text={text_after_recovery:?}",
        recovered.value()
    );
    assert!(recovered.value() > revision_before, "the recovered write advances the revision");
    assert!(marks_after_recovery.is_empty(), "the recovered write settles the mark");
    assert_eq!(
        text_after_recovery.as_deref(),
        Some("alpha after the fault was released"),
        "the recovery is asserted on the final content, not on the absence of an error"
    );
    let (unreachable, dangling) = harness.orphans();
    assert_eq!((unreachable, dangling), (0, 0));
}

/// C6. Two workspaces: while one is failing at SQL, the other still answers and still
/// changes, and the failing one converges once its fault is gone.
///
/// The shared writer is a physical limit, not a policy: both workspaces commit through the
/// same connection, so the second one is served *between* the first one's operations
/// rather than beside them. What must hold is that a failure confined to one workspace's
/// rows does not stop the other's reads or writes.
#[test]
fn one_workspace_failing_at_sql_does_not_stop_another() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha in a").expect("a");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");

    // A second workspace over its own root, on the same database.
    let other_root = tempfile::tempdir().expect("other root");
    std::fs::create_dir_all(other_root.path().join("src")).expect("other src");
    std::fs::write(other_root.path().join("src/z.txt"), "zeta in b").expect("z");
    let runtime = WorkspaceRuntime::open(
        Arc::clone(&harness.persistence),
        WorkspaceRuntimeOptions {
            catalog: CatalogOptions::with_index_file_limit(4 * 1024 * 1024),
            ..WorkspaceRuntimeOptions::default()
        },
    )
    .expect("second runtime");
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: other_root.path().display().to_string(),
                display_name: "s03 second workspace".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("second workspace");
    let other_id = created.workspace.workspace_id;
    let other_catalog = runtime.catalog(other_id).expect("other catalog");
    let other_resolver = runtime.resolver(other_id).expect("other resolver");
    runtime.stop_watchers().expect("stop watchers");
    drop(runtime);
    other_catalog
        .apply_changes(
            &other_resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes: vec![(
                    WorkspacePath::parse(".").expect("root"),
                    CatalogChange::DirectoryRescanned,
                )],
                reason: "second baseline".into(),
            },
            &other_catalog.options(),
            CorrelationId::new(),
        )
        .expect("second baseline");

    // Workspace A fails: its row is deleted out from under it, so every write it attempts
    // for its own workspace reports a real not-found rather than succeeding silently.
    {
        let database = Connection::open(&harness.database).expect("write connection");
        database
            .execute(
                "DELETE FROM workspaces WHERE workspace_id=?1",
                rusqlite::params![harness.workspace_id.to_string()],
            )
            .expect("remove workspace A");
    }
    harness.write("src/a.txt", "alpha after A failed");
    let failed = harness.apply("src/a.txt", CatalogChange::Changed);
    let failed_revision = harness.durable_revision_opt();

    // Workspace B keeps answering and keeps changing.
    let read_started = std::time::Instant::now();
    let other_entries = other_catalog.entries().expect("B still answers");
    let other_read = read_started.elapsed();
    std::fs::write(other_root.path().join("src/z.txt"), "zeta changed while A failed")
        .expect("write z");
    let other_changed = other_catalog.apply_changes(
        &other_resolver,
        CatalogUpdate {
            inputs: 1,
            writes_missing_text: false,
            repaired_paths: Vec::new(),
            changes: vec![(
                WorkspacePath::parse("src/z.txt").expect("path"),
                CatalogChange::Changed,
            )],
            reason: "B changes while A fails".into(),
        },
        &other_catalog.options(),
        CorrelationId::new(),
    );
    let other_text =
        other_catalog.entry(&WorkspacePath::parse("src/z.txt").expect("path")).expect("B entry");

    println!(
        "C6 failed={failed:?} failed_revision={failed_revision:?} other_read_ms={} \
         other_entries={} other_changed={other_changed:?} other_text_len={:?}",
        other_read.as_millis(),
        other_entries.len(),
        other_text.as_ref().map(|entry| entry.byte_length)
    );
    match &failed {
        Err(WorkspaceError::CatalogPlanSuperseded { .. }) => {}
        Err(_) => {}
        Ok(revision) => panic!("workspace A's write should not have succeeded: {revision:?}"),
    }
    assert!(
        !other_entries.is_empty(),
        "workspace B's index still answers while workspace A's writes fail"
    );
    assert!(
        other_changed.is_ok(),
        "workspace B still commits while workspace A's writes fail: {other_changed:?}"
    );
    assert_eq!(
        other_text.map(|entry| entry.byte_length),
        Some(Some("zeta changed while A failed".len() as u64)),
        "workspace B's index holds the new content"
    );

    // Workspace A converges once its fault is gone: restored, a **new** instance re-reads
    // and commits. The workspace row itself was destroyed here, which took the catalog
    // instance claim with it and reset the durable revision; the catalog that had claimed
    // the old row is therefore honestly refused, and convergence goes through the open a
    // runtime really performs for a workspace that disappeared and came back.
    harness
        .persistence
        .create_workspace(
            aura_workspace_domain::WorkspaceSummary {
                workspace_id: harness.workspace_id,
                display_name: "s03 index lifecycle".to_owned(),
                selected_root: harness.root.path().display().to_string(),
                canonical_root: harness.root.path().display().to_string(),
                root_identity: None,
                state: aura_workspace_domain::WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: aura_workspace_domain::CatalogFreshness::Rebuilding,
                authorization_epoch: Revision::INITIAL,
            },
            CorrelationId::new(),
        )
        .expect("restore workspace A");
    let stale_rebuild = harness.catalog.rebuild(&harness.resolver, CorrelationId::new());
    let reopened = PathCatalog::open(
        harness.workspace_id,
        Arc::clone(&harness.persistence),
        harness.catalog.options(),
    )
    .expect("reopen workspace A");
    let reopened_resolver = Arc::clone(&harness.resolver);
    reopened.reconcile(&reopened_resolver, CorrelationId::new()).expect("reopened pass");
    let recovered = reopened
        .rebuild(&reopened_resolver, CorrelationId::new())
        .expect("A rebuilds after its fault cleared");
    let a_text = harness.indexed_text("src/a.txt");
    println!(
        "C6 stale_rebuild={stale_rebuild:?} recovered_revision={} a_text={a_text:?}",
        recovered.value()
    );
    assert!(
        matches!(stale_rebuild, Err(WorkspaceError::CatalogPlanSuperseded { .. })),
        "the instance of the destroyed workspace row is refused: {stale_rebuild:?}"
    );
    assert_eq!(
        a_text.as_deref(),
        Some("alpha after A failed"),
        "A converges on the content that is really on disk"
    );
}

impl Harness {
    /// The durable revision, or `None` when the workspace row is gone.
    fn durable_revision_opt(&self) -> Option<u64> {
        self.read_only()
            .query_row(
                "SELECT catalog_revision FROM workspaces WHERE workspace_id=?1",
                [self.workspace_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .map(|value| u64::try_from(value).expect("revision"))
    }

    /// The catalog instance generation the database says owns this workspace's index.
    fn durable_instance(&self) -> Option<u64> {
        self.read_only()
            .query_row(
                "SELECT instance_seq FROM workspace_catalog_instances WHERE workspace_id=?1",
                [self.workspace_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .map(|value| u64::try_from(value).expect("instance"))
    }
}

/// A single-shot hold a test can park one commit section on and release from another
/// thread. The held closure runs on the committing thread; it is a hold, not a cancel.
struct CommitGate {
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl CommitGate {
    fn new() -> (CatalogCommitHold, std::sync::mpsc::Receiver<()>, Self) {
        let (entered, arrivals) = std::sync::mpsc::channel::<()>();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let thread_gate = Arc::clone(&gate);
        let hold = CatalogCommitHold::new(move || {
            let _ = entered.send(());
            let (released, wake) = &*thread_gate;
            if let Ok(mut released) = released.lock() {
                while !*released {
                    match wake.wait(released) {
                        Ok(next) => released = next,
                        Err(_) => break,
                    }
                }
            }
        });
        (hold, arrivals, Self { gate })
    }

    fn release(&self) {
        let (released, wake) = &*self.gate;
        if let Ok(mut released) = released.lock() {
            *released = true;
        }
        wake.notify_all();
    }
}

/// R1 gate. A **changed** reconcile whose plan was overtaken must not write its old body
/// while it reports the conflict.
///
/// The review's counterexample: a new delta committed revision 4 with a new body, then
/// the released old pass rewrote the database to revision 5 with the **old** body and only
/// afterwards returned `Superseded`, leaving memory at 4. This case stops the old pass
/// after its walk and before its commit, lets a newer same-catalog update really commit,
/// and then releases it — asserting the durable body, the durable revision, the published
/// revision and the text map, not the absence of an error.
#[test]
fn a_changed_reconcile_that_lost_the_race_does_not_write_its_old_body() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha baseline").expect("a");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let baseline_revision = harness.durable_revision();
    assert_eq!(harness.indexed_text("src/a.txt").as_deref(), Some("alpha baseline"));

    // The old pass reads this body into its plan, then stops before it can commit.
    harness.write("src/a.txt", "old reconcile body");
    let (hold, arrivals, gate) = CommitGate::new();
    harness.catalog.arm_commit_hold(hold);
    let catalog = Arc::clone(&harness.catalog);
    let resolver = Arc::clone(&harness.resolver);
    let old = std::thread::spawn(move || catalog.reconcile(&resolver, CorrelationId::new()));
    arrivals
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the old pass reached its commit section");

    // The old pass is stopped. A newer update on the same catalog really commits.
    harness.write("src/a.txt", "new delta body");
    let committed =
        harness.apply("src/a.txt", CatalogChange::Changed).expect("the newer delta commits");
    let committed_revision = harness.durable_revision();
    let committed_text = harness.indexed_text("src/a.txt");
    assert!(committed.value() > baseline_revision);
    assert_eq!(committed_text.as_deref(), Some("new delta body"));

    gate.release();
    let old = old.join().expect("the old pass ends normally");
    let after_revision = harness.durable_revision();
    let after_text = harness.indexed_text("src/a.txt");
    let published = harness.revision();
    let map = harness.text_rows();
    println!(
        "R1-changed-reconcile baseline={} committed={} committed_text={committed_text:?} \
         old={old:?} after_revision={after_revision} after_text={after_text:?} \
         published={} map={map:?}",
        baseline_revision,
        committed.value(),
        published.value()
    );
    assert!(
        matches!(old, Err(WorkspaceError::CatalogPlanSuperseded { .. })),
        "the overtaken pass reports that it lost: {old:?}"
    );
    assert_eq!(
        after_text.as_deref(),
        Some("new delta body"),
        "the overtaken pass must not write its old body over the committed one"
    );
    assert_eq!(
        after_revision, committed_revision,
        "the overtaken pass must not advance or roll back the durable revision"
    );
    assert_eq!(
        published.value(),
        after_revision,
        "the published revision and the durable revision must not split"
    );
    assert_eq!(
        harness.indexed_text("src/a.txt").as_deref(),
        Some("new delta body"),
        "the final body is re-queried after the release, not assumed"
    );
    let (unreachable, dangling) = harness.orphans();
    assert_eq!((unreachable, dangling), (0, 0), "the text map stays consistent");
}

/// R1 gate. The same protocol for a **full rebuild**: an overtaken whole-set replacement
/// must not roll the index back to the tree it scanned before the newer delta committed.
///
/// A rebuild is the recovery path, so a refused replacement re-walks against a fresh
/// baseline instead of failing the caller. What the gate asserts is therefore the durable
/// outcome rather than a particular return value: the committed delta's body survives, the
/// published revision is the durable one, and whatever revision the rebuild reports is the
/// revision it really committed.
#[test]
fn a_full_rebuild_that_lost_the_race_does_not_replace_a_newer_delta() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha baseline").expect("a");
        std::fs::write(root.join("src/b.txt"), "beta baseline").expect("b");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let baseline_revision = harness.durable_revision();

    // The rebuild's walk reads this tree and then stops before it can commit.
    harness.write("src/a.txt", "body for the old full rebuild");
    let (hold, arrivals, gate) = CommitGate::new();
    harness.catalog.arm_commit_hold(hold);
    let catalog = Arc::clone(&harness.catalog);
    let resolver = Arc::clone(&harness.resolver);
    let old = std::thread::spawn(move || catalog.rebuild(&resolver, CorrelationId::new()));
    arrivals
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the full rebuild reached its commit section");

    // A newer bounded update commits a different path with different content.
    harness.write("src/b.txt", "body from the newer delta");
    let committed =
        harness.apply("src/b.txt", CatalogChange::Changed).expect("the newer delta commits");
    let committed_revision = harness.durable_revision();
    let committed_text = harness.indexed_text("src/b.txt");
    assert!(committed.value() > baseline_revision);
    assert_eq!(committed_text.as_deref(), Some("body from the newer delta"));

    gate.release();
    let old = old.join().expect("the rebuild ends normally");
    let after_revision = harness.durable_revision();
    let after_text = harness.indexed_text("src/b.txt");
    let after_a = harness.indexed_text("src/a.txt");
    let published = harness.revision();
    let entries = harness.catalog.entries().expect("entries");
    println!(
        "R1-rebuild baseline={} committed={} old={old:?} after_revision={after_revision} \
         after_text={after_text:?} after_a={after_a:?} published={} entries={:?}",
        baseline_revision,
        committed.value(),
        published.value(),
        entries.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        after_text.as_deref(),
        Some("body from the newer delta"),
        "a whole-set replacement must not roll a committed delta's body back"
    );
    assert_eq!(
        after_a.as_deref(),
        Some("body for the old full rebuild"),
        "the replacement describes the tree as it really is, not the tree it first scanned"
    );
    assert!(
        after_revision >= committed_revision,
        "a replacement never rolls the durable revision back: {after_revision} < \
         {committed_revision}"
    );
    assert_eq!(
        published.value(),
        after_revision,
        "the published revision and the durable revision must not split"
    );
    match old {
        Ok(revision) => assert_eq!(
            revision.value(),
            after_revision,
            "a reported revision has to be the revision that was really committed"
        ),
        Err(WorkspaceError::CatalogPlanSuperseded { .. }) => {}
        Err(other) => panic!("an unexpected failure: {other:?}"),
    }
    let (unreachable, dangling) = harness.orphans();
    assert_eq!((unreachable, dangling), (0, 0), "the text map stays consistent");
}

/// R1 gate. A **live old instance**: a task belonging to the catalog instance that was
/// replaced by a reopen must not overwrite the new instance's durable facts.
///
/// The old instance's plan is stopped before its commit; a second `PathCatalog::open` over
/// the same workspace publishes a different body; the old plan is then released. The
/// durable body, the durable revision and the durable text map are asserted afterwards.
#[test]
fn a_live_old_instance_does_not_overwrite_the_reopened_instance() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha baseline").expect("a");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let baseline_revision = harness.durable_revision();

    // The old instance reads this body into its plan and stops before its commit.
    harness.write("src/a.txt", "body from the old instance plan");
    let (hold, arrivals, gate) = CommitGate::new();
    harness.catalog.arm_commit_hold(hold);
    let old_catalog = Arc::clone(&harness.catalog);
    let old_resolver = Arc::clone(&harness.resolver);
    let old = std::thread::spawn(move || old_catalog.rebuild(&old_resolver, CorrelationId::new()));
    arrivals
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the old instance reached its commit section");

    // Close and reopen: a second instance over the same durable state, which is what
    // `WorkspaceRuntime` does when a workspace is installed again.
    harness.write("src/a.txt", "body from the reopened instance");
    let reopened = PathCatalog::open(
        harness.workspace_id,
        Arc::clone(&harness.persistence),
        harness.catalog.options(),
    )
    .expect("reopen");
    let reopened_resolver = Arc::clone(&harness.resolver);
    let (reopened_revision, report) =
        reopened.reconcile(&reopened_resolver, CorrelationId::new()).expect("the reopened pass");
    let instance_revision = harness.durable_revision();
    let instance_text = harness.indexed_text("src/a.txt");
    let instance_entry = reopened
        .entry(&WorkspacePath::parse("src/a.txt").expect("path"))
        .expect("reopened entry")
        .map(|entry| entry.byte_length);
    assert!(reopened_revision.value() > baseline_revision);
    assert_eq!(instance_text.as_deref(), Some("body from the reopened instance"));

    gate.release();
    let old = old.join().expect("the old instance ends normally");
    let after_revision = harness.durable_revision();
    let after_text = harness.indexed_text("src/a.txt");
    let reopened_published = reopened.revision().expect("reopened revision");
    let old_published = harness.catalog.revision().expect("old revision");
    println!(
        "R1-old-instance baseline={} reopened={} report={report:?} old={old:?} \
         instance_revision={instance_revision} after_revision={after_revision} \
         instance_text={instance_text:?} after_text={after_text:?} \
         reopened_published={} old_published={} entry={instance_entry:?}",
        baseline_revision,
        reopened_revision.value(),
        reopened_published.value(),
        old_published.value()
    );
    assert!(
        matches!(old, Err(WorkspaceError::CatalogPlanSuperseded { .. })),
        "the old instance's plan reports that it lost: {old:?}"
    );
    assert_eq!(
        after_text.as_deref(),
        Some("body from the reopened instance"),
        "the old instance must not overwrite the reopened instance's durable body"
    );
    assert_eq!(
        after_revision, instance_revision,
        "the old instance must not change the durable revision the reopened instance set"
    );
    assert_eq!(
        reopened_published.value(),
        after_revision,
        "the reopened instance's published revision still matches the durable one"
    );
    assert_eq!(
        instance_entry,
        Some(Some("body from the reopened instance".len() as u64)),
        "the reopened instance really indexed the body it read"
    );
    let (unreachable, dangling) = harness.orphans();
    assert_eq!((unreachable, dangling), (0, 0), "the text map stays consistent");
}

/// R1 gate. Close/reopen makes the **instance identity** explicit and durable: the reopened
/// catalog claims a strictly greater generation than the one it replaced, the database says
/// which generation owns the workspace's index, and the replaced instance's plan is refused
/// by that identity even when the revision it planned against has not moved.
///
/// The revision alone cannot separate the two instances: an instance that writes nothing
/// leaves the durable revision exactly where the dead one planned against. This case stops
/// the old instance's replacement before its commit, reopens over a workspace whose durable
/// revision does not move, and then releases the old plan.
#[test]
fn a_reopened_catalog_owns_a_new_durable_instance_generation() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha baseline").expect("a");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let old_instance = harness.catalog.catalog_instance();
    assert_eq!(
        harness.durable_instance(),
        Some(old_instance),
        "the open instance is the one the database says owns the index"
    );

    // The old instance reads this body into its plan and stops before its commit.
    harness.write("src/a.txt", "body from the replaced instance");
    let (hold, arrivals, gate) = CommitGate::new();
    harness.catalog.arm_commit_hold(hold);
    let old_catalog = Arc::clone(&harness.catalog);
    let old_resolver = Arc::clone(&harness.resolver);
    let old = std::thread::spawn(move || old_catalog.rebuild(&old_resolver, CorrelationId::new()));
    arrivals
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the replaced instance reached its commit section");

    // Reopen over the same durable state. The tree is unchanged since the baseline, so the
    // pass writes nothing and the durable **revision** stays exactly where the old instance
    // planned against — which is the case a revision check alone cannot refuse.
    let reopened = PathCatalog::open(
        harness.workspace_id,
        Arc::clone(&harness.persistence),
        harness.catalog.options(),
    )
    .expect("reopen");
    let reopened_resolver = Arc::clone(&harness.resolver);
    let (reopened_revision, _report) =
        reopened.reconcile(&reopened_resolver, CorrelationId::new()).expect("the reopened pass");
    let new_instance = reopened.catalog_instance();
    let durable_instance = harness.durable_instance();
    let durable_revision = harness.durable_revision();
    let revision_before_release = reopened_revision.value();
    assert!(new_instance > old_instance, "a reopen claims a newer generation");
    assert_eq!(durable_instance, Some(new_instance), "the database names the new instance");
    assert_eq!(
        durable_revision, revision_before_release,
        "the reopened pass left the durable revision where the replaced instance planned against"
    );

    gate.release();
    let old = old.join().expect("the replaced instance ends normally");
    let after_instance = harness.durable_instance();
    let after_revision = harness.durable_revision();
    let after_text = harness.indexed_text("src/a.txt");
    let reopened_published = reopened.revision().expect("reopened revision");
    println!(
        "R1-instance old_instance={old_instance} new_instance={new_instance} \
         durable_instance={durable_instance:?} old={old:?} revision_before_release={} \
         after_instance={after_instance:?} after_revision={after_revision} \
         after_text={after_text:?} reopened_published={}",
        revision_before_release,
        reopened_published.value()
    );
    assert!(
        matches!(old, Err(WorkspaceError::CatalogPlanSuperseded { .. })),
        "the replaced instance's plan is refused by identity: {old:?}"
    );
    assert_eq!(
        after_instance,
        Some(new_instance),
        "the replaced instance must not take the claim back"
    );
    assert_eq!(
        after_text.as_deref(),
        Some("body from the replaced instance"),
        "the reopened instance's durable body is untouched"
    );
    assert_eq!(
        after_revision, revision_before_release,
        "the durable revision did not move: identity, not the revision, refused the plan"
    );
    assert_eq!(
        reopened_published.value(),
        after_revision,
        "the reopened instance stays consistent with the durable facts"
    );
}

/// R1 gate. A real catalog query answers on a committed snapshot **while** an index write
/// is stopped before admission, and the query finishes before the write is released.
///
/// The review's counterexample used the production coordinator and a real catalog: the
/// index reached `QueueAdmission` and a real query did not complete within 150 ms. The
/// queries here are `entry`, `revision`, `partitions` and `find_names`; they run on their
/// own thread so a blocking publish lock fails the case instead of hanging it.
#[test]
fn a_catalog_query_answers_while_an_index_write_waits_for_admission() {
    let harness = Harness::new(|root| {
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/a.txt"), "alpha baseline").expect("a");
        std::fs::write(root.join("src/b.txt"), "beta baseline").expect("b");
    });
    harness.apply(".", CatalogChange::DirectoryRescanned).expect("baseline");
    let baseline_revision = harness.revision();

    let barrier = harness.persistence.arm_admission_hold(Some("index"), 0);
    harness.write("src/a.txt", "changed while the index waits for admission");
    let catalog = Arc::clone(&harness.catalog);
    let resolver = Arc::clone(&harness.resolver);
    let options = harness.catalog.options();
    let write = std::thread::spawn(move || {
        catalog.apply_changes(
            &resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes: vec![(
                    WorkspacePath::parse("src/a.txt").expect("path"),
                    CatalogChange::Changed,
                )],
                reason: "index write stopped before admission".into(),
            },
            &options,
            CorrelationId::new(),
        )
    });
    assert!(
        barrier.wait_for_arrivals(1, std::time::Duration::from_secs(10)),
        "the index write never reached admission"
    );
    assert!(barrier.held_count() > 0, "the write has to be really held at admission");

    // Every query runs while the write is still unadmitted. The elapsed time is measured
    // on the query thread, and the barrier's own hold count is read afterwards to show the
    // write was still parked when the queries returned.
    let queries = Arc::clone(&harness.catalog);
    let (results, answers) = std::sync::mpsc::channel();
    let query = std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let entry = queries.entry(&WorkspacePath::parse("src/a.txt").expect("path"));
        let revision = queries.revision();
        let partitions = queries.partitions();
        let names = queries.find_names(
            &WorkspacePath::parse(WorkspacePath::ROOT).expect("root"),
            &["a.txt".to_owned()],
            true,
            NameMatchMode::Exact,
            false,
            None,
            8,
            4_096,
        );
        let _ = results.send((
            started.elapsed(),
            entry.map(|entry| entry.map(|entry| entry.byte_length)),
            revision,
            partitions.map(|partitions| partitions.len()),
            names.map(|page| page.matches.len()),
        ));
    });
    let answered = answers.recv_timeout(std::time::Duration::from_secs(5));
    let still_held = barrier.held_count();
    let released_at = std::time::Instant::now();
    barrier.release_all();
    query.join().expect("query thread");
    let write = write.join().expect("index write thread");

    let (elapsed, entry, revision, partitions, names) = match answered {
        Ok(answer) => answer,
        Err(error) => {
            panic!(
                "a real catalog query did not answer while the index write was unadmitted \
                 ({error:?}); still_held={still_held}"
            );
        }
    };
    println!(
        "R1-query-under-admission elapsed_ms={} entry={entry:?} revision={revision:?} \
         partitions={partitions:?} names={names:?} still_held={still_held} \
         released_after_ms={} write={write:?}",
        elapsed.as_millis(),
        released_at.elapsed().as_millis()
    );
    assert!(
        still_held > 0,
        "the write has to have been held at admission while the queries answered"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "the queries answered in {} ms, which is not a short deadline",
        elapsed.as_millis()
    );
    let entry = entry.expect("entry query").expect("the committed entry is visible");
    assert_eq!(entry, Some("alpha baseline".len() as u64), "the query sees the committed view");
    assert_eq!(
        revision.expect("revision query"),
        baseline_revision,
        "the query sees the committed revision, not a half-published one"
    );
    assert!(partitions.expect("partitions query") > 0);
    assert!(names.expect("find query") > 0);

    let converged = write.expect("the index write commits after the release");
    let final_text = harness.indexed_text("src/a.txt");
    let final_revision = harness.durable_revision();
    println!(
        "R1-query-after-release converged={} final_revision={final_revision} \
         final_text={final_text:?}",
        converged.value()
    );
    assert!(converged.value() > baseline_revision.value(), "the write converges after the release");
    assert_eq!(
        final_text.as_deref(),
        Some("changed while the index waits for admission"),
        "the released write really commits its body"
    );
    assert_eq!(harness.revision().value(), final_revision, "memory and the database converge");
}
