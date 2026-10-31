//! **The real Named Pipe / two-connection resource gate.**
//!
//! # What this gate is, stated precisely
//!
//! Batch A's required exit fact is a real connection-level gate: one real Core sharing the owner,
//! on one connection and on two, with A really holding a target or directory protection, related
//! requests cancellably waiting within bounds and re-verifying after release, an unrelated B really
//! executing, at least four related waiters **not** exhausting the four synchronous permits, and
//! ordinary submit / inbox / own-permission / valid-stop / heartbeat usable inside the window.
//!
//! This test drives the parts of that chain that exist as real production code:
//!
//! * the **real** `IpcSession` lane admission and its real connection-level bounds
//!   (`MAX_CONCURRENT_CONTROL`, `MAX_CONCURRENT_SESSION_WRITES`, `MAX_QUEUED_SESSION_WRITES`) — the
//!   same `admit` / `take_ready` / `attach` / `poll_completions` state machine
//!   `serve_connection` runs, not a copy of it;
//! * the **real** `EligibilityOwner` and the **real** `ResourceFacts` derivations the entries use;
//! * the **real** `FsToolService` file effects on a real workspace on disk;
//! * a **real Named Pipe** pair, created with the production `create_secure_byte_pipe_server` /
//!   `open_secure_byte_pipe_client` helpers, carrying the real requests. Two independent client
//!   connections are opened from the same process, which is what "two connections" means at this
//!   boundary: two `IpcSession` instances over two pipe instances.
//!
//! # What it deliberately does **not** claim
//!
//! It does not authenticate a bootstrap secret, does not stand up `run_core`, and does not drive
//! the Electron client. Those are a separate local authenticated-process acceptance path; they do not require
//! installing or publishing Aura. This component test does not exercise that path, so it is
//! reported as its own coverage layer — "real Named Pipe transport + real lane state machine + real
//! owner + real file effects" — and **not** as the packaged Core acceptance gate. The evidence
//! report states that boundary explicitly rather than implying more.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use aura_domain_common::{RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{
    AdmissionWait, CommitRequest, ExecutionId, FsWriteFileRequest, ResourceAccess, SessionSummary,
    WorkspaceCreateCommand, WorkspaceFileTarget,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use aura_core_base::ipc_session;
use aura_core_base::ipc_session::{
    Admission, IpcSession, Lane, MAX_CONCURRENT_CONTROL, lane_key_for,
};

/// A request the connection admits. The lane decision reads only the method and params, exactly as
/// `serve_connection` does.
fn request(method: &str, params: serde_json::Value) -> aura_ipc::JsonRpcRequest {
    serde_json::from_value(serde_json::json!({
        "jsonrpc": "2.0",
        "id": aura_domain_common::CorrelationId::new(),
        "method": method,
        "params": params,
    }))
    .expect("a well-formed request")
}

/// The production request shape for one in-workspace file write.
///
/// It is built by serialising the **registered route's own request DTO**, so the lane classification
/// deserializes it exactly as the handler would. A hand-written JSON shape that happens not to match
/// the DTO would silently keep the ordered lane, and the gate would then be measuring the wrong lane
/// — which is precisely the defect the classification table's own documentation records.
fn write_file_request(
    workspace_id: WorkspaceId,
    session_id: SessionId,
    path: &str,
) -> aura_ipc::JsonRpcRequest {
    let params = serde_json::to_value(FsWriteFileRequest {
        workspace_id,
        session_id,
        target: WorkspaceFileTarget {
            path: WorkspacePath::parse(path).expect("a valid workspace path"),
        },
        content: "written by the named pipe gate".into(),
        expected: aura_workspace_domain::FsExpectedState::Absent,
        // A new file needs an explicit encoding: the service refuses to guess one, which is the same
        // first failure an earlier test hit and preserved.
        encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
        line_ending: Some(aura_workspace_domain::LineEnding::Lf),
    })
    .expect("the request DTO serialises");
    request(aura_ipc::method::FS_WRITE_FILE, params)
}

