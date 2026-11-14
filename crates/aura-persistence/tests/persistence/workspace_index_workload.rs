//! The cost of one incremental index update, and what the index does while a write
//! is already in flight.
//!
//! The incident this suite comes from had two halves that share one persistence
//! writer. An access-control change produced a large incremental batch whose
//! per-path text-index deletes had no index to use, so each one scanned the whole
//! text index; and the ordinary status route queued behind that transaction on the
//! single writer thread and did not answer for nearly thirteen minutes.
//!
//! These tests therefore report absolute work rather than a wall-clock threshold:
//! SQLite's own virtual-machine step count for the writer connection, measured before
//! and after one operation. A change that keeps the same rows but costs a different
//! number of steps is visible, and the growth between two input sizes says whether the
//! cost is linear or quadratic without depending on how busy the machine is.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, WorkspaceId, WorkspacePath};
use aura_persistence::{
    CatalogDeltaRecord, CatalogEntryRecord, CatalogPartitionRecord, PersistenceArtifactCounters,
    PersistenceCoordinator, PersistenceOptions, PersistenceStartup,
};
use aura_workspace_domain::{CatalogEntryKind, CatalogIndexMode, TextEncoding};

/// How many virtual-machine steps one measured operation took.
#[derive(Clone, Copy, Debug)]
struct Work {
    vm_steps: i64,
    changed_rows: i64,
}

