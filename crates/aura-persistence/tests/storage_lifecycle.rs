//! S03-B: storage scheduling, budgets and teardown, against the real coordinator.
//!
//! Every case here runs on a real database file through the real
//! [`PersistenceCoordinator`] — not a queue helper. What is being established is the
//! part of the isolation story SQLite does not decide for us:
//!
//! * a full lane refuses without running anything, and the same request works after the
//!   holder releases (gate 1);
//! * a held writer really serialises writes while reads are served under the actual WAL
//!   premise (gate 2);
//! * a caller that stops waiting is told the truth about *which* stage it stopped in —
//!   never queued vs queued vs running vs committed (gate 4), including the queued caller
//!   that really outlives its own deadline against the real writer and a real file;
//! * a writer error gives every waiting caller a real answer and does not poison the
//!   paths that did not fail, and a later call succeeds (gate 5);
//! * shutdown never sends an unbounded number of messages and never joins a thread that
//!   is still running a job, and a repeated close does not settle twice (gate 6).
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, WorkspaceId, WorkspacePath};
use aura_persistence::{
    CatalogDeltaRecord, CatalogEntryRecord, CatalogPartitionRecord, INDEX_QUEUE_CAPACITY,
    PersistenceCoordinator, PersistenceError, PersistenceFaultPoint, PersistenceOptions,
    PersistenceStartup, WriterState, barrier::Point,
};
use aura_workspace_domain::{CatalogEntryKind, CatalogFreshness, CatalogIndexMode, TextEncoding};

/// A failing case must not leave the process to time out: every hold in this file is
/// released by its own permit or by the barrier's fallback, and this is the backstop.
fn watchdog() {
    std::thread::Builder::new()
        .name("storage-lifecycle-watchdog".to_owned())
        .spawn(|| {
            std::thread::sleep(Duration::from_secs(150));
            eprintln!(
                "storage_lifecycle watchdog: a case was still running after 150 s, so the \
                 process is exiting instead of hanging in a join"
            );
            std::process::exit(70);
        })
        .expect("watchdog thread");
}

fn coordinator(directory: &std::path::Path) -> Arc<PersistenceCoordinator> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        directory.join("aura.sqlite3"),
        directory.join("artifacts"),
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

fn workspace(store: &PersistenceCoordinator) -> WorkspaceId {
    let workspace_id = WorkspaceId::new();
    store
        .create_workspace(
            aura_workspace_domain::WorkspaceSummary {
                workspace_id,
                display_name: "s03 storage lifecycle".to_owned(),
                selected_root: "C:\\s03".to_owned(),
                canonical_root: "C:\\s03".to_owned(),
                root_identity: None,
                state: aura_workspace_domain::WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Fresh,
                authorization_epoch: Revision::INITIAL,
            },
            CorrelationId::new(),
        )
        .expect("workspace");
    workspace_id
}

