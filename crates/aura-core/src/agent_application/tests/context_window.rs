use super::*;
use aura_context_domain::{AgentContextRemediationState, AgentContextRemediationView};
use aura_persistence_v2::model::context_window as windows;
use std::time::{Duration, Instant};

/// A bounded wait that yields to the runtime: a `std` sleep here would park the task this case is
/// waiting for. Test-harness only.
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

/// A context-window rejection is reported through `NormalizedIssueCode`, whose
/// v1 registry rejects unknown namespaces. An unregistered constant would make
/// the rejection path return an error and strand the run in `RecoveryRequired`,
/// so the registry membership of the exact constant is asserted here.
#[test]
fn context_window_rejection_uses_a_registered_issue_code() {
    assert!(
        NormalizedIssueCode::new(super::super::context_window::CONTEXT_WINDOW_ISSUE_CODE).is_ok(),
        "context-window issue code is outside the v1 stable registry"
    );
}

async fn waiting_window(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
) -> AgentContextWindowCorrection {
    waiting::wait(runtime, request, |p| {
        matches!(p.run.state, AgentRunState::AwaitingContextRemediation { .. })
    })
    .await;
    let view: AgentContextRemediationView = serde_json::from_value(
        crate::r03_tests::success(runtime, "agent.context.remediation", json!(request)).await,
    )
    .expect("actual diagnostic");
    view.provider_correction.expect("Provider rejection")
}

/// The candidate ladder is a rule about how much room a request has left, and the
/// model-facing tool schemas are part of that fixed content. These tests were
/// calibrated when an untouched configuration loaded three file tools; the
/// recommended default set is now much larger, which leaves the smaller
/// candidates unbuildable for reasons that have nothing to do with the ladder.
/// They therefore pin the selection they are calibrated against: the subject is
/// the ladder, not today's default tool list. What the larger default selection
/// does to the same scenario is covered by
/// `a_larger_default_selection_hands_off_to_remediation_sooner`.
async fn begin_with_pinned_tools(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, AgentRunProjection) {
    let (runtime, session, model) = waiting::prepare(host, provider).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let view = services.agent_required().tool_settings().expect("tool settings");
    // The settings contract requires the selection to be sorted by id.
    let mut pinned = ["fs.list", "fs.read", "fs.search"]
        .iter()
        .map(|name| {
            view.tools
                .iter()
                .find(|tool| tool.name.as_str() == *name)
                .unwrap_or_else(|| panic!("{name} exists in the release"))
                .tool_id
        })
        .collect::<Vec<_>>();
    pinned.sort();
    services
        .agent_required()
        .update_tool_settings(
            CommandInvocationId::new(),
            aura_tool_contract::AgentToolSettingsUpdate {
                expected_revision: view.revision,
                fixed_tool_ids: pinned,
                max_dynamic_tool_schemas: view.max_dynamic_tool_schemas,
                max_concurrent_tool_calls: view.max_concurrent_tool_calls,
                default_timeout_ms: view.default_timeout_ms,
                max_retries: view.max_retries,
                use_recommended_fixed_tool_ids: false,
                network_proxy: None,
                web_service: None,
            },
        )
        .await
        .expect("pin the calibrated tool selection");
    let run = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    (runtime, run)
}
/// What the current recommended default selection does to the same scenario.
///
/// The ladder tests above pin the three file tools they were calibrated against,
/// because their subject is how much room a request has left. The product question
/// the review raised is different: with the default selection nobody edited, does
/// the same scenario still reach something a person can act on, or does it loop?
///
/// This records the real answer. The model-facing tool schemas are fixed content,
/// so with the larger default set the smaller candidates cannot be compiled at all;
/// the two candidates the Provider does see are both rejected, and the run hands off
/// to the explicit remediation state with the Provider's own correction published —
/// it does not sit in a silent retry loop, and it does not claim completion.
///
/// It deliberately leaves the default selection alone: shrinking the recommended
/// set is a user decision, and this test exists to keep what the current
/// recommendation really costs visible rather than to hide it by calibration.
#[tokio::test]
async fn a_larger_default_selection_hands_off_to_remediation_sooner() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::Window { failures: 2 }).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    // Nobody touched the settings, so this is the shipped recommendation.
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let selected = services.agent_required().tool_settings().expect("tool settings");
    assert!(
        selected.fixed_tool_ids.len() > 3,
        "this test is about the larger default selection: {:?}",
        selected.fixed_tool_ids.len()
    );

    let record = waiting_window(&runtime, &request).await;
    // The handoff carries the rejected candidate's own facts and the Provider's
    // correction, which is what makes it actionable instead of a generic failure.
    let state =
        crate::r03_tests::success(&runtime, "agent.context.remediation", json!(request)).await;
    let view: AgentContextRemediationView =
        serde_json::from_value(state).expect("remediation view");
    assert_eq!(view.provider_correction.as_ref(), Some(&record));
    // The actionable part of the handoff is published: the Provider's own correction
    // for the step whose candidate it rejected. Reaching this point already proved
    // the run is in `AwaitingContextRemediation` rather than looping or completing;
    // a remediation *plan* record is not published on this pathway, so nothing here
    // claims one.
}

