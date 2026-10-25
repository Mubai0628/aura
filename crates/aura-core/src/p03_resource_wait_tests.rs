//! The actual P03 async owner, blocking capacity and filesystem; no copied queue algorithm.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use aura_domain_common::{ContentHash, SessionId, WorkspaceId, WorkspacePath};
use aura_workspace_domain::{
    CommitRequest, ExecutionId, FsExpectedState, LineEnding, ResourceAccess, TextEncoding,
    WorkspaceFileTarget,
};

struct Case {
    tree: tempfile::TempDir,
    _db: tempfile::TempDir,
    services: Arc<P03Services>,
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    workspace: WorkspaceId,
    session: SessionId,
}

impl Case {
    fn new() -> Self {
        let (tree, db, persistence, workspace, id, session) =
            crate::s045b_pipe_gate_tests::workspace_fixture();
        let fs = FsToolService::new(
            Arc::clone(&workspace),
            Arc::clone(&persistence),
            FsToolPolicy::default(),
        )
        .unwrap();
        let journal = Arc::new(
            crate::broker::BrokerJournal::open(&db.path().join("v2-editor"))
                .expect("open isolated editor receipt authority"),
        );
        Self {
            tree,
            _db: db,
            services: Arc::new(P03Services {
                fs,
                workspace,
                journal,
                blocking: crate::blocking_pool::BlockingPool::default(),
            }),
            persistence,
            workspace: id,
            session,
        }
    }

    fn params(&self, path: &str) -> Value {
        serde_json::to_value(FsWriteFileRequest {
            workspace_id: self.workspace,
            session_id: self.session,
            target: WorkspaceFileTarget { path: WorkspacePath::parse(path).unwrap() },
            content: "new data".into(),
            expected: FsExpectedState::Absent,
            encoding: Some(TextEncoding::Utf8),
            line_ending: Some(LineEnding::Lf),
        })
        .unwrap()
    }

    fn hold(&self) -> aura_workspace_domain::ResourceCommit {
        let facts = self.services.workspace.resource_facts(self.workspace, self.session).unwrap();
        self.services
            .workspace
            .eligibility_owner()
            .commit(
                &CommitRequest::one(
                    facts.slot(&WorkspacePath::parse("held.txt").unwrap()).unwrap(),
                    ResourceAccess::Write,
                ),
                self.session,
                ExecutionId::next(),
                None,
            )
            .unwrap()
    }

    fn spawn(&self) -> tokio::task::JoinHandle<P03Dispatch> {
        let services = Arc::clone(&self.services);
        let params = self.params("held.txt");
        tokio::spawn(async move {
            services
                .dispatch_main_command_bounded(
                    method::FS_WRITE_FILE,
                    &params,
                    CorrelationId::new(),
                    Instant::now() + Duration::from_secs(3),
                )
                .await
        })
    }

