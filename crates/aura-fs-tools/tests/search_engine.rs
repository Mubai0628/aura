use std::{path::Path, sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService, SearchCancel};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    BufferEditCommand, BufferOpenCommand, CatalogEntryKind, FileStorageSource, FsSearchCompletion,
    FsSearchIncompleteReason, FsSearchPattern, FsSearchTextRequest, LargeFileReadMode,
    SessionSummary, WorkspaceCreateCommand,
};

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
                    selected_root: root.path().display().to_string(),
                    display_name: "search engine".to_owned(),
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
                    title: "search".to_owned(),
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

    fn request(
        &self,
        pattern: FsSearchPattern,
        include_ignored: bool,
        include_generated: bool,
        limit: u32,
    ) -> FsSearchTextRequest {
        FsSearchTextRequest {
            workspace_id: self.workspace_id,
            session_id: self.session,
            pattern,
            root: WorkspacePath::parse(".").expect("root"),
            path_glob: None,
            include_ignored,
            include_generated,
            limit,
        }
    }

    fn search(&self, text: &str, limit: u32) -> aura_workspace_domain::FsSearchTextResult {
        self.fs
            .search_text(self.request(
                FsSearchPattern::Literal { text: text.to_owned(), case_sensitive: true },
                false,
                false,
                limit,
            ))
            .expect("search")
    }
}

#[test]
fn literal_and_regex_are_deterministic_and_omit_elapsed() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("a.txt"), "alpha token\n").expect("a");
        std::fs::write(root.join("b.txt"), "beta token\n").expect("b");
    });
    let first = harness.search("token", 50);
    let second = harness.search("token", 50);
    assert_eq!(first.matches.len(), 2);
    assert_eq!(first.matches, second.matches);
    assert!(matches!(first.completion, FsSearchCompletion::Complete));
    let json = serde_json::to_value(&first).expect("json");
    assert!(json.get("elapsed").is_none());
    assert!(json.get("elapsed_ms").is_none());
    let regex = harness
        .fs
        .search_text(FsSearchTextRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session,
            pattern: FsSearchPattern::Regex { pattern: "al+pha".to_owned(), case_sensitive: true },
            root: WorkspacePath::parse(".").expect("root"),
            path_glob: None,
            include_ignored: false,
            include_generated: false,
            limit: 50,
        })
        .expect("regex");
    assert_eq!(regex.matches.len(), 1);
    assert_eq!(regex.matches[0].path.as_str(), "a.txt");
}

#[test]
fn binary_is_skipped_silently_and_large_files_are_scanned() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("ok.txt"), "keep_me\n").expect("ok");
        let mut binary = vec![0_u8; 32];
        binary.extend_from_slice(b"keep_me");
        std::fs::write(root.join("blob.bin"), binary).expect("binary");
        let mut large = vec![b'x'; 3 * 1024 * 1024];
        large.extend_from_slice(b"keep_me\n");
        std::fs::write(root.join("large.txt"), large).expect("large");
    });
    let result = harness.search("keep_me", 50);
    assert!(result.matches.iter().any(|item| item.path.as_str() == "ok.txt"));
    assert!(result.matches.iter().any(|item| item.path.as_str() == "large.txt"));
    assert!(result.matches.iter().all(|item| item.path.as_str() != "blob.bin"));
    assert!(!matches!(
        result.completion,
        FsSearchCompletion::Partial { reasons } if reasons.contains(&FsSearchIncompleteReason::UnsupportedEncoding)
    ));
}

#[test]
fn dirty_buffer_is_the_logical_overlay_and_hides_disk() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("note.txt"), "disk_only_token\n").expect("note");
    });
    let opened = harness
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: harness.workspace_id,
            session_id: harness.session,
            path: WorkspacePath::parse("note.txt").expect("path"),
            writable: true,
        })
        .expect("open");
    harness
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: harness.workspace_id,
                session_id: harness.session,
                path: WorkspacePath::parse("note.txt").expect("path"),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: aura_domain_common::Revision::new(
                    opened.buffer_revision.value() + 1,
                ),
                base_core_revision: opened.buffer_revision,
                content: "buffer_only_token\n".to_owned(),
                active_editing: true,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit");
    let buffer_hit = harness.search("buffer_only_token", 50);
    assert_eq!(buffer_hit.matches.len(), 1);
    assert_eq!(buffer_hit.matches[0].source, FileStorageSource::EditorBuffer);
    let disk_hidden = harness.search("disk_only_token", 50);
    assert!(disk_hidden.matches.is_empty());
}

