//! Per-case search benchmark with stage attribution and an interleaved
//! old-engine reference.
//!
//! Run explicitly (not part of the default suite):
//! `cargo test -p aura-fs-tools --offline --test search_bench -- --ignored --nocapture --test-threads=1`
//!
//! Every run writes a fresh timestamped directory under `target/` and never
//! overwrites an earlier run's JSON.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use aura_domain_common::{
    CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_fs_tools::{
    FsToolPolicy, FsToolService, SearchCancel, SearchPoolLoad, StageDiagnosticsSnapshot,
    search_pool_load,
};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    BufferEditCommand, BufferOpenCommand, FsSearchCompletion, FsSearchPattern, FsSearchTextRequest,
    LargeFileReadMode, SessionSummary, WorkspaceCreateCommand,
};
use serde::Serialize;

const RELEASE_NOTE: &str = "measurement-only; production behaviour unchanged";

struct Harness {
    fs: Arc<FsToolService>,
    workspace: Arc<WorkspaceRuntime>,
    workspace_id: WorkspaceId,
    session: SessionId,
    root: PathBuf,
    _data: tempfile::TempDir,
}

impl Harness {
    /// Open a workspace over an existing corpus, reusing `root` so every
    /// repetition measures the same bytes.
    fn open(root: &Path) -> Self {
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
                    display_name: "bench".to_owned(),
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
                    title: "bench".to_owned(),
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
        .expect("fs");
        Self { fs, workspace, workspace_id, session, root: root.to_path_buf(), _data: data }
    }

    fn request(&self, query: &str, limit: u32) -> FsSearchTextRequest {
        FsSearchTextRequest {
            workspace_id: self.workspace_id,
            session_id: self.session,
            pattern: FsSearchPattern::Literal { text: query.to_owned(), case_sensitive: true },
            root: WorkspacePath::parse(".").expect("root"),
            path_glob: None,
            include_ignored: false,
            include_generated: false,
            limit,
        }
    }
}

#[derive(Serialize)]
struct Sample {
    cold_ms: u128,
    hot_ms: u128,
    matches: usize,
    candidate_file_count: u64,
    scanned_file_count: u64,
    scanned_bytes: u64,
    skipped_file_count: u64,
    completion: String,
    stages: StageSummary,
    pool: PoolSummary,
}

#[derive(Serialize, Default)]
struct StageSummary {
    catalog_ms: f64,
    buffer_probe_ms: f64,
    queue_wait_ms: f64,
    open_ms: f64,
    read_ms: f64,
    verify_ms: f64,
    commit_ms: f64,
    catalog_candidates: u64,
    buffer_probes: u64,
    buffer_snapshots: u64,
    open_calls: u64,
    verify_calls: u64,
    verify_bytes: u64,
    matched_lines: u64,
    matched_line_bytes: u64,
    matched_hits: u64,
    char_offset_bytes: u64,

    window_high_water: u64,
    pool_active_high_water: u64,
}

impl From<StageDiagnosticsSnapshot> for StageSummary {
    fn from(value: StageDiagnosticsSnapshot) -> Self {
        Self {
            catalog_ms: micros_to_ms(value.catalog_micros),
            buffer_probe_ms: micros_to_ms(value.buffer_probe_micros),
            queue_wait_ms: micros_to_ms(value.queue_wait_micros),
            open_ms: micros_to_ms(value.open_micros),
            read_ms: micros_to_ms(value.read_micros),
            verify_ms: micros_to_ms(value.verify_micros),
            commit_ms: micros_to_ms(value.commit_micros),
            catalog_candidates: value.catalog_candidates,
            buffer_probes: value.buffer_probes,
            buffer_snapshots: value.buffer_snapshots,
            open_calls: value.open_calls,
            verify_calls: value.verify_calls,
            verify_bytes: value.verify_bytes,
            matched_lines: value.matched_lines,
            matched_line_bytes: value.matched_line_bytes,
            matched_hits: value.matched_hits,
            char_offset_bytes: value.char_offset_bytes,

            window_high_water: value.window_high_water,
            pool_active_high_water: value.pool_active_high_water,
        }
    }
}

#[derive(Serialize)]
struct PoolSummary {
    active: usize,
    queued: usize,
}

fn micros_to_ms(micros: u64) -> f64 {
    (micros as f64) / 1000.0
}

