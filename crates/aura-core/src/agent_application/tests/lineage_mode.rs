use super::*;
use aura_projection_contract::AgentSessionProjection;
use provider_fixture::Scenario;
use waiting::{begin, wait};

async fn wait_state(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
    predicate: impl Fn(&AgentRunProjection) -> bool,
) -> AgentRunProjection {
    tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let view: AgentRunProjection = serde_json::from_value(
                crate::r03_tests::success(runtime, "agent.status", json!(request)).await,
            )
            .expect("Agent test setup and persisted assertion");
            if predicate(&view) {
                return view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("Agent test setup and persisted assertion")
}

#[tokio::test]
async fn plan_and_goal_survive_normal_completion_and_the_next_user_turn() {
    for mode in [AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::FinishTurn).await;
        let (runtime, start) = begin(&host, &provider, mode).await;
        let request = AgentRunRequest {
            session_id: start.turn.session_id,
            agent_run_id: start.run.agent_run_id,
        };
        let done =
            wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
                .await;
        // The frozen request the model really received states this mode's single
        // completion route instead of promising that prose ends the turn.
        let model_request = provider
            .requests
            .lock()
            .expect("Agent test setup and persisted assertion")
            .first()
            .map(ToString::to_string)
            .expect("one real model request");
        assert!(
            model_request.contains("finish a single turn through normal.complete"),
            "{mode:?} request keeps the explicit single-turn completion"
        );
        assert!(
            model_request.contains("mode.disposition"),
            "{mode:?} request states the intent disposition"
        );
        assert!(
            !model_request.contains("the next user message continues Goal"),
            "{mode:?} request must not promise silent Goal resumption"
        );
        assert!(
            !model_request.contains("submit your final answer through normal.complete"),
            "{mode:?} request must not carry the superseded Normal mandate"
        );
        assert_eq!(done.turn.mode, mode);
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
            panic!("Core ready")
        };
        services.agent_required().idle(done.run.agent_run_id).await.expect("completion boundary");
        let history: AgentSessionProjection = serde_json::from_value(
            crate::r03_tests::success(
                &runtime,
                "agent.history",
                json!({
                    "session_id": start.turn.session_id,
                    "before_turn_task_id": null,
                    "within_turn": null,
                    "limit": 10
                }),
            )
            .await,
        )
        .expect("history");
        let episode = history.mode_episode.expect("mode episode");
        assert_eq!(episode.mode, mode);
        let model = {
            let store = services.broker.lock().expect("store");
            aura_persistence_v2::model::step_snapshot(
                store.database.connection(),
                done.model_steps[0].model_step_id,
            )
            .expect("completed step snapshot")
            .model_profile_id
        };
        crate::r03_tests::success(
            &runtime,
            "models.session.set_binding",
            json!({
                "expected_revision": "0",
                "session_id": start.turn.session_id,
                "model_profile_id": model
            }),
        )
        .await;
        crate::r03_tests::success(
            &runtime,
            "agent.submit",
            json!(AgentSubmitRequest {
                session_id: start.turn.session_id,
                text: "Stay in the same mode".into(),
                destination: AgentInputLane::NextTurn,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                attachments: Vec::new(),
            }),
        )
        .await;
        let next = tokio::time::timeout(std::time::Duration::from_secs(45), async {
            loop {
                let history: AgentSessionProjection = serde_json::from_value(
                    crate::r03_tests::success(
                        &runtime,
                        "agent.history",
                        json!({
                            "session_id": start.turn.session_id,
                            "before_turn_task_id": null,
                            "within_turn": null,
                            "limit": 10
                        }),
                    )
                    .await,
                )
                .expect("history");
                if let Some(latest) =
                    history.turns.last().filter(|t| t.turn.turn_task_id != done.turn.turn_task_id)
                {
                    let run_id = latest.turn.agent_run_id;
                    let turn_mode = latest.turn.mode;
                    let episode = history.mode_episode.clone();
                    return (run_id, turn_mode, episode);
                }
                assert!(
                    !history.inbox.paused,
                    "queued send did not start: {:?}",
                    history.inbox.pause_reason
                );
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        })
        .await
        .expect("next turn");
        assert_eq!(next.1, mode);
        let episode = next.2.clone().expect("kept");
        assert_eq!(episode.mode, mode);
        let terminated: AgentModeEpisodeView = serde_json::from_value(
            crate::r03_tests::success(
                &runtime,
                "agent.mode.control",
                json!({
                    "session_id": start.turn.session_id,
                    "expected_revision": episode.revision,
                    "action": { "action": "terminate" }
                }),
            )
            .await,
        )
        .expect("terminate");
        assert_eq!(terminated.mode, AgentTaskMode::Normal);
        assert!(!terminated.paused);
    }
}

#[tokio::test]
async fn protocol_end_turn_loop_stops_without_marking_the_turn_completed() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ProtocolLoop).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Plan).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let stopped = wait_state(&runtime, &request, |p| {
        matches!(
            &p.turn.state,
            TurnTaskState::RecoveryRequired { reason } if reason == "agent.protocol_not_completed"
        )
    })
    .await;
    assert_eq!(stopped.turn.mode, AgentTaskMode::Plan);
    let history: AgentSessionProjection = serde_json::from_value(
        crate::r03_tests::success(
            &runtime,
            "agent.history",
            json!({
                "session_id": start.turn.session_id,
                "before_turn_task_id": null,
                "within_turn": null,
                "limit": 10
            }),
        )
        .await,
    )
    .expect("history");
    assert_eq!(history.mode_episode.expect("plan kept").mode, AgentTaskMode::Plan);
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        2,
        "one reminder request then stop"
    );
}

#[tokio::test]
async fn deterministic_lineage_failures_stop_after_two_post_feedback_corrections() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::InvalidReconcile).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let stopped = wait_state(&runtime, &request, |p| {
        matches!(
            &p.turn.state,
            TurnTaskState::RecoveryRequired { reason }
                if reason == "agent.deterministic_correction_exhausted"
        )
    })
    .await;
    assert!(!matches!(stopped.turn.state, TurnTaskState::Completed { .. }));
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        3,
        "first error plus two post-feedback retries"
    );
}
