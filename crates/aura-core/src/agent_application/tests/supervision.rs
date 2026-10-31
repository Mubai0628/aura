//! S04-C: per-run supervision, stop, panic and generation, through the real entries.
//!
//! Every case keeps the facts apart that used to be one boolean: how many runs are admitted,
//! whether the run's **real task** ended (the supervisor's own guard records it), who owns the
//! task and its cancellation signals, what the persisted turn says, and what happened to the
//! side effect. A `stopped` flag is never accepted as evidence of any of them.
use super::*;
use aura_core_broker::agent_store::{self, ReceiptState};
use provider_fixture::{Provider, Scenario};
use std::time::{Duration, Instant};

/// How long a Store hold parks the real work in these cases.
const HOLD_MS: u64 = 4_000;

#[tokio::test(flavor = "current_thread")]
async fn s04_codex_a_real_task_aborted_before_its_first_poll_releases_its_entry() {
    let host = crate::r03_tests::Fixture::new();
    let state = host.start().await;
    let crate::StartupMode::Ready(services) = &state.startup_mode else { panic!("Core ready") };
    let agent = services.agent_required();
    let run = AgentRunId::new();
    agent.launch(run).expect("real launch");
    assert!(!agent.supervisor().started(run), "the current task has not yielded yet");
    assert!(agent.supervisor().abort_task_for_test(run), "the installed real handle is aborted");
    let end = agent.supervisor().wait_ended(run, Instant::now() + Duration::from_millis(150)).await;
    assert!(end.real_end, "an unpolled aborted task must release admission: {end:?}");
    assert!(!agent.supervisor().is_active(run));
    assert_eq!(agent.supervisor().ledger().last().map(|fact| fact.end), Some("aborted"));
}

struct TwoRuns {
    _fixture: crate::r03_tests::Fixture,
    runtime: Arc<crate::RuntimeState>,
    provider_a: Provider,
    model_a: ModelProfileId,
    session_a: SessionId,
    provider_b: Provider,
    model_b: ModelProfileId,
    session_b: SessionId,
}

impl TwoRuns {
    async fn new(scenario_a: Scenario, scenario_b: Scenario) -> Self {
        let fixture = crate::r03_tests::Fixture::new();
        let runtime = fixture.start().await;
        let root = fixture.root.path().join("s04-supervision-workspace");
        std::fs::create_dir(&root).expect("workspace root");
        let created = crate::r03_tests::success(
            &runtime,
            "workspace.create",
            json!({"selected_root":root,"display_name":"S04 supervision"}),
        )
        .await;
        let workspace: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(created["workspace"].clone()).expect("workspace summary");
        let provider_a = Provider::scenario(scenario_a).await;
        let model_a = profile_after_existing(&runtime, &provider_a.url).await;
        let session_a =
            create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
        let provider_b = Provider::scenario(scenario_b).await;
        let model_b = profile_after_existing(&runtime, &provider_b.url).await;
        let session_b =
            create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
        Self {
            _fixture: fixture,
            runtime: Arc::new(runtime),
            provider_a,
            model_a,
            session_a,
            provider_b,
            model_b,
            session_b,
        }
    }

    fn services(&self) -> &crate::RuntimeServices {
        match &self.runtime.startup_mode {
            crate::StartupMode::Ready(services) => services,
            _ => panic!("Core must be ready"),
        }
    }

    fn agent(&self) -> Arc<crate::agent_application::AgentApplication> {
        self.services().agent_required()
    }

    async fn start(&self, session: SessionId, model: ModelProfileId) -> AgentRunRequest {
        let started = waiting::start(&self.runtime, session, model, AgentTaskMode::Normal).await;
        AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id }
    }
}