#[test]
fn cancel_stops_workers_without_partial_success() {
    let harness = Harness::new(|root| {
        for index in 0..200 {
            std::fs::write(root.join(format!("f{index}.txt")), "cancel_token\n").expect("file");
        }
    });
    let cancel = SearchCancel::new();
    cancel.cancel();
    let error = harness
        .fs
        .search_text_with_cancel(
            FsSearchTextRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session,
                pattern: FsSearchPattern::Literal {
                    text: "cancel_token".to_owned(),
                    case_sensitive: true,
                },
                root: WorkspacePath::parse(".").expect("root"),
                path_glob: None,
                include_ignored: false,
                include_generated: false,
                limit: 50,
            },
            &cancel,
        )
        .expect_err("cancelled");
    assert!(matches!(error, aura_fs_tools::FsToolError::Cancelled));
}

#[test]
fn utf16_bom_is_searched_and_git_is_excluded() {
    let harness = Harness::new(|root| {
        let mut utf16 = vec![0xff, 0xfe];
        for unit in "keep_utf16\n".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(root.join("wide.txt"), utf16).expect("utf16");
        std::fs::create_dir_all(root.join(".git")).expect("git dir");
        std::fs::write(root.join(".git").join("config"), "keep_utf16 hidden\n").expect("git");
    });
    let result = harness.search("keep_utf16", 50);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].path.as_str(), "wide.txt");
}

#[test]
fn windows_1252_text_is_searched_and_null_bytes_stay_binary() {
    // Latin-1 text has no reliable declaration in this workspace: the catalog
    // only records encodings it decoded (UTF-8 and BOM forms). The search must
    // report it as unsupported rather than guessing windows-1252 to raise the
    // hit rate, and must never treat the NUL-bearing blob as text.
    let harness = Harness::new(|root| {
        std::fs::write(root.join("latin.txt"), [b'c', b'a', b'f', 0xe9, b'\n']).expect("1252");
        std::fs::write(root.join("blob.bin"), [0, b'c', b'a', b'f', 0xe9]).expect("binary");
        std::fs::write(root.join("ok.txt"), "café\n").expect("utf8");
    });
    let result = harness.search("café", 50);
    assert!(result.matches.iter().all(|item| item.path.as_str() != "blob.bin"));
    assert!(
        result.matches.iter().all(|item| item.path.as_str() != "latin.txt"),
        "undeclared non-UTF-8 text must not be guessed into a hit"
    );
    assert!(result.matches.iter().any(|item| item.path.as_str() == "ok.txt"));
    match result.completion {
        FsSearchCompletion::Partial { reasons } => {
            assert!(reasons.contains(&FsSearchIncompleteReason::UnsupportedEncoding));
        }
        other => panic!("undeclared encoding must stay partial, got {other:?}"),
    }
}

#[test]
fn bomeless_utf8_and_bom_text_still_search() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("plain.txt"), "bomeless_token ok\n").expect("plain");
        std::fs::write(
            root.join("bom.txt"),
            [0xef, 0xbb, 0xbf, b'b', b'o', b'm', b'_', b't', b'o', b'k', b'e', b'n', b'\n'],
        )
        .expect("bom");
        let mut wide = vec![0xff, 0xfe];
        for unit in "wide_token".encode_utf16() {
            wide.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(root.join("wide.txt"), wide).expect("utf16");
    });
    assert_eq!(harness.search("bomeless_token", 50).matches.len(), 1);
    assert!(harness.search("bom_token", 50).matches.iter().any(|m| m.path.as_str() == "bom.txt"));
    assert!(harness.search("wide_token", 50).matches.iter().any(|m| m.path.as_str() == "wide.txt"));
}