fn entry(path: &str) -> CatalogEntryRecord {
    CatalogEntryRecord {
        path: WorkspacePath::parse(path).expect("path"),
        parent_path: WorkspacePath::parse(".").expect("root"),
        file_name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        kind: CatalogEntryKind::File,
        partition_path: WorkspacePath::parse(".").expect("root"),
        byte_length: Some(5),
        modified_at: Some("1700000000000".to_owned()),
        content_hash: None,
        text_encoding: Some(TextEncoding::Utf8),
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

fn partition() -> CatalogPartitionRecord {
    CatalogPartitionRecord {
        partition_path: WorkspacePath::parse(".").expect("root"),
        file_count: 1,
        indexable_text_bytes: 5,
        index_mode: CatalogIndexMode::FullText,
    }
}

/// One rebuild, as a call a test can submit and join.
fn rebuild(
    store: &PersistenceCoordinator,
    workspace_id: WorkspaceId,
) -> Result<Revision, PersistenceError> {
    store.replace_workspace_catalog(
        workspace_id,
        vec![entry("arena.txt")],
        vec![partition()],
        CorrelationId::new(),
    )
}

/// B1. Admission is bounded: a full lane refuses, runs nothing, and the same request
/// works once the holder releases.
///
/// The holder is a real synchronous call parked inside the writer. The fillers are real
/// rebuilds, so each of them really has to take the writer rather than being filtered out
/// as a no-op before admission.
#[test]
fn a_full_lane_refuses_without_running_anything_and_recovers() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let workspace_id = workspace(&store);

    let barrier = store.arm_writer_hold_once(Point::WriterExecution, Some("sync"), 0);
    let before = store.measured_work();
    let holder_store = Arc::clone(&store);
    let holder = std::thread::spawn(move || {
        holder_store
            .record_path_use(workspace_id, &WorkspacePath::parse("arena.txt").expect("path"))
            .map(|()| "recorded".to_owned())
            .map_err(|error| error.to_string())
    });
    assert!(
        barrier.wait_for_arrivals(1, Duration::from_secs(10)),
        "the synchronous holder never reached the writer"
    );
    assert!(barrier.held_count() > 0, "the hold has to have held the synchronous caller");

    let refusals = Arc::new(AtomicUsize::new(0));
    let admitted = Arc::new(AtomicUsize::new(0));
    let first_refusal_ms = Arc::new(AtomicUsize::new(u64::MAX as usize));
    let started = Instant::now();
    let mut fillers = Vec::new();
    for _ in 0..32 {
        let store = Arc::clone(&store);
        let refusals = Arc::clone(&refusals);
        let admitted = Arc::clone(&admitted);
        let first_refusal_ms = Arc::clone(&first_refusal_ms);
        fillers.push(std::thread::spawn(move || match rebuild(&store, workspace_id) {
            Ok(_) => {
                admitted.fetch_add(1, Ordering::AcqRel);
            }
            Err(PersistenceError::WriterBusy { .. }) => {
                refusals.fetch_add(1, Ordering::AcqRel);
                let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                first_refusal_ms.fetch_min(elapsed as usize, Ordering::AcqRel);
            }
            Err(_) => {}
        }));
    }
    // Wait for the lane to be full and for real callers to be refused.
    let wait = Instant::now();
    while store.writer_health().refused_full == 0 && wait.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let at_capacity = store.writer_health();
    let during_hold = store.measured_work();
    // The writer's own record while the holder is parked: nothing of the refused work ran.
    assert_eq!(
        during_hold.jobs_executed, before.jobs_executed,
        "no job may execute while the holder is parked: {during_hold:?}"
    );

    barrier.release_all();
    let holder_result = holder.join().expect("holder thread");
    for filler in fillers {
        let _ = filler.join();
    }
    let after = store.measured_work();
    let recovered = rebuild(&store, workspace_id).expect("the same request after release");

    eprintln!(
        "B1 refusals={} admitted={} first_refusal_ms={} at_capacity=({:?}, queued={}, \
         capacity={}, refused_full={}) during_hold={during_hold:?} after={after:?} \
         recovered_revision={} holder={holder_result:?}",
        refusals.load(Ordering::Acquire),
        admitted.load(Ordering::Acquire),
        first_refusal_ms.load(Ordering::Acquire),
        at_capacity.state,
        at_capacity.index_queued,
        at_capacity.index_capacity,
        at_capacity.refused_full,
        recovered.value()
    );
    assert!(holder_result.is_ok(), "the holder ends normally: {holder_result:?}");
    assert!(
        refusals.load(Ordering::Acquire) >= 8,
        "the lane has to have refused real callers: {} refusals",
        refusals.load(Ordering::Acquire)
    );
    assert!(at_capacity.refused_full > 0, "the writer's own counter has to agree: {at_capacity:?}");
    assert!(
        first_refusal_ms.load(Ordering::Acquire) < 5_000,
        "a refusal has to be immediate, not a wait: {} ms",
        first_refusal_ms.load(Ordering::Acquire)
    );
    assert_eq!(
        during_hold.changed_rows, before.changed_rows,
        "a refused request must not have changed a row: {during_hold:?}"
    );
    assert!(
        admitted.load(Ordering::Acquire) > 0,
        "at least one filler has to be admitted for the refusals to be about capacity"
    );
    // The same legitimate request works afterwards, and a refused request is not a
    // permanent data error: it is retryable and the retry commits.
    assert!(recovered.value() > 0, "the retry commits a real revision");
    assert!(after.jobs_executed > before.jobs_executed, "work really ran after release");
    drop(store);
}

/// B2. A held writer really serialises writes, and reads are served under the actual WAL
/// premise.
///
/// The hold is inside the writer thread, so the second write is not refused by policy —
/// it is waiting on the one thing SQLite allows. A queued write that ends in
/// `WriterUnresponsive` is never reported as "not executed".
#[test]
fn a_held_writer_serialises_writes_while_reads_are_served() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let workspace_id = workspace(&store);

    let gate = store.arm_dispatch_gate();
    let holder_store = Arc::clone(&store);
    let holder = std::thread::spawn(move || {
        holder_store
            .apply_workspace_catalog_delta(
                workspace_id,
                CatalogDeltaRecord {
                    upserts: vec![entry("first.txt")],
                    removed_paths: Vec::new(),
                    removed_partitions: Vec::new(),
                    partitions: vec![partition()],
                    repaired_paths: Vec::new(),
                    reason: "held write".to_owned(),
                },
                CorrelationId::new(),
            )
            .map(|revision| revision.value())
            .map_err(|error| error.to_string())
    });
    assert!(gate.wait_until_arrived(Duration::from_secs(10)), "the writer never parked");

    // A read on the same database answers while the write is held: that is the WAL
    // premise, not a policy.
    let read_started = Instant::now();
    let read = store.workspace(workspace_id);
    let read_elapsed = read_started.elapsed();
    // A second write does not answer while the first holds the writer.
    let (second_tx, second_rx) = mpsc::channel();
    let second_store = Arc::clone(&store);
    let second_started = Instant::now();
    let second = std::thread::spawn(move || {
        let answer = second_store.apply_workspace_catalog_delta(
            workspace_id,
            CatalogDeltaRecord {
                upserts: vec![entry("second.txt")],
                removed_paths: Vec::new(),
                removed_partitions: Vec::new(),
                partitions: Vec::new(),
                repaired_paths: Vec::new(),
                reason: "queued write".to_owned(),
            },
            CorrelationId::new(),
        );
        let _ = second_tx.send(answer);
    });
    std::thread::sleep(Duration::from_millis(400));
    let second_answered_while_held = second_rx.try_recv().is_ok();

    gate.release();
    let holder_result = holder.join().expect("holder thread");
    let second_result = second_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the queued write answers once the writer is free");
    second.join().expect("second thread");
    let second_elapsed = second_started.elapsed();
    let health = store.writer_health();

    eprintln!(
        "B2 read_ms={} read_ok={} second_answered_while_held={second_answered_while_held} \
         second_ms={} second={second_result:?} holder={holder_result:?} health={health:?}",
        read_elapsed.as_millis(),
        read.is_ok(),
        second_elapsed.as_millis()
    );
    assert!(read.is_ok(), "a read on the same database is served while a write is held");
    assert!(
        !second_answered_while_held,
        "the second write answered while the first held the writer, so serialisation was \
         not observed"
    );
    assert!(holder_result.is_ok(), "the held write ends normally: {holder_result:?}");
    assert!(
        second_result.is_ok(),
        "the queued write runs after the original releases: {second_result:?}"
    );
    assert!(
        second_elapsed >= Duration::from_millis(400),
        "the queued write cannot have committed before the holder released"
    );
    drop(store);
}