async fn park_at_provider(provider: &Provider, budget: Duration) {
    let parked = tokio::time::timeout(budget, async {
        while provider.requests.lock().expect("fixture requests").is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(parked.is_ok(), "the real Provider request must be parked");
}

fn wait_until(deadline: Duration, predicate: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    predicate()
}

/// The same wait for a `#[tokio::test]`: a std sleep would park the single-threaded runtime and
/// stop the very task it is waiting for. This is a test-harness fact, not a product one.
async fn wait_until_async(deadline: Duration, predicate: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if predicate() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    predicate()
}

/// C1 - normal completion closes the run and its resources.
#[tokio::test]
async fn c1_a_normal_completion_really_ends_the_task_and_releases_its_entry() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, started) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: started.turn.session_id, agent_run_id: run };
    let done = waiting::wait(&runtime, &request, |p| {
        matches!(p.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let ended = wait_until_async(Duration::from_secs(10), || {
        !services.agent_required().supervisor().is_active(run)
    })
    .await;
    let fact =
        services.agent_required().supervisor().ledger().into_iter().find(|fact| fact.run == run);
    println!(
        "C1 ended={ended} active={:?} ledger={fact:?} turn={:?}",
        services.agent_required().supervisor().active_runs(),
        done.turn.state
    );
    assert!(ended, "a completed run must not stay admitted");
    assert!(services.agent_required().supervisor().active_runs().is_empty(), "no residue");
    assert_eq!(fact.map(|fact| fact.end), Some("completed"), "the real end is recorded");
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
}

/// C2 - a handler panic still ends the run, and the side effect's terminal state stays unknown.
#[tokio::test]
async fn c2_a_handler_panic_ends_the_run_and_records_an_unknown_interruption() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required().probe_arm_drive_panic();
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: session, agent_run_id: run };
    let ended = wait_until_async(Duration::from_secs(15), || {
        !services.agent_required().supervisor().is_active(run)
    })
    .await;
    let fact =
        services.agent_required().supervisor().ledger().into_iter().find(|fact| fact.run == run);
    let projection =
        services.agent_required().status(&request).await.expect("the projection is readable");
    let diagnostics = services.agent_required().store().diagnostics().of(run);
    // The panic fact is visible where it belongs: persisted on the run when the record could be
    // written, and as a run-local diagnostic when it could not.
    let persisted = services
        .agent_required()
        .store()
        .read(agent_store::StoreStep::run(run, "c2.persisted_panic"), move |db| {
            let mut q = db.prepare(
                "SELECT payload FROM events WHERE kind='agent.application_error' AND owner=?1",
            )?;
            q.query_map([run.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(Into::into)
        })
        .await
        .expect("read");
    println!(
        "C2 ended={ended} ledger={fact:?} turn={:?} diagnostics={diagnostics:?} \
         persisted={persisted:?}",
        projection.turn.state
    );
    assert!(ended, "a panic must not leave a permanent active entry");
    assert_eq!(fact.map(|fact| fact.end), Some("panicked"));
    assert!(
        matches!(projection.turn.state, TurnTaskState::RecoveryRequired { .. }),
        "a panic keeps an unknown/interrupted fact: {:?}",
        projection.turn.state
    );
    assert!(
        persisted.iter().any(|payload| payload.contains("agent.run_panicked")),
        "the panic fact is persisted on the run: {persisted:?}"
    );
    assert!(
        diagnostics.is_empty(),
        "a durable record spends the local diagnostic: {diagnostics:?}"
    );
    assert!(
        services.agent_required().store_health(agent_store::StoreDependency::Journal).available,
        "a panic in a handler is not a store-wide fact"
    );
}

/// C3 - the failure record itself failing does not block resource reclamation.
///
/// A's real run fails at its Provider timeout while the run's own record write is parked inside
/// the production call, so the record's caller ends bounded with a retained result. The run's
/// task still ends, its entry is released, the unrecorded fact stays local to that run, and
/// another session's status is served throughout.
#[test]
fn c3_a_failing_error_record_does_not_block_the_run_s_teardown() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let sessions = runtime.block_on(TwoRuns::new(Scenario::Hold, Scenario::Query));
    let agent = sessions.agent();
    // B is a real, independent session with a served status.
    let request_b = runtime.block_on(sessions.start(sessions.session_b, sessions.model_b));
    runtime.block_on(waiting::wait(&sessions.runtime, &request_b, |p| {
        matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. })
    }));
    // A parks at the Provider, then its own record write is held.
    let request_a = runtime.block_on(sessions.start(sessions.session_a, sessions.model_a));
    runtime.block_on(park_at_provider(&sessions.provider_a, Duration::from_secs(20)));
    sessions.runtime.sync.set_budget(Duration::from_millis(400));
    let hold = agent.store().arm_hold("agent.drive_failure_record", 4_000);
    // A's Provider never answers: the transport's first-event timeout fails the step, and the
    // record of that failure really runs and is really held. Each of the step's three attempts
    // waits out the endpoint's 10 s first-event limit (P05 §8.5; no longer cut at the 5 s
    // connect limit).
    let reached = wait_until(Duration::from_secs(45), || hold.was_reached());
    let ended = wait_until(Duration::from_secs(15), || {
        !agent.supervisor().is_active(request_a.agent_run_id)
    });
    let diagnostics = agent.store().diagnostics().of(request_a.agent_run_id);
    let b_status = runtime.block_on(agent.status(&request_b));
    println!(
        "C3 hold_reached={reached} run_ended={ended} diagnostics={diagnostics:?} b_status_ok={}",
        b_status.is_ok()
    );
    assert!(reached, "the run's own record write must really reach the production call");
    assert!(ended, "a failing record must not block task-resource reclamation");
    assert!(
        !diagnostics.is_empty(),
        "the fact that could not be recorded stays visible on the run itself"
    );
    assert!(b_status.is_ok(), "another session's status is served throughout");
    // Release: the record really commits, exactly once, and its result settles the receipt.
    hold.release_all();
    let settled = wait_until(Duration::from_secs(15), || {
        agent.store().receipts().iter().any(|receipt| {
            receipt.operation == "agent.drive_failure_record"
                && receipt.state == ReceiptState::Settled { committed: true, code: None }
        })
    });
    let errors = runtime.block_on(agent.store().read(
        agent_store::StoreStep::run(request_a.agent_run_id, "c3.error_events"),
        move |db| {
            let mut q =
                db.prepare("SELECT COUNT(*) FROM events WHERE kind='agent.application_error'")?;
            q.query_row([], |row| row.get::<_, i64>(0)).map_err(Into::into)
        },
    ));
    println!("C3 committed={settled} error_events={errors:?}");
    assert!(settled, "the parked record really commits once it is released");
    assert_eq!(errors.expect("read"), 1, "the commit happened exactly once");
}

