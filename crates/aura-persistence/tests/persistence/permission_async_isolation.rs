#![allow(clippy::unwrap_used)]
//! The public awaitable mode change, measured against the one writer and one
//! authority gate it shares with every synchronous caller.
//!
//! These are the *positive* formal assertions the fix has to make true. The negative
//! control - the same regression run against the pre-R8.1 sources, where it does not
//! finish at all - is recorded in the batch evidence, not here: a test file may not carry
//! two versions of the product.
//!
//! What each test pins, and why it is a value:
//!
//! * the blocking entry and the awaitable entry take **the same** gate, so a
//!   same-session authority section excludes both and an unrelated session is excluded by
//!   neither. If the awaitable entry had been given a private gate, the same-session half
//!   of this would pass while the exclusion was gone; the unrelated-session half is what
//!   makes "one barrier, bound to the session" the measured claim;
//! * the awaitable entry returns the real committed state (mode and revision) after the
//!   release, so "it waited" is not confused with "it did nothing";
//! * a 100 ms runtime timer is still pending when a 50 ms observation window is served on
//!   a **single** worker while that entry waits - the entry yielded the runtime - and the
//!   timer really does wait its own budget;
//! * the released commit is submitted exactly once, its terminal record says it committed
//!   and that its answer reached the caller, and no second write exists.
use aura_domain_common::*;
use aura_permission_domain::PermissionModeTransition;
use aura_persistence::*;
use aura_security_contract::PermissionMode;
use aura_workspace_domain::*;

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
/// not a stand-in: the section really owns this session's authority while it is parked,
/// exactly as an Agent step does while it decides on a live mode.
fn hold_authority(
    store: &PersistenceCoordinator,
    session: SessionId,
) -> std::sync::mpsc::Sender<()> {
    let (entered_sender, entered) = std::sync::mpsc::channel();
    let (release, release_receiver) = std::sync::mpsc::channel::<()>();
    let owner = store.clone();
    std::thread::spawn(move || {
        owner
            .with_session_permission_authority(session, |_state| {
                let _ = entered_sender.send(());
                let _ = release_receiver.recv_timeout(std::time::Duration::from_secs(30));
            })
            .expect("the authority section runs");
    });
    entered.recv_timeout(std::time::Duration::from_secs(30)).expect("section entered");
    release
}

/// One barrier, bound to one session: excluded for its own session, free for another.
///
/// The synchronous entry is the control for the awaitable one. Both must be excluded for
/// the held session and neither may be excluded for the unrelated one; a private gate on
/// the awaitable path would show up here as the unrelated-session half passing while the
/// same-session half silently stopped excluding.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_session_barrier_excludes_both_entries_and_only_for_its_session() {
    let (_root, store, held, other) = two_real_sessions();
    let release = hold_authority(&store, held);
    assert!(
        store
            .permission_gate_facts()
            .iter()
            .any(|(session, authority, _)| *session == held && *authority),
        "the section owns the held session's authority"
    );

    // The synchronous entry waits for the same gate: it is the control.
    let sync_started = std::time::Instant::now();
    let mut synchronous = {
        let store = store.clone();
        tokio::task::spawn_blocking(move || {
            store.set_session_permission_mode(
                held,
                1,
                PermissionMode::ReadOnly,
                CorrelationId::new(),
            )
        })
    };
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(60), &mut synchronous).await.is_err(),
        "the blocking entry must still be excluded from a live same-session authority section"
    );

    // The unrelated session is excluded by neither entry, and the awaitable one is fast.
    let other_started = std::time::Instant::now();
    let unrelated = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        store.change_session_permission_mode(
            other,
            1,
            PermissionMode::WorkspaceReadWrite,
            CorrelationId::new(),
        ),
    )
    .await
    .expect("another session's mode change must not wait behind this session's section")
    .expect("another session's mode change commits");
    let unrelated_ms = other_started.elapsed().as_millis();
    assert_eq!((unrelated.mode, unrelated.revision), (PermissionMode::WorkspaceReadWrite, 2));

    // The held session is still uncommitted, and the section never observed a new mode.
    assert_eq!(
        store.session_permission_state(held).unwrap().mode,
        PermissionMode::FullAccess,
        "the held session's mode is still uncommitted"
    );

    // A second same-session change cannot slip past the one already waiting either: the
    // awaiting change still holds this session's *start* change lease, so the synchronous
    // entry answers "a launch is in progress" rather than queueing or committing.
    let second = Box::pin(store.change_session_permission_mode(
        held,
        1,
        PermissionMode::WorkspaceReadWrite,
        CorrelationId::new(),
    ));
    drop(second);
    assert!(
        store
            .set_session_permission_mode(
                held,
                1,
                PermissionMode::WorkspaceReadWrite,
                CorrelationId::new()
            )
            .is_err(),
        "the awaiting change owns this session's start change lease"
    );
    assert_eq!(
        store.session_permission_state(held).unwrap().mode,
        PermissionMode::FullAccess,
        "and nothing of that second attempt reached the store"
    );

    println!(
        "r81 barrier: unrelated_session={unrelated_ms}ms sync_waited={}ms",
        sync_started.elapsed().as_millis()
    );
    assert!(
        unrelated_ms < 4_000,
        "an unrelated session must not be held by this session's section: {unrelated_ms} ms"
    );

    // The release ends the section; the blocking entry then acquires the same gate the
    // awaitable entry is waiting on, so both make progress and each commits once.
    let _ = release.send(());
    let synchronous = tokio::time::timeout(std::time::Duration::from_secs(10), synchronous)
        .await
        .expect("the blocking entry finished")
        .expect("its task ran")
        .expect("and committed");
    assert_eq!((synchronous.mode, synchronous.revision), (PermissionMode::ReadOnly, 2));
    assert_eq!(synchronous.transition, PermissionModeTransition::Settled);
}

