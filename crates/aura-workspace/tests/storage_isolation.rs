//! S03-A: what the storage and index layers really do while they are occupied.
//!
//! Every test here holds a **real** hold point on the production path — the writer
//! thread before it runs a job, the writer thread after a job committed, or a pooled
//! read connection after it was taken out of the pool — and then calls the ordinary
//! production entry points. A sleep helper would only prove that the test waited; the
//! barrier is what makes "this read was served while that write was open" an
//! observation.
//!
//! The first thing each test does is assert the barrier **reached the target layer**.
//! A hold that never happened cannot support any conclusion about what ran beside it.
//!
//! Timing is reported as an observed value, never as a pass condition: the machine may
//! be busy, and a threshold on a shared CI host would be a claim about the host rather
//! than about the code. What the assertions rest on is which call answered, with which
//! value, and in which order.
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use aura_domain_common::{
    CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_persistence::{
    PersistenceCoordinator, PersistenceError, PersistenceOptions, PersistenceStartup,
    barrier::Point,
};
use aura_workspace::{
    CatalogChange, CatalogEntry, CatalogOptions, CatalogUpdate, PathCatalog, SecurePathResolver,
    WorkspaceRuntime, WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{CatalogIndexMode, SessionSummary, WorkspaceCreateCommand};

/// A long enough hold that "did this call wait for it" is unambiguous, and short enough
/// that a failing test still ends quickly. Nothing asserts on this value.
const HOLD: Duration = Duration::from_millis(600);

/// How many index callers are submitted against a writer occupied by a synchronous call.
/// It has to exceed the index lane's capacity comfortably, so the refusals come from
/// capacity rather than from timing.
const SYNC_LANE_FILLERS: usize = 32;
/// How many refusals the admission gate requires. The lane holds
/// `INDEX_QUEUE_CAPACITY` jobs, so anything beyond that is a real refusal.
const LANE_REFUSALS_EXPECTED: usize = 8;

/// A failing concurrency case must not leave the process to time out. If any case is
/// still running well past every internal budget, the process exits with a non-zero code
/// and says so, rather than sitting in a `Drop::join` that never returns. Every hold in
/// this file is released by its own RAII permit or by the barrier's own fallback, so this
/// is a backstop for a bug in the test, not a substitute for releasing anything.
fn watchdog() {
    std::thread::Builder::new()
        .name("storage-isolation-watchdog".to_owned())
        .spawn(|| {
            std::thread::sleep(Duration::from_secs(150));
            eprintln!(
                "storage_isolation watchdog: a case was still running after 150 s, so the \
                 process is exiting instead of hanging in a join"
            );
            std::process::exit(70);
        })
        .expect("watchdog thread");
}

fn coordinator(data: &Path) -> Arc<PersistenceCoordinator> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        data.join("aura.sqlite3"),
        data.join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => Arc::new(value),
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

/// One recorded observation: what was called, what it answered, how long it took.
#[derive(Debug)]
struct Observation {
    label: &'static str,
    elapsed: Duration,
    answer: String,
}

fn observe(
    label: &'static str,
    call: impl FnOnce() -> Result<String, PersistenceError>,
) -> Observation {
    let started = Instant::now();
    let answer = match call() {
        Ok(value) => format!("ok:{value}"),
        Err(error) => format!("err:{error}"),
    };
    Observation { label, elapsed: started.elapsed(), answer }
}

fn report(observations: &[Observation]) {
    let mut lines = BTreeMap::new();
    for observation in observations {
        lines.insert(
            observation.label,
            format!(
                "{} ms={} answer={}",
                observation.label,
                observation.elapsed.as_millis(),
                observation.answer
            ),
        );
    }
    for line in lines.values() {
        println!("{line}");
    }
}

/// One catalog row for the admission-lane fillers: a real, complete entry, so the
/// rebuild they ask for is real work rather than an empty transaction.
fn arena_entry(path: &str) -> aura_persistence::CatalogEntryRecord {
    aura_persistence::CatalogEntryRecord {
        path: WorkspacePath::parse(path).expect("path"),
        parent_path: WorkspacePath::parse(".").expect("root"),
        file_name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        kind: aura_workspace_domain::CatalogEntryKind::File,
        partition_path: WorkspacePath::parse(".").expect("root"),
        byte_length: Some(5),
        modified_at: Some("1700000000000".to_owned()),
        content_hash: None,
        text_encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
        ignored: false,
        generated: false,
        hidden_from_search: false,
        reparse_tag: None,
        content: Some("hello".to_owned()),
        change_usn: None,
        volume_serial: None,
        file_id: None,
    }
}

/// The partition summary that goes with [`arena_entry`].
fn arena_partition() -> aura_persistence::CatalogPartitionRecord {
    aura_persistence::CatalogPartitionRecord {
        partition_path: WorkspacePath::parse(".").expect("root"),
        file_count: 1,
        indexable_text_bytes: 5,
        index_mode: CatalogIndexMode::FullText,
    }
}

struct Harness {
    root: tempfile::TempDir,
    /// Held so the data root outlives the coordinator for the whole test.
    _data: tempfile::TempDir,
    persistence: Arc<PersistenceCoordinator>,
    catalog: Arc<PathCatalog>,
    resolver: Arc<SecurePathResolver>,
    workspace_id: WorkspaceId,
}

impl Harness {
    fn new(files: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        for (path, body) in files {
            let full = root.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("parent");
            }
            std::fs::write(full, body).expect("file");
        }
        let data = tempfile::tempdir().expect("data root");
        let persistence = coordinator(data.path());
        let runtime = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions::with_index_file_limit(64 * 1024 * 1024),
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let created = runtime
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "s03 isolation".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        let workspace_id = created.workspace.workspace_id;
        let current = persistence.workspace(workspace_id).expect("workspace");
        persistence
            .upsert_workspace_session(
                current,
                SessionSummary {
                    session_id: SessionId::new(),
                    workspace_id,
                    title: "s03".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("session");
        let catalog = runtime.catalog(workspace_id).expect("catalog");
        let resolver = runtime.resolver(workspace_id).expect("resolver");
        // Stop the watchers *first* and wait for their threads to end. A live watcher would
        // deliver this test's own filesystem change a second time and race the update under
        // observation: the plan the test holds would be superseded by the watcher's, and the
        // case would be measuring that race instead of the hold it names. Stopping them
        // before the baseline also makes the baseline the last word on what is indexed,
        // instead of one writer among two.
        runtime.stop_watchers().expect("stop watchers");
        drop(runtime);
        // The whole tree is indexed once, so a later single-path change is a real
        // incremental delta rather than "the first time this path was seen".
        catalog
            .apply_changes(
                &resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse(".").expect("root"),
                        CatalogChange::DirectoryRescanned,
                    )],
                    reason: "s03 isolation baseline".into(),
                },
                &catalog.options(),
                CorrelationId::new(),
            )
            .expect("baseline scan");
        eprintln!("A6 baseline report={:?}", catalog.last_update());
        Self { root, _data: data, persistence, catalog, resolver, workspace_id }
    }

    fn write(&self, path: &str, body: &str) {
        std::fs::write(self.root.path().join(path), body).expect("write file");
    }

    fn revision(&self) -> Revision {
        self.catalog.revision().expect("revision")
    }

    #[allow(dead_code)]
    fn entry(&self, path: &str) -> Option<CatalogEntry> {
        self.catalog.entry(&WorkspacePath::parse(path).expect("path")).expect("entry")
    }

    /// Applies one path change, retrying a plan the catalog rejected because another
    /// update committed first.
    ///
    /// `CatalogPlanSuperseded` means the plan was discarded **without writing anything**
    /// and the filesystem facts are still current, so describing the same bounded change
    /// again is the correct response and not a hidden replay of a write: no write
    /// happened the first time. The bound keeps a pathological contention loop from
    /// turning into an unbounded one.
    fn apply_path(&self, path: &str) -> Result<Revision, aura_workspace::WorkspaceError> {
        let mut last = None;
        for _ in 0..16 {
            match self.catalog.apply_changes(
                &self.resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse(path).expect("path"),
                        CatalogChange::Changed,
                    )],
                    reason: "s03 isolation".into(),
                },
                &self.catalog.options(),
                CorrelationId::new(),
            ) {
                Ok(revision) => return Ok(revision),
                Err(error @ aura_workspace::WorkspaceError::CatalogPlanSuperseded { .. }) => {
                    last = Some(error);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        Err(last.expect("a retry that ran at least once has a last error"))
    }
}

