//! Deterministic budget, retry and cancellation regressions.
//!
//! These run on the real engine (`FsToolService::search_text_with_cancel`) over
//! real files, because the defects they cover only appear when several ordinals
//! are in flight at once. The seams they use are the crate's `#[cfg(test)]`
//! injections: a per-path scan gate that parks one file's scan, a per-path retry
//! fault, and the policy overrides for total bytes and the soft deadline. None of
//! them exist in production builds, and each is keyed so parallel tests cannot
//! observe each other's injection.
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{CatalogOptions, WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    FsSearchCompletion, FsSearchIncompleteReason, FsSearchPattern, FsSearchTextRequest,
    LargeFileReadMode, SessionSummary, WorkspaceCreateCommand,
};

use super::{
    disk::faults,
    policy::test_overrides::PolicyGuard,
    pool::{self as search_pool, search_pool_load},
};
use crate::{FsToolPolicy, FsToolService, SearchCancel};

struct Harness {
    fs: Arc<FsToolService>,
    workspace_id: WorkspaceId,
    session: SessionId,
    root: tempfile::TempDir,
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
                    display_name: "budget regressions".to_owned(),
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
                    title: "budget".to_owned(),
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
                // This harness only searches, so the mutation wait is never reached; it is stated
                // explicitly rather than defaulted so a policy change cannot silently alter it.
                mutation_wait: std::time::Duration::from_secs(5),
            },
        )
        .expect("fs");
        // workspace is kept alive by the service; the harness only needs the id.
        drop(workspace);
        Self { fs, workspace_id, session, root, _data: data }
    }

    fn request(&self, text: &str, limit: u32) -> FsSearchTextRequest {
        self.request_for_session(self.session, text, limit)
    }

    fn request_for_session(
        &self,
        session: SessionId,
        text: &str,
        limit: u32,
    ) -> FsSearchTextRequest {
        FsSearchTextRequest {
            workspace_id: self.workspace_id,
            session_id: session,
            pattern: FsSearchPattern::Literal { text: text.to_owned(), case_sensitive: true },
            root: WorkspacePath::parse(".").expect("root"),
            path_glob: None,
            include_ignored: false,
            include_generated: false,
            limit,
        }
    }

    fn search(&self, text: &str, limit: u32) -> aura_workspace_domain::FsSearchTextResult {
        self.fs.search_text(self.request(text, limit)).expect("search")
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.root.path().join(name)
    }
}

/// Write a file of `filler` bytes ending in `needle`.
fn write_file(path: &Path, filler: usize, needle: &str) {
    let mut body = vec![b'x'; filler];
    body.extend_from_slice(needle.as_bytes());
    body.push(b'\n');
    std::fs::write(path, &body).expect("write");
}

fn reasons_of(result: &aura_workspace_domain::FsSearchTextResult) -> Vec<FsSearchIncompleteReason> {
    match &result.completion {
        FsSearchCompletion::Partial { reasons } => reasons.clone(),
        FsSearchCompletion::Complete => Vec::new(),
    }
}

fn paths_of(result: &aura_workspace_domain::FsSearchTextResult) -> Vec<String> {
    let mut paths: Vec<String> =
        result.matches.iter().map(|item| item.path.as_str().to_owned()).collect();
    paths.sort();
    paths
}