    async fn wait_for_attempts(&self, count: u64) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.services.blocking_counters().completed < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("real handlers made the refused attempts");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn four_resource_waiters_leave_workers_available_for_an_unrelated_write() {
    let case = Case::new();
    let hold = case.hold();
    let waiters = (0..4).map(|_| case.spawn()).collect::<Vec<_>>();
    case.wait_for_attempts(4).await;
    assert!(waiters.iter().all(|task| !task.is_finished()), "all four still really wait");
    let outcome = case
        .services
        .dispatch_main_command_bounded(
            method::FS_WRITE_FILE,
            &case.params("unrelated.txt"),
            CorrelationId::new(),
            Instant::now() + Duration::from_millis(800),
        )
        .await;
    assert!(
        matches!(outcome, P03Dispatch::Completed(Ok(_))),
        "unrelated effect must finish: {}",
        match outcome {
            P03Dispatch::Completed(Err(e)) => e.message,
            P03Dispatch::Refused { .. } => "worker refused".into(),
            _ => "unknown".into(),
        }
    );
    assert_eq!(
        std::fs::read_to_string(case.tree.path().join("unrelated.txt")).unwrap(),
        "new data"
    );
    assert!(!case.tree.path().join("held.txt").exists());
    for waiter in waiters {
        waiter.abort();
        let Err(error) = waiter.await else { panic!("the waiting task must be cancelled") };
        assert!(error.is_cancelled());
    }
    case.services.workspace.eligibility_owner().release_commit(hold);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_resource_wait_does_not_leave_a_worker_that_writes_later() {
    let case = Case::new();
    let hold = case.hold();
    let waiter = case.spawn();
    case.wait_for_attempts(1).await;
    waiter.abort();
    let Err(error) = waiter.await else { panic!("the waiting task must be cancelled") };
    assert!(error.is_cancelled());
    case.services.workspace.eligibility_owner().release_commit(hold);
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(!case.tree.path().join("held.txt").exists());
    assert_eq!(case.services.workspace.eligibility_owner().snapshot().commits, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resource_retries_never_extend_the_original_request_deadline() {
    let case = Case::new();
    let hold = case.hold();
    let began = Instant::now();
    let outcome = case
        .services
        .dispatch_main_command_bounded(
            method::FS_WRITE_FILE,
            &case.params("held.txt"),
            CorrelationId::new(),
            began + Duration::from_millis(180),
        )
        .await;
    let P03Dispatch::Completed(Err(error)) = outcome else {
        panic!("a resource-only refusal has no unconfirmed effect")
    };
    assert_eq!(error.code.as_str(), FILE_RESOURCE_BUSY_CODE);
    assert!(
        began.elapsed() < Duration::from_secs(1),
        "the original deadline may not restart with each retry"
    );
    assert!(case.services.blocking_counters().completed > 1);
    assert!(!case.tree.path().join("held.txt").exists());
    case.services.workspace.eligibility_owner().release_commit(hold);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn release_rechecks_the_original_absent_condition_instead_of_overwriting_a_competitor() {
    let case = Case::new();
    let hold = case.hold();
    let waiter = case.spawn();
    case.wait_for_attempts(1).await;
    std::fs::write(case.tree.path().join("held.txt"), "competitor").unwrap();
    case.services.workspace.eligibility_owner().release_commit(hold);
    let outcome = waiter.await.unwrap();
    assert!(
        matches!(outcome, P03Dispatch::Completed(Err(ref e)) if e.code.as_str() == "fs.expected_state_conflict")
    );
    assert_eq!(std::fs::read_to_string(case.tree.path().join("held.txt")).unwrap(), "competitor");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_authority_is_rechecked_between_resource_attempts() {
    let case = Case::new();
    let hold = case.hold();
    let allowed = Arc::new(AtomicBool::new(true));
    let check = Arc::clone(&allowed);
    let services = Arc::clone(&case.services);
    let params = case.params("held.txt");
    let waiter = tokio::spawn(async move {
        services
            .dispatch_main_command_owned(
                method::FS_WRITE_FILE,
                &params,
                CorrelationId::new(),
                Instant::now() + Duration::from_secs(3),
                None,
                Arc::new(move || {
                    if check.load(Ordering::Acquire) {
                        Ok(())
                    } else {
                        Err(envelope(
                            "permission.denied",
                            ErrorCategory::Permission,
                            "live authority was revoked".to_owned(),
                            false,
                            CorrelationId::new(),
                        ))
                    }
                }),
            )
            .await
    });
    case.wait_for_attempts(1).await;
    allowed.store(false, Ordering::Release);
    let outcome = waiter.await.unwrap();
    assert!(
        matches!(outcome, P03Dispatch::Completed(Err(ref e)) if e.code.as_str() == "permission.denied")
    );
    case.services.workspace.eligibility_owner().release_commit(hold);
    assert!(!case.tree.path().join("held.txt").exists());
}

/// **R1d: a real cancellation recorded while a request waits stops it from ever writing.**
///
/// The requirement is exact about the boundary. An intent that arrives while the request is parked
/// in the resource wait names work that has not started — no journal row, no side effect — so
/// honouring it costs nothing and is the whole point: releasing the holder afterwards must **not**
/// produce a late write. This drives the real `aura.cancel` entry (`cancel_resource_wait`, the same
/// function the route calls) rather than aborting a task, because an abort is not a cancellation the
/// product can produce.
///
/// The second half is the boundary itself: an intent recorded for a request that already started is
/// deliberately **not** handled by this path, so the assertion is that only the waiting request is
/// affected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancellation_recorded_while_waiting_stops_the_write_on_release() {
    let case = Case::new();
    let hold = case.hold();
    let correlation = CorrelationId::new();
    let services = Arc::clone(&case.services);
    let params = case.params("held.txt");
    let waiter = tokio::spawn(async move {
        services
            .dispatch_main_command_bounded(
                method::FS_WRITE_FILE,
                &params,
                correlation,
                Instant::now() + Duration::from_secs(3),
            )
            .await
    });
    // The request really reaches the resource wait — a real refused attempt exists — and it is still
    // waiting, which is the state a cancellation is allowed to stop.
    case.wait_for_attempts(1).await;
    assert!(!waiter.is_finished(), "the request is really waiting for the resource");

    // The real cancellation entry the `aura.cancel` route uses.
    aura_fs_tools::cancel_resource_wait(correlation);

    // The holder releases. The pre-fix shape wrote here; this one must not.
    case.services.workspace.eligibility_owner().release_commit(hold);
    let outcome = waiter.await.unwrap();
    let P03Dispatch::Completed(Err(error)) = outcome else {
        panic!("a cancelled wait must answer, not run the request");
    };
    assert_eq!(
        error.code.as_str(),
        "fs.search_cancelled",
        "a cancellation is reported as a cancellation, not as a resource conflict: {error:?}"
    );
    assert_eq!(
        error.terminal,
        aura_runtime_contract::IpcRequestTerminal::Cancelled,
        "the terminal records that the intent was honoured"
    );
    assert!(
        !case.tree.path().join("held.txt").exists(),
        "**no late write may happen after a cancellation**: releasing the holder must not publish"
    );
    assert_eq!(
        case.services.workspace.eligibility_owner().snapshot().commits,
        0,
        "the cancelled request holds no resource after it stopped waiting"
    );
    // The mark is cleared, so it cannot affect a later request.
    assert!(
        !aura_fs_tools::resource_wait_cancelled(correlation),
        "a request that stopped waiting holds no live cancellation mark"
    );
}

/// The legitimate control: with **no** cancellation the same wait really writes on release.
///
/// Without this, a blanket refusal to write after any wait would pass the test above while breaking
/// the ordinary case the feature exists for.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_uncancelled_wait_really_writes_when_the_holder_releases() {
    let case = Case::new();
    let hold = case.hold();
    let waiter = case.spawn();
    case.wait_for_attempts(1).await;
    assert!(!waiter.is_finished(), "the request is really waiting for the resource");
    case.services.workspace.eligibility_owner().release_commit(hold);
    let outcome = waiter.await.unwrap();
    assert!(
        matches!(outcome, P03Dispatch::Completed(Ok(_))),
        "an uncancelled wait must complete its write: {outcome:?}"
    );
    assert_eq!(
        std::fs::read_to_string(case.tree.path().join("held.txt")).unwrap(),
        "new data",
        "the real file effect happened"
    );
}

/// Component coverage: real blocking capacity, file effects and session authority readability.
/// This fixture does not invoke the Agent backend or its global tool scheduler and cannot prove
/// that an Agent wait releases that scheduler's permit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resource_wait_releases_the_worker_and_the_session_authority() {
    let case = Case::new();
    let hold = case.hold();
    let capacity = case.services.blocking_capacity();
    // Every worker's worth of related requests, so the pool would be fully occupied if a waiting
    // request still held its worker.
    let waiters = (0..capacity).map(|_| case.spawn()).collect::<Vec<_>>();
    case.wait_for_attempts(capacity as u64).await;
    assert!(
        waiters.iter().all(|waiter| !waiter.is_finished()),
        "every related request is still waiting, so nothing has been written"
    );

    // **An unrelated request really completes** with the whole capacity waiting. This is the fact
    // that a held worker would make impossible.
    let outcome = case
        .services
        .dispatch_main_command_bounded(
            method::FS_WRITE_FILE,
            &case.params("unrelated.txt"),
            CorrelationId::new(),
            Instant::now() + Duration::from_secs(5),
        )
        .await;
    assert!(
        matches!(outcome, P03Dispatch::Completed(Ok(_))),
        "an unrelated write must complete while a full set of related requests waits: {outcome:?}"
    );
    assert_eq!(
        std::fs::read_to_string(case.tree.path().join("unrelated.txt")).unwrap(),
        "new data",
        "the unrelated request really reached the disk"
    );

    // **The session's long authority is readable.** A waiter that still held it would block an
    // ordinary mode change for this session; the read is synchronous, so it runs on its own thread
    // exactly as the production authority read does.
    let persistence = Arc::clone(&case.persistence);
    let session = case.session;
    let authority = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            persistence.with_session_permission_authority(session, |_mode| ()).is_ok()
        }),
    )
    .await;
    assert!(
        authority.is_ok(),
        "the session's authority must be readable while the waiters wait: a waiter that still held \
         it would block an ordinary mode change"
    );
    let read = authority.expect("the authority read returned inside its own budget");
    assert!(
        read.is_ok(),
        "the session permission authority must really be readable while the waiters wait: {:?}",
        read.err()
    );

