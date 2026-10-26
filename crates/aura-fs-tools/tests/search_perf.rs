//! Full performance matrix. Ignored in the default suite so check-all stays
//! bounded; closeout runs it once with `--ignored` and records the JSON.
use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use aura_domain_common::{
    CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_fs_tools::{FsToolPolicy, FsToolService, SearchCancel, search_pool_load};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_windows_platform::current_process_working_set;
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    BufferEditCommand, BufferOpenCommand, FsSearchCompletion, FsSearchPattern, FsSearchTextRequest,
    LargeFileReadMode, SessionSummary, WorkspaceCreateCommand,
};
use serde::Serialize;

struct Harness {
    fs: Arc<FsToolService>,
    workspace: Arc<WorkspaceRuntime>,
    workspace_id: WorkspaceId,
    session: SessionId,
    _root: Option<tempfile::TempDir>,
    _data: tempfile::TempDir,
}

impl Harness {
    fn new(setup: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        setup(root.path());
        Self::open(root.path().display().to_string(), Some(root), "perf")
    }

    fn open(selected_root: String, root: Option<tempfile::TempDir>, title: &str) -> Self {
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
                catalog: CatalogOptions {
                    max_partition_files: 200_000,
                    max_partition_text_bytes: 1024 * 1024 * 1024,
                    max_index_file_bytes: 0,
                },
                watcher_coalesce: Duration::from_millis(20),
                watcher_poll_interval: Duration::from_secs(1),
                watcher_rescan_attempts: 1,
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace");
        let created = workspace
            .create(
                WorkspaceCreateCommand { selected_root, display_name: title.to_owned() },
                CorrelationId::new(),
            )
            .expect("create");
        workspace.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
        let workspace_id = created.workspace.workspace_id;
        let session = SessionId::new();
        persistence
            .upsert_workspace_session(
                persistence.workspace(workspace_id).expect("workspace row"),
                SessionSummary {
                    session_id: session,
                    workspace_id,
                    title: title.to_owned(),
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
        Self { fs, workspace, workspace_id, session, _root: root, _data: data }
    }
}

#[derive(Serialize)]
struct Case {
    name: String,
    coverage: &'static str,
    files: u64,
    bytes: u64,
    query: String,
    matches: usize,
    completion: String,
    candidate_file_count: u64,
    scanned_file_count: u64,
    scanned_bytes: u64,
    skipped_file_count: u64,
    cold_ms: u128,
    hot_ms: u128,
}

fn measure(
    harness: &Harness,
    query: &str,
    limit: u32,
) -> (Case, aura_workspace_domain::FsSearchTextResult) {
    let request = || FsSearchTextRequest {
        workspace_id: harness.workspace_id,
        session_id: harness.session,
        pattern: FsSearchPattern::Literal { text: query.to_owned(), case_sensitive: true },
        root: WorkspacePath::parse(".").expect("root"),
        path_glob: None,
        include_ignored: false,
        include_generated: false,
        limit,
    };
    let cold_started = Instant::now();
    let first = harness.fs.search_text(request()).expect("cold");
    let cold = cold_started.elapsed();
    let hot_started = Instant::now();
    let second = harness.fs.search_text(request()).expect("hot");
    let hot = hot_started.elapsed();
    assert_eq!(first.matches, second.matches);
    let completion = match &first.completion {
        FsSearchCompletion::Complete => "complete".to_owned(),
        FsSearchCompletion::Partial { reasons } => format!("{reasons:?}"),
    };
    (
        Case {
            name: String::new(),
            coverage: "",
            files: 0,
            bytes: 0,
            query: query.to_owned(),
            matches: first.matches.len(),
            completion,
            candidate_file_count: first.candidate_file_count,
            scanned_file_count: first.scanned_file_count,
            scanned_bytes: first.scanned_bytes,
            skipped_file_count: first.skipped_file_count,
            cold_ms: cold.as_millis(),
            hot_ms: hot.as_millis(),
        },
        first,
    )
}

fn runtime_miss_needle() -> String {
    format!(
        "AURA_FS_SEARCH_MISS_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    )
}

fn assert_needle_absent(root: &Path, needle: &str) {
    fn walk(path: &Path, needle: &str) -> bool {
        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if walk(&path, needle) {
                    return true;
                }
            } else if fs::read(&path)
                .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(needle))
            {
                return true;
            }
        }
        false
    }
    assert!(!walk(root, needle), "miss needle {needle} was present in {}", root.display());
}