#[test]
fn buffer_snapshot_keeps_the_editors_encoding() {
    // A UTF-16 BOM file opened as a writable editor buffer carries the decoded
    // encoding; the search reuses that declaration instead of re-detecting the
    // encoded bytes it is handed.
    let harness = Harness::new(|root| {
        let mut wide = vec![0xff, 0xfe];
        for unit in "disk_only_text".encode_utf16() {
            wide.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(root.join("note.txt"), wide).expect("utf16");
    });
    let opened = harness
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: harness.workspace_id,
            session_id: harness.session,
            path: WorkspacePath::parse("note.txt").expect("path"),
            writable: true,
        })
        .expect("open");
    harness
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: harness.workspace_id,
                session_id: harness.session,
                path: WorkspacePath::parse("note.txt").expect("path"),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: aura_domain_common::Revision::new(
                    opened.buffer_revision.value() + 1,
                ),
                base_core_revision: opened.buffer_revision,
                content: "buffer_utf16_token\n".to_owned(),
                active_editing: true,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit");
    let result = harness.search("buffer_utf16_token", 50);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].source, FileStorageSource::EditorBuffer);
}

#[test]
fn queue_saturation_and_concurrent_sessions_stay_consistent() {
    let harness = Harness::new(|root| {
        for index in 0..300 {
            std::fs::write(root.join(format!("f{index}.txt")), "shared_queue_token\n")
                .expect("file");
        }
    });
    let first = std::thread::spawn({
        let fs = Arc::clone(&harness.fs);
        let request = harness.request(
            FsSearchPattern::Literal {
                text: "shared_queue_token".to_owned(),
                case_sensitive: true,
            },
            false,
            false,
            20,
        );
        move || fs.search_text(request)
    });
    let second = std::thread::spawn({
        let fs = Arc::clone(&harness.fs);
        let request = harness.request(
            FsSearchPattern::Literal {
                text: "shared_queue_token".to_owned(),
                case_sensitive: true,
            },
            false,
            false,
            20,
        );
        move || fs.search_text(request)
    });
    let left = first.join().expect("join").expect("search");
    let right = second.join().expect("join").expect("search");
    assert_eq!(left.matches.len(), 20);
    assert_eq!(right.matches.len(), 20);
    assert_eq!(left.candidate_file_count, right.candidate_file_count);
    assert!(left.scanned_file_count >= 20);
    assert!(left.scanned_file_count <= 300);
}

#[test]
fn without_editor_buffers_no_per_file_probe_runs() {
    // A session with no writable buffer must not pay a per-file buffer probe;
    // the search asks once and dispatches the candidates straight to disk.
    let harness = Harness::new(|root| {
        for index in 0..50 {
            std::fs::write(root.join(format!("f{index:02}.txt")), "probe_free_token\n")
                .expect("file");
        }
    });
    let result = harness.search("probe_free_token", 50);
    assert_eq!(result.matches.len(), 50);
    let diagnostics = harness.fs.last_search_diagnostics().expect("stage diagnostics");
    assert_eq!(diagnostics.buffer_probes, 0, "no live buffer means no probe");
    assert_eq!(diagnostics.buffer_snapshots, 0);
    assert_eq!(diagnostics.open_calls, 50);
}

#[test]
fn stale_catalog_size_hint_does_not_hide_a_shrunken_file() {
    // The catalog recorded a file beyond the per-file limit; the file was then
    // truncated to a few bytes. The opened handle, not the stale hint, decides
    // the limit — the old code skipped it in `prepare_job` from the hint alone.
    let harness = Harness::new(|root| {
        let path = root.join("shrunk.txt");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(65 * 1024 * 1024).expect("sparse size");
        drop(file);
    });
    let path = harness._root.path().join("shrunk.txt");
    let stale = std::fs::metadata(&path).expect("metadata").len();
    assert!(stale > 64 * 1024 * 1024, "the catalog hint must exceed the per-file limit");
    std::fs::write(&path, b"shrink_token\n").expect("truncate");
    let result = harness.search("shrink_token", 50);
    let matched = result
        .matches
        .iter()
        .find(|item| item.path.as_str() == "shrunk.txt")
        .expect("a file recorded as too large but actually small must still be searched");
    assert_eq!(matched.logical_hash, aura_domain_common::ContentHash::blake3(b"shrink_token\n"));
    match result.completion {
        FsSearchCompletion::Complete => {}
        other => panic!("a readable small file must not leave the search partial: {other:?}"),
    }
}