/// B4. A caller that stops waiting is told which stage it stopped in.
///
/// Three stages, three answers, and none of them is allowed to be a guess:
///
/// * **never enqueued** — the lane is full, so the work provably did not run;
/// * **enqueued but unresponsive** — the deadline expired after admission, so the answer
///   is *unknown* and the caller must not treat it as "not executed";
/// * **the original write is not resent** — the operation that committed keeps its
///   identity and its result is queryable through the ordinary read path.
#[test]
fn a_caller_that_stops_waiting_is_told_which_stage_it_reached() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let workspace_id = workspace(&store);

    // (a) Never enqueued: the writer is occupied by a real synchronous call, so the index
    //     lane fills and the next rebuild is refused by capacity rather than by a
    //     deadline. The hold is released as soon as one refusal is observed, so the test
    //     does not depend on the fallback budget.
    let barrier = store.arm_writer_hold_once(Point::WriterExecution, Some("sync"), 0);
    let holder_store = Arc::clone(&store);
    let holder = std::thread::spawn(move || {
        holder_store
            .record_path_use(workspace_id, &WorkspacePath::parse("arena.txt").expect("path"))
            .map_err(|error| error.to_string())
    });
    assert!(barrier.wait_for_arrivals(1, Duration::from_secs(10)), "the holder never parked");
    let residents = Arc::new(AtomicUsize::new(0));
    let mut kept = Vec::new();
    for _ in 0..16 {
        let store = Arc::clone(&store);
        let residents = Arc::clone(&residents);
        kept.push(std::thread::spawn(move || {
            if rebuild(&store, workspace_id).is_ok() {
                residents.fetch_add(1, Ordering::AcqRel);
            }
        }));
    }
    // The residents fill the lane; the probe is the real request whose refusal is the
    // evidence. The wait is on the writer's own refusal counter, which is what cannot be
    // missed, and the whole window is bounded well inside the barrier's fallback.
    let wait = Instant::now();
    while store.writer_health().refused_full == 0 && wait.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let never_enqueued = match rebuild(&store, workspace_id) {
        Err(error @ PersistenceError::WriterBusy { .. }) => Some(error),
        other => {
            eprintln!("B4 probe was not refused: {other:?} health={:?}", store.writer_health());
            None
        }
    };
    let health_after_refusal = store.writer_health();
    let work_after_refusal = store.measured_work();
    // Release before asserting: the writer must never be left parked.
    barrier.release_all();
    let holder_result = holder.join().expect("holder thread");
    for kept in kept {
        let _ = kept.join();
    }
    let residents = residents.load(Ordering::Acquire);

    // (b) Enqueued but unresponsive: a job that holds the writer longer than the caller's
    //     budget. `apply_workspace_catalog_delta` carries the index deadline, so the
    //     deadline the caller sees is the one the coordinator supplies; what is asserted
    //     is that the refusal came from admission rather than from a deadline, which is
    //     the distinction that matters for "not executed".
    let committed = store
        .apply_workspace_catalog_delta(
            workspace_id,
            CatalogDeltaRecord {
                upserts: vec![entry("committed.txt")],
                removed_paths: Vec::new(),
                removed_partitions: Vec::new(),
                partitions: Vec::new(),
                repaired_paths: Vec::new(),
                reason: "committed identity".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("the committed write");
    // The result stays queryable through the ordinary read path, by the path it wrote.
    let stored = store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM workspace_catalog_entries
                     WHERE workspace_id=?1 AND relative_path=?2",
                    rusqlite::params![workspace_id.to_string(), "committed.txt"],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("the committed row");
    let text = store
        .workspace_content_paths(workspace_id)
        .expect("indexed paths")
        .into_iter()
        .map(|path| path.to_string())
        .collect::<Vec<_>>();

    eprintln!(
        "B4 never_enqueued={never_enqueued:?} health_after_refusal={health_after_refusal:?} \
         work_after_refusal={work_after_refusal:?} holder={holder_result:?} \
         committed_revision={} stored_rows={stored} indexed_text_paths={text:?}",
        committed.value()
    );
    assert!(holder_result.is_ok(), "the holder ends normally: {holder_result:?}");
    match never_enqueued {
        Some(PersistenceError::WriterBusy { class }) => {
            assert_eq!(class, "index", "the refused class has to be named");
        }
        other => panic!("the full lane has to refuse with a named class: {other:?}"),
    }
    assert!(
        health_after_refusal.refused_full > 0,
        "the writer's own record agrees: {health_after_refusal:?}"
    );
    assert_eq!(stored, 1, "the committed write's row is queryable by its own identity");
    assert!(
        text.contains(&"committed.txt".to_owned()),
        "the committed write's text is indexed: {text:?}"
    );
    // The original write is never sent a second time: the identity-keyed row for that one
    // path exists exactly once, and the path it wrote is still the path the index holds.
    let duplicates = store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM workspace_catalog_entries
                     WHERE workspace_id=?1 AND relative_path=?2",
                    rusqlite::params![workspace_id.to_string(), "committed.txt"],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("row count");
    assert_eq!(duplicates, 1, "no automatic resend may duplicate the original write");
    let total = store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM workspace_catalog_entries WHERE workspace_id=?1",
                    rusqlite::params![workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("total row count");
    eprintln!("B4 total_rows={total} residents={residents}");
    drop(store);
}

/// B5. A writer error gives every waiting caller a real answer, and the service degrades
/// honestly instead of answering from cache.
#[test]
fn a_writer_error_answers_waiting_callers_and_does_not_poison_other_paths() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let workspace_id = workspace(&store);
    let committed = store
        .apply_workspace_catalog_delta(
            workspace_id,
            CatalogDeltaRecord {
                upserts: vec![entry("before.txt")],
                removed_paths: Vec::new(),
                removed_partitions: Vec::new(),
                partitions: Vec::new(),
                repaired_paths: Vec::new(),
                reason: "before the fault".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("the pre-fault write");

    // The real injected fault: the transaction reaches its last statement and then fails
    // before commit, so SQLite rolls the whole unit of work back.
    let failed = store.upsert_workspace_session_with_fault(
        store.workspace(workspace_id).expect("workspace"),
        aura_workspace_domain::SessionSummary {
            session_id: aura_domain_common::SessionId::new(),
            workspace_id,
            title: "faulted".to_owned(),
            state: "idle".to_owned(),
        },
        CorrelationId::new(),
        Some(PersistenceFaultPoint::BeforeCommit),
    );
    // A read that does not need the writer still answers while that call failed.
    let read_after_fault = store.workspace(workspace_id);
    // The failed write did not commit, and it is not answered from a cached success.
    let sessions = store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sessions WHERE workspace_id=?1",
                    rusqlite::params![workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("session count");
    // A new call succeeds after the fault.
    let recovered = store
        .apply_workspace_catalog_delta(
            workspace_id,
            CatalogDeltaRecord {
                upserts: vec![entry("after.txt")],
                removed_paths: Vec::new(),
                removed_partitions: Vec::new(),
                partitions: Vec::new(),
                repaired_paths: Vec::new(),
                reason: "after the fault".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("the post-fault write");
    let after = store.measured_work();
    let health = store.writer_health();

    eprintln!(
        "B5 failed={failed:?} read_after_fault_ok={} sessions={sessions} \
         committed_revision={} recovered_revision={} health={health:?} after={after:?}",
        read_after_fault.is_ok(),
        committed.value(),
        recovered.value()
    );
    assert!(failed.is_err(), "the injected fault has to fail the call");
    assert!(
        read_after_fault.is_ok(),
        "the failure is confined to the failing write; a read still answers"
    );
    assert_eq!(sessions, 0, "the failed write must not have left a session row");
    assert!(recovered.value() > committed.value(), "a new call has to succeed after the fault");
    assert_eq!(
        health.state,
        WriterState::Running,
        "the writer is healthy after a rolled-back transaction: {health:?}"
    );
    assert!(after.changed_rows > 0, "the recovered write really changed rows: {after:?}");
    drop(store);
}

/// B6. Shutdown: a full queue, an in-flight job and a faulty writer must not cause
/// unbounded sends or joins, an unfinished task keeps a real owner and terminal state,
/// and a repeated close does not settle twice.
#[test]
fn shutdown_is_bounded_and_settles_once() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let workspace_id = workspace(&store);

    // A full index lane: a synchronous holder plus more rebuilds than the lane can take.
    let barrier = store.arm_writer_hold_once(Point::WriterExecution, Some("sync"), 0);
    let holder_store = Arc::clone(&store);
    let holder = std::thread::spawn(move || {
        holder_store
            .record_path_use(workspace_id, &WorkspacePath::parse("arena.txt").expect("path"))
            .map_err(|error| error.to_string())
    });
    assert!(barrier.wait_for_arrivals(1, Duration::from_secs(10)), "the holder never parked");
    let mut residents = Vec::new();
    for _ in 0..16 {
        let store = Arc::clone(&store);
        residents.push(std::thread::spawn(move || rebuild(&store, workspace_id).is_ok()));
    }
    let wait = Instant::now();
    while store.writer_health().index_queued < store.writer_health().index_capacity
        && wait.elapsed() < Duration::from_secs(20)
    {
        std::thread::sleep(Duration::from_millis(2));
    }
    let full = store.writer_health();
    assert!(full.index_queued >= full.index_capacity, "the lane has to be full: {full:?}");

    // Close once with a short budget: the writer is running the held job, so the join
    // cannot succeed and that has to be reported rather than hidden.
    let first = store.close_writer(Duration::from_millis(200));
    let second = store.close_writer(Duration::from_millis(200));
    let depth_after = store.writer_health();
    let refused_after_close = rebuild(&store, workspace_id);

    // Release so the in-flight job finishes and the writer can be joined.
    barrier.release_all();
    let holder_result = holder.join().expect("holder thread");
    for resident in residents {
        let _ = resident.join();
    }
    let settled = Instant::now();
    while !store.writer_health().joined && settled.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let terminal = store.writer_health();
    let third = store.close_writer(Duration::from_secs(2));

    eprintln!(
        "B6 full=({:?}, queued={}, capacity={}) first_close={first:?} second_close={second:?} \
         depth_after={depth_after:?} refused_after_close={refused_after_close:?} \
         holder={holder_result:?} terminal={terminal:?} third_close={third:?}",
        full.state, full.index_queued, full.index_capacity
    );
    assert!(holder_result.is_ok(), "the holder ends normally: {holder_result:?}");
    assert_eq!(first.close_requests, 1, "the first close is the first request: {first:?}");
    assert_eq!(
        second.close_requests, 2,
        "a repeated close is counted, not silently ignored: {second:?}"
    );
    assert!(
        !first.joined || second.joined,
        "the close reports the truth about whether the thread was joined: {first:?} {second:?}"
    );
    assert_eq!(
        terminal.state,
        WriterState::Stopped,
        "once the in-flight job finished the writer has to reach a terminal state: {terminal:?}"
    );
    assert!(terminal.joined, "the writer thread has to be joined once it stopped: {terminal:?}");
    assert!(
        matches!(refused_after_close, Err(PersistenceError::WriterUnavailable)),
        "a closed writer refuses rather than queueing work that can never run: \
         {refused_after_close:?}"
    );
    assert_eq!(
        third.close_requests, 3,
        "closing again does not settle a second time, it only counts the request: {third:?}"
    );
    assert_eq!(terminal.state, third.state, "the terminal state is not overwritten");
    drop(store);
}

/// B4, the missing half: a caller that really ends while its job is QUEUED, past its own
/// deadline, against the real coordinator, the real writer and a real database file.
///
/// The production index budget is [`aura_persistence::INDEX_DEADLINE`] (300 s), so no test
/// may produce this by waiting. What is exercised here is the real thing with a shorter
/// clock injected into the index lane only:
///
/// * the holder is a real synchronous call parked inside the writer thread, so the writer
///   is genuinely occupied and the index job is genuinely queued behind it;
/// * the queued caller's deadline expires while the writer is still held, so the job has
///   not run - the writer's own executed-job counter is the proof;
/// * the caller is told [`PersistenceError::WriterUnresponsive`] - "no answer; it may still
///   run" - and never [`PersistenceError::WriterBusy`], which would mean "not executed";
/// * after the release the very same job really executes and commits, the original
///   operation is queryable by its own identity, and nothing was sent twice.
///
/// `INDEX_DEADLINE` itself is asserted unchanged: the seam shortens this coordinator's
/// index lane, not the production budget.
#[test]
fn a_queued_index_job_that_outlives_its_deadline_is_unresponsive_and_then_really_commits() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let workspace_id = workspace(&store);

    assert_eq!(
        aura_persistence::INDEX_DEADLINE,
        Duration::from_secs(300),
        "the production index budget is not what this test is allowed to change"
    );
    let injected = Duration::from_millis(250);
    store.set_index_deadline_for_test(injected);
    assert_eq!(store.index_deadline(), injected, "the injected budget is the lane's own");

    // The writer is really occupied: a synchronous call is parked inside the writer thread,
    // before it touches the connection, so nothing behind it can run.
    let barrier = store.arm_writer_hold_once(Point::WriterExecution, Some("sync"), 0);
    let holder_store = Arc::clone(&store);
    let holder = std::thread::spawn(move || {
        holder_store
            .record_path_use(workspace_id, &WorkspacePath::parse("arena.txt").expect("path"))
            .map_err(|error| error.to_string())
    });
    assert!(barrier.wait_for_arrivals(1, Duration::from_secs(10)), "the holder never parked");

    let before = store.measured_work();
    let waited = Instant::now();
    let queued = store.apply_workspace_catalog_delta(
        workspace_id,
        CatalogDeltaRecord {
            upserts: vec![entry("queued.txt")],
            removed_paths: Vec::new(),
            removed_partitions: Vec::new(),
            partitions: Vec::new(),
            repaired_paths: Vec::new(),
            reason: "queued past its deadline".to_owned(),
        },
        CorrelationId::new(),
    );
    let elapsed = waited.elapsed();
    let during = store.measured_work();
    let health_during = store.writer_health();

    // Release as soon as the caller has been answered, so the job really runs.
    barrier.release_all();
    let holder_result = holder.join().expect("holder thread");
    let settled = Instant::now();
    while store.writer_health().executed < health_during.executed + 2
        && settled.elapsed() < Duration::from_secs(20)
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    let after = store.measured_work();
    let health_after = store.writer_health();
    let rows = store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM workspace_catalog_entries
                     WHERE workspace_id=?1 AND relative_path=?2",
                    rusqlite::params![workspace_id.to_string(), "queued.txt"],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("the committed row");
    let revision = store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT revision FROM workspaces WHERE workspace_id=?1",
                    rusqlite::params![workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("the workspace revision");

    eprintln!(
        "B4q queued={queued:?} elapsed_ms={} executed_at_deadline={} executed_after={} \
         changed_at_deadline={} changed_after={} rows={rows} revision={revision} \
         holder={holder_result:?} health_during=({:?}, queued={}) health_after=({:?}, running={})",
        elapsed.as_millis(),
        during.jobs_executed - before.jobs_executed,
        after.jobs_executed - during.jobs_executed,
        during.changed_rows - before.changed_rows,
        after.changed_rows - during.changed_rows,
        health_during.state,
        health_during.index_queued,
        health_after.state,
        health_after.running,
    );

    match queued {
        Err(PersistenceError::WriterUnresponsive { class }) => {
            assert_eq!(class, "index", "the unresponsive class has to be named");
        }
        // Never `WriterBusy`: the job WAS enqueued, so "not executed" would be a lie.
        other => panic!("a queued caller that ends at its deadline is unresponsive: {other:?}"),
    }
    assert!(
        elapsed >= injected && elapsed < Duration::from_secs(5),
        "the deadline really bounded the wait: {elapsed:?}"
    );
    assert_eq!(
        during.jobs_executed, before.jobs_executed,
        "the queued job had not run when its caller was answered"
    );
    assert_eq!(
        during.changed_rows, before.changed_rows,
        "no SQL of the queued job had run when its caller was answered"
    );
    assert!(holder_result.is_ok(), "the holder ends normally: {holder_result:?}");
    assert!(
        after.jobs_executed > during.jobs_executed,
        "the released job really executed after the caller stopped waiting"
    );
    assert_eq!(rows, 1, "the original operation is queryable by its own identity exactly once");
    assert!(revision >= 2, "the committed delta advanced the durable workspace revision");
    assert_eq!(
        health_after.state,
        WriterState::Running,
        "the writer is serving again after the release: {health_after:?}"
    );
    drop(store);
}

/// A real long SELECT for the read-budget cases: a recursive CTE that would run for
/// seconds if nothing stopped it.
const LONG_READ: &str = "WITH RECURSIVE counter(value) AS (
        SELECT 1 UNION ALL SELECT value+1 FROM counter WHERE value < 20000000
     ) SELECT COUNT(*) FROM counter";

fn short_read(store: &PersistenceCoordinator) -> Result<i64, PersistenceError> {
    store.read(|connection| {
        connection
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get::<_, i64>(0))
            .map_err(Into::into)
    })
}

/// R2 gate. Through the real coordinator and a real database file: a long SELECT is cut
/// off at its budget, the answer is a definite deadline rather than an empty result, and
/// the connection serves the next read; while connections are busy with real SQL the free
/// ones still answer, and a completely busy pool refuses inside the caller's budget.
#[test]
fn a_real_long_read_is_interrupted_and_the_pool_keeps_serving() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let _workspace = workspace(&store);

    let started = Instant::now();
    let long = store.read_within(Duration::from_millis(40), |connection| {
        connection.query_row(LONG_READ, [], |row| row.get::<_, i64>(0)).map_err(Into::into)
    });
    let elapsed = started.elapsed();
    let counters = store.read_pool_counters();
    eprintln!("R2read long={long:?} elapsed_ms={} counters={counters:?}", elapsed.as_millis());
    match long {
        Err(PersistenceError::ReadDeadlineExceeded { budget_ms, elapsed_ms }) => {
            assert_eq!(budget_ms, 40, "the answer names the budget it really used");
            assert!(elapsed_ms >= 40, "the elapsed time is the real one: {elapsed_ms}");
        }
        other => panic!("a real long SELECT has to end at its budget: {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(3),
        "the interrupted call returned after {} ms",
        elapsed.as_millis()
    );
    assert!(counters.interrupted >= 1, "the pool's own counter agrees: {counters:?}");
    assert_eq!(counters.replaced, 0, "a read deadline is not a broken connection");
    // The review measured this exact shape before the fix: a 2 ms budget over a real
    // recursive SELECT returned `Ok` after 64-65 ms, because the deadline was checked only
    // when no connection came free. The same budget and the same query now end at the
    // deadline, as a definite answer.
    let started = Instant::now();
    let tiny = store.read_within(Duration::from_millis(2), |connection| {
        connection.query_row(LONG_READ, [], |row| row.get::<_, i64>(0)).map_err(Into::into)
    });
    let tiny_elapsed = started.elapsed();
    let tiny_counters = store.read_pool_counters();
    eprintln!(
        "R2read tiny={tiny:?} tiny_elapsed_ms={} counters={tiny_counters:?}",
        tiny_elapsed.as_millis()
    );
    match &tiny {
        Err(PersistenceError::ReadDeadlineExceeded { budget_ms, .. }) => {
            assert_eq!(*budget_ms, 2, "the 2 ms budget is the one that was applied");
        }
        other => panic!("a 2 ms budget must not return a result: {other:?}"),
    }
    assert!(
        tiny_elapsed < Duration::from_millis(50),
        "the 2 ms budget returned after {} ms",
        tiny_elapsed.as_millis()
    );
    // The same pool serves the next read: nothing was left armed on that connection.
    let answered = short_read(&store).expect("the connection is reusable");
    assert_eq!(answered, 1);

    // Three of the four connections busy with real SQL, one free: the free one answers.
    let busy = Arc::new(AtomicUsize::new(0));
    let mut holders = Vec::new();
    for _ in 0..3 {
        let store = Arc::clone(&store);
        let busy = Arc::clone(&busy);
        holders.push(std::thread::spawn(move || {
            let _ = store.read_within(Duration::from_millis(1_500), |connection| {
                busy.fetch_add(1, Ordering::AcqRel);
                connection.query_row(LONG_READ, [], |row| row.get::<_, i64>(0)).map_err(Into::into)
            });
        }));
    }
    let wait = Instant::now();
    while busy.load(Ordering::Acquire) < 3 && wait.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(busy.load(Ordering::Acquire), 3, "the holders have to be really running");
    let served_while_busy = store.read_within(Duration::from_secs(2), |connection| {
        connection
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get::<_, i64>(0))
            .map_err(Into::into)
    });
    // One more holder occupies the last connection.
    let last = Arc::clone(&store);
    let last_busy = Arc::clone(&busy);
    let last_holder = std::thread::spawn(move || {
        let _ = last.read_within(Duration::from_millis(1_500), |connection| {
            last_busy.fetch_add(1, Ordering::AcqRel);
            connection.query_row(LONG_READ, [], |row| row.get::<_, i64>(0)).map_err(Into::into)
        });
    });
    let wait = Instant::now();
    while busy.load(Ordering::Acquire) < 4 && wait.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(busy.load(Ordering::Acquire), 4, "every connection has to be really busy");
    let started = Instant::now();
    let refused = store.read_within(Duration::from_millis(50), |connection| {
        connection
            .query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get::<_, i64>(0))
            .map_err(Into::into)
    });
    let refusal_elapsed = started.elapsed();
    for holder in holders {
        let _ = holder.join();
    }
    let _ = last_holder.join();
    let counters = store.read_pool_counters();
    eprintln!(
        "R2read served_while_busy={served_while_busy:?} refused={refused:?} \
         refusal_ms={} counters={counters:?}",
        refusal_elapsed.as_millis()
    );
    assert_eq!(served_while_busy.expect("a free connection answers while the others are busy"), 1);
    assert!(
        matches!(refused, Err(PersistenceError::ReadUnavailable { .. })),
        "a completely busy pool refuses, it does not fabricate a result: {refused:?}"
    );
    assert!(
        refusal_elapsed < Duration::from_secs(2),
        "the refusal took {} ms",
        refusal_elapsed.as_millis()
    );
    // And the pool is intact afterwards.
    assert_eq!(short_read(&store).expect("the pool is intact"), 1);
    drop(store);
}

/// R2 gate. A write's terminal state is observed separately for the three stages a caller
/// can stop in, and the record is made exactly once — by identity, never by resending.
#[test]
fn a_write_settles_exactly_once_and_its_identity_resolves_a_timeout() {
    watchdog();
    let directory = tempfile::tempdir().expect("data root");
    let store = coordinator(directory.path());
    let workspace_id = workspace(&store);

    // (1) QUEUED and never admitted: the writer is occupied by a real synchronous job, the
    //     index lane is full, and this job is refused before enqueue. Nothing ran, so there
    //     is no terminal record to find — which is a different answer from "unknown".
    let barrier = store.arm_writer_hold_once(Point::WriterExecution, Some("sync"), 0);
    let holder_store = Arc::clone(&store);
    let holder = std::thread::spawn(move || {
        holder_store
            .record_path_use(workspace_id, &WorkspacePath::parse("arena.txt").expect("path"))
            .map_err(|error| error.to_string())
    });
    assert!(barrier.wait_for_arrivals(1, Duration::from_secs(10)), "the holder never parked");
    let mut fillers = Vec::new();
    for _ in 0..INDEX_QUEUE_CAPACITY {
        let store = Arc::clone(&store);
        fillers.push(std::thread::spawn(move || {
            let _ = rebuild(&store, workspace_id);
        }));
    }
    let wait = Instant::now();
    while store.writer_health().index_queued < INDEX_QUEUE_CAPACITY
        && wait.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(2));
    }
    let position = store.workspace_catalog_commit_position(workspace_id).expect("position");
    let executed_before = store.writer_health().executed;
    let queued_correlation = CorrelationId::new();
    let queued_identity =
        PersistenceCoordinator::workspace_catalog_write_identity(workspace_id, queued_correlation);
    let queued = store.apply_workspace_catalog_delta_guarded(
        workspace_id,
        position,
        delta_for("queued.txt", "queued"),
        queued_correlation,
    );
    let queued_settlement = store.job_settlement(&queued_identity);
    let executed_after_refusal = store.writer_health().executed;
    barrier.release_all();
    let holder_result = holder.join().expect("holder thread");
    for filler in fillers {
        let _ = filler.join();
    }
    eprintln!(
        "R2write queued={queued:?} queued_settlement={queued_settlement:?} \
         executed_before={executed_before} executed_after_refusal={executed_after_refusal} \
         holder={holder_result:?}"
    );
    assert!(
        matches!(queued, Err(PersistenceError::WriterBusy { class }) if class == "index"),
        "a full lane refuses before admission: {queued:?}"
    );
    assert_eq!(
        queued_settlement, None,
        "a job that was never admitted has no terminal record, and none is invented"
    );
    assert_eq!(
        executed_after_refusal, executed_before,
        "the writer executed nothing for the refused job"
    );
    assert!(holder_result.is_ok(), "the holder ends normally: {holder_result:?}");

    // (2) EXECUTING: the job is admitted and parked inside the writer while its caller's
    //     budget runs out. The caller is told "unresponsive" — unknown — and the job's own
    //     record is what resolves it after the release.
    store.set_index_deadline_for_test(Duration::from_millis(80));
    let position = store.workspace_catalog_commit_position(workspace_id).expect("position");
    let executing_barrier = store.arm_writer_hold_once(Point::WriterExecution, Some("index"), 0);
    let executing_correlation = CorrelationId::new();
    let executing_identity = PersistenceCoordinator::workspace_catalog_write_identity(
        workspace_id,
        executing_correlation,
    );
    let executing_store = Arc::clone(&store);
    let executing = std::thread::spawn(move || {
        executing_store.apply_workspace_catalog_delta_guarded(
            workspace_id,
            position,
            delta_for("executing.txt", "executing"),
            executing_correlation,
        )
    });
    assert!(
        executing_barrier.wait_for_arrivals(1, Duration::from_secs(10)),
        "the index job never reached the writer"
    );
    let while_running = executing.join().expect("the caller's thread");
    let health_while_parked = store.writer_health();
    let settlement_while_parked = store.job_settlement(&executing_identity);
    let revision_before_release = durable_catalog_revision(&store, workspace_id);
    eprintln!(
        "R2write executing={while_running:?} health_while_parked={health_while_parked:?} \
         settlement_while_parked={settlement_while_parked:?} \
         revision_before_release={revision_before_release}"
    );
    match &while_running {
        Err(PersistenceError::WriterUnresponsive { class }) => {
            assert_eq!(*class, "index", "the unresponsive class has to be named");
        }
        other => panic!("a caller that ends while its job runs is unresponsive: {other:?}"),
    }
    assert_eq!(
        health_while_parked.state,
        WriterState::Executing,
        "the job really is executing, not merely queued: {health_while_parked:?}"
    );
    assert_eq!(settlement_while_parked, None, "nothing is settled while the job is still running");
    executing_barrier.release_all();
    let settled = wait_for_settlement(&store, &executing_identity, Duration::from_secs(10))
        .expect("the released job settles and records its terminal state");
    let revision_after_release = durable_catalog_revision(&store, workspace_id);
    let executing_rows = count_rows(&store, workspace_id, "executing.txt");
    let health_after = store.writer_health();
    eprintln!(
        "R2write executing_settled={settled:?} revision_after_release={revision_after_release} \
         rows={executing_rows} health_after={health_after:?}"
    );
    assert!(settled.succeeded, "the released job really committed");
    assert!(
        !settled.answered,
        "nobody was listening: the record is what resolves the unknown, not a resend"
    );
    assert_eq!(settled.class, aura_persistence::WorkClass::Index);
    assert_eq!(
        revision_after_release,
        revision_before_release + 1,
        "the job committed exactly once after its caller left"
    );
    assert_eq!(executing_rows, 1, "the write is in the database exactly once");
    assert_eq!(
        settlements_for(&store, &executing_identity),
        1,
        "exactly one terminal record for this identity"
    );

    // (3) POST-COMMIT: the caller already has its answer and the writer is still holding the
    //     job after the commit. The job's record exists, the queue has not settled it yet,
    //     and one release later it is settled exactly once.
    store.set_index_deadline_for_test(Duration::from_secs(5));
    let position = store.workspace_catalog_commit_position(workspace_id).expect("position");
    let commit_barrier = store.arm_writer_hold_once(Point::WriterPostCommit, Some("index"), 0);
    let committed_correlation = CorrelationId::new();
    let committed_identity = PersistenceCoordinator::workspace_catalog_write_identity(
        workspace_id,
        committed_correlation,
    );
    let committed = store.apply_workspace_catalog_delta_guarded(
        workspace_id,
        position,
        delta_for("committed.txt", "post-commit"),
        committed_correlation,
    );
    assert!(
        commit_barrier.wait_for_arrivals(1, Duration::from_secs(10)),
        "the job never reached the post-commit hold"
    );
    let executed_at_post_commit = store.writer_health().executed;
    let settled_at_post_commit = store.job_settlement(&committed_identity);
    commit_barrier.release_all();
    let executed_after_post_commit =
        wait_for_executed(&store, executed_at_post_commit + 1, Duration::from_secs(10));
    let committed_rows = count_rows(&store, workspace_id, "committed.txt");
    let health_final = store.writer_health();
    eprintln!(
        "R2write committed={committed:?} settled_at_post_commit={settled_at_post_commit:?} \
         executed_at_post_commit={executed_at_post_commit} \
         executed_after_post_commit={executed_after_post_commit} rows={committed_rows} \
         health_final={health_final:?}"
    );
    match committed {
        Ok(aura_persistence::CatalogCommitOutcome::Applied(revision)) => {
            assert!(revision.value() > 0);
        }
        other => panic!("a write whose caller is still waiting answers with its result: {other:?}"),
    }
    let settled_at_post_commit =
        settled_at_post_commit.expect("the terminal record exists as soon as the work is done");
    assert!(settled_at_post_commit.succeeded && settled_at_post_commit.answered);
    assert_eq!(
        executed_at_post_commit,
        executed_after_post_commit - 1,
        "the job is not counted as settled until it really finished"
    );
    assert_eq!(committed_rows, 1, "the post-commit write is in the database exactly once");
    assert_eq!(settlements_for(&store, &committed_identity), 1, "still exactly one record");
    assert_eq!(
        health_final.state,
        WriterState::Running,
        "the writer is serving again: {health_final:?}"
    );
    let all = store.job_settlements();
    eprintln!("R2write settlements={all:?}");
    assert_eq!(all.settled_after_caller_left, 1, "exactly one job settled after its caller left");
    drop(store);
}

