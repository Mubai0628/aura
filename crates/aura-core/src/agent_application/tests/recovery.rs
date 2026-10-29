//! Dropping a dedicated runtime aborts the live workers without the graceful
//! cancellation path. Reopening the actual stores exercises startup recovery;
//! process-level killing is separately covered by the final packaged harness.
use super::*;
use provider_fixture::Scenario;
use std::time::Duration;

/// Continuing the turns a restart cut off.
#[path = "auto_continue.rs"]
mod auto_continue;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interrupted_real_provider_and_compaction_are_terminal_without_replay() {
    for (scenario, requests) in [(Scenario::Hold, 1), (Scenario::CompactHold, 3)] {
        let host = Arc::new(crate::r03_tests::Fixture::new());
        let provider = Arc::new(Provider::scenario(scenario).await);
        let child_host = host.clone();
        let child_provider = provider.clone();
        let (request, before) = tokio::task::spawn_blocking(move || {
            let process = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("persisted recovery fixture and evidence");
            let value = process.block_on(async {
                let (runtime, start) =
                    waiting::begin(&child_host, &child_provider, AgentTaskMode::Normal).await;
                tokio::time::timeout(Duration::from_secs(30), async {
                    while child_provider
                        .requests
                        .lock()
                        .expect("persisted recovery fixture and evidence")
                        .len()
                        < requests
                    {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .expect("persisted recovery fixture and evidence");
                let before = attempts(&runtime);
                assert!(before.iter().any(|v| !terminal(v.state)));
                (
                    AgentRunRequest {
                        session_id: start.turn.session_id,
                        agent_run_id: start.run.agent_run_id,
                    },
                    before,
                )
            });
            process.shutdown_timeout(Duration::from_secs(2));
            value
        })
        .await
        .expect("persisted recovery fixture and evidence");
        let runtime = host.start().await;
        let restored: AgentRunProjection = serde_json::from_value(
            crate::r03_tests::success(&runtime, "agent.status", json!(request)).await,
        )
        .expect("persisted recovery fixture and evidence");
        assert!(matches!(restored.run.state, AgentRunState::RecoveryRequired { .. }));
        assert_eq!(restored.recovery_episodes.len(), 1);
        let after = attempts(&runtime);
        assert_eq!(before.len(), after.len());
        for (old, recovered) in before.iter().zip(&after) {
            assert_eq!(old.id, recovered.id);
            assert_eq!(old.request_evidence, recovered.request_evidence);
            if terminal(old.state) {
                assert_eq!(old, recovered);
            } else {
                assert_eq!(recovered.state, TransportAttemptState::Failed);
                assert!(recovered.finished_at.is_some());
                let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
                let journal = s.broker.lock().expect("persisted recovery fixture and evidence");
                let raw: String = journal.database.connection().query_row(
                    "SELECT payload FROM events WHERE kind='model.failure' AND json_extract(payload,'$.id')=?1",
                    [recovered.failure_id.expect("persisted recovery fixture and evidence").to_string()], |r| r.get(0),
                ).expect("persisted recovery fixture and evidence");
                let failure: PersistedModelFailure =
                    serde_json::from_str(&raw).expect("persisted recovery fixture and evidence");
                assert_eq!(failure.failure.retry_class, ModelRetryClass::NotRetryable);
                assert_eq!(failure.failure.transport_attempt_id, Some(old.id));
                assert_eq!(failure.failure.code, "agent.transport_interrupted");
            }
        }
        if requests == 3 {
            let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
            let journal = s.broker.lock().expect("persisted recovery fixture and evidence");
            let state: String = journal
                .database
                .connection()
                .query_row("SELECT state FROM context_compactions", [], |r| r.get(0))
                .expect("persisted recovery fixture and evidence");
            assert_eq!(state, "interrupted");
        }
        let reopened = host.restart(runtime).await;
        assert_eq!(
            attempts(&reopened),
            after,
            "second recovery cannot create new failures or attempts"
        );
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(
            provider.requests.lock().expect("persisted recovery fixture and evidence").len(),
            requests,
            "no automatic remote replay"
        );
    }
}

fn attempts(runtime: &crate::RuntimeState) -> Vec<TransportAttemptRecord> {
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
    let j = s.broker.lock().expect("persisted recovery fixture and evidence");
    let mut q = j
        .database
        .connection()
        .prepare("SELECT payload FROM transport_attempts ORDER BY id")
        .expect("persisted recovery fixture and evidence");
    q.query_map([], |r| r.get::<_, String>(0))
        .expect("persisted recovery fixture and evidence")
        .map(|r| {
            serde_json::from_str(&r.expect("persisted recovery fixture and evidence"))
                .expect("persisted recovery fixture and evidence")
        })
        .collect()
}
fn terminal(state: TransportAttemptState) -> bool {
    matches!(
        state,
        TransportAttemptState::Completed
            | TransportAttemptState::Failed
            | TransportAttemptState::Cancelled
    )
}

/// Minimal facts from session 1: an unfinished lineage, an interrupted request,
/// and a later turn that reserves its identity before reconciliation. No private
/// messages, credentials or fixture writes are involved.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unfinished_session_can_send_and_send_again_without_replaying_history() {
    let host = Arc::new(crate::r03_tests::Fixture::new());
    let provider = Arc::new(Provider::scenario(Scenario::RecoveredSession).await);
    let child_host = host.clone();
    let child_provider = provider.clone();
    let request = tokio::task::spawn_blocking(move || {
        let process =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        let request = process.block_on(async {
            let (_runtime, start) =
                waiting::begin(&child_host, &child_provider, AgentTaskMode::Normal).await;
            tokio::time::timeout(Duration::from_secs(30), async {
                while child_provider.requests.lock().expect("requests").is_empty() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("request started");
            AgentRunRequest {
                session_id: start.turn.session_id,
                agent_run_id: start.run.agent_run_id,
            }
        });
        process.shutdown_timeout(Duration::from_secs(2));
        request
    })
    .await
    .expect("interrupted host");
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let before = services.agent_required().status(&request).await.expect("recovered history");
    assert!(matches!(before.turn.state, TurnTaskState::RecoveryRequired { .. }));
    let model = {
        let store = services.broker.lock().expect("store");
        aura_persistence_v2::model::step_snapshot(
            store.database.connection(),
            before.model_steps[0].model_step_id,
        )
        .expect("original immutable model snapshot")
        .model_profile_id
    };
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({
            "expected_revision":"0","session_id":request.session_id,"model_profile_id":model
        }),
    )
    .await;
    let rejected = crate::r03_tests::request(
        &runtime,
        "agent.related.start",
        json!(AgentRelatedTurnRequest {
            session_id: request.session_id,
            source_turn_task_id: before.turn.turn_task_id,
            expected_revision: before.turn.revision,
            kind: AgentRelatedTurnKind::Recover,
            goal_budget: None
        }),
    )
    .await;
    assert!(rejected.get("error").is_some());
    submit_read_only(&runtime, request.session_id).await;
    let recovered = next_completed(&runtime, request.session_id, before.turn.turn_task_id).await;
    assert!(!recovered.turn.lineage_resolution_required);
    assert_ne!(recovered.turn.lineage_id, before.turn.lineage_id);
    assert_eq!(
        services.agent_required().status(&request).await.expect("old history").turn,
        before.turn
    );
    services.agent_required().idle(recovered.run.agent_run_id).await.expect("completion boundary");
    submit_read_only(&runtime, request.session_id).await;
    let next = next_completed(&runtime, request.session_id, recovered.turn.turn_task_id).await;
    assert_ne!(next.run.agent_run_id, recovered.run.agent_run_id);
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        3,
        "one interrupted request, first new completion and next send; no replay"
    );
}

async fn submit_read_only(runtime: &crate::RuntimeState, session_id: SessionId) {
    crate::r03_tests::success(
        runtime,
        "agent.submit",
        json!(AgentSubmitRequest {
            session_id,
            text: "Continue the read-only answer".into(),
            destination: AgentInputLane::NextTurn,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
}

async fn next_completed(
    runtime: &crate::RuntimeState,
    session_id: SessionId,
    previous: TurnTaskId,
) -> AgentRunProjection {
    let run = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
                panic!("Core ready")
            };
            let history = services
                .agent_required()
                .history(AgentHistoryRequest {
                    session_id,
                    before_turn_task_id: None,
                    within_turn: None,
                    limit: 10,
                })
                .await
                .expect("history");
            if let Some(latest) = history.turns.last().filter(|t| t.turn.turn_task_id != previous) {
                return latest.turn.agent_run_id;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("automatic queue start");
    waiting::wait(runtime, &AgentRunRequest { session_id, agent_run_id: run }, |view| {
        matches!(view.turn.state, TurnTaskState::Completed { .. })
    })
    .await
}