fn percentile(samples: &mut [u128], fraction: f64) -> u128 {
    samples.sort_unstable();
    if samples.is_empty() {
        return 0;
    }
    let index = ((samples.len() as f64 - 1.0) * fraction).round() as usize;
    samples[index.min(samples.len() - 1)]
}

fn measure_samples(
    harness: &Harness,
    query: &str,
    limit: u32,
    repeats: usize,
) -> (Case, Vec<u128>, aura_workspace_domain::FsSearchTextResult) {
    let (case, first) = measure(harness, query, limit);
    let mut hot = Vec::with_capacity(repeats);
    hot.push(case.hot_ms);
    let request = || FsSearchTextRequest {
        workspace_id: harness.workspace_id,
        session_id: harness.session,
        pattern: FsSearchPattern::Literal { text: query.to_owned(), case_sensitive: true },
        root: WorkspacePath::parse(".").expect("root"),
        path_glob: None,
        include_ignored: false,
        include_generated: false,
        limit,
    };
    for _ in 1..repeats {
        let started = Instant::now();
        let result = harness.fs.search_text(request()).expect("sample");
        hot.push(started.elapsed().as_millis());
        assert_eq!(result.matches.len(), first.matches.len());
    }
    (case, hot, first)
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
        bytes += u64::try_from(body.len()).expect("len");
        fs::write(path, body).expect("file");
    }
    bytes
}

