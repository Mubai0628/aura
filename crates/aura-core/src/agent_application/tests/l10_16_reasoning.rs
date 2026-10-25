//! The thinking level a conversation chose
//! goes with its turn, in every request the model is sent; a turn with no level takes the model's
//! default (none here); a level the model does not offer is not sent. A message waiting in the
//! queue keeps its level for the turn it starts.
use super::*;

async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

async fn start(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    text: &str,
    level: Option<TurnReasoningEffort>,
) -> AgentRunRequest {
    let started: AgentRunProjection = serde_json::from_value(
        call(
            runtime,
            "agent.start",
            json!(AgentStartRequest {
                session_id: session,
                model_profile_id: model,
                text: text.to_owned(),
                mode: AgentTaskMode::Normal,
                goal_budget: None,
                explicit_backend_id: None,
                skill_ids: Vec::new(),
                reasoning_effort: level,
                attachments: Vec::new(),
            }),
        )
        .await,
    )
    .expect("run projection");
    AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id }
}

async fn finish(runtime: &crate::RuntimeState, run: &AgentRunRequest) {
    waiting::wait(runtime, run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. })).await;
}

/// The level each request the model was sent named, oldest first.
fn sent(provider: &Provider) -> Vec<Value> {
    provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .map(|body| body.get("reasoning_effort").cloned().unwrap_or(Value::Null))
        .collect()
}

#[tokio::test]
async fn a_conversations_level_goes_with_its_turns_and_a_turn_without_one_sends_none() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;

    let chosen =
        start(&runtime, session, model, "AURA-L1016-HIGH", Some(TurnReasoningEffort::High)).await;
    finish(&runtime, &chosen).await;
    let plain = start(&runtime, session, model, "AURA-L1016-NONE", None).await;
    finish(&runtime, &plain).await;
    // The fixture model's format offers low, medium and high: `max` is not sent.
    let unoffered =
        start(&runtime, session, model, "AURA-L1016-MAX", Some(TurnReasoningEffort::Max)).await;
    finish(&runtime, &unoffered).await;

    // A message sent to the idle conversation starts its turn from the queue, with its level.
    call(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    call(
        &runtime,
        "agent.submit",
        json!(AgentSubmitRequest {
            reasoning_effort: Some(TurnReasoningEffort::Medium),
            ..AgentSubmitRequest::new(session, "AURA-L1016-QUEUED", AgentInputLane::NextTurn)
        }),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(45), async {
        while provider.requests.lock().expect("requests").len() < 4 {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the queued message started a turn");

    assert_eq!(sent(&provider), [json!("high"), Value::Null, Value::Null, json!("medium")]);
}

/// A model the catalog does not know takes the levels its user ticked in its settings
///: the composer's choices list them, and 最高 goes out as `max`.
#[tokio::test]
async fn the_levels_a_profile_lists_are_the_composers_choices_and_go_out() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let revision =
        call(&runtime, "models.settings.get_snapshot", json!({})).await["models_revision"].clone();
    call(
        &runtime,
        "models.profile.update",
        json!({"expected_revision":revision,"model_profile_id":model,"display_name":"Fixture",
            "notes":null,"enabled":true,"request_defaults":{"max_output_tokens":4096,
            "temperature":null,"top_p":null,"chat_output_token_field":null,
            "chat_instruction_role":null,"reasoning_efforts":["low","max","high"]},
            "historical_snapshot_max_tokens":1024}),
    )
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let change = services.p05().expect("P05").session_selector(session).expect("selector");
    let aura_projection_contract::ProjectionChange::SessionModelSelector {
        payload: selector, ..
    } = change
    else {
        panic!("the session's model selector")
    };
    let option = selector
        .models
        .iter()
        .find(|option| option.model_profile_id == model)
        .expect("the model is a choice");
    use aura_model_domain::ReasoningEffort::{High, Low, Max};
    assert_eq!(option.reasoning_efforts, [Low, High, Max]);

    let top =
        start(&runtime, session, model, "AURA-L1016-TOP", Some(TurnReasoningEffort::Max)).await;
    finish(&runtime, &top).await;
    assert_eq!(sent(&provider), [json!("max")]);
}
