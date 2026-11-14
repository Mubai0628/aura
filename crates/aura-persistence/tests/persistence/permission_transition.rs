#![allow(clippy::unwrap_used)]
use aura_domain_common::*;
use aura_permission_domain::PermissionModeTransition;
use aura_persistence::*;
use aura_security_contract::PermissionMode;
use aura_workspace_domain::*;

#[tokio::test]
async fn permission_commit_and_final_launch_share_one_ordered_barrier() {
    let root = tempfile::tempdir().unwrap();
    let PersistenceStartup::Ready(store) = PersistenceCoordinator::start(PersistenceOptions::new(
        root.path().join("aura.db"),
        root.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .unwrap() else {
        panic!("recovery")
    };
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let correlation = CorrelationId::new();
    store
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id: workspace,
                display_name: "Barrier fixture".into(),
                selected_root: "C:\\fixture".into(),
                canonical_root: "C:\\fixture".into(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id: session,
                workspace_id: workspace,
                title: "barrier".into(),
                state: "idle".into(),
            },
            correlation,
        )
        .unwrap();
    store
        .ensure_session_permission_state(
            session,
            workspace,
            PermissionMode::FullAccess,
            correlation,
        )
        .unwrap();
    let launch = store.permission_start_lease(session).await;
    let change =
        store.change_session_permission_mode(session, 1, PermissionMode::ReadOnly, correlation);
    tokio::pin!(change);
    assert!(tokio::time::timeout(std::time::Duration::from_millis(25), &mut change).await.is_err());
    assert_eq!(store.session_permission_state(session).unwrap().mode, PermissionMode::FullAccess);
    assert!(
        store
            .set_session_permission_mode(session, 1, PermissionMode::ReadOnly, correlation)
            .is_err(),
        "sync callers cannot bypass a pending start"
    );
    drop(launch);
    let current = change.await.unwrap();
    assert_eq!(current.mode, PermissionMode::ReadOnly);
    assert_eq!(current.transition, PermissionModeTransition::Settled);
    let _new_start = store.permission_start_lease(session).await;
    assert_eq!(
        store.session_permission_state(session).unwrap().revision,
        2,
        "later dispatch must see new authority"
    );
}

#[test]
fn legacy_transition_evidence_survives_and_new_mode_commits_settled() {
    let root = tempfile::tempdir().unwrap();
    let options = || {
        PersistenceOptions::new(
            root.path().join("aura.db"),
            root.path().join("artifacts"),
            RuntimeInstanceId::new(),
        )
    };
    let PersistenceStartup::Ready(store) = PersistenceCoordinator::start(options()).unwrap() else {
        panic!("recovery")
    };
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    let correlation = CorrelationId::new();
    store
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id: workspace,
                display_name: "Transition fixture".into(),
                selected_root: "C:\\fixture".into(),
                canonical_root: "C:\\fixture".into(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id: session,
                workspace_id: workspace,
                title: "mode".into(),
                state: "idle".into(),
            },
            correlation,
        )
        .unwrap();
    store
        .ensure_session_permission_state(
            session,
            workspace,
            PermissionMode::FullAccess,
            correlation,
        )
        .unwrap();
    let changed = store
        .set_session_permission_mode(session, 1, PermissionMode::WorkspaceReadWrite, correlation)
        .unwrap();
    assert_eq!(changed.transition, PermissionModeTransition::Settled);
    // Simulate an old interrupted V2-0007 transition; R10 does not erase it.
    store.finish_session_permission_transition(session, 2, false, correlation).unwrap();
    drop(store);
    let PersistenceStartup::Ready(store) = PersistenceCoordinator::start(options()).unwrap() else {
        panic!("recovery")
    };
    assert_eq!(
        store.session_permission_state(session).unwrap().transition,
        PermissionModeTransition::StopUnconfirmed
    );
    assert_eq!(store.pending_session_permission_transitions().unwrap().len(), 1);
    let tightened = store
        .set_session_permission_mode(session, 2, PermissionMode::ReadOnly, correlation)
        .unwrap();
    assert_eq!(tightened.revision, 3);
    assert!(store.finish_session_permission_transition(session, 2, true, correlation).is_err());
    assert_eq!(
        store.session_permission_state(session).unwrap().transition,
        PermissionModeTransition::Settled
    );
    let final_state =
        store.finish_session_permission_transition(session, 3, true, correlation).unwrap();
    assert_eq!(final_state.mode, PermissionMode::ReadOnly);
    assert_eq!(final_state.revision, 3);
    assert_eq!(final_state.transition, PermissionModeTransition::Settled);
    assert!(store.pending_session_permission_transitions().unwrap().is_empty());
}