#[test]
fn a_real_oversized_file_is_still_rejected_by_its_handle() {
    // The opposite direction: the hint says nothing useful, the handle does.
    let harness = Harness::new(|root| {
        let path = root.join("huge.txt");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(65 * 1024 * 1024).expect("sparse size");
        drop(file);
    });
    let result = harness.search("anything_at_all", 50);
    assert!(result.matches.is_empty());
    match result.completion {
        FsSearchCompletion::Partial { reasons } => {
            assert!(reasons.contains(&FsSearchIncompleteReason::FileTooLarge), "{reasons:?}");
        }
        other => panic!("an oversized file must be reported as partial: {other:?}"),
    }
}

#[test]
fn search_statistics_account_for_each_candidate_exactly_once() {
    // A mixed corpus (hits, misses, a binary, a limit) must never report more
    // scanned or skipped files than candidates, which is what the old tail
    // accounting did when it counted the undispatched range on top of the
    // outcomes it had already folded. Note that a silently skipped binary is
    // both `scanned` and `skipped` by design, so the two counters are not
    // mutually exclusive and their sum is not compared against the total.
    let harness = Harness::new(|root| {
        for index in 0..120 {
            std::fs::write(root.join(format!("hit{index:03}.txt")), "account_token\n")
                .expect("hit");
        }
        for index in 0..60 {
            std::fs::write(root.join(format!("miss{index:03}.txt")), "nothing to see\n")
                .expect("miss");
        }
        std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).expect("binary");
    });
    for limit in [3_u32, 25, 100] {
        let result = harness.search("account_token", limit);
        assert!(
            result.scanned_file_count <= result.candidate_file_count,
            "limit {limit}: scanned {} exceeds candidates {}",
            result.scanned_file_count,
            result.candidate_file_count
        );
        assert!(
            result.skipped_file_count <= result.candidate_file_count,
            "limit {limit}: skipped {} exceeds candidates {}",
            result.skipped_file_count,
            result.candidate_file_count
        );
        assert!(result.scanned_bytes > 0, "limit {limit}: a scanned corpus must report bytes");
        assert!(
            result.candidate_file_count >= 181,
            "limit {limit}: the corpus was not fully enumerated: {}",
            result.candidate_file_count
        );
    }
}

#[test]
fn repeated_search_is_byte_stable_across_worker_completion_order() {
    // Files with very different scan costs finish in a different order every
    // run; the committed result must not depend on that order.
    let harness = Harness::new(|root| {
        std::fs::write(root.join("a_small.txt"), "order_token\n").expect("small");
        let mut big = vec![b'z'; 512 * 1024];
        big.extend_from_slice(b"order_token\n");
        std::fs::write(root.join("b_big.txt"), &big).expect("big");
        std::fs::write(root.join("c_medium.txt"), "order_token\n".repeat(4_000)).expect("medium");
        std::fs::write(root.join("d_small.txt"), "order_token\n").expect("small");
    });
    let first = harness.search("order_token", 4);
    let second = harness.search("order_token", 4);
    let paths: Vec<&str> = first.matches.iter().map(|item| item.path.as_str()).collect();
    // The result window is filled in candidate order, so a heavy file cannot
    // push a later file out just because it finished last.
    assert_eq!(paths.first().copied(), Some("a_small.txt"));
    assert!(paths.contains(&"b_big.txt"));
    assert!(paths.contains(&"c_medium.txt"));
    assert!(paths.windows(2).all(|pair| pair[0] <= pair[1]), "paths must stay sorted: {paths:?}");
    assert_eq!(
        first.matches.iter().map(|m| m.logical_hash.clone()).collect::<Vec<_>>(),
        second.matches.iter().map(|m| m.logical_hash.clone()).collect::<Vec<_>>()
    );
    assert_eq!(first.scanned_file_count, second.scanned_file_count);
    assert_eq!(first.scanned_bytes, second.scanned_bytes);
}

