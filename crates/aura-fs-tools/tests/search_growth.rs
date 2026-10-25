//! Whether repeated search work *grows* anything, measured in one process.
//!
//! A single search is covered elsewhere; this asks a different question. The
//! search engine owns a process-wide worker pool, a process-wide flight registry
//! and one sweeper, so the risk is not "does one scan work" but "does the tenth
//! thousand leave a thread, an orphan or a budget behind". Three readings that
//! fail differently are taken together:
//!
//! * registry counters — a leaked registration or orphan is exact and immediate;
//! * OS thread count — a leaked thread is invisible to memory readings and to any
//!   in-process counter;
//! * working set and committed private bytes — a pool that keeps re-growing its
//!   buffers shows up here and nowhere else.
//!
//! Run it with:
//! `cargo test -p aura-fs-tools --offline --test search_growth -- --ignored --nocapture`
//! The disconnect phase additionally needs `--features search-test-faults`, since
//! it parks a real worker inside one file's scan on purpose.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_windows_platform::{current_process_thread_count, current_process_working_set};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsSearchPattern, FsSearchTextRequest, LargeFileReadMode, SessionSummary, WorkspaceCreateCommand,
};
use serde::Serialize;

const WARMUP_ROUNDS: usize = 5;
const MEASURED_ROUNDS: usize = 25;
const DISCONNECT_ROUNDS: usize = 10;
const NEEDLE: &str = "growth_needle_token";
/// The needle the disconnect rounds search for. Kept apart from [`NEEDLE`] so the
/// gated files never change what the steady-state rounds must match.
const GATE_NEEDLE: &str = "growth_gate_token";
const GATED_FILES: usize = DISCONNECT_ROUNDS;

#[derive(Serialize, Clone, Debug)]
struct Reading {
    registry_active: usize,
    registry_orphans: usize,
    pool_active: usize,
    pool_queued: usize,
    threads: u32,
    working_set_bytes: u64,
    private_bytes: u64,
}

fn reading() -> Reading {
    let load = aura_fs_tools::search_pool_load();
    let memory = current_process_working_set().expect("working set");
    Reading {
        registry_active: aura_fs_tools::search_flight_count(),
        registry_orphans: aura_fs_tools::search_orphan_count(),
        pool_active: load.active,
        pool_queued: load.queued,
        threads: current_process_thread_count().expect("thread count"),
        working_set_bytes: u64::try_from(memory.working_set_bytes).unwrap_or(u64::MAX),
        private_bytes: u64::try_from(memory.private_bytes).unwrap_or(u64::MAX),
    }
}

fn build_corpus(root: &Path) -> u64 {
    let mut bytes = 0_u64;
    for index in 0..300 {
        let directory = root.join(format!("d{}", index % 20));
        fs::create_dir_all(&directory).expect("corpus directory");
        let mut body = format!("fn fixture_{index}() {{ let value = {index}; }}\n").repeat(8);
        if index % 7 == 0 {
            body.push_str(NEEDLE);
            body.push('\n');
        }
        bytes += u64::try_from(body.len()).unwrap_or(0);
        fs::write(directory.join(format!("f{index}.txt")), body).expect("corpus file");
    }
    // The disconnect rounds park a worker on these files, so they must exist
    // before the catalog snapshot is taken — a file created afterwards would never
    // be scanned and the gate would never be reached.
    for round in 0..GATED_FILES {
        let path = gated_path(round);
        let body = format!("{GATE_NEEDLE}\ngated round {round}\n");
        bytes += u64::try_from(body.len()).unwrap_or(0);
        fs::write(root.join(&path), body).expect("gated corpus file");
    }
    bytes
}

/// The workspace-relative path of a disconnect round's gated file.
fn gated_path(round: usize) -> String {
    format!("d0/growth_gate_{round}.txt")
}

struct Harness {
    fs: Arc<FsToolService>,
    workspace_id: WorkspaceId,
    session: SessionId,
    /// The persistence root, kept alive with the harness.
    _data: tempfile::TempDir,
}