/// A counterexample, at the real store: a start lease held by one session must not
/// delay another session's mode change, and must still exclude its own.
#[tokio::test]
async fn a_held_start_lease_excludes_only_its_own_session() {
    let root = tempfile::tempdir().unwrap();
    let PersistenceStartup::Ready(store) = PersistenceCoordinator::start(PersistenceOptions::new(
        root.path().join("aura.db"),
        root.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .unwrap() else {
        panic!("recovery")
    };
    let workspace = WorkspaceId::new();
    let held_session = SessionId::new();
    let other_session = SessionId::new();
    let correlation = CorrelationId::new();
    for (index, session) in [held_session, other_session].into_iter().enumerate() {
        store
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id: workspace,
                    display_name: "Isolation fixture".into(),
                    selected_root: "C:\\fixture".into(),
                    canonical_root: "C:\\fixture".into(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Unavailable,
                    authorization_epoch: Revision::INITIAL,
                },
                SessionSummary {
                    session_id: session,
                    workspace_id: workspace,
                    title: format!("isolation {index}"),
                    state: "idle".into(),
                },
                correlation,
            )
            .unwrap();
        store
            .ensure_session_permission_state(
                session,
                workspace,
                PermissionMode::FullAccess,
                correlation,
            )
            .unwrap();
    }
    // A's long tool holds A's own start lease for its whole duration.
    let held = store.permission_start_lease(held_session).await;
    assert!(
        store
            .permission_gate_facts()
            .iter()
            .any(|(session, _, start)| *session == held_session && *start),
        "the held session really owns its start gate"
    );
    // B's ordinary control is not A's: this is the fact the R6 regression exists for.
    let changed = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        store.change_session_permission_mode(
            other_session,
            1,
            PermissionMode::ReadOnly,
            correlation,
        ),
    )
    .await
    .expect("B's own mode change must not wait behind A's held start lease")
    .unwrap();
    assert_eq!(changed.mode, PermissionMode::ReadOnly);
    assert_eq!(changed.revision, 2);
    assert_eq!(
        store.session_permission_state(held_session).unwrap().mode,
        PermissionMode::FullAccess,
        "A's authority is untouched by B's change"
    );
    // The same ordering inside A is not weakened: A's own change still waits for A's lease.
    let same = store.change_session_permission_mode(
        held_session,
        1,
        PermissionMode::WorkspaceReadWrite,
        correlation,
    );
    tokio::pin!(same);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut same).await.is_err(),
        "A's own mode change still cannot commit inside A's live start"
    );
    assert!(
        store
            .set_session_permission_mode(held_session, 1, PermissionMode::ReadOnly, correlation)
            .is_err(),
        "the synchronous caller still refuses on its own session"
    );
    drop(held);
    let settled = same.await.unwrap();
    assert_eq!(settled.mode, PermissionMode::WorkspaceReadWrite);
    assert_eq!(settled.revision, 2);
}