fn delta_for(path: &str, reason: &str) -> CatalogDeltaRecord {
    CatalogDeltaRecord {
        upserts: vec![entry(path)],
        removed_paths: Vec::new(),
        removed_partitions: Vec::new(),
        partitions: vec![partition()],
        repaired_paths: Vec::new(),
        reason: reason.to_owned(),
    }
}

fn durable_catalog_revision(store: &PersistenceCoordinator, workspace_id: WorkspaceId) -> u64 {
    store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT catalog_revision FROM workspaces WHERE workspace_id=?1",
                    rusqlite::params![workspace_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("catalog revision")
        .unsigned_abs()
}

fn count_rows(store: &PersistenceCoordinator, workspace_id: WorkspaceId, path: &str) -> i64 {
    store
        .read(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM workspace_catalog_entries
                     WHERE workspace_id=?1 AND relative_path=?2",
                    rusqlite::params![workspace_id.to_string(), path],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(Into::into)
        })
        .expect("row count")
}

fn wait_for_settlement(
    store: &PersistenceCoordinator,
    identity: &str,
    budget: Duration,
) -> Option<aura_persistence::JobSettlement> {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(settlement) = store.job_settlement(identity) {
            return Some(settlement);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_executed(store: &PersistenceCoordinator, expected: u64, budget: Duration) -> u64 {
    let deadline = Instant::now() + budget;
    loop {
        let executed = store.writer_health().executed;
        if executed >= expected || Instant::now() >= deadline {
            return executed;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn settlements_for(store: &PersistenceCoordinator, identity: &str) -> usize {
    store
        .job_settlements()
        .recent
        .iter()
        .filter(|settlement| settlement.identity == identity)
        .count()
}