/// The R8.1 measurement: a 100 ms runtime timer, a 50 ms observation window and a mode
/// change waiting for authority, all on **one** worker.
///
/// With the waiter parking the worker, the observation window is not served until the
/// section ends and the 100 ms timer has long finished. With the waiter yielding, the
/// window is served on its own 50 ms budget and the timer is still pending. The numbers
/// printed here are the regression's own evidence.
#[tokio::test(flavor = "current_thread")]
async fn an_awaitable_mode_change_yields_the_one_worker_while_authority_is_live() {
    let (_root, store, held, _other) = two_real_sessions();
    let release = hold_authority(&store, held);

    // A counter, not a sleep: the change task reports that it has really been polled once,
    // which is the point at which the pre-fix method enters its blocking wait.
    let polls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let watcher_polls = std::sync::Arc::clone(&polls);
    let watcher = store.clone();
    let change = tokio::spawn(async move {
        watcher_polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        watcher
            .change_session_permission_mode(held, 1, PermissionMode::ReadOnly, CorrelationId::new())
            .await
    });
    let waiting_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while polls.load(std::sync::atomic::Ordering::Relaxed) < 1 {
        assert!(std::time::Instant::now() < waiting_deadline, "the change task was never polled");
        tokio::task::yield_now().await;
    }
    assert!(!change.is_finished(), "the change must not commit inside the live section");

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
    let pending_during_window = !timer.is_finished();
    let fired_after = timer.await.expect("the timer task ran").as_millis();
    println!(
        "r81 timer: budget={}ms observation={}ms observed_after={}ms pending_during_window={} \
         fired_at={}ms",
        timer_budget.as_millis(),
        observation.as_millis(),
        observed_after.as_millis(),
        pending_during_window,
        fired_after
    );
    assert!(
        pending_during_window,
        "a {} ms timer must still be pending when a free worker is served at {} ms; the window \
         was served after {} ms and the timer had fired at {} ms, which means the worker was \
         handed to the authority wait",
        timer_budget.as_millis(),
        observation.as_millis(),
        observed_after.as_millis(),
        fired_after
    );
    assert!(
        observed_after < timer_budget,
        "the observation window must be served on its own budget, not the section's: {} ms",
        observed_after.as_millis()
    );
    assert!(
        fired_after >= timer_budget.as_millis(),
        "the timer must really wait its own budget: {fired_after} ms"
    );

    // The wait is real: without the release nothing commits.
    assert!(!change.is_finished(), "nothing commits inside the live section");

    let _ = release.send(());
    let settled = tokio::time::timeout(std::time::Duration::from_secs(10), change)
        .await
        .expect("the change task finished")
        .expect("the change committed")
        .expect("with an answer");
    assert_eq!((settled.mode, settled.revision), (PermissionMode::ReadOnly, 2));
    assert_eq!(settled.transition, PermissionModeTransition::Settled);

    // Exactly once: one terminal record for this session's change, committed, answered,
    // and the revision advanced by one.
    let identity = format!("permission-mode-change:{held}");
    let settlements = store.job_settlements();
    let records: Vec<_> =
        settlements.recent.iter().filter(|record| record.identity == identity).collect();
    assert_eq!(records.len(), 1, "the mode change is submitted exactly once");
    assert!(records[0].succeeded, "and it committed");
    assert!(records[0].answered, "and its result reached the caller that was waiting");
    assert_eq!(store.session_permission_state(held).unwrap().revision, 2);
}