#[derive(Serialize)]
struct ReferenceSample {
    cold_ms: u128,
    hot_ms: u128,
    matches: usize,
    scanned_file_count: u64,
    scanned_bytes: u64,
    /// The old engine scans whole files with no budget, cancel or catalog
    /// work; only the latency is comparable, never the coverage.
    coverage: &'static str,
}

#[derive(Serialize)]
struct CaseResult {
    name: String,
    corpus: CorpusSummary,
    query: String,
    limit: u32,
    rounds: usize,
    new_engine: Vec<Sample>,
    reference_scanner: Vec<ReferenceSample>,
    median_new_hot_ms: u128,
    p95_new_hot_ms: u128,
    median_reference_hot_ms: u128,
}

#[derive(Serialize, Clone)]
struct CorpusSummary {
    files: u64,
    bytes: u64,
    layout: String,
}

fn utc_stamp() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_secs();
    let days = seconds / 86_400;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(0));
    format!(
        "{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z",
        hour = time / 3600,
        minute = (time % 3600) / 60,
        second = time % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days-from-civil inverse; valid for the ranges we use.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn percentile(samples: &mut [u128], fraction: f64) -> u128 {
    samples.sort_unstable();
    if samples.is_empty() {
        return 0;
    }
    let index = ((samples.len() as f64 - 1.0) * fraction).round() as usize;
    samples[index.min(samples.len() - 1)]
}

fn completion_label(completion: &FsSearchCompletion) -> String {
    match completion {
        FsSearchCompletion::Complete => "complete".to_owned(),
        FsSearchCompletion::Partial { reasons } => {
            let mut labels: Vec<String> =
                reasons.iter().map(|reason| format!("{reason:?}")).collect();
            labels.sort();
            labels.dedup();
            format!("partial[{}]", labels.join(","))
        }
    }
}

fn measure_new(harness: &Harness, query: &str, limit: u32) -> Sample {
    let request = harness.request(query, limit);
    let cold_started = Instant::now();
    let cold = harness.fs.search_text(request.clone()).expect("cold search");
    let cold_ms = cold_started.elapsed().as_millis();
    let cold_stages = harness.fs.last_search_diagnostics().expect("cold stages");
    let hot_started = Instant::now();
    let hot = harness.fs.search_text(request).expect("hot search");
    let hot_ms = hot_started.elapsed().as_millis();
    let hot_stages = harness.fs.last_search_diagnostics().expect("hot stages");
    assert_eq!(cold.matches.len(), hot.matches.len(), "cold and hot must agree");
    let pool = search_pool_load();
    let _ = cold_stages;
    Sample {
        cold_ms,
        hot_ms,
        matches: hot.matches.len(),
        candidate_file_count: hot.candidate_file_count,
        scanned_file_count: hot.scanned_file_count,
        scanned_bytes: hot.scanned_bytes,
        skipped_file_count: hot.skipped_file_count,
        completion: completion_label(&hot.completion),
        stages: StageSummary::from(hot_stages),
        pool: PoolSummary { active: pool.active, queued: pool.queued },
    }
}

/// Old-engine reference: enumerate under the root, read whole files, decode as
/// UTF-8 and count literal matches. No catalog snapshot, no budget, no cancel,
/// Simplified reference scanner, NOT the historical production engine.
///
/// It recurses the tree, reads whole files, lossily decodes UTF-8 and counts
/// literal occurrences. It has no catalog snapshot, no Buffer semantics, no
/// per-file or total budget, no cancellation and no consistency re-hash, and it
/// does not build equivalent matches. Its only purpose is a rough latency
/// reference on the same corpus. The previous release's real production numbers
/// live in `target/fs-search-new-perf.json` and
/// `target/fs-search-old-baseline.json`, and this scanner must never be
/// presented as that baseline.
fn measure_reference(harness: &Harness, query: &str, limit: u32) -> ReferenceSample {
    let run = |root: &Path| -> (u128, usize, u64, u64) {
        let started = Instant::now();
        let mut files = Vec::new();
        collect_files(root, &mut files);
        files.sort();
        let mut matches = 0_usize;
        let mut scanned_files = 0_u64;
        let mut scanned_bytes = 0_u64;
        'outer: for file in &files {
            let Ok(bytes) = fs::read(file) else {
                continue;
            };
            scanned_files += 1;
            scanned_bytes = scanned_bytes.saturating_add(u64::try_from(bytes.len()).unwrap_or(0));
            if bytes.contains(&0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines() {
                let mut offset = 0_usize;
                while let Some(found) = line[offset..].find(query) {
                    matches += 1;
                    if matches >= limit as usize {
                        break 'outer;
                    }
                    offset += found + query.len().max(1);
                    if offset >= line.len() {
                        break;
                    }
                }
            }
        }
        (started.elapsed().as_millis(), matches, scanned_files, scanned_bytes)
    };
    let (cold_ms, _, _, _) = run(&harness.root);
    let (hot_ms, matches, scanned_files, scanned_bytes) = run(&harness.root);
    ReferenceSample {
        cold_ms,
        hot_ms,
        matches,
        scanned_file_count: scanned_files,
        scanned_bytes,
        coverage: "simplified_reference_scanner_no_catalog_budget_cancel_or_verify",
    }
}

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn write_tree(root: &Path, file_count: usize, payload: &[u8], needle: &str) -> u64 {
    let mut bytes = 0_u64;
    for index in 0..file_count {
        let dir = root.join(format!("p{}/d{}", index % 50, index % 200));
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(format!("f{index}.txt"));
        let mut body = payload.to_vec();
        if index % 17 == 0 {
            body.extend_from_slice(needle.as_bytes());
            body.push(b'\n');
        }
        bytes += u64::try_from(body.len()).unwrap_or(0);
        fs::write(path, body).expect("file");
    }
    bytes
}

fn runtime_miss_needle() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
    format!("AURA_FS_BENCH_MISS_{nanos}")
}