/// Builds a real store with two real sessions, both in `FullAccess` at revision 1.
fn two_real_sessions() -> (tempfile::TempDir, PersistenceCoordinator, SessionId, SessionId) {
    let root = tempfile::tempdir().unwrap();
    let PersistenceStartup::Ready(store) = PersistenceCoordinator::start(PersistenceOptions::new(
        root.path().join("aura.db"),
        root.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .unwrap() else {
        panic!("recovery")
    };
    let workspace = WorkspaceId::new();
    let first = SessionId::new();
    let second = SessionId::new();
    let correlation = CorrelationId::new();
    for (index, session) in [first, second].into_iter().enumerate() {
        store
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id: workspace,
                    display_name: "R8.1 fixture".into(),
                    selected_root: "C:\\fixture".into(),
                    canonical_root: "C:\\fixture".into(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Unavailable,
                    authorization_epoch: Revision::INITIAL,
                },
                SessionSummary {
                    session_id: session,
                    workspace_id: workspace,
                    title: format!("r81 {index}"),
                    state: "idle".into(),
                },
                correlation,
            )
            .unwrap();
        store
            .ensure_session_permission_state(
                session,
                workspace,
                PermissionMode::FullAccess,
                correlation,
            )
            .unwrap();
    }
    (root, store, first, second)
}

/// A real same-session authority section, held from a non-runtime thread.
///
/// It is the product's own reader ([`PersistenceCoordinator::with_session_permission_authority`]),
/// not a stand-in: the section really owns this session's authority for the whole time it
/// is parked, exactly as an Agent step does while it decides on a live mode.
struct HeldAuthoritySection {
    entered: std::sync::mpsc::Receiver<()>,
    release: std::sync::mpsc::Sender<()>,
    section_returned: std::sync::mpsc::Receiver<Option<PermissionMode>>,
    observed_while_held: std::sync::Arc<std::sync::Mutex<Vec<PermissionMode>>>,
    join: std::thread::JoinHandle<()>,
}

impl HeldAuthoritySection {
    fn enter(store: &PersistenceCoordinator, session: SessionId) -> Self {
        let (entered_sender, entered) = std::sync::mpsc::channel();
        let (release, release_receiver) = std::sync::mpsc::channel::<()>();
        let (permission_sender, section_returned) =
            std::sync::mpsc::channel::<Option<PermissionMode>>();
        let observed_while_held = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&observed_while_held);
        let owner = store.clone();
        let join = std::thread::spawn(move || {
            let section = owner
                .with_session_permission_authority(session, |state| {
                    sink.lock().expect("observed modes").push(state.mode);
                    let _ = entered_sender.send(());
                    let _ = release_receiver.recv_timeout(std::time::Duration::from_secs(30));
                    state.mode
                })
                .expect("the authority section runs");
            let _ = permission_sender.send(Some(section));
        });
        Self { entered, release, section_returned, observed_while_held, join }
    }

    /// Waits until the section really owns this session's authority.
    fn wait_until_held(&self) -> std::time::Duration {
        let started = std::time::Instant::now();
        self.entered
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("the authority section entered");
        started.elapsed()
    }

    fn release(self) -> Option<PermissionMode> {
        let _ = self.release.send(());
        let observed =
            self.section_returned.recv_timeout(std::time::Duration::from_secs(30)).ok().flatten();
        self.join.join().expect("the authority section finished");
        observed
    }

    fn observed_modes(&self) -> Vec<PermissionMode> {
        self.observed_while_held.lock().expect("observed modes").clone()
    }
}