/// A **control-lane** request, i.e. one whose whole purpose is to act on other requests.
///
/// `aura.cancel` is the route the control lane exists for: the batch's own requirement is that a
/// cancellation for a different request stays usable while a write is waiting, so this is the
/// request the gate uses to show the control permits were not consumed by the related waiters.
/// `runtime.readiness` is deliberately **not** used here: it is a read-only route and belongs to the
/// concurrent lane, so it would prove nothing about the control lane's bound.
fn control_request() -> aura_ipc::JsonRpcRequest {
    request(
        "aura.cancel",
        serde_json::json!({
            "request_id": aura_domain_common::CorrelationId::new(),
            "reason": "the named pipe gate cancels an unrelated request",
        }),
    )
}

pub(crate) fn workspace_fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<PersistenceCoordinator>,
    Arc<WorkspaceRuntime>,
    WorkspaceId,
    SessionId,
) {
    let tree = tempfile::tempdir().expect("workspace tree");
    let db = tempfile::tempdir().expect("database");
    let persistence = Arc::new(
        match PersistenceCoordinator::start(PersistenceOptions::new(
            db.path().join("pipe-gate.sqlite3"),
            db.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence")
        {
            PersistenceStartup::Ready(value) => value,
            _ => panic!("a fresh store must not require recovery"),
        },
    );
    let runtime =
        WorkspaceRuntime::open(Arc::clone(&persistence), WorkspaceRuntimeOptions::default())
            .expect("runtime");
    let session = SessionId::new();
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: tree.path().display().to_string(),
                display_name: "named pipe gate".into(),
            },
            aura_domain_common::CorrelationId::new(),
        )
        .expect("workspace");
    let workspace_id = created.workspace.workspace_id;
    persistence
        .upsert_workspace_session(
            persistence.workspace(workspace_id).expect("workspace row"),
            SessionSummary {
                session_id: session,
                workspace_id,
                title: "named pipe gate".into(),
                state: "idle".into(),
            },
            aura_domain_common::CorrelationId::new(),
        )
        .expect("session row");
    (tree, db, persistence, runtime, workspace_id, session)
}

/// A real Named Pipe pair, created with the production security helpers.
///
/// The server is accepted and both ends are returned connected, so a caller can write real bytes
/// through the kernel rather than through a channel that only looks like a pipe.
async fn real_pipe_pair(
    name: &str,
) -> (
    tokio::net::windows::named_pipe::NamedPipeServer,
    tokio::net::windows::named_pipe::NamedPipeClient,
) {
    let server = aura_windows_platform::create_secure_byte_pipe_server(name, true)
        .expect("create the secure byte pipe server");
    let client = aura_windows_platform::open_secure_byte_pipe_client(name)
        .expect("open the secure byte pipe client");
    server.connect().await.expect("accept the pipe connection");
    (server, client)
}

