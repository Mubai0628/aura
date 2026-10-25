//! One-shot old-engine baseline. Run before replacing `search_text`.
//! Writes `target/fs-search-old-baseline.json`. Not part of the default suite.
use std::{
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use aura_domain_common::{
    CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    BufferEditCommand, BufferOpenCommand, FsSearchTextRequest, LargeFileReadMode, SessionSummary,
    WorkspaceCreateCommand,
};
use serde::Serialize;

struct Harness {
    fs: Arc<FsToolService>,
    workspace: Arc<WorkspaceRuntime>,
    workspace_id: WorkspaceId,
    session: SessionId,
    _root: tempfile::TempDir,
    _data: tempfile::TempDir,
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
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "old baseline".to_owned(),
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
                    title: "baseline".to_owned(),
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
    files: u64,
    bytes: u64,
    query: String,
    matches: usize,
    truncated: bool,
    elapsed_ms: u128,
    peak_working_set_hint: &'static str,
}

fn search(harness: &Harness, query: &str, limit: u32) -> (Duration, usize, bool) {
    let started = Instant::now();
    let result = harness
        .fs
        .search_text(FsSearchTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session,
            pattern: aura_workspace_domain::FsSearchPattern::Literal {
                text: query.to_owned(),
                case_sensitive: true,
            },
            root: WorkspacePath::parse(".").expect("root"),
            path_glob: None,
            include_ignored: false,
            include_generated: false,
            limit,
        })
        .expect("old search");
    (
        started.elapsed(),
        result.matches.len(),
        !matches!(result.completion, aura_workspace_domain::FsSearchCompletion::Complete),
    )
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
#[ignore = "old-engine baseline; run once before replacing search_text"]
fn capture_old_engine_baseline() {
    let mut cases = Vec::new();
    let small = Harness::new(|root| {
        let bytes = write_tree(root, 200, b"fn example() { 1 }\n", "unique_small_token");
        fs::write(root.join("readme.md"), "unique_small_token in docs\n").expect("readme");
        let _ = bytes;
    });
    let (elapsed, matches, truncated) = search(&small, "unique_small_token", 50);
    cases.push(Case {
        name: "small_200_files_hit".into(),
        files: 201,
        bytes: 0,
        query: "unique_small_token".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "old engine full-file decode",
    });
    let (elapsed, matches, truncated) = search(&small, "no_such_token_zzz", 50);
    cases.push(Case {
        name: "small_200_files_miss".into(),
        files: 201,
        bytes: 0,
        query: "no_such_token_zzz".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "old engine full-file decode",
    });

    let ten_k = Harness::new(|root| {
        write_tree(root, 10_000, b"aaaaaaaaaaaaaaaa\n", "token_10k");
    });
    let (elapsed, matches, truncated) = search(&ten_k, "token_10k", 50);
    cases.push(Case {
        name: "10k_files_hit".into(),
        files: 10_000,
        bytes: 0,
        query: "token_10k".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "old engine full-file decode",
    });

    let large_skip = Harness::new(|root| {
        fs::write(root.join("tiny.txt"), "keep_me\n").expect("tiny");
        fs::write(root.join("huge.txt"), vec![b'x'; 8 * 1024 * 1024]).expect("huge");
        fs::write(root.join("huge.txt"), {
            let mut body = vec![b'x'; 8 * 1024 * 1024];
            body.extend_from_slice(b"keep_me\n");
            body
        })
        .expect("huge needle");
    });
    let (elapsed, matches, truncated) = search(&large_skip, "keep_me", 50);
    cases.push(Case {
        name: "8mib_file_skipped_by_2mib_read_limit".into(),
        files: 2,
        bytes: 8 * 1024 * 1024,
        query: "keep_me".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "old engine skips files above full_text_read_limit_bytes",
    });

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
    let (elapsed, matches, truncated) = search(&dirty, "buffer_only_token", 50);
    cases.push(Case {
        name: "dirty_buffer_hit".into(),
        files: 1,
        bytes: 0,
        query: "buffer_only_token".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "logical snapshot",
    });
    let (elapsed, matches, truncated) = search(&dirty, "disk_only_token", 50);
    cases.push(Case {
        name: "dirty_buffer_hides_disk".into(),
        files: 1,
        bytes: 0,
        query: "disk_only_token".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "logical snapshot",
    });

    let corpus_250 = Harness::new(|root| {
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
    let (elapsed, matches, truncated) = search(&corpus_250, "needle_250mib", 50);
    cases.push(Case {
        name: "250mib_corpus_old_skips_50mib_blobs".into(),
        files: 6,
        bytes: 250 * 1024 * 1024,
        query: "needle_250mib".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "old engine 2MiB skip; small.txt only",
    });

    let catalog_100k = Harness::new(|root| {
        write_tree(root, 100_000, b"z\n", "token_100k");
    });
    let (elapsed, matches, truncated) = search(&catalog_100k, "token_100k", 50);
    cases.push(Case {
        name: "100k_catalog_items_hit".into(),
        files: 100_000,
        bytes: 0,
        query: "token_100k".into(),
        matches,
        truncated,
        elapsed_ms: elapsed.as_millis(),
        peak_working_set_hint: "old sequential full-file decode of tiny files",
    });

    let payload = serde_json::json!({
        "engine": "old_search_text_full_file_decode",
        "full_text_read_limit_bytes": 2097152,
        "comparison_method": {
            "same_coverage": "compare latency only on files the old engine actually scanned",
            "new_coverage": "report large-file scans separately; do not claim speedup from scanning less",
            "cold_cache": "first search after process start; not a guaranteed OS-cold cache",
            "hot_cache": "not captured in this one-shot; new engine repeats twice"
        },
        "machine": std::env::var("COMPUTERNAME").ok(),
        "cases": cases,
    });
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target/fs-search-old-baseline.json");
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).expect("target");
    }
    fs::write(&out, serde_json::to_vec_pretty(&payload).expect("json")).expect("write baseline");
    eprintln!("old baseline written to {}", out.display());
}