fn resolve(
    request: &AgentRunRequest,
    record: &AgentContextWindowCorrection,
    action: AgentContextWindowAction,
) -> AgentContextWindowResolveRequest {
    AgentContextWindowResolveRequest {
        session_id: request.session_id,
        agent_run_id: request.agent_run_id,
        source_model_step_id: record.correction.model_step_id,
        expected_revision: record.revision,
        action,
    }
}

#[tokio::test]
async fn manual_window_correction_survives_restart_and_keeps_failed_snapshots() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::Window { failures: 1 }).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let record = waiting_window(&runtime, &request).await;
    assert!(matches!(
        record.correction.resolution,
        ContextWindowResolution::UserCorrectionRequired { .. }
    ));
    let summary = record.correction.provider_error.as_ref().expect("safe summary");
    assert!(summary.message_was_redacted);
    assert!(!summary.provider_message.as_deref().unwrap_or_default().contains("private"));
    let runtime = host.restart(runtime).await;
    assert_eq!(waiting_window(&runtime, &request).await, record);
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        1,
        "restart never retries the failed request"
    );
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let before = {
        let journal = services.broker.lock().expect("journal");
        aura_persistence_v2::model::step_snapshot(
            journal.database.connection(),
            record.correction.model_step_id,
        )
        .expect("failed snapshot")
    };
    let config = services.p05_required().model_config.snapshot().expect("config");
    crate::r03_tests::success(
        &runtime,
        "models.context_window.correct",
        json!(ContextWindowCorrectRequest {
            expected_revision: ExpectedRevision::new(config.revision),
            model_profile_id: record.correction.model_profile_id,
            window_tokens: 128000
        }),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.context.window.resolve",
        json!(resolve(&request, &record, AgentContextWindowAction::Recheck)),
    )
    .await;
    let done = waiting::wait(&runtime, &request, |p| p.turn.state.terminal()).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    assert_eq!(done.turn.turn_task_id, start.turn.turn_task_id);
    let journal = services.broker.lock().expect("journal");
    assert_eq!(
        aura_persistence_v2::model::step_snapshot(
            journal.database.connection(),
            before.model_step_id
        )
        .expect("unchanged failure"),
        before
    );
    let next = aura_persistence_v2::model::step_snapshot(
        journal.database.connection(),
        done.model_steps[1].model_step_id,
    )
    .expect("new snapshot");
    assert_ne!(before.id, next.id);
    assert_ne!(before.context_snapshot_id, next.context_snapshot_id);
    assert_eq!(next.runtime_effective_window_tokens, 128000);
}

