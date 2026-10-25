//! Review regressions for mode control. A rejected or replayed command must not
//! cancel the live run.
#![allow(clippy::unwrap_used)]
use super::provider_fixture::Scenario;
use super::waiting::{begin, wait};
use super::*;

#[tokio::test]
async fn invalid_plan_pause_must_not_cancel_the_current_run() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Plan).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.run.state, AgentRunState::ModelRunning { .. })).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let episode = {
        let store = services.broker.lock().unwrap();
        aura_persistence_v2::agent::mode::view(store.database.connection(), request.session_id)
            .unwrap()
    };
    let result = services
        .agent_required()
        .control_mode(
            CommandInvocationId::new(),
            AgentModeControlRequest {
                session_id: request.session_id,
                expected_revision: episode.revision,
                action: AgentModeControlAction::PauseGoal,
            },
        )
        .await;
    assert!(result.is_err(), "Plan pause must be rejected");
    let store = services.broker.lock().unwrap();
    assert!(
        !aura_persistence_v2::agent::application::cancellation_requested(
            store.database.connection(),
            request.agent_run_id
        )
        .unwrap(),
        "a rejected mode command still wrote a cancellation into the current run"
    );
}

#[tokio::test]
async fn retrying_an_old_terminate_command_must_not_cancel_a_new_normal_run() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Goal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let running =
        wait(&runtime, &request, |p| matches!(p.run.state, AgentRunState::ModelRunning { .. }))
            .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let (episode, model_profile_id) = {
        let store = services.broker.lock().unwrap();
        let db = store.database.connection();
        (
            aura_persistence_v2::agent::mode::view(db, request.session_id).unwrap(),
            aura_persistence_v2::model::step_snapshot(db, running.model_steps[0].model_step_id)
                .unwrap()
                .model_profile_id,
        )
    };
    let command = CommandInvocationId::new();
    let control = AgentModeControlRequest {
        session_id: request.session_id,
        expected_revision: episode.revision,
        action: AgentModeControlAction::Terminate,
    };
    services.agent_required().control_mode(command, control.clone()).await.unwrap();
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: request.session_id,
            model_profile_id,
            text: "A new ordinary request".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let next: AgentRunProjection = serde_json::from_value(started).unwrap();
    let next_request =
        AgentRunRequest { session_id: request.session_id, agent_run_id: next.run.agent_run_id };
    wait(&runtime, &next_request, |p| matches!(p.run.state, AgentRunState::ModelRunning { .. }))
        .await;
    services.agent_required().control_mode(command, control).await.unwrap();
    let store = services.broker.lock().unwrap();
    assert!(
        !aura_persistence_v2::agent::application::cancellation_requested(
            store.database.connection(),
            next_request.agent_run_id
        )
        .unwrap(),
        "retrying an acknowledged command cancelled an unrelated newer run"
    );
}