struct Case {
    name: &'static str,
    query: String,
    limit: u32,
    rounds: usize,
    corpus: CorpusSummary,
    root: tempfile::TempDir,
    prepare: Option<fn(&Harness)>,
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();

    let small = tempfile::tempdir().expect("small root");
    let small_bytes = write_tree(small.path(), 200, b"fn example() { 1 }\n", "unique_small_token");
    let small_miss = runtime_miss_needle();
    cases.push(Case {
        name: "small_200_files_hit",
        query: "unique_small_token".to_owned(),
        limit: 50,
        rounds: 3,
        corpus: CorpusSummary {
            files: 200,
            bytes: small_bytes,
            layout: "200 small files in 50x200 buckets".to_owned(),
        },
        root: small,
        prepare: None,
    });
    let small_miss_root = tempfile::tempdir().expect("small miss root");
    let miss_bytes = write_tree(small_miss_root.path(), 200, b"fn example() { 1 }\n", "unused");
    cases.push(Case {
        name: "small_200_files_miss_full_scan",
        query: small_miss,
        limit: 100,
        rounds: 5,
        corpus: CorpusSummary {
            files: 200,
            bytes: miss_bytes,
            layout: "same corpus, runtime-generated absent needle".to_owned(),
        },
        root: small_miss_root,
        prepare: None,
    });

    let ten_k = tempfile::tempdir().expect("10k root");
    let ten_k_bytes = write_tree(ten_k.path(), 10_000, b"aaaaaaaaaaaaaaaa\n", "token_10k");
    cases.push(Case {
        name: "10k_files_hit_prefix",
        query: "token_10k".to_owned(),
        limit: 50,
        rounds: 3,
        corpus: CorpusSummary {
            files: 10_000,
            bytes: ten_k_bytes,
            layout: "10k tiny files, every 17th carries the needle".to_owned(),
        },
        root: ten_k,
        prepare: None,
    });
    let ten_k_miss_root = tempfile::tempdir().expect("10k miss root");
    let ten_k_miss_bytes =
        write_tree(ten_k_miss_root.path(), 10_000, b"aaaaaaaaaaaaaaaa\n", "unused");
    cases.push(Case {
        name: "10k_files_miss_full_scan",
        query: runtime_miss_needle(),
        limit: 100,
        rounds: 3,
        corpus: CorpusSummary {
            files: 10_000,
            bytes: ten_k_miss_bytes,
            layout: "10k tiny files, no needle".to_owned(),
        },
        root: ten_k_miss_root,
        prepare: None,
    });