#[test]
fn commit_window_stays_bounded_while_an_early_ordinal_is_slow() {
    // The first candidate in order is far more expensive than the rest. The
    // window must still bound how many uncommitted results can pile up.
    let harness = Harness::new(|root| {
        let mut slow = vec![b's'; 4 * 1024 * 1024];
        slow.extend_from_slice(b"window_token\n");
        std::fs::write(root.join("a_slow.txt"), &slow).expect("slow");
        for index in 0..400 {
            std::fs::write(root.join(format!("b{index:03}.txt")), "window_token\n").expect("fast");
        }
    });
    let result = harness.search("window_token", 50);
    let diagnostics = harness.fs.last_search_diagnostics().expect("stage diagnostics");
    assert_eq!(result.matches.len(), 50);
    assert_eq!(result.matches[0].path.as_str(), "a_slow.txt");
    assert!(
        diagnostics.window_high_water <= 21,
        "uncommitted dispatch window grew to {}",
        diagnostics.window_high_water
    );
    assert!(diagnostics.pool_active_high_water <= 4);
}

#[test]
fn cancel_before_dispatch_is_cancelled_without_scanning() {
    let harness = Harness::new(|root| {
        for index in 0..200 {
            std::fs::write(root.join(format!("f{index}.txt")), "cancel_wait_token\n")
                .expect("file");
        }
    });
    let cancel = SearchCancel::new();
    cancel.cancel();
    let error = harness
        .fs
        .search_text_with_cancel(
            harness.request(
                FsSearchPattern::Literal {
                    text: "cancel_wait_token".to_owned(),
                    case_sensitive: true,
                },
                false,
                false,
                50,
            ),
            &cancel,
        )
        .expect_err("cancelled before dispatch");
    assert!(matches!(error, aura_fs_tools::FsToolError::Cancelled));
    let diagnostics = harness.fs.last_search_diagnostics().expect("stage diagnostics");
    assert_eq!(diagnostics.open_calls, 0);
}

#[test]
fn result_limit_is_partial_with_time_budget_reason_unused() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("a.txt"), "limit_token\n").expect("a");
        std::fs::write(root.join("b.txt"), "limit_token\n").expect("b");
        std::fs::write(root.join("c.txt"), "limit_token\n").expect("c");
    });
    let result = harness.search("limit_token", 1);
    assert_eq!(result.matches.len(), 1);
    match result.completion {
        FsSearchCompletion::Partial { reasons } => {
            assert!(reasons.contains(&FsSearchIncompleteReason::ResultLimit));
            assert!(!reasons.contains(&FsSearchIncompleteReason::TimeBudget));
        }
        other => panic!("expected partial, got {other:?}"),
    }
}