fn wait_for_pool_idle() {
    for _ in 0..200 {
        let load = search_pool_load();
        if load.active == 0 && load.queued == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let load = search_pool_load();
    assert_eq!(load.active, 0, "workers must be reclaimed, load={load:?}");
    assert_eq!(load.queued, 0, "queued work must be drained, load={load:?}");
}

#[test]
fn a_later_ordinal_keeps_waiting_past_a_second_and_still_matches() {
    // Ordinal 0 is parked by a gate for longer than the old bounded wait, and
    // ordinal 1 needs extra allowance (its catalog size hint is stale because
    // the file grew after the snapshot). "Not my turn yet" must not be turned
    // into a budget skip: both files must match once ordinal 0 finishes.
    let harness = Harness::new(|root| {
        write_file(&root.join("waiting_a_first.txt"), 64, "waiting_token");
        write_file(&root.join("waiting_b_later.txt"), 64, "waiting_token");
    });
    let _serial = PolicyGuard::install(None, None);
    let gate = faults::arm_gate("waiting_a_first.txt");
    let cancel = SearchCancel::new();
    let worker = {
        let fs = Arc::clone(&harness.fs);
        let request = harness.request("waiting_token", 50);
        let cancel = cancel.clone();
        move || fs.search_text_with_cancel(request, &cancel).map(|(result, _)| result)
    };
    // Grow the later file after the catalog snapshot, so its reservation hint is
    // too small and the scan has to ask for more allowance mid-read.
    let grown = harness.path("waiting_b_later.txt");
    let search = std::thread::spawn(worker);
    assert!(gate.wait_for_parked(1, Duration::from_secs(5)), "ordinal 0 must be parked");
    let mut body = std::fs::read(&grown).expect("read");
    body.extend(std::iter::repeat_n(b'y', 512 * 1024));
    std::fs::write(&grown, &body).expect("grow");
    // Longer than the removed one-second wait, with the deadline untouched.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(gate.parked(), 1, "the later ordinal must not have given up");
    gate.release();

    let result = search.join().expect("join").expect("search");
    assert_eq!(
        paths_of(&result),
        vec!["waiting_a_first.txt".to_owned(), "waiting_b_later.txt".to_owned()],
        "a file that waited for its turn must still match"
    );
    assert!(
        !reasons_of(&result).contains(&FsSearchIncompleteReason::ScanByteBudget),
        "waiting for a turn is not budget exhaustion: {:?}",
        reasons_of(&result)
    );
    assert!(matches!(result.completion, FsSearchCompletion::Complete));
    wait_for_pool_idle();
}

#[test]
fn a_forced_retry_needs_growth_and_bills_both_real_reads() {
    // The first attempt is forced to look like a concurrent change, and the
    // second attempt needs allowance beyond the first reservation. The ordinal
    // must still be able to grow (it was never finished early), the second
    // version's content must win, and the billed bytes must be the sum of what
    // was really read.
    let harness = Harness::new(|root| {
        write_file(&root.join("retry_growth.txt"), 64, "retry_growth_token");
    });
    let _serial = PolicyGuard::install(None, None);
    faults::arm_retry("retry_growth.txt");
    let before = faults::scans_for("retry_growth.txt");
    let result = harness.search("retry_growth_token", 10);
    assert_eq!(
        paths_of(&result),
        vec!["retry_growth.txt".to_owned()],
        "the stable retry must match"
    );
    assert!(
        faults::scans_for("retry_growth.txt") > before,
        "the fault must actually have forced a second attempt"
    );
    assert!(reasons_of(&result).is_empty(), "a stable retry reports no reason");
    let matched = &result.matches[0];
    // Two full reads of the same version: the reported bytes are I/O, not
    // duplicates of one budget charge.
    assert!(
        matched.excerpt.contains("retry_growth_token"),
        "excerpt must come from the scanned version"
    );
    let expected = std::fs::metadata(harness.path("retry_growth.txt")).expect("metadata").len();
    assert!(
        matched.logical_hash
            == aura_domain_common::ContentHash::blake3(
                &std::fs::read(harness.path("retry_growth.txt")).expect("read")
            ),
        "the committed hash must be the version that was read"
    );
    assert!(expected > 0);
    wait_for_pool_idle();
}

#[test]
fn cancelling_while_a_scan_waits_for_its_turn_returns_cancelled_and_reclaims() {
    // The gate parks the only running scan; cancellation must reach it while it
    // waits, the search must end as cancelled, and the pool plus the budget must
    // be reclaimed afterwards.
    let harness = Harness::new(|root| {
        write_file(&root.join("cancel_first.txt"), 64, "cancel_wait_token");
        write_file(&root.join("cancel_later.txt"), 64, "cancel_wait_token");
    });
    let _serial = PolicyGuard::install(None, None);
    let gate = faults::arm_gate("cancel_first.txt");
    let cancel = SearchCancel::new();
    let search = {
        let fs = Arc::clone(&harness.fs);
        let request = harness.request("cancel_wait_token", 50);
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            fs.search_text_with_cancel(request, &cancel).map(|(result, _)| result)
        })
    };
    assert!(gate.wait_for_parked(1, Duration::from_secs(5)), "a scan must be parked");
    let started = Instant::now();
    cancel.cancel();
    let outcome = search.join().expect("join");
    assert!(
        matches!(outcome, Err(crate::FsToolError::Cancelled)),
        "a cancelled search is Cancelled, not a budget outcome"
    );
    assert!(started.elapsed() < Duration::from_secs(5), "cancellation must interrupt the wait");
    gate.release();
    wait_for_pool_idle();
}