/// The reply precedes the writer's statistics publication. Sample only after the
/// isolated fixture's queue has settled, including its executed-job counter.
fn settled_counters(persistence: &PersistenceCoordinator) -> PersistenceArtifactCounters {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let health = persistence.writer_health();
        let counters = persistence.measured_work();
        if health.running == 0
            && health.sync_queued == 0
            && health.index_queued == 0
            && u64::try_from(counters.jobs_executed).ok() == Some(health.executed)
        {
            return counters;
        }
        assert!(
            Instant::now() < deadline,
            "writer did not settle before measurement: {health:?}; {counters:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn start(directory: &std::path::Path) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        directory.join("aura.sqlite3"),
        directory.join("artifacts"),
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

/// A workspace with `files` indexed text files under one partition.
fn workspace_with_text(persistence: &PersistenceCoordinator, files: usize) -> WorkspaceId {
    let workspace_id = WorkspaceId::new();
    let mut workspace = aura_workspace_domain::WorkspaceSummary {
        workspace_id,
        display_name: "text workload".to_owned(),
        selected_root: "C:\\workload".to_owned(),
        canonical_root: "C:\\workload".to_owned(),
        root_identity: None,
        state: aura_workspace_domain::WorkspaceState::Online,
        revision: Revision::INITIAL,
        catalog_freshness: aura_workspace_domain::CatalogFreshness::Fresh,
        authorization_epoch: Revision::INITIAL,
    };
    workspace.revision = Revision::INITIAL;
    persistence.create_workspace(workspace, CorrelationId::new()).expect("workspace");
    let entries =
        (0..files).map(|index| entry(&format!("file{index:05}.txt"), index)).collect::<Vec<_>>();
    persistence
        .replace_workspace_catalog(
            workspace_id,
            entries,
            vec![CatalogPartitionRecord {
                partition_path: WorkspacePath::parse(".").expect("root"),
                file_count: files as u64,
                indexable_text_bytes: (files * 64) as u64,
                index_mode: CatalogIndexMode::FullText,
            }],
            CorrelationId::new(),
        )
        .expect("seed catalog");
    workspace_id
}

fn entry(path: &str, index: usize) -> CatalogEntryRecord {
    CatalogEntryRecord {
        path: WorkspacePath::parse(path).expect("path"),
        parent_path: WorkspacePath::parse(".").expect("root"),
        file_name: path.to_owned(),
        kind: CatalogEntryKind::File,
        partition_path: WorkspacePath::parse(".").expect("root"),
        byte_length: Some(64),
        modified_at: Some(format!("{}", 1_700_000_000_000_u64 + index as u64)),
        content_hash: None,
        text_encoding: Some(TextEncoding::Utf8),
        ignored: false,
        generated: false,
        hidden_from_search: false,
        reparse_tag: None,
        content: Some(format!("synthetic body {index:05} with enough words to tokenize")),
        // This suite measures writer work, not index reuse: the entries carry no
        // change generation, exactly like rows written before that column existed.
        change_usn: None,
        volume_serial: None,
        file_id: None,
    }
}

/// Rewrites `updates` paths of one workspace and reports the writer's work delta.
fn rewrite(
    persistence: &PersistenceCoordinator,
    workspace_id: WorkspaceId,
    updates: usize,
) -> Work {
    rewrite_generation(persistence, workspace_id, updates, 0)
}

/// The same, with the written content named by `generation`.
///
/// Re-committing rows whose text is byte-for-byte what the index already holds is a no-op
/// that changes no row, so a workload that has to *show a cost* has to write something
/// new. Naming the generation is what makes the second and third measurement in a series
/// real writes rather than a measurement of the no-op path.
fn rewrite_generation(
    persistence: &PersistenceCoordinator,
    workspace_id: WorkspaceId,
    updates: usize,
    generation: usize,
) -> Work {
    persistence.measure_work();
    let before = settled_counters(persistence);
    persistence
        .apply_workspace_catalog_delta(
            workspace_id,
            CatalogDeltaRecord {
                upserts: (0..updates)
                    .map(|index| {
                        let mut record = entry(&format!("file{index:05}.txt"), index);
                        if generation > 0 {
                            record.content = Some(format!(
                                "synthetic body {index:05} generation {generation:03} with enough \
                                 words to tokenize"
                            ));
                        }
                        record
                    })
                    .collect(),
                removed_paths: Vec::new(),
                removed_partitions: Vec::new(),
                partitions: Vec::new(),
                repaired_paths: Vec::new(),
                reason: "workload regression".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("delta");
    let work = settled_counters(persistence).since(before);
    if generation > 0 {
        let workspace = workspace_id.to_string();
        let pattern = format!("%generation {generation:03}%");
        let applied: i64 = persistence
            .read(move |connection| {
                Ok(connection.query_row(
                    "SELECT COUNT(*) FROM workspace_content_fts
                     WHERE workspace_id=?1 AND content LIKE ?2",
                    rusqlite::params![workspace, pattern],
                    |row| row.get(0),
                )?)
            })
            .expect("read back the committed text generation");
        assert_eq!(
            usize::try_from(applied).expect("nonnegative row count"),
            updates,
            "the text index must contain every update from generation {generation}"
        );
    }
    Work { vm_steps: work.vm_steps, changed_rows: work.changed_rows }
}

/// The quadratic delete, measured on the real entry point.
///
/// The old statement deleted from the text index by two `UNINDEXED` columns, so the
/// plan was a full scan per path and the total grew with the square of the number of
/// paths the batch touched. The counts below are absolute, so the growth between them
/// — not a machine-dependent millisecond budget — is what the assertion is about.
#[test]
fn incremental_text_updates_do_not_scan_the_whole_index_once_per_path() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let workspace_id = workspace_with_text(&persistence, 4_000);

    let mut measured = BTreeMap::new();
    // Each batch writes content the index does not hold yet, so every measurement is of a
    // real rewrite. Re-committing identical text is a no-op that changes no row, and a
    // workload that measured that would be measuring the no-op path instead.
    for (generation, batch) in [1_000_usize, 2_000, 4_000].into_iter().enumerate() {
        let work = rewrite_generation(&persistence, workspace_id, batch, generation + 1);
        measured.insert(batch, work);
    }

    for (batch, work) in &measured {
        println!("batch={batch} vm_steps={} changed_rows={}", work.vm_steps, work.changed_rows);
    }
    assert!(
        measured.values().all(|work| work.vm_steps > 0),
        "the instruction count has to be positive for the measurement to mean anything: {measured:?}"
    );
    assert!(
        measured.values().all(|work| work.changed_rows > 0),
        "the workload has to have changed something to be a measurement: {measured:?}"
    );
    // Doubling the batch must not multiply the work by anything like four. The factor
    // is asserted loosely on purpose: what is being pinned is the shape of the cost,
    // and a small constant factor must not be able to hide a quadratic one.
    let small = measured[&1_000].vm_steps;
    let large = measured[&4_000].vm_steps;
    assert!(
        large <= small.saturating_mul(5).saturating_add(200_000),
        "four times the paths cost {large} steps against {small} for one quarter of them, \
         which is the quadratic shape the fixed delete removes"
    );
}

/// The cost of rewriting a path that already has a text row, on its own.
///
/// One update of one path must not depend on how many other paths the index holds.
/// Both runs rewrite the same hundred paths and change the same hundred rows, so the
/// only difference between them is the size of the index around them: a per-path full
/// scan shows up as a tenfold difference in work, and an addressed delete does not.
#[test]
fn one_path_update_does_not_depend_on_the_size_of_the_index() {
    let small_directory = tempfile::tempdir().expect("data root");
    let small = start(small_directory.path());
    let small_workspace = workspace_with_text(&small, 4_000);
    let small_work = rewrite(&small, small_workspace, 100);

    let large_directory = tempfile::tempdir().expect("data root");
    let large = start(large_directory.path());
    let large_workspace = workspace_with_text(&large, 40_000);
    let large_work = rewrite(&large, large_workspace, 100);

    println!(
        "one hundred paths: index=4000 vm_steps={} changed_rows={} | index=40000 vm_steps={} \
         changed_rows={}",
        small_work.vm_steps, small_work.changed_rows, large_work.vm_steps, large_work.changed_rows
    );
    assert!(small_work.vm_steps > 0 && large_work.vm_steps > 0);
    // The changed-row counts are reported but deliberately not compared: FTS5 does its
    // own internal maintenance, so the same hundred updates touch a different number of
    // internal rows depending on how much of the index is already merged. That is
    // exactly why the assertion is about instructions — the cost of the statements
    // SQLite chose — and not about how many rows they happened to write.
    assert!(
        large_work.vm_steps <= small_work.vm_steps.saturating_mul(4),
        "the same hundred paths cost {} steps on a ten-times larger index against {} on the \
         smaller one, which means the update walks the index instead of addressing its rows",
        large_work.vm_steps,
        small_work.vm_steps
    );
}

/// The text index and its rowid map agree after a mixed batch: removals, upserts and
/// a partition summary change in one transaction.
#[test]
fn a_mixed_delta_keeps_the_map_and_the_text_index_in_step() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let workspace_id = workspace_with_text(&persistence, 40);

    persistence
        .apply_workspace_catalog_delta(
            workspace_id,
            CatalogDeltaRecord {
                upserts: vec![entry("file00001.txt", 1), entry("brand-new.txt", 99)],
                removed_paths: vec![
                    WorkspacePath::parse("file00002.txt").expect("path"),
                    WorkspacePath::parse("file00003.txt").expect("path"),
                ],
                removed_partitions: Vec::new(),
                partitions: vec![CatalogPartitionRecord {
                    partition_path: WorkspacePath::parse(".").expect("root"),
                    file_count: 40,
                    indexable_text_bytes: 2_560,
                    index_mode: CatalogIndexMode::FullText,
                }],
                repaired_paths: Vec::new(),
                reason: "mixed delta".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("delta");

    let database = rusqlite::Connection::open_with_flags(
        directory.path().join("aura.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read-only database");
    let mapped = database
        .query_row("SELECT COUNT(*) FROM workspace_content_index", [], |row| row.get::<_, i64>(0))
        .expect("map count");
    let addressable = database
        .query_row(
            "SELECT COUNT(*) FROM workspace_content_index AS map
             JOIN workspace_content_fts AS text ON text.rowid = map.fts_rowid",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("join count");
    assert_eq!(mapped, addressable, "every map row must address a text row that exists");
    let orphaned = database
        .query_row(
            "SELECT COUNT(*) FROM workspace_content_fts AS text
             WHERE NOT EXISTS (
                SELECT 1 FROM workspace_content_index AS map WHERE map.fts_rowid = text.rowid
             )",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("orphan count");
    assert_eq!(orphaned, 0, "no text row may be unreachable from the map");
    let removed = database
        .query_row(
            "SELECT COUNT(*) FROM workspace_content_fts
             WHERE content LIKE '%00002%' OR content LIKE '%00003%'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("removed content count");
    assert_eq!(removed, 0, "a removed path must not keep its text");
}

/// A real write held open on the writer thread, while a real read is attempted.
///
/// This is the shape the incident had: the status route did its read and its expiry
/// settlement on the same single writer, so a long catalog transaction delayed it. The
/// test proves the queueing is real — which is what makes "the status route must not
/// need that writer" a verifiable statement instead of an assumption — and that a
/// reader connection is unaffected by the held write.
#[test]
fn a_read_is_not_blocked_by_a_write_that_is_still_running() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let workspace_id = workspace_with_text(&persistence, 50);

    let gate = persistence.arm_dispatch_gate();
    let write_persistence = persistence.clone();
    let writer = std::thread::spawn(move || {
        write_persistence
            .apply_workspace_catalog_delta(
                workspace_id,
                CatalogDeltaRecord {
                    upserts: vec![entry("held.txt", 7)],
                    removed_paths: Vec::new(),
                    removed_partitions: Vec::new(),
                    partitions: Vec::new(),
                    repaired_paths: Vec::new(),
                    reason: "held write".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("held delta");
    });

    assert!(
        gate.wait_until_arrived(std::time::Duration::from_secs(10)),
        "the writer never reached the gate, so no write was in flight to observe"
    );
    // The reader pool is a separate connection, so a read that does not need the writer
    // still answers while the writer is stopped mid-task.
    let workspace = persistence.workspace(workspace_id).expect("workspace read");
    assert_eq!(workspace.workspace_id, workspace_id);

    // A second write does queue: that is the mechanism the status route must not need.
    let queued = persistence.clone();
    let started = std::time::Instant::now();
    let queued_writer = std::thread::spawn(move || {
        queued.apply_workspace_catalog_delta(
            workspace_id,
            CatalogDeltaRecord {
                upserts: vec![entry("queued.txt", 8)],
                removed_paths: Vec::new(),
                removed_partitions: Vec::new(),
                partitions: Vec::new(),
                repaired_paths: Vec::new(),
                reason: "queued write".to_owned(),
            },
            CorrelationId::new(),
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(
        !queued_writer.is_finished(),
        "the second write finished while the first was still held, so nothing was observed"
    );

    gate.release();
    writer.join().expect("held writer");
    queued_writer.join().expect("queued writer").expect("queued delta");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "the queue drained only after the gate was released"
    );
    drop(persistence);
}