/// A5. The writer is held inside a real index write, and every other class of caller is
/// observed against it.
///
/// The hold sits on the writer thread **before** the job runs, so the write has not
/// touched the connection yet — the mark of "the storage layer is occupied by a task
/// that is really in flight". While it is held:
///
/// * a state read on the same database answers (its own read connection);
/// * a second write on the same database does not answer (there is one writer);
/// * a read and a write on an **independent** database both answer.
///
/// The last two are the distinction the brief asks for, and they are asserted together
/// so neither can be satisfied by simply making everything fast.
#[test]
fn a_held_index_write_is_observed_by_reads_writes_and_an_independent_database() {
    watchdog();
    let harness = Harness::new(&[("src/a.txt", "alpha"), ("src/b.txt", "beta")]);
    let independent_root = tempfile::tempdir().expect("independent data root");
    let independent = coordinator(independent_root.path());

    // The gate holds the next writer job before it runs, inside the writer thread and
    // after the job was taken off the queue: "the write was in flight" is then an
    // observation of the coordinator's own state, not an assumption.
    let gate = harness.persistence.arm_dispatch_gate();
    harness.write("src/a.txt", "alpha changed");

    let held_persistence = Arc::clone(&harness.persistence);
    let held_catalog = Arc::clone(&harness.catalog);
    let held_resolver = Arc::clone(&harness.resolver);
    let held = std::thread::spawn(move || {
        let answer = held_catalog.apply_changes(
            &held_resolver,
            CatalogUpdate {
                inputs: 1,
                writes_missing_text: false,
                repaired_paths: Vec::new(),
                changes: vec![(
                    WorkspacePath::parse("src/a.txt").expect("path"),
                    CatalogChange::Changed,
                )],
                reason: "a held index write".into(),
            },
            &held_catalog.options(),
            CorrelationId::new(),
        );
        // The writer's own health is read from the detached path, so "the write was in
        // flight" is a fact about the coordinator and not only about this thread.
        let health = held_persistence.writer_health();
        (answer.map(|revision| revision.value()), health.state)
    });

    // The writer thread has taken the job and is parked inside it: that is the named
    // hold point this case needs, and it is the coordinator's own state.
    assert!(gate.wait_until_arrived(Duration::from_secs(10)), "the writer never picked the job up");
    let parked = harness.persistence.writer_health();
    assert_eq!(
        parked.state,
        aura_persistence::WriterState::Executing,
        "the writer has to be inside the held job: {parked:?}"
    );
    assert_eq!(parked.running, 1, "exactly one job is running: {parked:?}");

    // --- same database, read path: must answer ---
    let same_db_read = observe("same_db_read(workspace)", || {
        harness
            .persistence
            .workspace(harness.workspace_id)
            .map(|workspace| workspace.workspace_id.to_string())
    });

    // --- same database, another write: must not answer while the writer is held ---
    let (queued_tx, queued_rx) = mpsc::channel::<Result<String, PersistenceError>>();
    let queued_persistence = Arc::clone(&harness.persistence);
    let queued_catalog = Arc::clone(&harness.catalog);
    let queued_resolver = Arc::clone(&harness.resolver);
    let queued_started = Instant::now();
    let queued = std::thread::spawn(move || {
        let answer = queued_catalog
            .apply_changes(
                &queued_resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse("src/b.txt").expect("path"),
                        CatalogChange::Changed,
                    )],
                    reason: "a queued index write".into(),
                },
                &queued_catalog.options(),
                CorrelationId::new(),
            )
            .map(|revision| revision.value().to_string())
            .map_err(|error| PersistenceError::InvalidRequest(format!("workspace error: {error}")));
        let _ = queued_tx.send(answer);
        drop(queued_persistence);
    });
    std::thread::sleep(HOLD);
    let queued_answered_while_held = queued_rx.try_recv().is_ok();

    // --- independent database: read and write must both answer ---
    let independent_read = observe("independent_db_read(event_count)", || {
        independent.event_count().map(|count| count.to_string())
    });
    let independent_started = Instant::now();
    let independent_workspace = WorkspaceId::new();
    let independent_create = independent
        .create_workspace(
            aura_workspace_domain::WorkspaceSummary {
                workspace_id: independent_workspace,
                display_name: "independent".to_owned(),
                selected_root: "C:\\independent".to_owned(),
                canonical_root: "C:\\independent".to_owned(),
                root_identity: None,
                state: aura_workspace_domain::WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: aura_workspace_domain::CatalogFreshness::Fresh,
                authorization_epoch: Revision::INITIAL,
            },
            CorrelationId::new(),
        )
        .expect("independent workspace");
    assert!(
        independent_create
            .payload
            .workspaces
            .iter()
            .any(|workspace| workspace.workspace_id == independent_workspace),
        "the independent database has to hold its own workspace"
    );
    let independent_write = independent
        .apply_workspace_catalog_delta(
            independent_workspace,
            aura_persistence::CatalogDeltaRecord {
                upserts: vec![aura_persistence::CatalogEntryRecord {
                    path: WorkspacePath::parse("independent.txt").expect("path"),
                    parent_path: WorkspacePath::parse(".").expect("root"),
                    file_name: "independent.txt".to_owned(),
                    kind: aura_workspace_domain::CatalogEntryKind::File,
                    partition_path: WorkspacePath::parse(".").expect("root"),
                    byte_length: Some(5),
                    modified_at: Some("1700000000000".to_owned()),
                    content_hash: None,
                    text_encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
                    ignored: false,
                    generated: false,
                    hidden_from_search: false,
                    reparse_tag: None,
                    content: Some("hello".to_owned()),
                    change_usn: None,
                    volume_serial: None,
                    file_id: None,
                }],
                removed_paths: Vec::new(),
                removed_partitions: Vec::new(),
                partitions: vec![aura_persistence::CatalogPartitionRecord {
                    partition_path: WorkspacePath::parse(".").expect("root"),
                    file_count: 1,
                    indexable_text_bytes: 5,
                    index_mode: CatalogIndexMode::FullText,
                }],
                repaired_paths: Vec::new(),
                reason: "independent database write".to_owned(),
            },
            CorrelationId::new(),
        )
        .map(|revision| revision.value().to_string());
    let independent_write_elapsed = independent_started.elapsed();

    // Release, then let everything finish. The release is unconditional: a failing
    // assertion above must not leave the writer parked.
    gate.release();
    let held_result = held.join().expect("held thread");
    let queued_result = queued_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the queued write answers once the writer is free");
    queued.join().expect("queued thread");
    let queued_elapsed = queued_started.elapsed();

    assert_eq!(
        same_db_read.answer.as_str(),
        format!("ok:{}", harness.workspace_id).as_str(),
        "a state read on the same database must be served while the writer is held"
    );
    assert!(
        !queued_answered_while_held,
        "a second write answered while the first was still held, so SQLite one-writer \
         serialisation was not actually observed"
    );
    assert!(
        queued_result.is_ok(),
        "the second write must run once the first released the writer: {queued_result:?}"
    );
    assert!(
        independent_read.answer.starts_with("ok:"),
        "an independent database's read must be unaffected: {}",
        independent_read.answer
    );
    assert!(
        independent_write.is_ok(),
        "an independent database's write must be unaffected: {independent_write:?}"
    );
    assert!(held_result.0.is_ok(), "the held write ends normally: {held_result:?}");
    let executed = harness.persistence.measured_work();
    let health = harness.persistence.writer_health();
    eprintln!(
        "A5 observed: held={held_result:?} queued={queued_result:?} queued_elapsed_ms={} \
         independent_write={independent_write:?} executed={executed:?} health={health:?} \
         catalog_revision={}",
        queued_elapsed.as_millis(),
        harness.revision().value()
    );
    assert!(
        executed.jobs_executed >= 2,
        "both writes really executed on the writer: {executed:?} {health:?}"
    );
    let final_revision = harness.revision();
    println!("final_catalog_revision={}", final_revision.value());
    report(&[
        same_db_read,
        independent_read,
        Observation {
            label: "independent_db_write",
            elapsed: independent_write_elapsed,
            answer: match &independent_write {
                Ok(value) => format!("ok:{value}"),
                Err(error) => format!("err:{error}"),
            },
        },
        Observation {
            label: "same_db_second_write",
            elapsed: queued_elapsed,
            answer: match &queued_result {
                Ok(value) => format!("ok:{value}"),
                Err(error) => format!("err:{error}"),
            },
        },
    ]);
    drop(harness);
}