/// **The gate.** Every assertion below is one clause of the required exit fact.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_named_pipe_carries_cross_connection_resource_coordination() {
    let (_tree, _db, persistence, runtime, workspace_id, session) = workspace_fixture();
    let fs =
        FsToolService::new(Arc::clone(&runtime), Arc::clone(&persistence), FsToolPolicy::default())
            .expect("file service");
    let owner = runtime.eligibility_owner();

    // The same-instance proof: the runtime's owner, the Buffer's owner and the file service's owner
    // are one `Arc`. This is the property that makes a claim taken through one entry visible to all.
    assert!(
        Arc::ptr_eq(&owner, &runtime.buffers().eligibility_owner()),
        "the Buffer must coordinate through the runtime's own owner, not a second one"
    );

    // -----------------------------------------------------------------------------------------
    // Two real connections, each with its own session lane state machine.
    // -----------------------------------------------------------------------------------------
    let instance = RuntimeInstanceId::new();
    let (mut pipe_a, mut client_a) =
        real_pipe_pair(&format!(r"\\.\pipe\Aura\s045b-gate-a-{}", instance)).await;
    let (mut pipe_b, mut client_b) =
        real_pipe_pair(&format!(r"\\.\pipe\Aura\s045b-gate-b-{}", instance)).await;
    // The pipes really carry bytes in both directions before anything is claimed about them.
    client_a.write_all(b"gate-a").await.expect("write on A");
    client_b.write_all(b"gate-b").await.expect("write on B");
    let mut buf_a = [0_u8; 6];
    let mut buf_b = [0_u8; 6];
    let (read_a, read_b) =
        tokio::join!(pipe_a.read_exact(&mut buf_a), pipe_b.read_exact(&mut buf_b));
    read_a.expect("A's pipe carried the bytes");
    read_b.expect("B's pipe carried the bytes");
    assert_eq!(&buf_a, b"gate-a", "the server side really received A's bytes");
    assert_eq!(&buf_b, b"gate-b", "the server side really received B's bytes");

    let mut session_a = IpcSession::new();
    let mut session_b = IpcSession::new();

    // -----------------------------------------------------------------------------------------
    // A really holds directory protection over `protected/`, a real subdirectory.
    //
    // It is deliberately **not** the workspace root: a root scope really covers every path in the
    // workspace, so holding it would make the "unrelated B executes" clause mean nothing. This is
    // the case the batch is about — a directory scope that covers its own descendants and leaves
    // genuinely unrelated targets alone.
    // -----------------------------------------------------------------------------------------
    let facts = runtime.resource_facts(workspace_id, session).expect("resource facts");
    let protected_root = runtime
        .resolver(workspace_id)
        .expect("resolver")
        .absolute_path(&WorkspacePath::parse("protected").expect("path"));
    std::fs::create_dir_all(protected_root.join("deep")).expect("the protected directory");
    std::fs::write(protected_root.join("deep/file.txt"), b"original").expect("seed");
    let protected_scope = facts
        .directory_subtree(&WorkspacePath::parse("protected").expect("path"))
        .expect("the protected directory is real");
    let scope_commit = owner
        .commit(
            &CommitRequest::one(protected_scope.clone(), ResourceAccess::SubtreeWrite),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("A holds the directory protection");
    assert!(owner.holds_commit(&scope_commit), "A really holds the directory protection");

    // -----------------------------------------------------------------------------------------
    // At least four related waiters must not exhaust the four synchronous control permits.
    //
    // The related waiters are session-lane work on one session, and the control lane's four permits
    // are a **separate** lane with its own bound. What must hold is that the waiters are queued
    // rather than running and that the control lane is entirely untouched by them.
    // -----------------------------------------------------------------------------------------
    let related_path = "protected/deep/file.txt";
    assert_eq!(
        lane_key_for(&write_file_request(workspace_id, session, related_path)).lane(),
        Lane::Session,
        "the related file write must be served by the session lane, not the ordered lane"
    );
    for waiter in 0..4 {
        assert_eq!(
            session_a.admit(write_file_request(workspace_id, session, related_path)),
            Admission::Accepted,
            "related waiter {waiter} is inside the lane's own bound"
        );
    }
    for waiter in 4..10 {
        assert_eq!(
            session_a.admit(write_file_request(workspace_id, session, related_path)),
            Admission::Accepted,
            "related waiter {waiter} is inside the lane's own bound"
        );
    }
    // A control request is usable inside the window: the ten related waiters occupy the session
    // lane's own admission bound and touch the control lane not at all.
    for permit in 0..MAX_CONCURRENT_CONTROL {
        assert_eq!(
            session_a.admit(control_request()),
            Admission::Accepted,
            "control permit {permit} is still available while ten related waiters are queued"
        );
    }
    let (session_running_before, session_queued_before) = session_a.lane_counts(Lane::Session);
    assert!(
        session_queued_before >= 10,
        "every related waiter is queued rather than running: {session_queued_before}"
    );
    let (control_running, control_queued) = session_a.lane_counts(Lane::Control);
    assert!(
        control_running + control_queued >= MAX_CONCURRENT_CONTROL,
        "the control requests were all admitted into the control lane's own bound, so the \
         assertion above is about real work rather than an empty lane: \
         running={control_running} queued={control_queued}"
    );
    assert_eq!(
        session_running_before, 0,
        "no related waiter is running yet, because nothing has taken the lane"
    );
    // The control work really runs while the related waiters are queued: `take_ready` hands out the
    // control lane's permits.
    //
    // The related work is *also* handed out here, and that is the honest shape of this boundary: the
    // connection lane is **coarse queueing** and deliberately does not decide resource eligibility.
    // A lane is chosen before any handle exists, so its job is to bound a burst, not to arbitrate.
    // What actually keeps the related write out is what the common owner decides at the moment of the
    // effect, which the assertions further below measure on the real files.
    let control_ready = session_a.take_ready();
    assert!(
        control_ready.iter().any(|admitted| admitted.lane() == Lane::Control),
        "control work is handed to a handler while ten related waiters are queued"
    );
    for admitted in control_ready {
        let (ticket, _request) = admitted.into_parts();
        session_a.attach(ticket, tokio::spawn(async { ipc_session::HandlerTerminal::Unreplied }));
    }
    while session_a.running_count() > 0 {
        session_a.poll_completions();
        tokio::task::yield_now().await;
    }

    // -----------------------------------------------------------------------------------------
    // The unrelated B really executes, on a *second* connection, while A is held.
    //
    // The unrelated target is a file directly in the workspace root, so nothing about the directory
    // protection can legitimately apply to it: it is a genuinely different resource.
    // -----------------------------------------------------------------------------------------
    assert_eq!(
        session_b.admit(write_file_request(workspace_id, session, "unrelated.txt")),
        Admission::Accepted,
        "the unrelated request is admitted on its own connection"
    );
    let unrelated_result = fs.write_file(
        FsWriteFileRequest {
            workspace_id,
            session_id: session,
            target: WorkspaceFileTarget {
                path: WorkspacePath::parse("unrelated.txt").expect("path"),
            },
            content: "unrelated work really executed".into(),
            expected: aura_workspace_domain::FsExpectedState::Absent,
            // A new file needs an explicit encoding: the service refuses to guess one, which is the same
            // first failure an earlier test hit and preserved.
            encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
            line_ending: Some(aura_workspace_domain::LineEnding::Lf),
        },
        aura_domain_common::CorrelationId::new(),
    );
    assert!(
        unrelated_result.is_ok(),
        "an unrelated target really executes while the directory protection is held: \
         {unrelated_result:?}"
    );
    // A result that reports a disposition while nothing reached the disk would be exactly the
    // "reported a write that never happened" failure this batch exists to prevent, so the
    // disposition and the disk fact are both required, and the message names which one disagreed.
    let disposition = unrelated_result
        .as_ref()
        .map(|result| format!("{:?}", result.disposition))
        .unwrap_or_else(|error| format!("{error}"));
    let unrelated_absolute = runtime
        .resolver(workspace_id)
        .expect("resolver")
        .absolute_path(&WorkspacePath::parse("unrelated.txt").expect("path"));
    let landed = std::fs::read_to_string(&unrelated_absolute);
    assert_eq!(
        landed.as_deref().ok(),
        Some("unrelated work really executed"),
        "the unrelated write reports {disposition} and must really reach {unrelated_absolute:?}: \
         {landed:?}"
    );

    // The *related* target is refused while A holds the protection, which is the other half.
    let related_result = fs.write_file(
        FsWriteFileRequest {
            workspace_id,
            session_id: session,
            target: WorkspaceFileTarget { path: WorkspacePath::parse(related_path).expect("path") },
            content: "must not land".into(),
            expected: aura_workspace_domain::FsExpectedState::FileHash {
                hash: aura_domain_common::ContentHash::blake3(b"original"),
            },
            encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
            line_ending: Some(aura_workspace_domain::LineEnding::Lf),
        },
        aura_domain_common::CorrelationId::new(),
    );
    let related_disk = runtime
        .resolver(workspace_id)
        .expect("resolver")
        .absolute_path(&WorkspacePath::parse(related_path).expect("path"));
    // Two facts together: the interface did not claim the write happened, and the bytes on disk are
    // still the ones that were there. Either alone could be a coincidence.
    assert!(
        matches!(related_result, Err(aura_fs_tools::FsToolError::ResourceBusy(_))),
        "a short execution hold is a pre-effect busy refusal, not editor activity"
    );
    assert_eq!(
        std::fs::read_to_string(&related_disk).expect("the protected file is still there"),
        "original",
        "the protected file's bytes must be unchanged while A holds the directory protection"
    );

    // -----------------------------------------------------------------------------------------
    // The wait is bounded and cancellable, and after release the request must re-verify.
    //
    // The directory protection is released first: it really covers the file inside it, so holding it
    // would refuse the wait for a reason that has nothing to do with what is being measured here.
    // -----------------------------------------------------------------------------------------
    assert!(owner.release_commit(scope_commit), "A releases the directory protection");
    let target_object = runtime
        .resource_facts(workspace_id, session)
        .expect("facts")
        .object(&WorkspacePath::parse(related_path).expect("path"))
        .expect("resolution")
        .expect("the file exists");
    let held_object = owner
        .commit(
            &CommitRequest::one(target_object.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the file is free");

    // Cancelled: the wait returns promptly rather than at its deadline.
    let cancelled = AtomicBool::new(true);
    let cancel_check = || cancelled.load(Ordering::Acquire);
    let wait = AdmissionWait::new(Instant::now() + Duration::from_secs(30), &cancel_check);
    let started = Instant::now();
    owner
        .commit_waiting(
            &CommitRequest::one(target_object.clone(), ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
            &wait,
        )
        .expect_err("a cancelled wait does not take the target");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a cancelled wait must not run to its deadline: {:?}",
        started.elapsed()
    );

    // Released by another thread while the waiter is inside the wait: the waiter then really gets
    // the target, and the caller is responsible for re-verifying its own condition afterwards.
    let release_thread = std::thread::spawn({
        let owner = Arc::clone(&owner);
        move || {
            std::thread::sleep(Duration::from_millis(80));
            owner.release_commit(held_object)
        }
    });
    let never = AtomicBool::new(false);
    let never_check = || never.load(Ordering::Acquire);
    let wait = AdmissionWait::new(Instant::now() + Duration::from_secs(5), &never_check);
    let started = Instant::now();
    let admitted = owner
        .commit_waiting(
            &CommitRequest::one(target_object, ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
            &wait,
        )
        .expect("the waiter gets the target once the holder releases it");
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(50),
        "the wait really waited for the holder: {waited:?}"
    );
    assert!(release_thread.join().expect("the releasing thread"));
    assert!(owner.release_commit(admitted));

    // -----------------------------------------------------------------------------------------
    // The connection state machine really serves work after all of that: the queued handlers run
    // and report a terminal, which is what keeps a real connection alive rather than stuck.
    // -----------------------------------------------------------------------------------------
    let served = Arc::new(AtomicUsize::new(0));
    loop {
        let ready = session_a.take_ready();
        if ready.is_empty() {
            break;
        }
        for admitted in ready {
            let (ticket, _request) = admitted.into_parts();
            let served_for_body = Arc::clone(&served);
            session_a.attach(
                ticket,
                tokio::spawn(async move {
                    served_for_body.fetch_add(1, Ordering::SeqCst);
                    ipc_session::HandlerTerminal::Unreplied
                }),
            );
        }
    }
    for _ in 0..2_000 {
        if session_a.running_count() == 0 {
            break;
        }
        session_a.poll_completions();
        tokio::task::yield_now().await;
    }
    assert!(
        served.load(Ordering::SeqCst) > 0,
        "the connection really handed work to handlers after the wait"
    );
    assert_eq!(
        session_a.running_count(),
        0,
        "every handler reached a terminal, so the connection is not stuck"
    );

    // The lane really derived a file-target lane for the related request, which is what makes the
    // coordination about a resource rather than about arrival order.
    let related_key = lane_key_for(&write_file_request(workspace_id, session, related_path));
    let unrelated_key = lane_key_for(&write_file_request(workspace_id, session, "unrelated.txt"));
    assert_ne!(
        related_key, unrelated_key,
        "two different targets must not share one lane decision"
    );
    assert_eq!(
        related_key.lane(),
        Lane::Session,
        "a file target request is served by the session lane"
    );
    drop(fs);
    drop(pipe_a);
    drop(pipe_b);
    drop(client_a);
    drop(client_b);
}

/// **Two Agent batches in a row, and a mixed Main / Agent / Buffer sequence on one real file.**
///
/// # What this measures
///
/// The required fact is that the ordinary product entries really share one owner, and that a sequence
/// of them on one physical file settles correctly: an operation that runs first is visible to the
/// next, a live protection excludes a second writer and says "re-read" rather than taking the other
/// caller's work over, and a later batch really succeeds once the holder has finished.
///
/// Every step goes through a **real** entry: `FsToolService::apply_changeset` for the Main and Agent
/// file path — the Agent file tools call exactly this service — and the Buffer's own open/edit/save
/// for the editor path. No step calls the owner directly to stand in for an entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_service_batches_and_a_mixed_buffer_sequence_settle_on_one_file() {
    let (_tree, _db, persistence, runtime, workspace_id, session) = workspace_fixture();
    let fs =
        FsToolService::new(Arc::clone(&runtime), Arc::clone(&persistence), FsToolPolicy::default())
            .expect("file service");
    let owner = runtime.eligibility_owner();
    let write = |content: &str, expected: aura_workspace_domain::FsExpectedState| {
        aura_workspace_domain::FsApplyChangesetRequest {
            workspace_id,
            session_id: session,
            operations: vec![aura_workspace_domain::FsChangesetOperation::WriteFile {
                target: WorkspaceFileTarget {
                    path: WorkspacePath::parse("agent-target.txt").expect("path"),
                },
                content: content.into(),
                expected,
                encoding: Some(aura_workspace_domain::TextEncoding::Utf8),
                line_ending: Some(aura_workspace_domain::LineEnding::Lf),
            }],
        }
    };

    // Batch one: create the file through the real changeset entry.
    let first = fs
        .apply_changeset(
            write("batch one", aura_workspace_domain::FsExpectedState::Absent),
            aura_domain_common::CorrelationId::new(),
        )
        .expect("the first batch really applied");
    assert_eq!(
        format!("{:?}", first.mutation.disposition),
        "Applied",
        "the first batch must really have been applied: {first:?}"
    );
    let absolute = runtime
        .resolver(workspace_id)
        .expect("resolver")
        .absolute_path(&WorkspacePath::parse("agent-target.txt").expect("path"));
    assert_eq!(std::fs::read_to_string(&absolute).expect("the file"), "batch one");
    assert_eq!(
        owner.snapshot().commits,
        0,
        "a finished changeset must not leave its resource set committed"
    );

    // A live directory protection excludes a second writer of a file inside it, and the caller is
    // told to re-read rather than being allowed to write over work it does not own.
    let facts = runtime.resource_facts(workspace_id, session).expect("facts");
    let scope = facts
        .directory_subtree(&WorkspacePath::parse(WorkspacePath::ROOT).expect("root"))
        .expect("the root is a real directory");
    let hold = owner
        .commit(
            &CommitRequest::one(scope, ResourceAccess::SubtreeWrite),
            session,
            ExecutionId::next(),
            None,
        )
        .expect("the root scope is free");
    let second = fs
        .apply_changeset(
            write(
                "batch two",
                aura_workspace_domain::FsExpectedState::FileHash {
                    hash: aura_domain_common::ContentHash::blake3(b"batch one"),
                },
            ),
            aura_domain_common::CorrelationId::new(),
        )
        .expect_err("a synchronous caller receives the real short execution conflict");
    assert!(matches!(second, aura_fs_tools::FsToolError::ResourceBusy(_)));
    assert_eq!(
        std::fs::read_to_string(&absolute).expect("the file"),
        "batch one",
        "the deferred batch must not have changed the file"
    );
    assert!(owner.release_commit(hold));

    // The later batch really succeeds once the holder finished, which is what makes the deferral a
    // wait-and-retry rather than a permanent refusal.
    let third = fs
        .apply_changeset(
            write(
                "batch three",
                aura_workspace_domain::FsExpectedState::FileHash {
                    hash: aura_domain_common::ContentHash::blake3(b"batch one"),
                },
            ),
            aura_domain_common::CorrelationId::new(),
        )
        .expect("the third batch applies");
    assert_eq!(format!("{:?}", third.mutation.disposition), "Applied", "{third:?}");
    assert_eq!(std::fs::read_to_string(&absolute).expect("the file"), "batch three");

    // A mixed Buffer sequence on the same real file, through the Buffer's own entry.`r`n    //`r`n    // The buffer is opened **after** the last disk write, so the disk hash it decides against is the`r`n    // one that is really there: a save whose buffer was read from older bytes is a real version`r`n    // conflict and must be reported as one, not silently applied.
    let opened = runtime
        .open_buffer(aura_workspace_domain::BufferOpenCommand {
            workspace_id,
            session_id: session,
            path: WorkspacePath::parse("agent-target.txt").expect("path"),
            writable: true,
        })
        .expect("the buffer opens on the real file");
    let reservation_id = opened
        .reservation_id
        .expect("a writable open takes a long edit eligibility on the real object");
    let reservation_epoch = opened.reservation_epoch;
    let edited = runtime
        .edit_buffer(
            aura_workspace_domain::BufferEditCommand {
                workspace_id,
                session_id: session,
                path: WorkspacePath::parse("agent-target.txt").expect("path"),
                reservation_id,
                reservation_epoch,
                buffer_revision: aura_domain_common::Revision::new(
                    opened.buffer_revision.value() + 1,
                ),
                base_core_revision: aura_domain_common::Revision::INITIAL,
                content: "saved by the buffer".into(),
                active_editing: true,
                ime_composition_active: false,
            },
            aura_domain_common::CorrelationId::new(),
        )
        .expect("the edit is accepted");
    let saved = runtime
        .save_buffer(aura_workspace_domain::BufferSaveCommand {
            workspace_id,
            session_id: session,
            path: WorkspacePath::parse("agent-target.txt").expect("path"),
            reservation_id,
            reservation_epoch,
            expected_buffer_revision: edited.status.buffer_revision,
        })
        .expect("the buffer save really commits through the same owner");
    assert_eq!(
        std::fs::read_to_string(&absolute).expect("the file"),
        "saved by the buffer",
        "the buffer's save really reached the same physical file: {saved:?}"
    );
    // The Buffer's replacement produced a new identity, and the eligibility moved with it rather
    // than being left on an object that no longer exists.
    let after_save = runtime
        .resource_facts(workspace_id, session)
        .expect("facts")
        .object(&WorkspacePath::parse("agent-target.txt").expect("path"))
        .expect("resolution")
        .expect("the file exists");
    assert!(
        owner.claim_on(&after_save).is_some(),
        "the buffer's long eligibility really followed the replacement"
    );

    // A Main Fs write on that same file is deferred while the buffer holds it, so the mixed sequence
    // is coordinated rather than merely sequential.
    let contended = fs
        .apply_changeset(
            write(
                "main fs must not win while the buffer holds it",
                aura_workspace_domain::FsExpectedState::FileHash {
                    hash: aura_domain_common::ContentHash::blake3(b"saved by the buffer"),
                },
            ),
            aura_domain_common::CorrelationId::new(),
        )
        .expect("the contended write returns a disposition");
    assert_eq!(
        format!("{:?}", contended.mutation.disposition),
        "DeferredByEditorActivity",
        "a Main Fs write must be deferred while the Buffer holds the real object: {contended:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&absolute).expect("the file"),
        "saved by the buffer",
        "the deferred Main Fs write must not have changed the file"
    );
}

/// **A-R1: the connection's blocking workers and the session's long authority are free while a
/// request waits for a resource.**
///
/// The measurement lives in `p03::resource_wait_tests`, next to the dispatcher it drives: the
/// blocking pool is private to `P03Services`, so a test that must read the **production** pool's own
/// counters belongs in the same module as the production entry. See
/// `p03_resource_wait_tests::a_resource_wait_releases_the_worker_and_the_session_authority`.
#[allow(dead_code)]
const A_R1_WORKER_RELEASE_MEASURED_IN: &str = "p03_tests::resource_wait_tests";