#[test]
#[ignore = "full fs.search performance matrix; run once at closeout"]
fn capture_new_engine_performance_matrix() {
    let baseline_rss = current_process_working_set().expect("baseline rss");
    let mut cases = Vec::new();
    let mut sample_sets = serde_json::Map::new();
    let small_miss = runtime_miss_needle();
    let ten_k_miss = runtime_miss_needle();
    let hundred_k_miss = runtime_miss_needle();
    let repo_miss = runtime_miss_needle();

    let small = Harness::new(|root| {
        write_tree(root, 200, b"fn example() { 1 }\n", "unique_small_token");
        fs::write(root.join("readme.md"), "unique_small_token in docs\n").expect("readme");
        assert_needle_absent(root, &small_miss);
    });
    let (mut hit, _) = measure(&small, "unique_small_token", 50);
    hit.name = "small_200_files_hit".into();
    hit.coverage = "same_coverage";
    hit.files = 201;
    cases.push(hit);
    let (mut miss, miss_samples, miss_result) = measure_samples(&small, &small_miss, 100, 5);
    assert!(miss_result.matches.is_empty());
    miss.name = "small_200_files_miss".into();
    miss.coverage = "same_coverage";
    miss.files = 201;
    cases.push(miss);
    sample_sets.insert(
        "small_200_files_miss_hot_ms".into(),
        serde_json::json!({
            "median": percentile(&mut miss_samples.clone(), 0.5),
            "p95": percentile(&mut miss_samples.clone(), 0.95),
            "scanned_bytes": miss_result.scanned_bytes,
            "samples": miss_samples,
        }),
    );

    let ten_k = Harness::new(|root| {
        write_tree(root, 10_000, b"aaaaaaaaaaaaaaaa\n", "token_10k");
        assert_needle_absent(root, &ten_k_miss);
    });
    let (mut ten, _) = measure(&ten_k, "token_10k", 50);
    ten.name = "10k_files_hit".into();
    ten.coverage = "same_coverage";
    ten.files = 10_000;
    cases.push(ten);
    let (mut ten_miss, ten_miss_samples, ten_miss_result) =
        measure_samples(&ten_k, &ten_k_miss, 100, 5);
    assert!(ten_miss_result.matches.is_empty());
    assert_eq!(ten_miss_result.candidate_file_count, 10_000);
    assert_eq!(
        ten_miss_result.scanned_file_count, 10_000,
        "tiny-file full miss leaked scan budget: {:?}",
        ten_miss_result.completion
    );
    assert!(
        matches!(ten_miss_result.completion, FsSearchCompletion::Complete),
        "10k tiny-file full miss must complete, got {:?}",
        ten_miss_result.completion
    );
    ten_miss.name = "10k_files_miss_full_scan".into();
    ten_miss.coverage = "same_coverage_full_scan";
    ten_miss.files = 10_000;
    cases.push(ten_miss);
    sample_sets.insert(
        "10k_files_miss_full_scan_hot_ms".into(),
        serde_json::json!({
            "median": percentile(&mut ten_miss_samples.clone(), 0.5),
            "p95": percentile(&mut ten_miss_samples.clone(), 0.95),
            "scanned_bytes": ten_miss_result.scanned_bytes,
            "scanned_file_count": ten_miss_result.scanned_file_count,
            "samples": ten_miss_samples,
        }),
    );
    let rss_after_10k = current_process_working_set().expect("rss after 10k");
    drop(ten_k);

    let large = Harness::new(|root| {
        fs::write(root.join("tiny.txt"), "keep_me\n").expect("tiny");
        let mut body = vec![b'x'; 8 * 1024 * 1024];
        body.extend_from_slice(b"keep_me\n");
        fs::write(root.join("huge.txt"), body).expect("huge");
    });
    let (mut scanned, _) = measure(&large, "keep_me", 50);
    scanned.name = "8mib_file_scanned_by_new_engine".into();
    scanned.coverage = "new_coverage_old_skipped_2mib";
    scanned.files = 2;
    scanned.bytes = 8 * 1024 * 1024;
    cases.push(scanned);

    let dirty = Harness::new(|root| {
        fs::write(root.join("note.txt"), "disk_only_token\n").expect("note");
    });
    let opened = dirty
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: dirty.workspace_id,
            session_id: dirty.session,
            path: WorkspacePath::parse("note.txt").expect("path"),
            writable: true,
        })
        .expect("open");
    dirty
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: dirty.workspace_id,
                session_id: dirty.session,
                path: WorkspacePath::parse("note.txt").expect("path"),
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
    let (mut buffer, _) = measure(&dirty, "buffer_only_token", 50);
    buffer.name = "dirty_buffer_hit".into();
    buffer.coverage = "same_coverage";
    buffer.files = 1;
    cases.push(buffer);
    let (mut hidden, _) = measure(&dirty, "disk_only_token", 50);
    hidden.name = "dirty_buffer_hides_disk".into();
    hidden.coverage = "same_coverage";
    hidden.files = 1;
    cases.push(hidden);

    let long_line = Harness::new(|root| {
        let mut body = vec![b'x'; 3 * 1024 * 1024];
        body.extend_from_slice(b"long_line_needle\n");
        fs::write(root.join("long.txt"), body).expect("long");
    });
    let (mut line, _) = measure(&long_line, "long_line_needle", 50);
    line.name = "3mib_single_line".into();
    line.coverage = "new_coverage_old_skipped_2mib";
    line.files = 1;
    line.bytes = 3 * 1024 * 1024;
    cases.push(line);

    let corpus = Harness::new(|root| {
        let chunk = vec![b'n'; 50 * 1024 * 1024];
        for index in 0..5 {
            let mut body = chunk.clone();
            if index == 2 {
                body.extend_from_slice(b"needle_250mib\n");
            }
            fs::write(root.join(format!("blob{index}.txt")), body).expect("blob");
        }
        fs::write(root.join("small.txt"), "needle_250mib also here\n").expect("small");
    });
    let (mut corpus_hit, _) = measure(&corpus, "needle_250mib", 50);
    corpus_hit.name = "250mib_corpus_new_scans_50mib_blobs".into();
    corpus_hit.coverage = "new_coverage_old_skipped_2mib";
    corpus_hit.files = 6;
    corpus_hit.bytes = 250 * 1024 * 1024;
    cases.push(corpus_hit);
    let rss_after_250mib = current_process_working_set().expect("rss after 250mib");
    drop(corpus);

    let catalog = Harness::new(|root| {
        write_tree(root, 100_000, b"z\n", "token_100k");
        assert_needle_absent(root, &hundred_k_miss);
    });
    let (mut catalog_hit, _) = measure(&catalog, "token_100k", 50);
    catalog_hit.name = "100k_catalog_items_hit".into();
    catalog_hit.coverage = "same_coverage";
    catalog_hit.files = 100_000;
    cases.push(catalog_hit);
    let (mut catalog_miss, catalog_miss_samples, catalog_miss_result) =
        measure_samples(&catalog, &hundred_k_miss, 100, 3);
    assert!(catalog_miss_result.matches.is_empty());
    catalog_miss.name = "100k_catalog_items_miss_full_scan".into();
    catalog_miss.coverage = "same_coverage_full_scan";
    catalog_miss.files = 100_000;
    cases.push(catalog_miss);
    sample_sets.insert(
        "100k_catalog_items_miss_full_scan_hot_ms".into(),
        serde_json::json!({
            "median": percentile(&mut catalog_miss_samples.clone(), 0.5),
            "p95": percentile(&mut catalog_miss_samples.clone(), 0.95),
            "scanned_bytes": catalog_miss_result.scanned_bytes,
            "scanned_file_count": catalog_miss_result.scanned_file_count,
            "samples": catalog_miss_samples,
        }),
    );
    let rss_after_100k = current_process_working_set().expect("rss after 100k");
    drop(catalog);

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_needle_absent(&repo.join("crates/aura-fs-tools/src"), &repo_miss);
    let repo_harness = Harness::open(repo.display().to_string(), None, "aura-repo");
    let (mut repo_hit, _) = measure(&repo_harness, "SearchCancel", 50);
    repo_hit.name = "aura_repo_hit".into();
    repo_hit.coverage = "same_coverage_repo_text";
    cases.push(repo_hit);
    let (mut repo_miss_case, _) = measure(&repo_harness, &repo_miss, 100);
    repo_miss_case.name = "aura_repo_miss".into();
    repo_miss_case.coverage = "same_coverage_repo_text";
    cases.push(repo_miss_case);

    let cancel_files = Harness::new(|root| {
        write_tree(root, 2_000, b"cancel_token\n", "cancel_token");
    });
    let mut cancel_samples = Vec::new();
    for _ in 0..8 {
        let sample = cancel_after_start(&cancel_files, "cancel_token");
        cancel_samples.push(sample);
    }
    let dense = Harness::new(|root| {
        fs::write(root.join("dense.txt"), "hit\n".repeat(20_000)).expect("dense");
    });
    let dense_cancel = cancel_after_start(&dense, "hit");
    let long_cancel = cancel_after_start(&long_line, "long_line_needle");
    let bulky = Harness::new(|root| {
        fs::write(root.join("note.txt"), "seed\n").expect("seed");
    });
    let opened = bulky
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: bulky.workspace_id,
            session_id: bulky.session,
            path: WorkspacePath::parse("note.txt").expect("path"),
            writable: true,
        })
        .expect("open bulky");
    let bulky_body = format!("{}buffer_only_token\n", "x".repeat(4 * 1024 * 1024));
    bulky
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: bulky.workspace_id,
                session_id: bulky.session,
                path: WorkspacePath::parse("note.txt").expect("path"),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: Revision::new(opened.buffer_revision.value() + 1),
                base_core_revision: opened.buffer_revision,
                content: bulky_body,
                active_editing: true,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit bulky");
    let buffer_cancel = cancel_after_start(&bulky, "buffer_only_token");
    let mut cancel_ms: Vec<u128> = cancel_samples.iter().map(|sample| sample.ms).collect();
    let p95 = percentile(&mut cancel_ms, 0.95);
    std::thread::sleep(Duration::from_millis(50));
    let pool_after = search_pool_load();
    let peak_rss = current_process_working_set().expect("peak rss");

    let payload = serde_json::json!({
        "engine": "in_process_grep_searcher",
        "comparison_method": {
            "same_coverage": "compare latency only on files the old engine actually scanned",
            "same_coverage_full_scan": "no-hit scans of the same file set the old engine would scan; limit=100 so prefix hits cannot hide full-corpus work",
            "new_coverage": "report large-file scans separately; do not claim speedup from scanning less",
            "cold_cache": "first search after process start; not a guaranteed OS-cold cache",
            "hot_cache": "second search in the same process"
        },
        "machine": std::env::var("COMPUTERNAME").ok(),
        "rss": {
            "baseline_working_set_bytes": baseline_rss.working_set_bytes,
            "baseline_peak_working_set_bytes": baseline_rss.peak_working_set_bytes,
            "after_10k_working_set_bytes": rss_after_10k.working_set_bytes,
            "after_10k_peak_working_set_bytes": rss_after_10k.peak_working_set_bytes,
            "after_250mib_working_set_bytes": rss_after_250mib.working_set_bytes,
            "after_250mib_peak_working_set_bytes": rss_after_250mib.peak_working_set_bytes,
            "after_100k_working_set_bytes": rss_after_100k.working_set_bytes,
            "after_100k_peak_working_set_bytes": rss_after_100k.peak_working_set_bytes,
            "end_working_set_bytes": peak_rss.working_set_bytes,
            "end_peak_working_set_bytes": peak_rss.peak_working_set_bytes
        },
        "cancel": {
            "samples_ms": cancel_ms,
            "p95_ms": p95,
            "target_p95_ms": 250,
            "disk_started_cancelled": cancel_samples.iter().all(|sample| sample.cancelled),
            "dense_line_ms": dense_cancel.ms,
            "dense_line_cancelled": dense_cancel.cancelled,
            "huge_line_ms": long_cancel.ms,
            "huge_line_cancelled": long_cancel.cancelled,
            "buffer_ms": buffer_cancel.ms,
            "buffer_cancelled": buffer_cancel.cancelled,
            "pool_active_after": pool_after.active,
            "pool_queued_after": pool_after.queued
        },
        "full_scan_samples": sample_sets,
        "miss_needles": {
            "small": small_miss,
            "ten_k": ten_k_miss,
            "hundred_k": hundred_k_miss,
            "repo": repo_miss
        },
        "cases": cases,
    });
    let out =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("target/fs-search-new-perf.json");
    fs::create_dir_all(out.parent().expect("parent")).expect("target");
    fs::write(&out, serde_json::to_vec_pretty(&payload).expect("json")).expect("write");
    eprintln!("new engine matrix written to {}", out.display());
    assert!(p95 < 250, "cancel P95 {p95}ms exceeds 250ms");
    assert!(cancel_samples.iter().all(|sample| sample.cancelled));
    assert!(dense_cancel.cancelled && long_cancel.cancelled && buffer_cancel.cancelled);
    assert!(pool_after.active <= 4);
    assert_eq!(pool_after.queued, 0);
}

struct CancelSample {
    ms: u128,
    cancelled: bool,
}

fn cancel_after_start(harness: &Harness, query: &str) -> CancelSample {
    let cancel = SearchCancel::new();
    let handle = std::thread::spawn({
        let fs = Arc::clone(&harness.fs);
        let request = FsSearchTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session,
            pattern: FsSearchPattern::Literal { text: query.into(), case_sensitive: true },
            root: WorkspacePath::parse(".").expect("root"),
            path_glob: None,
            include_ignored: false,
            include_generated: false,
            limit: 50,
        };
        let cancel = cancel.clone();
        move || fs.search_text_with_cancel(request, &cancel)
    });
    let waited = Instant::now();
    while !cancel.is_started() {
        assert!(waited.elapsed() < Duration::from_secs(8), "search never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    let started = Instant::now();
    cancel.cancel();
    let joined = handle.join().expect("join");
    CancelSample {
        ms: started.elapsed().as_millis(),
        cancelled: matches!(joined, Err(aura_fs_tools::FsToolError::Cancelled)),
    }
}