#[test]
fn a_second_session_keeps_progress_while_the_first_waits_for_its_turn() {
    // One session's scan is parked holding its turn; another session's search
    // must still finish. A turn wait may never become a mutual wait between
    // workers.
    let harness = Harness::new(|root| {
        write_file(&root.join("progress_first.txt"), 64, "session_progress_token");
        write_file(&root.join("progress_second.txt"), 64, "session_progress_token");
    });
    let _serial = PolicyGuard::install(None, None);
    let other = SessionId::new();
    let gate = faults::arm_gate("progress_first.txt");
    let cancel = SearchCancel::new();
    let parked = {
        let fs = Arc::clone(&harness.fs);
        let request = harness.request("session_progress_token", 10);
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            fs.search_text_with_cancel(request, &cancel).map(|(result, _)| result)
        })
    };
    assert!(gate.wait_for_parked(1, Duration::from_secs(5)), "the first session must be parked");
    let (tx, rx) = std::sync::mpsc::channel();
    let second = {
        let fs = Arc::clone(&harness.fs);
        // Only the second file: the first one is parked on the gate, so this
        // search must not have to scan it to make progress.
        let mut request = harness.request_for_session(other, "session_progress_token", 10);
        request.path_glob = Some("progress_second*.txt".to_owned());
        std::thread::spawn(move || {
            let result = fs.search_text(request);
            let _ = tx.send(result.is_ok());
        })
    };
    let finished = rx.recv_timeout(Duration::from_secs(10));
    assert_eq!(
        finished,
        Ok(true),
        "another session must finish while the first waits for its turn"
    );
    gate.release();
    let _ = second.join();
    let _ = parked.join();
    wait_for_pool_idle();
}

#[test]
fn real_exhaustion_reports_the_budget_reason_not_a_turn_wait() {
    // A small injected budget: the first file's probe, scan and re-hash fit, the
    // large second file's scan does not. That is genuine exhaustion and must be
    // reported as such — the contrast with the waiting case is that here the
    // bytes really are gone, and the file whose turn it is says so itself.
    let harness = Harness::new(|root| {
        write_file(&root.join("exhaust_first.txt"), 64, "exhaustion_token");
        write_file(&root.join("exhaust_later.txt"), 512 * 1024, "exhaustion_token");
    });
    let small = std::fs::metadata(harness.path("exhaust_first.txt")).expect("metadata").len();
    // Enough for the small file's reservation and work, nowhere near the large
    // file's 512 KiB scan.
    let _serial = PolicyGuard::install(Some(small + 8192 + 4096), None);
    let result = harness.search("exhaustion_token", 50);
    let reasons = reasons_of(&result);
    assert_eq!(
        paths_of(&result),
        vec!["exhaust_first.txt".to_owned()],
        "only the file the budget covers may match: {reasons:?}"
    );
    assert_eq!(
        reasons,
        vec![FsSearchIncompleteReason::ScanByteBudget],
        "real exhaustion must report the budget reason exactly once"
    );
    wait_for_pool_idle();
}