#[test]
fn same_length_rewrite_does_not_commit_mixed_versions() {
    let needle = b"stable_needle\n";
    let version_a = {
        let mut body = vec![b'a'; 2 * 1024 * 1024];
        body.extend_from_slice(needle);
        body
    };
    let version_b = {
        let mut body = vec![b'b'; 2 * 1024 * 1024];
        body.extend_from_slice(needle);
        body
    };
    let hash_a = aura_domain_common::ContentHash::blake3(&version_a);
    let hash_b = aura_domain_common::ContentHash::blake3(&version_b);
    let harness = Harness::new(|root| {
        std::fs::write(root.join("mutable.txt"), &version_a).expect("seed");
    });
    let path = harness._root.path().join("mutable.txt");
    let rewrite = std::thread::spawn({
        let path = path.clone();
        let version_b = version_b.clone();
        move || {
            for _ in 0..40 {
                let _ = std::fs::write(&path, &version_b);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    });
    let result = harness.search("stable_needle", 50);
    let _ = rewrite.join();
    if let Some(matched) = result.matches.iter().find(|item| item.path.as_str() == "mutable.txt") {
        assert!(
            matched.logical_hash == hash_a || matched.logical_hash == hash_b,
            "committed hash must be one complete version, not a mix"
        );
    } else {
        match result.completion {
            FsSearchCompletion::Partial { reasons } => {
                assert!(
                    reasons.contains(&FsSearchIncompleteReason::FileChangedDuringSearch)
                        || reasons.contains(&FsSearchIncompleteReason::FileTemporarilyUnavailable)
                );
            }
            other => panic!("missing match without change reason: {other:?}"),
        }
    }
}

/// A search answers from the catalogue snapshot it froze when it started.
///
/// With the index already recording that the name is a directory, the path is not a text
/// candidate, so a search that matches nothing under it is complete: the walk searched
/// everything it knew about. This is the settled timing, and the test waits for the index
/// to hold the fact instead of sleeping and hoping it does.
#[test]
fn a_search_answers_from_the_catalogue_it_froze() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("probe.txt"), "fatal_token\n").expect("probe");
        std::fs::write(root.join("ok.txt"), "fatal_token\n").expect("ok");
    });
    let catalog = harness.workspace.catalog(harness.workspace_id).expect("catalog");
    let probe = WorkspacePath::parse("probe.txt").expect("relative path");
    let root = harness._root.path().join("probe.txt");
    std::fs::remove_file(&root).expect("remove file");
    std::fs::create_dir(&root).expect("directory at file path");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (entries, _) = catalog.snapshot().expect("catalog snapshot");
        let settled = entries
            .iter()
            .any(|entry| entry.path == probe && entry.kind == CatalogEntryKind::Directory);
        if settled {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the index must record that the name is a directory"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let result = harness.search("fatal_token", 50);
    assert_eq!(result.candidate_file_count, 1, "only the ordinary file is a candidate");
    assert!(
        matches!(result.completion, FsSearchCompletion::Complete),
        "a settled index answers completely for what it knows: {:?}",
        result.completion
    );
}

/// A candidate frozen as a file, replaced by a directory under the walk, is never a
/// silent "no match".
///
/// The scan of the frozen candidate is parked with the search-fault gate, so the object is
/// replaced *after* the walk took its candidate list and *while* it is about to read it.
/// Whether the engine retries and reports an incomplete search or refuses outright, the one
/// answer it may not give is a complete "no match": bytes nobody could read are not bytes
/// that do not contain the needle.
#[cfg(feature = "search-test-faults")]
#[test]
fn an_object_replaced_under_a_frozen_candidate_is_never_a_complete_no_match() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("probe.txt"), "fatal_token\n").expect("probe");
        std::fs::write(root.join("ok.txt"), "fatal_token\n").expect("ok");
    });
    let catalog = harness.workspace.catalog(harness.workspace_id).expect("catalog");
    let probe = WorkspacePath::parse("probe.txt").expect("relative path");
    let (entries, _) = catalog.snapshot().expect("catalog snapshot");
    assert!(
        entries.iter().any(|entry| entry.path == probe && entry.kind == CatalogEntryKind::File),
        "the frozen candidate list has to hold the file"
    );
    let gate = aura_fs_tools::test_faults::arm_gate("probe.txt");
    let search = std::thread::spawn({
        let fs = Arc::clone(&harness.fs);
        let request = harness.request(
            FsSearchPattern::Literal { text: "fatal_token".into(), case_sensitive: true },
            false,
            false,
            50,
        );
        move || fs.search_text(request)
    });
    assert!(
        gate.wait_for_parked(1, Duration::from_secs(10)),
        "the frozen candidate has to reach the parked scan"
    );
    let path = harness._root.path().join("probe.txt");
    std::fs::remove_file(&path).expect("remove the file under the walk");
    std::fs::create_dir(&path).expect("directory at file path");
    gate.release();

    match search.join().expect("search thread") {
        Err(_) => {}
        Ok(result) => assert!(
            !matches!(result.completion, FsSearchCompletion::Complete),
            "a replaced object must not be reported as a complete no-match: {result:?}"
        ),
    }
}
#[test]
fn dense_hits_are_capped_at_the_request_limit() {
    let harness = Harness::new(|root| {
        let body = "hit\n".repeat(5_000);
        std::fs::write(root.join("dense.txt"), body).expect("dense");
    });
    let result = harness.search("hit", 7);
    assert_eq!(result.matches.len(), 7);
    match result.completion {
        FsSearchCompletion::Partial { reasons } => {
            assert!(reasons.contains(&FsSearchIncompleteReason::ResultLimit));
        }
        other => panic!("expected result limit, got {other:?}"),
    }
}