/// A6. The queue and the read pool while they are genuinely occupied.
///
/// Three separate facts, each measured rather than described:
///
/// * a hold on a **read connection** is reached, and while it is held a real read that
///   can pick a free connection still answers, and one confined to a single connection
///   returns bounded instead of waiting forever;
/// * the writer's **admission lane** refuses within a short budget when it is full, and
///   the refusal carries no executed work;
/// * after the holder releases, the same legitimate request works.
#[test]
fn an_occupied_read_connection_and_a_full_admission_lane_answer_bounded() {
    watchdog();
    let harness = Harness::new(&[("src/a.txt", "alpha")]);
    assert!(
        harness.persistence.read_pool_size() >= 2,
        "the pool must have more than one connection"
    );
    eprintln!("A6 step 1: pool size {}", harness.persistence.read_pool_size());

    // --- the read pool: a real connection is held at a named point ---
    //
    // The hold is **one-shot**: the parked read really occupies one pooled connection,
    // and every later read passes the hold point untouched. That is what makes the two
    // calls below about the pool rather than about the hold.
    let read_hold = harness.persistence.arm_read_hold(Point::ReadConnection, 0);
    let held_read_persistence = Arc::clone(&harness.persistence);
    let held_read = std::thread::spawn(move || {
        held_read_persistence.event_count().map(|count| count.to_string())
    });
    assert!(
        read_hold.wait_for_arrivals(1, Duration::from_secs(10)),
        "the barrier never reached a read connection"
    );
    eprintln!("A6 step 2: read hold reached");
    // While one connection is held, a read that may use another still answers: the
    // rotation skips the busy slot instead of waiting on it.
    let rotation_started = Instant::now();
    let pool_rotation = harness
        .persistence
        .read_within(Duration::from_secs(5), |connection| {
            connection.query_row("SELECT 1", [], |row| row.get::<_, i64>(0)).map_err(Into::into)
        })
        .map(|value| value.to_string())
        .map_err(|error| error.to_string());
    let rotation_elapsed = rotation_started.elapsed();
    eprintln!("A6 step 3: rotation read done in {} ms", rotation_elapsed.as_millis());
    // A read under a short budget is answered inside that budget: with three of four
    // connections free it is served at once, and the point of the budget is that it can
    // never become an unbounded wait.
    let started = Instant::now();
    let bounded = harness.persistence.read_within(Duration::from_millis(50), |connection| {
        connection.query_row("SELECT 1", [], |row| row.get::<_, i64>(0)).map_err(Into::into)
    });
    let bounded_elapsed = started.elapsed();
    read_hold.release_all();
    let held_read_result = held_read.join().expect("held read");
    println!(
        "read_hold_arrivals={} pool_rotation={pool_rotation:?} rotation_ms={} bounded={bounded:?} \
         bounded_elapsed_ms={} counters={:?}",
        read_hold.arrivals().len(),
        rotation_elapsed.as_millis(),
        bounded_elapsed.as_millis(),
        harness.persistence.read_pool_counters()
    );
    assert!(held_read_result.is_ok(), "the held read really ran: {held_read_result:?}");
    assert!(
        pool_rotation.as_deref() == Ok("1"),
        "with one connection held, another served the read: {pool_rotation:?}"
    );
    assert!(
        bounded_elapsed < Duration::from_secs(5),
        "a short-budget read has to end inside its budget, not behind the held slot: {} ms",
        bounded_elapsed.as_millis()
    );
    let bounded_answer = bounded.as_ref().map(ToString::to_string).map_err(ToString::to_string);
    assert_eq!(bounded_answer, Ok("1".to_owned()), "the short-budget read is answered");
    let counters = harness.persistence.read_pool_counters();
    assert!(
        counters.served_immediately + counters.waited > 0,
        "the pool has to have served something for the counters to mean anything"
    );

    eprintln!("A6 step 4: bounded read done");
    // --- the writer's admission lane: full means refused, with no executed work ---
    //
    // The lane is filled deterministically rather than by racing a free writer:
    //
    // 1. one real **synchronous** call is parked at the writer's hold point, so the
    //    writer thread is occupied and cannot drain anything;
    // 2. a batch of real **index** calls is submitted. The first `INDEX_QUEUE_CAPACITY`
    //    are admitted and sit in the lane; every later one is refused at once, and the
    //    writer's own `refused_full` counter says so.
    //
    // The synchronous call is used as the holder because the admission hold can be
    // restricted to one class: holding the index class itself would stop the lane from
    // ever filling.
    let barrier = harness.persistence.arm_writer_hold_once(Point::WriterExecution, Some("sync"), 0);
    let before = harness.persistence.measured_work();
    let holder_persistence = Arc::clone(&harness.persistence);
    let holder_workspace = harness.workspace_id;
    let (holder_ready_tx, holder_ready_rx) = mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _ = holder_ready_tx.send(());
        holder_persistence
            .record_path_use(holder_workspace, &WorkspacePath::parse("src/a.txt").expect("path"))
            .map(|()| "recorded".to_owned())
            .map_err(|error| error.to_string())
    });
    holder_ready_rx.recv().expect("the holder thread started");
    assert!(
        barrier.wait_for_arrivals(1, Duration::from_secs(10)),
        "the synchronous holder never reached the writer's hold point: health={:?}",
        harness.persistence.writer_health()
    );
    // The holder is the case the hold matched, so it is parked inside the writer and the
    // writer cannot drain the index lane while the fillers arrive.
    assert!(barrier.held_count() > 0, "the hold has to have held the synchronous caller");
    // The fillers are **rebuilds**: each one really has to take the writer, so every one
    // of them reaches admission. An incremental update whose facts already match is a
    // no-op that never reaches the writer at all, which would measure the catalog's
    // no-op detection instead of the lane's capacity.
    let fillers_entries = vec![arena_entry("src/b.txt")];
    let fillers_partitions = vec![arena_partition()];
    let (filler_tx, filler_rx) = mpsc::channel::<Result<Revision, String>>();
    let mut fillers = Vec::new();
    for _ in 0..(SYNC_LANE_FILLERS) {
        let persistence = Arc::clone(&harness.persistence);
        let workspace = harness.workspace_id;
        let entries = fillers_entries.clone();
        let partitions = fillers_partitions.clone();
        let sender = filler_tx.clone();
        fillers.push(std::thread::spawn(move || {
            let answer = persistence
                .replace_workspace_catalog(workspace, entries, partitions, CorrelationId::new())
                .map_err(|error| error.to_string());
            let _ = sender.send(answer);
        }));
    }
    drop(filler_tx);
    // Two independent records of the same fact: the writer's own refusal counter, and the
    // refusals the real callers really received. Either alone can be missed by a badly
    // timed sample; together they cannot.
    let mut refusals_from_callers = 0_usize;
    let mut refusal_elapsed = Duration::ZERO;
    let mut admitted_early: Vec<Result<Revision, String>> = Vec::new();
    let wait_started = Instant::now();
    while wait_started.elapsed() < Duration::from_secs(30) {
        match filler_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(answer) => {
                if answer.as_ref().is_err_and(|e| e.contains("queue is at capacity")) {
                    refusals_from_callers += 1;
                    if refusal_elapsed.is_zero() {
                        refusal_elapsed = wait_started.elapsed();
                    }
                } else {
                    admitted_early.push(answer);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if harness.persistence.writer_health().refused_full > 0 && refusals_from_callers > 0
                {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if refusals_from_callers >= LANE_REFUSALS_EXPECTED {
            break;
        }
    }
    let at_capacity = harness.persistence.writer_health();
    let after_refusals = harness.persistence.measured_work();
    eprintln!(
        "A6 lane probe: last_update={:?}",
        harness.catalog.last_update().map(|report| (
            report.inputs,
            report.read,
            report.no_op,
            report.upserted,
            report.persisted,
            report.replans
        ))
    );

    // Release before asserting: the holder must never be left parked.
    barrier.release_all();
    let holder_result = holder.join().expect("holder thread");
    let mut admitted = admitted_early;
    for _ in 0..SYNC_LANE_FILLERS {
        match filler_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(answer) => admitted.push(answer),
            Err(_) => break,
        }
    }
    for filler in fillers {
        let _ = filler.join();
    }
    let admitted_ok = admitted.iter().filter(|answer| answer.is_ok()).count();
    let admitted_refused = admitted
        .iter()
        .filter(|answer| answer.as_ref().is_err_and(|e| e.contains("queue is at capacity")))
        .count();
    let after_release_revision =
        harness.apply_path("src/b.txt").expect("the same request after release");
    let after_release = harness.persistence.measured_work();

    println!(
        "admission: refusals_from_callers={refusals_from_callers} admitted_refused={admitted_refused} \
         at_capacity=({:?}, queued={}, capacity={}, refused_full={}) first_refusal_ms={} \
         admitted_ok={admitted_ok} holder={:?} admitted_work={:?} after_release_work={:?} \
         after_release_revision={} refusal_example={:?}",
        at_capacity.state,
        at_capacity.index_queued,
        at_capacity.index_capacity,
        at_capacity.refused_full,
        refusal_elapsed.as_millis(),
        holder_result,
        after_refusals.since(before),
        after_release.since(after_refusals),
        after_release_revision.value(),
        admitted
            .iter()
            .find(|answer| answer.as_ref().is_err_and(|e| e.contains("capacity")))
            .map(|answer| answer.as_ref().err().cloned().unwrap_or_default())
    );
    assert!(holder_result.is_ok(), "the holder write ends normally: {holder_result:?}");
    // (1) The lane really refused, and the refusals came from capacity.
    assert!(
        refusals_from_callers >= LANE_REFUSALS_EXPECTED,
        "the index lane did not refuse enough callers to show back-pressure: \
         {refusals_from_callers} from callers, {admitted_refused} by message, {at_capacity:?}"
    );
    assert!(
        at_capacity.refused_full > 0,
        "the writer's own counter has to agree that it refused: {at_capacity:?}"
    );
    // (2) A refusal is a short bounded answer, not a wait.
    assert!(
        refusal_elapsed < Duration::from_secs(10),
        "the refusal took {} ms, so it was not a short bounded answer",
        refusal_elapsed.as_millis()
    );
    // (3) It is not a permanent data error: the same legitimate request works once the
    //     holder releases, and it really commits.
    assert!(
        admitted_ok > 0,
        "at least one index caller has to be admitted, so the refusals are about capacity \
         rather than about a writer that never ran anything"
    );
    assert!(
        after_release_revision.value() > admitted_ok as u64,
        "the same request has to work after the holder released: revision {}",
        after_release_revision.value()
    );
    assert!(
        after_release.jobs_executed > after_refusals.jobs_executed,
        "the recovered request really ran on the writer: {:?}",
        after_release.since(after_refusals)
    );
    drop(harness);
}

/// A7. The real catalog lock, held through a real production update, and what a
/// production query does while it is held.
///
/// The hold is the writer's post-commit point, which is inside `publish_plan` while the
/// catalog's state write lock is held and after the durable delta committed. That is
/// the exact span the brief asks about: not a self-built `RwLock` replica, but the
/// production lock taken by the production publish step.
///
/// The query under test is a production catalog query (`entries`, `children_page`,
/// `find_names`), and the revision it actually read is recorded.
#[test]
fn a_production_catalog_query_while_the_index_lock_is_held() {
    watchdog();
    let harness = Harness::new(&[("src/a.txt", "alpha"), ("src/b.txt", "beta")]);
    let revision_before = harness.catalog.revision().expect("revision").value();

    // One-shot: the writer parks on the first real publish and every later job passes,
    // so the queries below are measured against a held index lock rather than against
    // the hold itself.
    let barrier = harness.persistence.arm_writer_hold_once(Point::WriterPostCommit, None, 0);
    let held_catalog = Arc::clone(&harness.catalog);
    let held_resolver = Arc::clone(&harness.resolver);
    let held_root = harness.root.path().to_path_buf();
    let (announce_tx, announce_rx) = mpsc::channel::<()>();
    let held = std::thread::spawn(move || {
        // The content is written here, by the very thread that then asks the index to
        // re-read it, so the change this update describes is one this update is the first
        // to see: a watcher that got there first would leave nothing to publish and the
        // hold point would never be reached.
        std::fs::write(held_root.join("src/a.txt"), "alpha changed under the hold")
            .expect("write under the hold");
        let _ = announce_tx.send(());
        held_catalog
            .apply_changes(
                &held_resolver,
                CatalogUpdate {
                    inputs: 1,
                    writes_missing_text: false,
                    repaired_paths: Vec::new(),
                    changes: vec![(
                        WorkspacePath::parse("src/a.txt").expect("path"),
                        CatalogChange::Changed,
                    )],
                    reason: "held publish".into(),
                },
                &held_catalog.options(),
                CorrelationId::new(),
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    announce_rx.recv().expect("the update thread started");
    assert!(
        barrier.wait_for_arrivals(1, Duration::from_secs(15)),
        "the barrier never reached the publish step, so the index lock was never held"
    );
    // The writer is inside the job that took the catalog's state write lock. Whether that
    // job is the one this test submitted or a job that got in just ahead of it, the lock
    // is held at this instant and that is what the queries below are measured against.
    let parked = harness.persistence.writer_health();
    assert_eq!(
        parked.state,
        aura_persistence::WriterState::Executing,
        "a real publish has to be in flight for the lock to be held: {parked:?}"
    );
    assert_eq!(parked.running, 1, "exactly one job is running: {parked:?}");
    // The durable commit of the update that holds the lock has already happened: the
    // revision below is the one the queries really saw, and the whole span they had to
    // queue through is the hold the barrier recorded.
    let started = Instant::now();
    let entries = harness.catalog.entries().expect("entries");
    let query_elapsed = started.elapsed();
    let revision_read = harness.catalog.revision().expect("revision");
    // A bounded page query and a name query, on the same locked catalog.
    let page_started = Instant::now();
    let page = harness
        .catalog
        .children_page(&WorkspacePath::parse("src").expect("path"), None, 10)
        .expect("page");
    let page_elapsed = page_started.elapsed();
    let names_started = Instant::now();
    let names = harness
        .catalog
        .find_names(
            &WorkspacePath::parse(".").expect("root"),
            &["a.txt".to_owned()],
            true,
            aura_workspace::NameMatchMode::Exact,
            false,
            None,
            10,
            1_000,
        )
        .expect("names");
    let names_elapsed = names_started.elapsed();

    barrier.release_all();
    let held_result = held.join().expect("held thread");
    let revision_after = harness.catalog.revision().expect("revision");
    // The hold's duration is published when the held job's permit is dropped, which is
    // when the writer loop finishes that job. Waiting for the writer to leave the job is
    // therefore what makes the measurement below the finished one rather than the
    // placeholder; it is bounded so a stuck writer cannot park this test.
    let settled = Instant::now();
    while settled.elapsed() < Duration::from_secs(10) {
        let health = harness.persistence.writer_health();
        if health.running == 0 && health.state != aura_persistence::WriterState::Executing {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // The hold itself is the writer's own account of how long the publish step was
    // parked: it is a measurement of the window the queries had to queue behind.
    let holds = barrier
        .arrivals()
        .iter()
        .filter(|arrival| arrival.held)
        .map(|arrival| (arrival.hold_micros, arrival.hold_nanos))
        .collect::<Vec<_>>();
    let hold_micros = holds.iter().map(|(micros, _)| *micros).max().unwrap_or_default();
    let hold_span = holds.iter().map(|(_, nanos)| *nanos).max().unwrap_or_default();

    println!(
        "index_lock_hold_ns={hold_span} index_lock_hold_us={hold_micros} query_ms={} entries={} \
         revision_read={} page_ms={} page_rows={} page_revision={} names_ms={} names={} \
         revision_after={}",
        query_elapsed.as_millis(),
        entries.len(),
        revision_read.value(),
        page_elapsed.as_millis(),
        page.0.len(),
        page.1.value(),
        names_elapsed.as_millis(),
        names.matches.len(),
        revision_after.value()
    );
    assert!(held_result.is_ok(), "the held update ends normally: {held_result:?}");
    let reported = harness.catalog.last_update();
    // The held update committed, and the published view moved with it. The durable
    // revision is read back from the database, not from the catalog's memory, so "the
    // write really landed" is the commit's own fact.
    let durable =
        harness.persistence.load_workspace_catalog(harness.workspace_id).expect("durable catalog");
    println!(
        "index_lock_hold_ns={hold_span} index_lock_hold_us={hold_micros} query_ms={} entries={} \
         revision_read={} page_ms={} page_rows={} page_revision={} names_ms={} names={} \
         revision_after={} revision_before={revision_before} durable_revision={} report={reported:?}",
        query_elapsed.as_millis(),
        entries.len(),
        revision_read.value(),
        page_elapsed.as_millis(),
        page.0.len(),
        page.1.value(),
        names_elapsed.as_millis(),
        names.matches.len(),
        revision_after.value(),
        durable.2.value()
    );
    assert!(
        hold_span > 0,
        "at least one publish has to have been held, or the lock was never really occupied: \
         holds={holds:?} arrivals_now={:?}",
        barrier.arrivals()
    );
    assert!(
        revision_after.value() > revision_before,
        "the held update has to advance the catalog revision: before {revision_before} after {}",
        revision_after.value()
    );
    assert!(
        durable.2.value() == revision_after.value(),
        "the published revision has to be the one the commit wrote: durable {} published {}",
        durable.2.value(),
        revision_after.value()
    );
    // The queries were served, and each reports a revision that really was published:
    // either the pre-update one (the query ran before the swap) or the post-update one.
    assert!(!entries.is_empty(), "the index holds the workspace's files");
    assert!(
        revision_read.value() <= revision_after.value(),
        "a query must never report a revision newer than the catalog's"
    );
    assert_eq!(
        page.1, revision_after,
        "the page's own directory version is read under the same lock as its rows"
    );
    assert_eq!(page.0.len(), 2, "the page holds the directory's two children");
    assert_eq!(names.matches.len(), 1, "the name query finds the named file");
    // The content the update wrote under the hold is what the index now holds. This is
    // read out of the durable text index, not out of the catalog's memory, so it asserts
    // the committed state rather than the view of it.
    let indexed_text = harness
        .persistence
        .read_within(Duration::from_secs(5), |connection| {
            connection
                .query_row(
                    "SELECT content FROM workspace_content_fts
                     WHERE workspace_id=?1 AND relative_path=?2",
                    rusqlite::params![harness.workspace_id.to_string(), "src/a.txt"],
                    |row| row.get::<_, String>(0),
                )
                .map_err(Into::into)
        })
        .expect("the durable text row");
    assert_eq!(
        indexed_text, "alpha changed under the hold",
        "the index has to hold the content that really landed on disk"
    );
    drop(harness);
}

/// A8. Source-confirmed reachability of the V2 and Broker transaction paths.
///
/// The brief is explicit that a V1 result must not be extrapolated to V2 or to the
/// Broker ledger. This case records what can be established **from the source** about
/// where those locks are taken and what they cover, and marks the part that no
/// independent measurement here covers as untested. It asserts only the structural
/// facts a reader can re-check in the files it names.
#[test]
fn v2_and_broker_lock_reachability_is_source_confirmed_and_marked_untested() {
    let broker = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../aura-core/src/broker/journal.rs"
    ))
    .expect("broker journal source");
    let core = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../aura-persistence-core/src/lib.rs"
    ))
    .expect("persistence core source");
    let v2 = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../aura-persistence-v2/src/lib.rs"
    ))
    .expect("persistence v2 source");

    // The Broker ledger really is one mutex around one V2Store...
    assert!(
        broker.contains("pub(crate) struct BrokerJournal(\n    Arc<Mutex<V2Store>>,"),
        "the Broker journal ownership fact changed; re-derive it before reusing any conclusion"
    );
    // ...and its mutating entries hold it across the whole unit of work, statement by
    // statement, so authorization/claim/complete accounting is never split.
    assert_eq!(
        broker.matches("self.lock()?.database.unit_of_work").count(),
        3,
        "reserve/claim/complete each take the journal lock around one unit of work"
    );
    assert!(
        core.contains("TransactionBehavior::Immediate"),
        "the unit of work must still open an IMMEDIATE transaction, which is what takes \
         the write lock at BEGIN rather than at the first write"
    );
    assert!(
        core.contains("busy_timeout(Duration::from_secs(5))"),
        "the V2 connection's own SQLite lock wait has to stay a named, bounded value"
    );
    assert!(
        v2.contains("pub struct V2Store") && v2.contains("pub database: DatabaseConnection"),
        "V2Store still owns one DatabaseConnection; a second authority ledger was not created"
    );
    println!(
        "v2_broker_reachability=source_confirmed measurement=not_exercised \
         (this batch holds the V1 coordinator's writer; it does not independently \
          measure the Broker mutex or the V2 IMMEDIATE transaction)"
    );
}