/// C4 - stopping A leaves B alive, and B really completes afterwards.
///
/// B is parked at a real Provider barrier (ready, not done) when A is stopped; B's active entry
/// and its task are still there afterwards, and releasing B's barrier lets B finish for real.
#[test]
fn c4_stopping_one_run_leaves_the_other_alive_and_it_still_completes() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(6)
        .enable_all()
        .build()
        .expect("test runtime");
    let sessions = runtime.block_on(TwoRuns::new(Scenario::Hold, Scenario::SearchThenText));
    let agent = sessions.agent();
    let request_b = runtime.block_on(sessions.start(sessions.session_b, sessions.model_b));
    runtime.block_on(park_at_provider(&sessions.provider_b, Duration::from_secs(20)));
    let request_a = runtime.block_on(sessions.start(sessions.session_a, sessions.model_a));
    runtime.block_on(park_at_provider(&sessions.provider_a, Duration::from_secs(20)));
    assert!(agent.supervisor().is_active(request_a.agent_run_id));
    assert!(agent.supervisor().is_active(request_b.agent_run_id));
    let stopped = runtime.block_on(agent.cancel(&request_a)).expect("A stops");
    let a_state = stopped.turn.state.clone();
    let a_active = agent.supervisor().is_active(request_a.agent_run_id);
    let b_active = agent.supervisor().is_active(request_b.agent_run_id);
    let b_started = agent.supervisor().started(request_b.agent_run_id);
    println!(
        "C4 a_state={a_state:?} a_active={a_active} b_active={b_active} b_started={b_started} \
         ledger={:?}",
        agent.supervisor().ledger()
    );
    assert_eq!(a_state, TurnTaskState::Cancelled, "only A stops");
    assert!(!a_active, "A's task really ended");
    assert!(b_active, "B must still be admitted: stopping A is not stopping B");
    assert!(b_started, "B's own task is really running");
    // Release B's barrier: B completes for real.
    sessions.provider_b.release_window.notify_one();
    let done = runtime.block_on(waiting::wait(&sessions.runtime, &request_b, |p| {
        matches!(p.turn.state, TurnTaskState::Completed { .. })
    }));
    let b_ended = wait_until(Duration::from_secs(10), || {
        !agent.supervisor().is_active(request_b.agent_run_id)
    });
    println!("C4 b_done={:?} b_ended={b_ended}", done.turn.state);
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "B really completes");
    assert!(b_ended, "B's task ends on its own completion");
    assert!(agent.supervisor().active_runs().is_empty(), "no residue after both ends");
}