#[test]
fn empty_line_regex_and_dollar_are_found() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("lines.txt"), "alpha\n\nomega\n").expect("lines");
    });
    let empty = harness
        .fs
        .search_text(harness.request(
            FsSearchPattern::Regex { pattern: "^$".into(), case_sensitive: true },
            false,
            false,
            50,
        ))
        .expect("empty-line regex");
    assert_eq!(empty.matches.len(), 1);
    assert_eq!(empty.matches[0].line_index, 1);
    let dollar = harness
        .fs
        .search_text(harness.request(
            FsSearchPattern::Regex { pattern: "alpha$".into(), case_sensitive: true },
            false,
            false,
            50,
        ))
        .expect("dollar regex");
    assert_eq!(dollar.matches.len(), 1);
}

#[test]
fn buffer_logical_size_is_reserved_instead_of_disk_size() {
    let harness = Harness::new(|root| {
        let mut disk = vec![b'x'; 4 * 1024 * 1024];
        disk.extend_from_slice(b"disk_only_token\n");
        std::fs::write(root.join("note.txt"), disk).expect("disk");
    });
    let opened = harness
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: harness.workspace_id,
            session_id: harness.session,
            path: WorkspacePath::parse("note.txt").expect("path"),
            writable: true,
        })
        .expect("open");
    harness
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: harness.workspace_id,
                session_id: harness.session,
                path: WorkspacePath::parse("note.txt").expect("path"),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: aura_domain_common::Revision::new(
                    opened.buffer_revision.value() + 1,
                ),
                base_core_revision: opened.buffer_revision,
                content: "buffer_only_token\n".to_owned(),
                active_editing: true,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit");
    let result = harness.search("buffer_only_token", 50);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].source, FileStorageSource::EditorBuffer);
    assert!(
        result.scanned_bytes < 64 * 1024,
        "buffer inversion must not bill the 4MiB disk view, billed {}",
        result.scanned_bytes
    );
}

#[test]
fn ignored_and_generated_paths_follow_request_flags() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join(".gitignore"), "secret.txt\n").expect("gitignore");
        std::fs::write(root.join("secret.txt"), "flag_token\n").expect("ignored");
        std::fs::create_dir_all(root.join("node_modules/pkg")).expect("generated dir");
        std::fs::write(root.join("node_modules/pkg/mod.js"), "flag_token\n").expect("generated");
        std::fs::write(root.join("ok.txt"), "flag_token\n").expect("ok");
    });
    let hidden = harness.search("flag_token", 50);
    assert_eq!(hidden.matches.len(), 1);
    assert_eq!(hidden.matches[0].path.as_str(), "ok.txt");
    let with_ignored = harness
        .fs
        .search_text(harness.request(
            FsSearchPattern::Literal { text: "flag_token".into(), case_sensitive: true },
            true,
            false,
            50,
        ))
        .expect("ignored");
    assert!(with_ignored.matches.iter().any(|item| item.path.as_str() == "secret.txt"));
    assert!(
        with_ignored.matches.iter().all(|item| item.path.as_str() != "node_modules/pkg/mod.js")
    );
    let with_generated = harness
        .fs
        .search_text(harness.request(
            FsSearchPattern::Literal { text: "flag_token".into(), case_sensitive: true },
            false,
            true,
            50,
        ))
        .expect("generated");
    assert!(
        with_generated.matches.iter().any(|item| item.path.as_str() == "node_modules/pkg/mod.js")
    );
    assert!(with_generated.matches.iter().all(|item| item.path.as_str() != "secret.txt"));
}