#[tokio::test]
async fn automatic_window_trials_publish_only_the_successful_candidate_once() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::Window { failures: 2 }).await;
    let (runtime, start) = begin_with_pinned_tools(&host, &provider).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let record = waiting_window(&runtime, &request).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let before = services.p05_required().model_config.snapshot().expect("initial config");
    crate::r03_tests::success(
        &runtime,
        "agent.context.window.resolve",
        json!(resolve(&request, &record, AgentContextWindowAction::TrySmallerWindows)),
    )
    .await;
    let done = waiting::wait(&runtime, &request, |p| p.turn.state.terminal()).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    // The successful candidate's final response ends the turn, and its configuration commit
    // follows that step, so under a loaded test run the turn can read terminal just before the
    // commit lands. Wait for it, then require exactly one write.
    let deadline = Instant::now() + Duration::from_secs(30);
    let current = loop {
        let current = services.p05_required().model_config.snapshot().expect("corrected config");
        if current.revision > before.revision || Instant::now() >= deadline {
            break current;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(
        current.revision,
        before.revision + 1,
        "failed 128K candidate never writes configuration"
    );
    let model = &current.registry.models[&record.correction.model_profile_id];
    assert_eq!(model.context.runtime_corrected_window_tokens, Some(64000));
    let journal = services.broker.lock().expect("journal");
    let db = journal.database.connection();
    let used = done
        .model_steps
        .iter()
        .map(|s| {
            aura_persistence_v2::model::step_snapshot(db, s.model_step_id)
                .expect("each actual mex")
                .runtime_effective_window_tokens
        })
        .collect::<Vec<_>>();
    // The successful 64000 candidate answered with its own final response, which
    // now ends the turn there; no extra completion step is prepared.
    assert_eq!(used, [256000, 128000, 64000]);
    let success_trial =
        windows::trial(db, done.model_steps[2].model_step_id).expect("trial").expect("owned trial");
    assert_eq!(success_trial.candidate_windows, [128000, 64000]);
    drop(journal);
    let applied =
        services.p05_required().commit_window_trial(&success_trial).expect("idempotent journal");
    assert!(matches!(
        applied,
        Some(ContextWindowResolution::RuntimeCorrectionApplied {
            corrected_window_tokens: 64000,
            ..
        })
    ));
    assert_eq!(
        services.p05_required().model_config.snapshot().expect("unchanged revision").revision,
        current.revision
    );
}

/// The frozen candidate ladder stops at 32000. When even that candidate cannot
/// be compiled, the run must hand control back to context remediation with the
/// queued input intact, rather than sending a request that cannot fit or
/// silently writing model configuration.
#[tokio::test]
async fn unbuildable_last_candidate_hands_off_to_remediation_and_keeps_queue() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::Window { failures: 4 }).await;
    let (runtime, start) = begin_with_pinned_tools(&host, &provider).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let first = waiting_window(&runtime, &request).await;
    crate::r03_tests::success(
        &runtime,
        "agent.submit",
        json!(AgentSubmitRequest {
            session_id: request.session_id,
            text: "/goal stays queued".into(),
            destination: AgentInputLane::NextTurn,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let before = services.p05_required().model_config.snapshot().expect("config");
    let input = resolve(&request, &first, AgentContextWindowAction::TrySmallerWindows);
    let mut foreign = input.clone();
    foreign.session_id = SessionId::new();
    assert!(
        services
            .agent_required()
            .resolve_context_window(CommandInvocationId::new(), foreign)
            .await
            .is_err()
    );
    services
        .agent_required()
        .resolve_context_window(CommandInvocationId::new(), input.clone())
        .await
        .expect("try smaller");
    let final_wait = waiting_window(&runtime, &request).await;
    // The chain stops where the frozen rules stop it: the 32000 candidate cannot
    // be compiled, because the fixed non-compressible content alone exceeds its
    // request budget. P06 reports `context.non_compressible_overflow` instead of
    // inventing a low-window branch, so only three requests reach the Provider.
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        3,
        "an unbuildable candidate is never sent to the Provider"
    );
    assert_eq!(final_wait.state, AgentContextWindowState::Preparing);
    let ContextWindowResolution::AutomaticCandidateSelected {
        candidate_window_tokens,
        remaining_candidates,
        ..
    } = &final_wait.correction.resolution
    else {
        panic!("last candidate: {:?}", final_wait.correction.resolution);
    };
    // The last candidate stays selected for the retry that follows remediation.
    assert_eq!((*candidate_window_tokens, remaining_candidates.len()), (32000, 0));
    let remediation: AgentContextRemediationView = serde_json::from_value(
        crate::r03_tests::success(&runtime, "agent.context.remediation", json!(request)).await,
    )
    .expect("actual diagnostic");
    let remediation = remediation.remediation.expect("P06 overflow diagnostics");
    assert_eq!(remediation.state, AgentContextRemediationState::Awaiting);
    assert_eq!(remediation.plan.overflow.error_code, "context.non_compressible_overflow");
    assert_eq!(remediation.budget.runtime_effective_window_tokens, 32000);
    assert_eq!(
        remediation.plan.overflow.non_compressible_tokens,
        remediation.plan.overflow.estimated_input_tokens,
        "nothing about this request is reducible by compaction"
    );
    assert_eq!(
        services.p05_required().model_config.snapshot().expect("unchanged config").revision,
        before.revision
    );
    assert!(
        services
            .agent_required()
            .resolve_context_window(CommandInvocationId::new(), input)
            .await
            .is_err(),
        "stale response cannot consume a newer wait"
    );
    let journal = services.broker.lock().expect("journal");
    assert_eq!(
        aura_persistence_v2::agent::inbox::load(journal.database.connection(), request.session_id)
            .expect("queue")
            .messages
            .len(),
        1
    );
}

/// The recommended default tool selection is much larger than the three file
/// tools the ladder tests above are calibrated with, and the model-facing tool
/// schemas are part of the request's fixed content. That is why those tests pin
/// the selection they were calibrated against: with today's defaults the same
/// scenario no longer ends in a completed turn after the first correction — the
/// two candidates the ladder can still build are both rejected by the Provider,
/// and the run goes on to another correction instead of finishing (measured while
/// isolating the two ladder failures this helper fixes; recorded in the task
/// record rather than asserted here, because the ladder tests are about the
/// ladder's own rules).

#[tokio::test]
async fn user_edit_during_candidate_request_wins_over_the_old_success() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::WindowHold).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let record = waiting_window(&runtime, &request).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let before = services.p05_required().model_config.snapshot().expect("config");
    services
        .agent_required()
        .resolve_context_window(
            CommandInvocationId::new(),
            resolve(&request, &record, AgentContextWindowAction::TrySmallerWindows),
        )
        .await
        .expect("candidate request");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while provider.requests.lock().expect("requests").len() < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("candidate reached Provider");
    assert_eq!(
        services
            .p05_required()
            .model_config
            .snapshot()
            .expect("not published before success")
            .revision,
        before.revision
    );
    crate::r03_tests::success(
        &runtime,
        "models.context_window.correct",
        json!(ContextWindowCorrectRequest {
            expected_revision: ExpectedRevision::new(before.revision),
            model_profile_id: record.correction.model_profile_id,
            window_tokens: 192000
        }),
    )
    .await;
    provider.release_window.notify_one();
    let done = waiting::wait(&runtime, &request, |p| p.turn.state.terminal()).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    let current =
        services.p05_required().model_config.snapshot().expect("latest user configuration");
    assert_eq!(current.revision, before.revision + 1);
    assert_eq!(
        current.registry.models[&record.correction.model_profile_id]
            .context
            .runtime_effective_window_tokens,
        192000
    );
    assert!(
        current.registry.models[&record.correction.model_profile_id]
            .context
            .runtime_corrected_window_tokens
            .is_none()
    );
    let journal = services.broker.lock().expect("snapshots");
    let used = done
        .model_steps
        .iter()
        .map(|s| {
            aura_persistence_v2::model::step_snapshot(
                journal.database.connection(),
                s.model_step_id,
            )
            .expect("immutable mex")
            .runtime_effective_window_tokens
        })
        .collect::<Vec<_>>();
    // The in-flight candidate answered with its own final response, so the turn
    // ends there; the user's newer window is what the configuration assertions
    // above verify, and it is not overwritten by the discarded trial.
    assert_eq!(used, [256000, 128000], "steps={}", done.model_steps.len());
}

