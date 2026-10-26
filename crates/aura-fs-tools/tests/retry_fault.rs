//! Retry-path regression: a stable second read must be kept.
//!
//! The fault injection lives in the crate's unit tests (thread- and path-scoped),
//! so this binary only exercises the production path: a file that changes under
//! a concurrent writer must end in one of the defined terminals, never in a
//! dropped-match state.
use std::{path::Path, sync::Arc, time::Duration};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsSearchCompletion, FsSearchIncompleteReason, FsSearchPattern, FsSearchTextRequest,
    LargeFileReadMode, SessionSummary, WorkspaceCreateCommand,
};

struct Harness {
    fs: Arc<FsToolService>,
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
                    display_name: "retry fault".to_owned(),
                },
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
                    title: "retry".to_owned(),
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
        Self { fs, workspace_id, session, _root: root, _data: data }
    }

    fn search(&self, text: &str, limit: u32) -> aura_workspace_domain::FsSearchTextResult {
        self.fs
            .search_text(FsSearchTextRequest {
                workspace_id: self.workspace_id,
                session_id: self.session,
                pattern: FsSearchPattern::Literal { text: text.to_owned(), case_sensitive: true },
                root: WorkspacePath::parse(".").expect("root"),
                path_glob: None,
                include_ignored: false,
                include_generated: false,
                limit,
            })
            .expect("search")
    }
}

#[test]
fn a_stable_second_read_is_reported_as_a_hit_or_a_bounded_skip() {
    // A writer rewrites the file with a same-length body while the search runs.
    // The contract is: either the stable version's matches are reported, or the
    // file is skipped with `FileChangedDuringSearch`. An empty result with no
    // reason is exactly the C1 defect.
    let needle = b"retry_stable_token\n";
    let version_a = {
        let mut body = vec![b'a'; 512 * 1024];
        body.extend_from_slice(needle);
        body
    };
    let version_b = {
        let mut body = vec![b'b'; 512 * 1024];
        body.extend_from_slice(needle);
        body
    };
    let harness = Harness::new(|root| {
        std::fs::write(root.join("stable.txt"), &version_a).expect("seed");
    });
    let path = harness._root.path().join("stable.txt");
    let rewrite = std::thread::spawn({
        let path = path.clone();
        let version_b = version_b.clone();
        move || {
            for _ in 0..40 {
                let _ = std::fs::write(&path, &version_b);
                std::thread::sleep(Duration::from_millis(3));
            }
        }
    });
    let result = harness.search("retry_stable_token", 50);
    let _ = rewrite.join();

    let matched = result.matches.iter().find(|item| item.path.as_str() == "stable.txt");
    if let Some(matched) = matched {
        // A committed hit must carry one complete version's hash, never a mix.
        let hash_a = aura_domain_common::ContentHash::blake3(&version_a);
        let hash_b = aura_domain_common::ContentHash::blake3(&version_b);
        assert!(
            matched.logical_hash == hash_a || matched.logical_hash == hash_b,
            "committed hash must be one complete version"
        );
    } else {
        let reported = match &result.completion {
            FsSearchCompletion::Partial { reasons } => {
                reasons.contains(&FsSearchIncompleteReason::FileChangedDuringSearch)
            }
            FsSearchCompletion::Complete => false,
        };
        assert!(reported, "a missing hit must be reported as a change, not as a silent complete");
    }
}