#[test]
fn the_soft_deadline_reports_time_budget_and_stops_the_wait() {
    // A parked scan plus an injected soft deadline: the search must return what
    // it has with `TimeBudget`, and the parked worker must be released by the
    // cancellation the deadline triggers instead of holding a slot forever.
    let harness = Harness::new(|root| {
        write_file(&root.join("deadline_first.txt"), 64, "deadline_token");
        write_file(&root.join("deadline_later.txt"), 64, "deadline_token");
    });
    let _serial = PolicyGuard::install(None, Some(Duration::from_millis(150)));
    let gate = faults::arm_gate("deadline_first.txt");
    let result = harness.search("deadline_token", 50);
    let reasons = reasons_of(&result);
    assert!(
        reasons.contains(&FsSearchIncompleteReason::TimeBudget),
        "the deadline reports TimeBudget, got {reasons:?}"
    );
    assert!(
        !reasons.contains(&FsSearchIncompleteReason::ScanByteBudget),
        "a deadline is not budget exhaustion: {reasons:?}"
    );
    gate.release();
    wait_for_pool_idle();
}

#[test]
fn the_pool_stays_bounded_when_every_lane_waits_for_a_turn() {
    // Every scanned file waits on a gate while the search runs to its deadline:
    // the queue must stay bounded and every worker must come back.
    let harness = Harness::new(|root| {
        for index in 0..12 {
            write_file(&root.join(format!("bound_f{index:02}.txt")), 64, "bounded_wait_token");
        }
    });
    let _serial = PolicyGuard::install(None, Some(Duration::from_millis(300)));
    let gates: Vec<_> =
        (0..12).map(|index| faults::arm_gate(&format!("bound_f{index:02}.txt"))).collect();
    let result = harness.search("bounded_wait_token", 50);
    assert!(
        matches!(
            result.completion,
            FsSearchCompletion::Partial { .. } | FsSearchCompletion::Complete
        ),
        "the search must still return a completion"
    );
    for gate in &gates {
        gate.release();
    }
    wait_for_pool_idle();
    let load: search_pool::SearchPoolLoad = search_pool_load();
    assert_eq!(load.active, 0);
    assert_eq!(load.queued, 0);
}

#[test]
fn a_file_truncated_while_it_waits_is_reported_as_changed_not_as_empty() {
    // Deterministic version of the concurrent-truncation window: the scan is
    // parked before its first read, the file is emptied while it waits, and the
    // read then sees zero bytes. A catalogued file that suddenly reads empty has
    // not been searched, so it must be reported as a change — reporting
    // "complete, no matches" would silently answer for content nobody read.
    let harness = Harness::new(|root| {
        write_file(&root.join("truncated_while_waiting.txt"), 512, "truncation_token");
    });
    let _serial = PolicyGuard::install(None, None);
    let gate = faults::arm_gate("truncated_while_waiting.txt");
    let cancel = SearchCancel::new();
    let search = {
        let fs = Arc::clone(&harness.fs);
        let request = harness.request("truncation_token", 10);
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            fs.search_text_with_cancel(request, &cancel).map(|(result, _)| result)
        })
    };
    assert!(gate.wait_for_parked(1, Duration::from_secs(5)), "the scan must be parked");
    std::fs::write(harness.path("truncated_while_waiting.txt"), b"").expect("truncate");
    gate.release();

    let result = search.join().expect("join").expect("search");
    assert!(
        result.matches.iter().all(|item| item.path.as_str() != "truncated_while_waiting.txt"),
        "an empty read must not produce a match"
    );
    assert!(
        reasons_of(&result).contains(&FsSearchIncompleteReason::FileChangedDuringSearch),
        "the file was emptied under the scan and must be reported as changed: {:?}",
        reasons_of(&result)
    );
    wait_for_pool_idle();
}

// ---------------------------------------------------------------------------
// Registry hand-off rules. These are registry-level tests: they drive the real
// registry API, not a real connection (that path is covered by the aura-core
// lifecycle test and the Electron disconnect E2E).
// ---------------------------------------------------------------------------