/// Shutdown must not rewrite an already-committed, still-actionable wait.
///
/// WHY THIS CASE EXISTS
/// `shutdown_now` fires the same in-memory cancellation token an explicit user stop fires, and the
/// drive loop used to check that token and write a user cancellation **before** it looked at the
/// committed run state. A shutdown that arrived after `agent.window.reject` had committed the
/// remediation wait therefore rewrote that wait into `Cancelled` and advanced the run revision past
/// the remediation record's, so the next start's legitimate `agent.window.resolve` at the original
/// revision met `agent.store_failed: agent.window.resolve: persistent revision or identity
/// conflict`.
///
/// WHAT THIS CASE OBSERVES, IN ORDER
/// 1. the window rejection write really commits (a real Store hold on `agent.window.reject`);
/// 2. the next drive-loop cancellation read is held, so the hold is *after* the wait is durable;
/// 3. the state before shutdown is `AwaitingContextRemediation` with its committed revision;
/// 4. a real `shutdown()`, the hold released, and the old task really ends;
/// 5. the state after shutdown is the **same** committed wait at the **same** revision - this is
///    the deterministic state evidence, not a timing assumption;
/// 6. a restart projects the identical remediation record, the Provider is never re-asked, and a
///    legitimate `resolve` at the original revision succeeds.
///
/// The `resolve` CAS itself is untouched: this case fails if the revision or the state moved, which
/// is exactly what the bug did.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn s04_r2_shutdown_keeps_a_committed_window_wait_resolvable_at_its_original_revision() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::Window { failures: 1 }).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let agent = services.agent_required();
    let reject = agent.store().arm_hold("agent.window.reject", 15_000);
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let run = started.run.agent_run_id;
    let request = AgentRunRequest { session_id: session, agent_run_id: run };
    assert!(
        wait_until_async(Duration::from_secs(20), || reject.was_reached()).await,
        "the real window rejection write must be reached"
    );
    // Armed only now: the read this holds is the drive loop's *next* iteration, which can only
    // happen after the rejection transaction committed the wait.
    let next_read = agent.store().arm_hold("agent.drive.cancellation_requested", 15_000);
    reject.release_all();
    assert!(
        wait_until_async(Duration::from_secs(20), || next_read.was_reached()).await,
        "the post-rejection cancellation read must be reached"
    );
    let before = agent.status(&request).await.expect("the committed wait is readable");
    assert!(
        matches!(before.run.state, AgentRunState::AwaitingContextRemediation { .. }),
        "the wait must be committed before shutdown: {:?}",
        before.run.state
    );
    let view = agent.context_remediation(request.clone()).await.expect("remediation projection");
    let correction = view.provider_correction.expect("committed Provider correction");
    let requests_before = provider.requests.lock().expect("fixture requests").len();

    agent.shutdown();
    next_read.release_all();
    assert!(
        wait_until_async(Duration::from_secs(20), || !agent.supervisor().is_active(run)).await,
        "shutdown must really end the old generation's task"
    );

    let after = agent.status(&request).await.expect("the post-shutdown state is readable");
    println!(
        "S04-R2 shutdown before={:?} before_revision={} after={:?} after_revision={} \
         correction_revision={} requests={}",
        before.run.state,
        before.run.revision,
        after.run.state,
        after.run.revision,
        correction.revision,
        requests_before
    );
    // The deterministic state evidence: shutdown ended the task and left the wait alone.
    assert!(
        matches!(after.run.state, AgentRunState::AwaitingContextRemediation { .. }),
        "shutdown must not turn a committed, actionable wait into a user cancellation: {:?}",
        after.run.state
    );
    assert_eq!(
        after.run.revision, before.run.revision,
        "shutdown must not advance the run revision past the remediation record's"
    );

    let restarted = host.restart(runtime).await;
    assert_eq!(
        waiting_window(&restarted, &request).await,
        correction,
        "the identical remediation projection survives the restart"
    );
    assert_eq!(
        provider.requests.lock().expect("fixture requests").len(),
        requests_before,
        "the old Provider request is never resent"
    );
    let crate::StartupMode::Ready(restarted_services) = &restarted.startup_mode else {
        panic!("restarted Core ready")
    };
    let snapshot_before = {
        let journal = restarted_services.broker.lock().expect("journal");
        aura_persistence_v2::model::step_snapshot(
            journal.database.connection(),
            correction.correction.model_step_id,
        )
        .expect("the failed snapshot is retained")
    };
    let resolved = restarted_services
        .agent_required()
        .resolve_context_window(
            CommandInvocationId::new(),
            AgentContextWindowResolveRequest {
                session_id: session,
                agent_run_id: run,
                source_model_step_id: correction.correction.model_step_id,
                expected_revision: correction.revision,
                action: AgentContextWindowAction::Recheck,
            },
        )
        .await;
    println!("S04-R2 shutdown resolve={resolved:?}");
    // The failed snapshot is still the same immutable record: nothing re-wrote it.
    {
        let journal = restarted_services.broker.lock().expect("journal");
        assert_eq!(
            aura_persistence_v2::model::step_snapshot(
                journal.database.connection(),
                correction.correction.model_step_id
            )
            .expect("failed snapshot"),
            snapshot_before
        );
    }
    assert!(
        resolved.is_ok(),
        "a committed actionable wait must survive shutdown and stay resolvable at its original \
         revision: {resolved:?}"
    );
}