    let hundred_k = tempfile::tempdir().expect("100k root");
    let hundred_k_bytes = write_tree(hundred_k.path(), 100_000, b"z\n", "token_100k");
    cases.push(Case {
        name: "100k_files_hit_prefix",
        query: "token_100k".to_owned(),
        limit: 50,
        rounds: 2,
        corpus: CorpusSummary {
            files: 100_000,
            bytes: hundred_k_bytes,
            layout: "100k tiny files, every 17th carries the needle".to_owned(),
        },
        root: hundred_k,
        prepare: None,
    });
    // One round only: a full catalog miss at this scale either completes inside
    // the 10 s default budget or reports coverage under TimeBudget; the number
    // of interest is how much of the catalog it actually covers.
    let hundred_k_miss_root = tempfile::tempdir().expect("100k miss root");
    let hundred_k_miss_bytes = write_tree(hundred_k_miss_root.path(), 100_000, b"z\n", "unused");
    cases.push(Case {
        name: "100k_files_miss_full_scan",
        query: runtime_miss_needle(),
        limit: 100,
        rounds: 1,
        corpus: CorpusSummary {
            files: 100_000,
            bytes: hundred_k_miss_bytes,
            layout: "100k tiny files, no needle".to_owned(),
        },
        root: hundred_k_miss_root,
        prepare: None,
    });

    let dirty = tempfile::tempdir().expect("dirty buffer root");
    let dirty_bytes = write_tree(dirty.path(), 5, b"disk text\n", "unused");
    cases.push(Case {
        name: "dirty_buffer_hit",
        query: "buffer_only_token".to_owned(),
        limit: 50,
        rounds: 3,
        corpus: CorpusSummary {
            files: 5,
            bytes: dirty_bytes,
            layout: "one open editor buffer with an unsaved needle".to_owned(),
        },
        root: dirty,
        prepare: Some(|harness| {
            let opened = harness
                .workspace
                .open_buffer(BufferOpenCommand {
                    workspace_id: harness.workspace_id,
                    session_id: harness.session,
                    path: WorkspacePath::parse("p0/d0/f0.txt").expect("path"),
                    writable: true,
                })
                .expect("open");
            harness
                .workspace
                .edit_buffer(
                    BufferEditCommand {
                        workspace_id: harness.workspace_id,
                        session_id: harness.session,
                        path: WorkspacePath::parse("p0/d0/f0.txt").expect("path"),
                        reservation_id: opened.reservation_id.expect("reservation"),
                        reservation_epoch: opened.reservation_epoch,
                        buffer_revision: Revision::new(opened.buffer_revision.value() + 1),
                        base_core_revision: opened.buffer_revision,
                        content: "buffer_only_token\n".to_owned(),
                        active_editing: true,
                        ime_composition_active: false,
                    },
                    CorrelationId::new(),
                )
                .expect("edit");
        }),
    });

    let long_line = tempfile::tempdir().expect("long line root");
    let mut body = vec![b'x'; 1024 * 1024];
    body.extend_from_slice(b"long_line_needle\n");
    fs::write(long_line.path().join("long.txt"), &body).expect("long line");
    cases.push(Case {
        name: "1mib_single_line",
        query: "long_line_needle".to_owned(),
        limit: 10,
        rounds: 3,
        corpus: CorpusSummary {
            files: 1,
            bytes: u64::try_from(body.len()).unwrap_or(0),
            layout: "one 1 MiB single-line file".to_owned(),
        },
        root: long_line,
        prepare: None,
    });

    // The same 1 MiB line, but with the needle repeated across it. This is the
    // shape that made per-hit projection quadratic: every hit used to re-walk
    // the whole line to find its character offset and build its excerpt.
    let dense_line = tempfile::tempdir().expect("dense long line root");
    let mut body = Vec::new();
    for index in 0..400 {
        body.extend_from_slice(b"long_line_needle");
        body.extend_from_slice(&vec![b'x'; 2600 - (index % 7)]);
    }
    body.push(b'\n');
    fs::write(dense_line.path().join("dense.txt"), &body).expect("dense long line");
    cases.push(Case {
        name: "1mib_single_line_many_hits",
        query: "long_line_needle".to_owned(),
        limit: 100,
        rounds: 3,
        corpus: CorpusSummary {
            files: 1,
            bytes: u64::try_from(body.len()).unwrap_or(0),
            layout: "one 1 MiB single-line file with 400 hits on that line".to_owned(),
        },
        root: dense_line,
        prepare: None,
    });

    cases
}