/// The counterexample, as a formal regression.
///
/// Real store, real sessions, real SQLite, real authority section and the **public async**
/// `change_session_permission_mode`, on a **single** runtime worker - the production shape
/// of `lib.rs` → `routes/security.rs` → `p04.rs` → this method, where one worker also owes
/// the timer, the heartbeat and every other session its progress.
///
/// Before the fix the method waited for the authority gate synchronously on that worker,
/// so the 100 ms timer did not run inside the 700 ms observation window and the mode
/// committed only after the section ended. Four facts are asserted here and all four are
/// values, not shapes:
///
/// 1. the timer *does* fire while the section is still live, and its budget is the
///    observation window rather than the section's length;
/// 2. another session's mode change completes inside this budget while the section is live;
/// 3. the waiting session's change really is excluded - the live section never observes the
///    new mode, and the state on disk is still the old one;
/// 4. after the release the change commits exactly once and returns the real state.
#[tokio::test(flavor = "current_thread")]
async fn an_awaitable_permission_change_yields_the_one_worker_while_authority_is_live() {
    let (_root, store, session, other_session) = two_real_sessions();
    let correlation = CorrelationId::new();
    let section = HeldAuthoritySection::enter(&store, session);
    let entered_after = section.wait_until_held();
    assert!(
        store
            .permission_gate_facts()
            .iter()
            .any(|(gate_session, authority, _)| *gate_session == session && *authority),
        "the section really owns this session's authority (entered after {entered_after:?})"
    );

    // The change is already waiting: it must not be able to commit inside the live section.
    //
    // `polls` counts completed polls of the change task, and the loop below drives the
    // runtime until that task has really been polled once. That is exactly the point at
    // which the *pre-fix* method enters its blocking wait, so the measurement underneath
    // does not depend on which of two freshly spawned tasks the executor polls first.
    let polls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let watcher_polls = std::sync::Arc::clone(&polls);
    let watcher_store = store.clone();
    let change = tokio::spawn(async move {
        watcher_polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        watcher_store
            .change_session_permission_mode(session, 1, PermissionMode::ReadOnly, correlation)
            .await
    });
    let waiting_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while polls.load(std::sync::atomic::Ordering::Relaxed) < 1 {
        assert!(std::time::Instant::now() < waiting_deadline, "the change task was never polled");
        tokio::task::yield_now().await;
    }
    assert!(!change.is_finished(), "the change is waiting, not committed");
    assert!(
        store
            .permission_gate_facts()
            .iter()
            .any(|(gate_session, authority, _)| *gate_session == session && *authority),
        "the section is still live while the change waits"
    );

    // THE R8.1 MEASUREMENT. A 100 ms timer under a 50 ms observation window, armed after
    // the change above has really been polled. The observation is the *runtime's own*
    // clock: with the worker free this task is served at 50 ms and reports the timer as
    // still pending; with the worker handed to the authority gate this task is not served
    // at all until the section ends, and by then the 100 ms timer has long finished.
    let timer_budget = std::time::Duration::from_millis(100);
    let observation = std::time::Duration::from_millis(50);
    let timer_started = std::time::Instant::now();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(timer_budget).await;
        timer_started.elapsed()
    });
    let window = std::time::Instant::now();
    tokio::time::sleep(observation).await;
    let observed_after = window.elapsed();
    let fired_in_window = timer.is_finished();
    println!(
        "r81 timer: budget={}ms observation={}ms observed_after={}ms fired_in_window={fired_in_window} \
         authority_entered_after={entered_after:?}",
        timer_budget.as_millis(),
        observation.as_millis(),
        observed_after.as_millis(),
    );
    assert!(
        !fired_in_window,
        "a {} ms timer must still be pending when a free worker is served at {} ms; it had \
         already finished, which means the observation window itself was handed to the \
         authority wait (window observed after {} ms)",
        timer_budget.as_millis(),
        observation.as_millis(),
        observed_after.as_millis()
    );
    assert!(
        observed_after < timer_budget,
        "the observation window must be served on its own budget, not the section's: \
         observed after {} ms",
        observed_after.as_millis()
    );
    let fired_after = timer.await.expect("the timer task ran").as_millis();
    assert!(
        fired_after >= timer_budget.as_millis(),
        "the timer must really wait its own budget: {fired_after} ms"
    );

    // Another session's ordinary control, on the same single worker and the same store.
    // It is measured rather than asserted first: while the one worker is parked this is
    // the second observable consequence, and its value belongs in the log.
    let other_started = std::time::Instant::now();
    let other = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        store.change_session_permission_mode(
            other_session,
            1,
            PermissionMode::WorkspaceReadWrite,
            correlation,
        ),
    )
    .await;
    let other_elapsed = other_started.elapsed();
    println!(
        "r81 other session: completed={} elapsed={}ms",
        other.is_ok(),
        other_elapsed.as_millis()
    );
    let other = other
        .unwrap_or_else(|_| {
            panic!(
                "another session's mode change must not wait behind this session's section; \
                 it hit the 5000 ms bound after {} ms",
                other_elapsed.as_millis()
            )
        })
        .expect("another session's mode change commits");
    assert_eq!((other.mode, other.revision), (PermissionMode::WorkspaceReadWrite, 2));
    assert!(
        other_elapsed < timer_budget,
        "another session's control must complete inside the timer's own budget: {} ms",
        other_elapsed.as_millis()
    );

    // The exclusion is real: the live section still reads the old authority, and the
    // waiting session's own row has not moved.
    assert!(
        section.observed_modes().iter().all(|mode| *mode == PermissionMode::FullAccess),
        "the live authority section must not observe the new mode: {:?}",
        section.observed_modes()
    );
    assert_eq!(
        store.session_permission_state(session).unwrap().mode,
        PermissionMode::FullAccess,
        "the waiting session's mode is still uncommitted"
    );

    let section_returned = section.release();
    assert_eq!(section_returned, Some(PermissionMode::FullAccess));
    let settled = change.await.expect("the mode change task ran").expect("and it committed");
    assert_eq!(settled.mode, PermissionMode::ReadOnly);
    assert_eq!(settled.revision, 2);
    assert_eq!(settled.transition, PermissionModeTransition::Settled);

    // Committed exactly once: one terminal record, its answer delivered, and the
    // revision advanced by exactly one.
    let identity = format!("permission-mode-change:{session}");
    let settlements = store.job_settlements();
    let records: Vec<_> =
        settlements.recent.iter().filter(|record| record.identity == identity).collect();
    assert_eq!(records.len(), 1, "the mode change is submitted exactly once");
    assert!(records[0].succeeded, "and it committed");
    assert!(records[0].answered, "and its result reached the caller that was waiting");
    assert_eq!(store.session_permission_state(session).unwrap().revision, 2);
}