fn open_harness(root: &Path) -> Harness {
    let data = tempfile::tempdir().expect("data root");
    let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
        data.path().join("aura.sqlite3"),
        data.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence")
    {
        PersistenceStartup::Ready(value) => Arc::new(value),
        PersistenceStartup::RecoveryRequired(value) => panic!("{}", value.reason),
    };
    let workspace = WorkspaceRuntime::open(
        Arc::clone(&persistence),
        WorkspaceRuntimeOptions {
            catalog: CatalogOptions::with_index_file_limit(1024 * 1024),
            watcher_coalesce: Duration::from_secs(30),
            watcher_poll_interval: Duration::from_secs(30),
            watcher_rescan_attempts: 1,
            ..WorkspaceRuntimeOptions::default()
        },
    )
    .expect("workspace");
    let created = workspace
        .create(
            WorkspaceCreateCommand {
                selected_root: root.display().to_string(),
                display_name: "growth".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create");
    let workspace_id = created.workspace.workspace_id;
    let session = SessionId::new();
    persistence
        .upsert_workspace_session(
            persistence.workspace(workspace_id).expect("workspace row"),
            SessionSummary {
                session_id: session,
                workspace_id,
                title: "growth".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("session");
    let fs = FsToolService::new(
        Arc::clone(&workspace),
        persistence,
        FsToolPolicy {
            large_file_read_mode: LargeFileReadMode::RequireRange,
            full_text_read_limit_bytes: 2 * 1024 * 1024,
            small_range_line_limit: 500,
            small_range_character_limit: 65_536,
            default_binary_range_bytes: 4 * 1024,
            max_binary_range_bytes: 64 * 1024,
            max_changeset_operations: 64,
            max_search_results: 100,
            mutation_wait: std::time::Duration::from_secs(5),
        },
    )
    .expect("fs service");
    Harness { fs, workspace_id, session, _data: data }
}

fn search_request(harness: &Harness, needle: &str) -> FsSearchTextRequest {
    FsSearchTextRequest {
        workspace_id: harness.workspace_id,
        session_id: harness.session,
        pattern: FsSearchPattern::Literal { text: needle.to_owned(), case_sensitive: true },
        root: WorkspacePath::parse(".").expect("root"),
        path_glob: None,
        include_ignored: false,
        include_generated: false,
        limit: 50,
    }
}

fn run_search(harness: &Harness, needle: &str) -> usize {
    harness.fs.search_text(search_request(harness, needle)).expect("search").matches.len()
}

#[derive(Serialize)]
struct PhaseReport {
    name: String,
    rounds: usize,
    first: Reading,
    last: Reading,
    settled: Reading,
    matches_per_round: usize,
    /// Every reading of the phase, so a reviewer can see a trend instead of a
    /// summary that could hide one.
    readings: Vec<Reading>,
}

#[derive(Serialize)]
struct GrowthReport {
    schema: String,
    warmup_rounds: usize,
    measured_rounds: usize,
    disconnect_rounds: usize,
    corpus_files: u64,
    corpus_bytes: u64,
    baseline: Reading,
    after_warmup: Reading,
    steady_state: PhaseReport,
    disconnect: PhaseReport,
    idle_settled: Reading,
    elapsed_ms: u128,
}

/// Repeated real searches must not grow the registry, the pool or the thread
/// population, and must not keep growing memory.
#[test]
#[ignore = "growth measurement over many real searches; run explicitly"]
fn repeated_searches_do_not_grow_registry_pool_threads_or_memory() {
    let started = Instant::now();
    let root = tempfile::tempdir().expect("corpus root");
    let corpus_bytes = build_corpus(root.path());
    let harness = open_harness(root.path());
    let baseline = reading();

    for _ in 0..WARMUP_ROUNDS {
        assert_eq!(run_search(&harness, NEEDLE), 43, "every 7th of 300 files carries the needle");
    }
    let after_warmup = reading();

    // Steady state: the same real search, many times. Nothing here is a fault
    // case — this is the shape the product actually runs.
    let mut readings = Vec::new();
    for _ in 0..MEASURED_ROUNDS {
        assert_eq!(run_search(&harness, NEEDLE), 43);
        readings.push(reading());
    }
    let steady_state = PhaseReport {
        name: "steady_state".to_owned(),
        rounds: MEASURED_ROUNDS,
        first: readings.first().expect("a first reading").clone(),
        last: readings.last().expect("a last reading").clone(),
        settled: reading(),
        matches_per_round: 43,
        readings,
    };

    // Disconnect during a real scan: park one worker inside a file, cancel, hand
    // the landing to the registry, then let the worker end. This is the path that
    // could leave an orphan behind on every dropped connection.
    let disconnect = disconnect_phase(&harness, root.path());

    // Idle: after everything, with a pause long enough for the sweeper to have
    // observed any completion it still owed.
    std::thread::sleep(Duration::from_millis(500));
    let idle_settled = reading();

    let report = GrowthReport {
        schema: "fs-search-growth-v1".to_owned(),
        warmup_rounds: WARMUP_ROUNDS,
        measured_rounds: MEASURED_ROUNDS,
        disconnect_rounds: DISCONNECT_ROUNDS,
        corpus_files: 300,
        corpus_bytes,
        baseline,
        after_warmup: after_warmup.clone(),
        steady_state,
        disconnect,
        idle_settled: idle_settled.clone(),
        elapsed_ms: started.elapsed().as_millis(),
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join(format!("fs-search-growth-{stamp}"));
    fs::create_dir_all(&directory).expect("evidence directory");
    fs::write(
        directory.join("growth.json"),
        serde_json::to_vec_pretty(&report).expect("report json"),
    )
    .expect("write report");
    println!("growth evidence: {}", directory.display());
    println!(
        "baseline={:?}\nafter_warmup={:?}\nidle_settled={:?}",
        report.baseline, report.after_warmup, report.idle_settled
    );

    // Registry: exact counters, so a leak is a failure and not a trend.
    assert_eq!(
        report.idle_settled.registry_active,
        0,
        "no registration may survive {} real searches and {DISCONNECT_ROUNDS} disconnects",
        WARMUP_ROUNDS + MEASURED_ROUNDS + DISCONNECT_ROUNDS
    );
    assert_eq!(
        report.idle_settled.registry_orphans, 0,
        "a completed disconnect must leave no orphan behind"
    );
    assert_eq!(report.idle_settled.pool_active, 0, "no pool worker may stay busy");
    assert_eq!(report.idle_settled.pool_queued, 0, "no pool job may stay queued");

    // Threads: the pool and the sweeper are reused, not re-created per search.
    assert!(
        report.idle_settled.threads <= report.after_warmup.threads,
        "repeated searches must not add threads: after_warmup={} idle={}",
        report.after_warmup.threads,
        report.idle_settled.threads
    );

    // Memory: the claim is "no continuous growth", not "identical numbers". A
    // bounded allowance above the warm-up reading separates a cache that settled
    // from a leak that keeps climbing; the per-round readings in the evidence show
    // which one happened.
    const ALLOWANCE_BYTES: u64 = 64 * 1024 * 1024;
    assert!(
        report.idle_settled.working_set_bytes
            <= report.after_warmup.working_set_bytes + ALLOWANCE_BYTES,
        "working set kept growing: after_warmup={} idle={}",
        report.after_warmup.working_set_bytes,
        report.idle_settled.working_set_bytes
    );
    assert!(
        report.idle_settled.private_bytes <= report.after_warmup.private_bytes + ALLOWANCE_BYTES,
        "committed private bytes kept growing: after_warmup={} idle={}",
        report.after_warmup.private_bytes,
        report.idle_settled.private_bytes
    );
}

/// Disconnect during a real search, `DISCONNECT_ROUNDS` times.
///
/// Without the fault feature the worker cannot be held open, so the phase is
/// reported as unrun rather than silently skipped or faked by a stand-in.
fn disconnect_phase(_harness: &Harness, corpus_root: &Path) -> PhaseReport {
    let _ = corpus_root;
    // Only the feature-gated block below pushes a reading.
    #[cfg_attr(not(feature = "search-test-faults"), allow(unused_mut))]
    let mut readings = Vec::new();
    if !cfg!(feature = "search-test-faults") {
        return PhaseReport {
            name: "disconnect_skipped_without_search_test_faults".to_owned(),
            rounds: 0,
            first: reading(),
            last: reading(),
            settled: reading(),
            matches_per_round: 0,
            readings,
        };
    }
    #[cfg(feature = "search-test-faults")]
    {
        for round in 0..DISCONNECT_ROUNDS {
            let path = gated_path(round);
            let gate = aura_fs_tools::test_faults::arm_blocking_gate(&path);
            let request_id = CorrelationId::new();
            let cancel = aura_fs_tools::SearchCancel::new();
            aura_fs_tools::register_search(request_id, cancel.clone());
            let search = std::thread::spawn({
                let fs = Arc::clone(&_harness.fs);
                let request = search_request(_harness, GATE_NEEDLE);
                move || fs.search_text_with_cancel(request, &cancel).map(|(result, _)| result)
            });
            assert!(
                gate.wait_for_parked(1, Duration::from_secs(30)),
                "round {round}: a real worker must be inside the gated scan"
            );
            // The connection is gone: cancel, then wait for the landing under a
            // deadline this worker cannot meet, exactly as the disconnect path
            // does before it hands the registration over.
            aura_fs_tools::cancel_search(request_id);
            let still_running = aura_fs_tools::wait_for_search_orphans_of(
                &[request_id],
                Duration::from_millis(200),
            );
            assert_eq!(
                still_running,
                vec![request_id],
                "round {round}: a parked worker is not reclaimed by a wait"
            );
            // Hand the landing over, exactly as the disconnect path does when the
            // bounded wait could not prove reclamation.
            aura_fs_tools::orphan_search([request_id]);
            gate.release();
            match search.join().expect("search thread") {
                Ok(outcome) => assert!(
                    outcome.matches.iter().any(|hit| hit.path.as_str() == path),
                    "round {round}: the gated file was really scanned before the cancel"
                ),
                // A cancelled search reports the cancellation instead of a partial
                // result. The parked worker asserted above is the evidence that
                // the gated file really was being scanned.
                Err(aura_fs_tools::FsToolError::Cancelled) => {}
                Err(error) => panic!("round {round}: unexpected search failure {error:?}"),
            }
            assert!(
                wait_for_registry_release(request_id, Duration::from_secs(30)),
                "round {round}: the registry must release the finished search"
            );
            readings.push(reading());
        }
    }
    PhaseReport {
        name: "disconnect_during_search".to_owned(),
        rounds: DISCONNECT_ROUNDS,
        first: readings.first().expect("a first reading").clone(),
        last: readings.last().expect("a last reading").clone(),
        settled: reading(),
        matches_per_round: 0,
        readings,
    }
}

/// Waits until the flight registry has released a finished search. Only the
/// disconnect phase uses it, and that phase is feature-gated.
#[cfg(feature = "search-test-faults")]
fn wait_for_registry_release(request_id: CorrelationId, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if aura_fs_tools::search_flight(request_id).is_none() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}