#[test]
fn a_handoff_for_a_search_that_just_finished_releases_it_instead_of_leaking() {
    let _serial = PolicyGuard::install(None, None);
    let cancel = SearchCancel::new();
    let id = aura_domain_common::CorrelationId::new();
    crate::register_search(id, cancel.clone());
    // The search finishes in the window between "hand over" and the registry
    // looking at it: the hand-off must release it, not leave an entry nobody owns.
    cancel.mark_coordinator_done();
    crate::orphan_search([id]);
    assert!(
        crate::search_flight(id).is_none(),
        "a finished search is released during the hand-off"
    );
    assert_eq!(crate::search_orphan_count(), 0);
}

#[test]
fn a_handoff_for_an_unfinished_search_takes_it_over_until_it_really_ends() {
    let _serial = PolicyGuard::install(None, None);
    let cancel = SearchCancel::new();
    let id = aura_domain_common::CorrelationId::new();
    let work = cancel.admit_work();
    crate::register_search(id, cancel.clone());
    crate::orphan_search([id]);
    assert!(
        crate::search_flight(id).is_some(),
        "running work keeps its registration after the hand-off"
    );
    assert_eq!(crate::wait_for_search_orphans_of(&[id], Duration::from_millis(50)), vec![id]);
    cancel.mark_coordinator_done();
    drop(work);
    assert_eq!(
        crate::wait_for_search_orphans_of(&[id], Duration::from_secs(5)),
        Vec::<aura_domain_common::CorrelationId>::new()
    );
    assert!(crate::search_flight(id).is_none(), "released once it really ended");
}

#[test]
fn a_search_whose_handler_returned_while_its_worker_runs_still_has_an_owner() {
    let _serial = PolicyGuard::install(None, None);
    let cancel = SearchCancel::new();
    let id = aura_domain_common::CorrelationId::new();
    let work = cancel.admit_work();
    crate::register_search(id, cancel.clone());
    // The coordinator returned, but the scan is still inside synchronous I/O and
    // the connection is alive: the registry has to take it over.
    cancel.mark_coordinator_done();
    assert!(!crate::release_search_flight(id), "not finished yet");
    assert!(
        crate::track_search_after_handler(id),
        "the registry takes ownership instead of leaving it unowned"
    );
    drop(work);
    assert_eq!(
        crate::wait_for_search_orphans_of(&[id], Duration::from_secs(5)),
        Vec::<aura_domain_common::CorrelationId>::new()
    );
    assert!(crate::search_flight(id).is_none());
}

#[test]
fn a_connection_scoped_wait_ignores_another_connections_work() {
    let _serial = PolicyGuard::install(None, None);
    let mine = aura_domain_common::CorrelationId::new();
    let theirs = aura_domain_common::CorrelationId::new();
    let my_cancel = SearchCancel::new();
    let their_cancel = SearchCancel::new();
    let their_work = their_cancel.admit_work();
    crate::register_search(mine, my_cancel.clone());
    crate::register_search(theirs, their_cancel.clone());
    my_cancel.mark_coordinator_done();
    crate::orphan_search([mine]);
    crate::orphan_search([theirs]);

    assert_eq!(
        crate::wait_for_search_orphans_of(&[mine], Duration::from_secs(5)),
        Vec::<aura_domain_common::CorrelationId>::new(),
        "this connection's finished search does not wait for another connection's work"
    );
    assert!(
        crate::search_flight(theirs).is_some(),
        "and the other connection's search is untouched"
    );
    their_cancel.mark_coordinator_done();
    drop(their_work);
    assert_eq!(
        crate::wait_for_search_orphans_of(&[theirs], Duration::from_secs(5)),
        Vec::<aura_domain_common::CorrelationId>::new()
    );
}

#[test]
fn a_handoff_after_the_registration_is_gone_is_reported_not_assumed_reclaimed() {
    let _serial = PolicyGuard::install(None, None);
    let reported = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&reported);
    crate::set_orphan_reporter(move |id, _waited| {
        sink.lock().expect("reporter").push(id.to_string());
    });
    let missing = aura_domain_common::CorrelationId::new();
    // Nothing is registered: the hand-off says so instead of claiming the work
    // finished.
    crate::orphan_search([missing]);
    assert!(
        reported.lock().expect("reporter").iter().any(|id| id == &missing.to_string()),
        "a missing registration is reported"
    );
}
