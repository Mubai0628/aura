//! A step's details carry what the whole conversation's finished requests sent and what the
//! Provider read from its cache, beside the step's own figures.
#![allow(clippy::unwrap_used)]
use super::*;
use aura_agent_domain::AgentStartRequest;
use aura_domain_common::CommandInvocationId;

async fn turn(runtime: &crate::RuntimeState, session: SessionId, model: ModelProfileId) {
    let agent = match &runtime.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required(),
        _ => panic!("ready"),
    };
    let request: AgentStartRequest = serde_json::from_value(json!({
        "session_id": session, "model_profile_id": model, "mode": "normal", "goal_budget": null,
        "text": "Say something short."}))
    .unwrap();
    let started = agent.start(CommandInvocationId::new(), request).await.unwrap();
    wait(
        runtime,
        &AgentRunRequest {
            session_id: started.turn.session_id,
            agent_run_id: started.run.agent_run_id,
        },
        |view| view.turn.state.terminal(),
    )
    .await;
}

async fn latest_step(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    let history = crate::r03_tests::success(
        runtime,
        "agent.history",
        json!({"session_id":session,"before_turn_task_id":null,"within_turn":null,"limit":10}),
    )
    .await;
    history["turns"].as_array().unwrap().last().unwrap()["assistant_messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["model_step_id"]
        .clone()
}

#[tokio::test]
async fn a_steps_details_carry_the_whole_conversations_cache_use() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CachedText).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    turn(&runtime, session, model).await;
    turn(&runtime, session, model).await;
    let step = latest_step(&runtime, session).await;
    let details = crate::r03_tests::success(
        &runtime,
        "agent.inspector",
        json!({"session_id": session, "model_step_id": step}),
    )
    .await;
    assert_eq!(details["projection"]["cache_usage"]["actual_input_tokens"], 1000);
    assert_eq!(details["projection"]["cache_usage"]["actual_cached_input_tokens"], 600);
    assert_eq!(
        details["session_cache"],
        json!({"steps": 2, "reported_steps": 2, "input_tokens": 2000, "cached_input_tokens": 1200}),
        "{details}"
    );
}