/// C5 - a later legitimate turn in the same session can start.
#[tokio::test]
async fn c5_a_later_legitimate_turn_starts_in_the_same_session_after_a_stop() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, started) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: started.turn.session_id, agent_run_id: run };
    park_at_provider(&provider, Duration::from_secs(20)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let stopped = services.agent_required().cancel(&request).await.expect("the stop is served");
    assert_eq!(stopped.turn.state, TurnTaskState::Cancelled);
    assert!(!services.agent_required().supervisor().is_active(run), "the stopped run really ended");
    // The same session accepts a new legitimate turn: the stop did not poison the session.
    let provider_next = Provider::scenario(Scenario::Text).await;
    let model_next = profile_after_existing(&runtime, &provider_next.url).await;
    let next =
        waiting::start(&runtime, request.session_id, model_next, AgentTaskMode::Normal).await;
    let next_request =
        AgentRunRequest { session_id: request.session_id, agent_run_id: next.run.agent_run_id };
    assert_ne!(next.run.agent_run_id, run, "a new run, not a replay of the stopped one");
    let done = waiting::wait(&runtime, &next_request, |p| {
        matches!(p.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    println!(
        "C5 next={:?} provider_requests={:?}",
        done.turn.state,
        provider_next.requests.lock().map(|requests| requests.len())
    );
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
}

/// C6 - a late completion cannot overwrite a newer generation.
#[tokio::test]
async fn c6_a_late_completion_from_an_old_generation_cannot_touch_a_newer_one() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, started) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: started.turn.session_id, agent_run_id: run };
    let done = waiting::wait(&runtime, &request, |p| {
        matches!(p.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let current = services
        .agent_required()
        .supervisor()
        .generation_of(run)
        .expect("the run has a generation");
    let stale = current.saturating_sub(1);
    assert!(stale < current, "an older generation exists to be late");
    let before = services.agent_required().status(&request).await.expect("projection");
    services
        .agent_required()
        .persist_drive_failure(
            run,
            stale,
            &anyhow::anyhow!("late failure from the previous generation"),
            false,
        )
        .await;
    let after = services.agent_required().status(&request).await.expect("projection");
    let diagnostics = services.agent_required().store().diagnostics().of(run);
    println!(
        "C6 generation={current} stale={stale} before={:?} after={:?}",
        before.turn.state, after.turn.state
    );
    assert_eq!(
        after.turn.state, before.turn.state,
        "an old generation must not overwrite a newer one's projection"
    );
    assert_eq!(after.turn.revision, done.turn.revision);
    assert!(
        diagnostics.iter().any(|entry| entry.code == "agent.stale_generation"),
        "the late fact is recorded where it belongs: {diagnostics:?}"
    );
}

/// C7 - shutdown lists real residue and real clean-up separately.
///
/// B's run can really end when its transport is cancelled; A's own interruption write is parked
/// inside the production call, so A cannot end inside the shutdown budget. The report must put
/// B in the clean-up list and A in the residue list - with where A really is and whether a real
/// observation owner took it - instead of calling the shutdown clean.
#[test]
fn c7_shutdown_separates_real_residue_from_real_cleanup() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(6)
        .enable_all()
        .build()
        .expect("test runtime");
    let sessions = runtime.block_on(TwoRuns::new(Scenario::Hold, Scenario::Hold));
    let agent = sessions.agent();
    let request_a = runtime.block_on(sessions.start(sessions.session_a, sessions.model_a));
    runtime.block_on(park_at_provider(&sessions.provider_a, Duration::from_secs(20)));
    let request_b = runtime.block_on(sessions.start(sessions.session_b, sessions.model_b));
    runtime.block_on(park_at_provider(&sessions.provider_b, Duration::from_secs(20)));
    assert!(agent.supervisor().is_active(request_a.agent_run_id));
    assert!(agent.supervisor().is_active(request_b.agent_run_id));
    // A's own interruption write is parked: its run cannot end inside the shutdown budget.
    let hold =
        agent.store().arm_hold(&format!("run={}/agent.interrupt", request_a.agent_run_id), HOLD_MS);
    // One shutdown budget for everything; it is short here so the observation is bounded.
    let outcome =
        runtime.block_on(agent.stop_all_within(Instant::now() + Duration::from_millis(300)));
    println!(
        "C7 ended={:?} residue={:?} waited_ms={} ledger={:?}",
        outcome.ended,
        outcome.residue,
        outcome.waited_ms,
        agent.supervisor().ledger()
    );
    assert_eq!(outcome.ended, vec![request_b.agent_run_id], "B really ended inside the budget");
    assert_eq!(outcome.residue.len(), 1, "A is reported as residue, not as cleaned up");
    let residue = &outcome.residue[0];
    assert_eq!(residue.run, request_a.agent_run_id);
    assert_eq!(residue.stage, "provider", "the residue names where the run really is");
    assert!(residue.observed, "the residue is handed to a real observation owner");
    assert!(residue.observation_budget_ms > 0, "with a real budget");
    assert!(
        !outcome.ended.contains(&request_a.agent_run_id),
        "a run that did not end is never listed as cleaned up"
    );
    // Release the held write: A really ends, and the fact is recorded.
    hold.release_all();
    let released = wait_until(Duration::from_secs(25), || {
        !agent.supervisor().is_active(request_a.agent_run_id)
    });
    let facts = agent.supervisor().ledger();
    println!("C7 a_ended={released} facts={facts:?}");
    assert!(released, "the handed-over run really ended and was recorded");
    assert!(
        facts.iter().any(|fact| fact.run == request_a.agent_run_id),
        "the real end of the handed-over run is a terminal fact: {facts:?}"
    );
    assert!(
        facts.iter().any(|fact| fact.run == request_b.agent_run_id && fact.end == "completed"),
        "B's own end is a terminal fact too: {facts:?}"
    );
}

/// C4b-1 - a cancel delivered **during the prepare stage** ends the run there: the run is really
/// parked inside the production prepare call when the stop arrives, and no model step completes.
///
/// The hold is on `agent.refresh_prompt.bundle`, the first Store step of `context::prepare`, so
/// "the run is in the prepare stage" is a property of the target production call rather than of a
/// sleep. Before this case only the Provider stage had a real barrier.
#[test]
fn c4b_a_cancel_during_the_prepare_stage_ends_the_run_where_it_really_was() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let host = crate::r03_tests::Fixture::new();
    let provider = runtime.block_on(Provider::scenario(Scenario::Text));
    let (state, session, model) = runtime.block_on(waiting::prepare(&host, &provider));
    let crate::StartupMode::Ready(services) = &state.startup_mode else { panic!("Core ready") };
    let agent = services.agent_required();
    let hold = agent.store().arm_hold("agent.refresh_prompt.bundle", HOLD_MS);
    let started = runtime.block_on(waiting::start(&state, session, model, AgentTaskMode::Normal));
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: session, agent_run_id: run };
    let reached = wait_until(Duration::from_secs(20), || hold.was_reached());
    let stopped = runtime.block_on(agent.cancel(&request)).expect("the stop is served");
    let ended = wait_until(Duration::from_secs(15), || !agent.supervisor().is_active(run));
    let fact = agent.supervisor().ledger().into_iter().find(|fact| fact.run == run);
    let requests = provider.requests.lock().expect("requests").len();
    println!(
        "C4b-prepare hold_reached={reached} turn={:?} ended={ended} fact={fact:?} \
         provider_requests={requests}",
        stopped.turn.state
    );
    assert!(reached, "the run must really be inside the prepare stage's own store step");
    assert_eq!(stopped.turn.state, TurnTaskState::Cancelled, "only a real stop is reported");
    assert!(ended, "the stopped run really ended");
    assert!(fact.is_some(), "the real end is a terminal fact");
    assert_eq!(requests, 0, "a run stopped in preparation never reached the Provider");
    assert!(
        !stopped
            .model_steps
            .iter()
            .any(|step| matches!(step.state, ModelStepState::Completed { .. })),
        "no model step completed: {:?}",
        stopped.model_steps
    );
}