#[test]
fn huge_line_cancel_after_start_returns_cancelled() {
    let harness = Harness::new(|root| {
        let mut body = vec![b'x'; 2 * 1024 * 1024];
        body.extend_from_slice(b"cancel_after_start\n");
        std::fs::write(root.join("long.txt"), body).expect("long");
    });
    let cancel = SearchCancel::new();
    let handle = std::thread::spawn({
        let fs = Arc::clone(&harness.fs);
        let request = harness.request(
            FsSearchPattern::Literal { text: "cancel_after_start".into(), case_sensitive: true },
            false,
            false,
            50,
        );
        let cancel = cancel.clone();
        move || fs.search_text_with_cancel(request, &cancel)
    });
    let waited = std::time::Instant::now();
    while !cancel.is_started() {
        assert!(waited.elapsed() < Duration::from_secs(5), "search never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    cancel.cancel();
    let error = handle.join().expect("join").expect_err("cancelled");
    assert!(matches!(error, aura_fs_tools::FsToolError::Cancelled));
}

#[test]
fn two_sessions_can_search_the_same_workspace() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("a.txt"), "shared_token\n").expect("a");
        std::fs::write(root.join("b.txt"), "shared_token\n").expect("b");
    });
    let first = {
        let fs = Arc::clone(&harness.fs);
        let request = harness.request(
            FsSearchPattern::Literal { text: "shared_token".into(), case_sensitive: true },
            false,
            false,
            50,
        );
        std::thread::spawn(move || fs.search_text(request))
    };
    let second = {
        let fs = Arc::clone(&harness.fs);
        let mut request = harness.request(
            FsSearchPattern::Literal { text: "shared_token".into(), case_sensitive: true },
            false,
            false,
            50,
        );
        request.session_id = SessionId::new();
        std::thread::spawn(move || fs.search_text(request))
    };
    let first = first.join().expect("first join").expect("first search");
    let second = second.join().expect("second join").expect("second search");
    assert_eq!(first.matches.len(), 2);
    assert_eq!(first.matches, second.matches);
}

#[test]
fn long_excerpt_match_stays_within_1024_scalars() {
    let harness = Harness::new(|root| {
        let line: String = "é".repeat(2048) + "\n";
        std::fs::write(root.join("wide.txt"), line).expect("wide");
    });
    let result = harness
        .fs
        .search_text(harness.request(
            FsSearchPattern::Regex { pattern: "é+".into(), case_sensitive: true },
            false,
            false,
            1,
        ))
        .expect("long match");
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].excerpt.chars().count(), 1024);
    assert_eq!(result.matches[0].match_character_length, 2048);
    assert!(result.matches[0].excerpt_truncated_after);
}

#[test]
fn empty_pattern_is_invalid_request() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("a.txt"), "token\n").expect("a");
    });
    let error = harness
        .fs
        .search_text(harness.request(
            FsSearchPattern::Literal { text: String::new(), case_sensitive: true },
            false,
            false,
            50,
        ))
        .expect_err("empty pattern");
    match error {
        aura_fs_tools::FsToolError::InvalidRequest(message) => {
            assert!(message.contains("pattern"));
        }
        other => panic!("expected invalid request, got {other:?}"),
    }
}

#[test]
fn file_over_max_file_bytes_is_partial_file_too_large() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("ok.txt"), "keep_me\n").expect("ok");
        let huge = std::fs::File::create(root.join("huge.txt")).expect("create huge");
        huge.set_len(64 * 1024 * 1024 + 16).expect("logical size over max");
    });
    let result = harness.search("keep_me", 50);
    assert!(result.matches.iter().any(|item| item.path.as_str() == "ok.txt"));
    assert!(result.matches.iter().all(|item| item.path.as_str() != "huge.txt"));
    match result.completion {
        FsSearchCompletion::Partial { reasons } => {
            assert!(reasons.contains(&FsSearchIncompleteReason::FileTooLarge));
        }
        other => panic!("expected FileTooLarge partial, got {other:?}"),
    }
}