    // Release the holder: the waiters really complete, and none of them is reported as a terminal
    // tool error or an unknown effect.
    case.services.workspace.eligibility_owner().release_commit(hold);
    let mut written = 0;
    for waiter in waiters {
        let outcome = waiter.await.unwrap();
        assert!(
            !matches!(outcome, P03Dispatch::Unknown { .. }),
            "a short-commit wait decided nothing, so it must never be recorded as an unknown effect"
        );
        if matches!(outcome, P03Dispatch::Completed(Ok(_))) {
            written += 1;
        }
    }
    assert!(
        written >= 1,
        "at least one waiter must really complete once the holder releases, so this is a wait and \
         not a blanket refusal"
    );
    assert_eq!(
        std::fs::read_to_string(case.tree.path().join("held.txt")).unwrap(),
        "new data",
        "the released waiters really wrote the bytes they were refused for"
    );
}

/// Real file refusals and effects through P03 and the production connection scheduler.
/// This is a component boundary; it does not claim bootstrap authentication or OS restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eight_real_file_waits_keep_terminals_and_let_an_unrelated_file_finish() {
    use crate::ipc_session::{
        Admission, HandlerTerminal, IpcSession, RESOURCE_WAIT_HANDLER, ResourceWaitSignals,
    };
    let case = Case::new();
    let original = case.tree.path().join("held.txt");
    std::fs::write(&original, b"old").unwrap();
    let facts = case.services.workspace.resource_facts(case.workspace, case.session).unwrap();
    let owner = case.services.workspace.eligibility_owner();
    let hold = owner
        .commit(
            &CommitRequest::one(
                facts.object(&WorkspacePath::parse("held.txt").unwrap()).unwrap().unwrap(),
                ResourceAccess::Write,
            ),
            case.session,
            ExecutionId::next(),
            None,
        )
        .unwrap();
    let mut session = IpcSession::new();
    let (reporter, mut signals) = ResourceWaitSignals::channel();
    let mut paths = Vec::new();
    for index in 0..8 {
        let path = format!("alias-{index}.txt");
        std::fs::hard_link(&original, case.tree.path().join(&path)).unwrap();
        let mut params = case.params(&path);
        params["expected"] =
            serde_json::to_value(FsExpectedState::FileHash { hash: ContentHash::blake3(b"old") })
                .unwrap();
        let request = aura_ipc::JsonRpcRequest {
            jsonrpc: aura_ipc::JSON_RPC_VERSION.into(),
            id: CorrelationId::new(),
            command_invocation_id: None,
            method: method::FS_WRITE_FILE.into(),
            params,
        };
        assert_eq!(session.admit(request), Admission::Accepted);
        paths.push(path);
    }
    let started = session.take_ready();
    assert_eq!(started.len(), 8, "eight aliases are eight lane keys for one real object");
    let mut ids = std::collections::BTreeSet::new();
    for started in started {
        let (ticket, request) = started.into_parts();
        ids.insert(request.id);
        let services = Arc::clone(&case.services);
        let handler = reporter.for_ticket(ticket);
        session.attach(
            ticket,
            tokio::spawn(RESOURCE_WAIT_HANDLER.scope(handler, async move {
                let result = services
                    .dispatch_main_command_bounded(
                        &request.method,
                        &request.params,
                        request.id,
                        Instant::now() + Duration::from_secs(10),
                    )
                    .await;
                assert!(
                    matches!(result, P03Dispatch::Completed(Ok(_))),
                    "real file result: {result:?}"
                );
                HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Succeeded)
            })),
        );
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while session.waiting_count() != 8 {
            session.apply_wait_signals(signals.drain());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("all eight file services refused the real held object");
    assert_eq!(session.running_count(), 0);
    assert_eq!(session.lane_counts_with_waiting(crate::ipc_session::Lane::Session), (0, 0, 8));
    assert!(case.services.blocking.available() > 0, "waits left blocking workers available");
    let unrelated_id = CorrelationId::new();
    assert_eq!(
        session.admit(aura_ipc::JsonRpcRequest {
            jsonrpc: aura_ipc::JSON_RPC_VERSION.into(),
            id: unrelated_id,
            command_invocation_id: None,
            method: method::FS_WRITE_FILE.into(),
            params: case.params("unrelated.txt"),
        }),
        Admission::Accepted
    );
    let ready = session.take_ready();
    assert_eq!(ready.len(), 1);
    let (ticket, request) = ready.into_iter().next().unwrap().into_parts();
    assert_eq!(request.id, unrelated_id);
    let services = Arc::clone(&case.services);
    session.attach(
        ticket,
        tokio::spawn(async move {
            let result = services
                .dispatch_main_command_bounded(
                    &request.method,
                    &request.params,
                    request.id,
                    Instant::now() + Duration::from_secs(2),
                )
                .await;
            assert!(matches!(result, P03Dispatch::Completed(Ok(_))));
            HandlerTerminal::Answered(aura_runtime_contract::IpcRequestTerminal::Succeeded)
        }),
    );
    let mut terminals = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            terminals.extend(session.poll_completions());
            if terminals.iter().any(|(id, _)| *id == unrelated_id) {
                break;
            }
            session.apply_wait_signals(signals.drain());
            assert!(session.take_ready().is_empty(), "a retry resumes its original handler");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(case.tree.path().join("unrelated.txt")).unwrap(),
        "new data"
    );
    assert!(paths.iter().all(|path| std::fs::read(case.tree.path().join(path)).unwrap() == b"old"));
    owner.release_commit(hold);
    tokio::time::timeout(Duration::from_secs(6), async {
        while terminals.len() != 9 {
            terminals.extend(session.poll_completions());
            session.apply_wait_signals(signals.drain());
            assert!(
                session.take_ready().is_empty(),
                "retry uses the original task, not a second handler"
            );
            assert!(session.running_count() <= 8);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert!(terminals.iter().all(|(_, terminal)| matches!(terminal, HandlerTerminal::Answered(_))));
    ids.insert(unrelated_id);
    assert_eq!(
        terminals.into_iter().map(|(id, _)| id).collect::<std::collections::BTreeSet<_>>(),
        ids
    );
    assert!(paths.iter().all(|path| std::fs::read_to_string(case.tree.path().join(path)).unwrap() == "new data"));
    assert!(!session.is_busy());
    assert_eq!(owner.snapshot().commits, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborting_the_wait_owner_stops_a_retry_still_in_its_worker_recheck() {
    use std::sync::{Condvar, Mutex, atomic::AtomicUsize};
    type ReleaseGate = Arc<(Mutex<bool>, Condvar)>;
    struct Release(ReleaseGate);
    impl Drop for Release {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }
    let case = Case::new();
    let hold = case.hold();
    let checks = Arc::new(AtomicUsize::new(0));
    let retry_entered = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(Arc::clone(&gate));
    let check_count = Arc::clone(&checks);
    let entered = Arc::clone(&retry_entered);
    let recheck: MutationRecheck = Arc::new(move || {
        if check_count.fetch_add(1, Ordering::SeqCst) == 1 {
            entered.notify_one();
            let (_guard, timeout) = gate
                .1
                .wait_timeout_while(gate.0.lock().unwrap(), Duration::from_secs(2), |released| {
                    !*released
                })
                .unwrap();
            assert!(!timeout.timed_out(), "the test released the worker before its own bound");
        }
        Ok(())
    });
    let services = Arc::clone(&case.services);
    let params = case.params("held.txt");
    let correlation = CorrelationId::new();
    let task = tokio::spawn(async move {
        services
            .dispatch_main_command_owned(
                method::FS_WRITE_FILE,
                &params,
                correlation,
                Instant::now() + Duration::from_secs(3),
                None,
                recheck,
            )
            .await
    });
    case.wait_for_attempts(1).await;
    case.services.workspace.eligibility_owner().release_commit(hold);
    tokio::time::timeout(Duration::from_secs(2), retry_entered.notified()).await.unwrap();
    assert!(!case.tree.path().join("held.txt").exists());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(release);
    case.wait_for_attempts(2).await;
    assert!(
        !case.tree.path().join("held.txt").exists(),
        "the outliving worker did not publish after its waiter was dropped"
    );
    assert!(!aura_fs_tools::resource_wait_cancelled(correlation));
    assert_eq!(case.services.workspace.eligibility_owner().snapshot().commits, 0);
}