/// C4b-2 - a cancel delivered **during the tool stage** is recorded before anything was dispatched.
///
/// The hold is on `agent.drive.step_tools`, the read the drive loop performs immediately before it
/// admits the model's tool batch, so the run is really in the tool stage when the stop arrives. The
/// admitted call must then carry the contract's own "cancelled before dispatch" reason and must
/// never acquire an Execution Attempt.
#[test]
fn c4b_a_cancel_during_the_tool_stage_is_recorded_before_any_dispatch() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("test runtime");
    let host = crate::r03_tests::Fixture::new();
    // `Write` asks for a real tool, so the first tool batch is reached through the production loop.
    let provider = runtime.block_on(Provider::scenario(Scenario::Write));
    let (state, session, model) = runtime.block_on(waiting::prepare(&host, &provider));
    let crate::StartupMode::Ready(services) = &state.startup_mode else { panic!("Core ready") };
    let agent = services.agent_required();
    let hold = agent.store().arm_hold("agent.drive.step_tools", HOLD_MS);
    let started = runtime.block_on(waiting::start(&state, session, model, AgentTaskMode::Normal));
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: session, agent_run_id: run };
    let reached = wait_until(Duration::from_secs(20), || hold.was_reached());
    let stopped = runtime.block_on(agent.cancel(&request)).expect("the stop is served");
    let ended = wait_until(Duration::from_secs(15), || !agent.supervisor().is_active(run));
    let calls: Vec<Value> = {
        let journal = services.broker.lock().expect("journal");
        let mut q = journal
            .database
            .connection()
            .prepare("SELECT payload FROM tool_calls ORDER BY rowid")
            .expect("query");
        q.query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("query")
            .iter()
            .map(|text| serde_json::from_str(text).expect("tool call record"))
            .collect()
    };
    println!(
        "C4b-tools hold_reached={reached} turn={:?} ended={ended} calls={calls:?}",
        stopped.turn.state
    );
    assert!(reached, "the run must really be inside the tool stage's own store step");
    assert_eq!(stopped.turn.state, TurnTaskState::Cancelled, "only a real stop is reported");
    assert!(ended, "the stopped run really ended");
    assert!(
        !calls.is_empty(),
        "the model's own tool call was admitted before the stop was observed"
    );
    for call in &calls {
        assert!(
            call["attempt_id"].is_null(),
            "a call stopped in the tool stage never acquired an Attempt: {call}"
        );
        assert_eq!(
            call["result"]["outcome"]["type"], "not_executed",
            "it is a not-executed result, not a failure or an unknown: {call}"
        );
        assert_eq!(
            call["result"]["outcome"]["reason"], "cancelled_before_dispatch",
            "and the reason is the contract's own: {call}"
        );
    }
}

/// The receipt of a retained write reports the real terminal fact, not the caller's guess.
#[tokio::test]
async fn c8_a_receipt_reports_the_real_terminal_fact() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, started) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let run = started.run.agent_run_id;
    services.agent_required().idle(run).await.expect("the run ended");
    let committed = services
        .agent_required()
        .store()
        .write_retained(agent_store::StoreStep::run(run, "c8.real_write"), move |journal| {
            journal.database.unit_of_work(|tx| {
                aura_persistence_v2::agent::application::request_cancel(
                    tx,
                    run,
                    UtcTimestamp::now(),
                )
            })
        })
        .await
        .expect("the real write commits");
    let state = committed.1.and_then(|receipt| services.agent_required().store().receipt(receipt));
    println!("C8 receipt={state:?}");
    assert_eq!(
        state.map(|receipt| receipt.state),
        Some(ReceiptState::Settled { committed: true, code: None })
    );
}
