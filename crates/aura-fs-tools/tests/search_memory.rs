//! Per-case memory measurement in separate processes.
//!
//! Parent mode (default, ignored by the default suite) spawns one child per
//! case so a process-lifetime peak cannot leak between cases:
//! `cargo test -p aura-fs-tools --offline --test search_memory -- --ignored --nocapture --test-threads=1`
//!
//! Child mode is selected by `AURA_SEARCH_MEMORY_CASE` and is not a test.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_windows_platform::current_process_working_set;
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsSearchPattern, FsSearchTextRequest, LargeFileReadMode, SessionSummary, WorkspaceCreateCommand,
};
use serde::Serialize;

const CASE_ENV: &str = "AURA_SEARCH_MEMORY_CASE";
const OUT_ENV: &str = "AURA_SEARCH_MEMORY_OUT";

#[derive(Serialize, Clone)]
struct MemoryReading {
    working_set_bytes: u64,
    peak_working_set_bytes: u64,
    private_bytes: u64,
}

#[derive(Serialize)]
struct MemoryReport {
    case: String,
    baseline_before_workspace: MemoryReading,
    after_catalog_ready: MemoryReading,
    during_search: MemoryReading,
    endpoint: MemoryReading,
    matches: usize,
    scanned_file_count: u64,
    scanned_bytes: u64,
    completion: String,
    corpus: CorpusFacts,
}

#[derive(Serialize)]
struct CorpusFacts {
    files: u64,
    bytes: u64,
    layout: String,
}

fn reading() -> MemoryReading {
    let value = current_process_working_set().expect("working set");
    MemoryReading {
        working_set_bytes: u64::try_from(value.working_set_bytes).unwrap_or(u64::MAX),
        peak_working_set_bytes: u64::try_from(value.peak_working_set_bytes).unwrap_or(u64::MAX),
        private_bytes: u64::try_from(value.private_bytes).unwrap_or(u64::MAX),
    }
}

struct CaseSpec {
    name: &'static str,
    query: String,
    limit: u32,
    files: u64,
    bytes: u64,
    layout: &'static str,
    build: fn(&Path) -> u64,
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

fn cases() -> Vec<CaseSpec> {
    vec![
        CaseSpec {
            name: "small_200_files_hit",
            query: "unique_small_token".to_owned(),
            limit: 50,
            files: 200,
            bytes: 0,
            layout: "200 small files",
            build: |root| write_tree(root, 200, b"fn example() { 1 }\n", "unique_small_token"),
        },
        CaseSpec {
            name: "10k_files_hit_prefix",
            query: "token_10k".to_owned(),
            limit: 50,
            files: 10_000,
            bytes: 0,
            layout: "10k tiny files, every 17th carries the needle",
            build: |root| write_tree(root, 10_000, b"aaaaaaaaaaaaaaaa\n", "token_10k"),
        },
        CaseSpec {
            name: "250mib_corpus",
            query: "needle_250mib".to_owned(),
            limit: 50,
            files: 6,
            bytes: 250 * 1024 * 1024,
            layout: "5 x 50 MiB blobs plus a small file",
            build: |root| {
                let chunk = vec![b'n'; 50 * 1024 * 1024];
                for index in 0..5 {
                    let mut body = chunk.clone();
                    if index == 2 {
                        body.extend_from_slice(b"needle_250mib\n");
                    }
                    fs::write(root.join(format!("blob{index}.txt")), &body).expect("blob");
                }
                fs::write(root.join("small.txt"), "needle_250mib also here\n").expect("small");
                (50 * 1024 * 1024_u64) * 5
            },
        },
    ]
}

fn open_harness(root: &Path) -> (Arc<FsToolService>, WorkspaceId, SessionId, tempfile::TempDir) {
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
                display_name: "memory".to_owned(),
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
                title: "memory".to_owned(),
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
    (fs, workspace_id, session, data)
}

fn run_case(name: &str, out: &Path) {
    let baseline_before_workspace = reading();
    let spec = cases()
        .into_iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("unknown memory case {name}"));
    let root = tempfile::tempdir().expect("corpus root");
    let bytes = (spec.build)(root.path());
    let (fs, workspace_id, session, _data) = open_harness(root.path());
    let after_catalog_ready = reading();
    let request = FsSearchTextRequest {
        workspace_id,
        session_id: session,
        pattern: FsSearchPattern::Literal { text: spec.query.clone(), case_sensitive: true },
        root: WorkspacePath::parse(".").expect("root"),
        path_glob: None,
        include_ignored: false,
        include_generated: false,
        limit: spec.limit,
    };
    let handle = std::thread::spawn({
        let fs = Arc::clone(&fs);
        let request = request.clone();
        move || fs.search_text(request)
    });
    std::thread::sleep(Duration::from_millis(25));
    let during_search = reading();
    let result = handle.join().expect("join").expect("search");
    // Reading immediately after the search shares the moment with the settled
    // reading; keep only the settled one to avoid a duplicate measurement.
    // Let the watcher and harness settle before the final reading is trusted.
    std::thread::sleep(Duration::from_millis(250));
    let settled = reading();
    let report = MemoryReport {
        case: spec.name.to_owned(),
        baseline_before_workspace,
        after_catalog_ready,
        during_search,
        endpoint: settled,
        matches: result.matches.len(),
        scanned_file_count: result.scanned_file_count,
        scanned_bytes: result.scanned_bytes,
        completion: format!("{:?}", result.completion),
        corpus: CorpusFacts {
            files: spec.files,
            bytes: if spec.bytes > 0 { spec.bytes } else { bytes },
            layout: spec.layout.to_owned(),
        },
    };
    fs::write(out, serde_json::to_vec_pretty(&report).expect("json")).expect("write report");
}

#[test]
#[ignore = "per-case memory measurement; spawns one child process per case"]
fn capture_search_memory() {
    if let Ok(case) = std::env::var(CASE_ENV) {
        let out = std::env::var(OUT_ENV).expect("child output path");
        run_case(&case, Path::new(&out));
        return;
    }

    let exe = std::env::current_exe().expect("current exe");
    let stamp = std::env::var("AURA_SEARCH_BENCH_STAMP").unwrap_or_else(|_| "run".to_owned());
    let run_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join(format!("fs-search-memory-{stamp}"));
    fs::create_dir_all(&run_dir).expect("run dir");
    println!("memory output: {}", run_dir.display());
    let mut failures = Vec::new();
    for case in cases() {
        let out = run_dir.join(format!("{}.json", case.name));
        let status = Command::new(&exe)
            .arg("--exact")
            .arg("capture_search_memory")
            .arg("--ignored")
            .arg("--nocapture")
            .env(CASE_ENV, case.name)
            .env(OUT_ENV, &out)
            .status()
            .expect("spawn child");
        if !status.success() {
            failures.push(case.name.to_owned());
            continue;
        }
        assert!(out.is_file(), "child did not write {}", out.display());
    }
    assert!(failures.is_empty(), "memory cases failed: {failures:?}");
    println!("memory measurement complete");
}