fn cancel_stage_samples(harness: &Harness) -> (Vec<u128>, bool, SearchPoolLoad) {
    let mut samples = Vec::new();
    for _ in 0..8 {
        let cancel = SearchCancel::new();
        let request = harness.request("cancel_probe_token", 50);
        let handle = std::thread::spawn({
            let fs = Arc::clone(&harness.fs);
            let cancel = cancel.clone();
            move || fs.search_text_with_cancel(request, &cancel)
        });
        let waited = Instant::now();
        while !cancel.is_started() {
            if waited.elapsed() > Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let started = Instant::now();
        cancel.cancel();
        let error = handle.join().expect("join");
        samples.push(started.elapsed().as_millis());
        assert!(
            matches!(error, Err(aura_fs_tools::FsToolError::Cancelled)),
            "started cancel must be Cancelled"
        );
    }
    std::thread::sleep(Duration::from_millis(50));
    let pool = search_pool_load();
    (samples, true, pool)
}

#[test]
#[ignore = "per-case search benchmark; run once per closeout batch"]
fn capture_search_benchmark() {
    let run_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join(format!("fs-search-bench-{}", utc_stamp()));
    fs::create_dir_all(&run_dir).expect("run dir");
    println!("benchmark output: {}", run_dir.display());

    let mut results: Vec<CaseResult> = Vec::new();
    for case in cases() {
        let harness = Harness::open(case.root.path());
        if let Some(prepare) = case.prepare {
            prepare(&harness);
        }
        let mut new_engine = Vec::new();
        let mut reference_scanner = Vec::new();
        for round in 0..case.rounds {
            if round > 0 {
                // Interleave: old reference first, then the new engine, so a
                // machine-level drift cannot favour either side consistently.
                reference_scanner.push(measure_reference(&harness, &case.query, case.limit));
                new_engine.push(measure_new(&harness, &case.query, case.limit));
            } else {
                new_engine.push(measure_new(&harness, &case.query, case.limit));
                reference_scanner.push(measure_reference(&harness, &case.query, case.limit));
            }
        }
        let mut hot: Vec<u128> = new_engine.iter().map(|sample| sample.hot_ms).collect();
        let mut reference_hot: Vec<u128> =
            reference_scanner.iter().map(|sample| sample.hot_ms).collect();
        results.push(CaseResult {
            name: case.name.to_owned(),
            corpus: case.corpus.clone(),
            query: case.query.clone(),
            limit: case.limit,
            rounds: case.rounds,
            median_new_hot_ms: percentile(&mut hot.clone(), 0.5),
            p95_new_hot_ms: percentile(&mut hot, 0.95),
            median_reference_hot_ms: percentile(&mut reference_hot, 0.5),
            new_engine,
            reference_scanner,
        });
        let _ = reference_hot.len();
    }

    let cancel_harness_root = tempfile::tempdir().expect("cancel root");
    write_tree(cancel_harness_root.path(), 2_000, b"cancel_probe_token\n", "cancel_probe_token");
    let cancel_harness = Harness::open(cancel_harness_root.path());
    let (mut cancel_samples, all_cancelled, pool_after) = cancel_stage_samples(&cancel_harness);
    let cancel_p95 = percentile(&mut cancel_samples, 0.95);

    let payload = serde_json::json!({
        "release_note": RELEASE_NOTE,
        "generated_at": utc_stamp(),
        "machine": std::env::var("COMPUTERNAME").ok(),
        "profile": "test",
        "comparison_method": {
            "reference_scanner": "simplified in-test scanner: recurse + read whole files + literal match; NOT the historical production engine, and no catalog, budget, cancel or verify",
            "new_engine": "production SearchCancel path through FsToolService; cold = first search after workspace open, hot = second",
            "stages": "per-search stage diagnostics; cumulated micros over the search",
        },
        "cancel": {
            "samples_ms": cancel_samples,
            "p95_ms": cancel_p95,
            "target_p95_ms": 250,
            "all_cancelled": all_cancelled,
            "pool_after": { "active": pool_after.active, "queued": pool_after.queued },
        },
        "cases": results,
    });
    fs::write(run_dir.join("results.json"), serde_json::to_vec_pretty(&payload).expect("json"))
        .expect("write results");
    println!("benchmark complete: {} cases", payload["cases"].as_array().map_or(0, Vec::len));
}
