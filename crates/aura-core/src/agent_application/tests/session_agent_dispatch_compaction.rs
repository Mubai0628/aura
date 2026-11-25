use super::*;

#[path = "session_agent_dispatch_compaction_diagnostics.rs"]
mod diagnostics;

#[tokio::test]
async fn session_agent_dispatch_compaction_keeps_the_original_source_readable() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, a, model) = Box::pin(prepare(&host, &provider)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = services
        .persistence
        .workspace_session_projection()
        .expect("projection")
        .sessions
        .into_iter()
        .find(|session| session.session_id == a)
        .expect("A")
        .workspace_id;
    for part in 0..3 {
        let text = (0..2200)
            .map(|i| format!("record-{part}-{i:016x} verified"))
            .collect::<Vec<_>>()
            .join("\n");
        Box::pin(turn(&runtime, a, model, &text)).await;
    }
    super::l10_19_compaction::resize(&runtime, model, 1_000_000, 4096).await;
    let prior_tokens = super::l10_19_compaction::used(&runtime, a, 2).await;
    let window = (prior_tokens + 20_000) * 100 / 95;
    super::l10_19_compaction::resize(&runtime, model, window, 4096).await;
    let started = crate::r03_tests::request(
        &runtime,
        "agent.compaction.start",
        json!(AgentCompactionStartRequest {
            computer_mode: None,
            session_id: a,
            model_profile_id: model,
            focus: Some("keep the acceptance condition".into()),
        }),
    )
    .await;
    let command: AgentRunProjection =
        serde_json::from_value(started["result"]["value"].clone()).expect("compaction task");
    let done = Box::pin(wait(
        &runtime,
        &AgentRunRequest { session_id: a, agent_run_id: command.run.agent_run_id },
        |projection| projection.turn.state.terminal(),
    ))
    .await;
    assert!(
        matches!(done.turn.state, TurnTaskState::Completed { .. }),
        "compaction ended {:?}",
        done.turn.state
    );
    assert!(
        provider
            .requests
            .lock()
            .expect("requests")
            .iter()
            .any(super::l10_19_compaction::asks_compaction),
        "the dev compaction really asked the model for a summary"
    );
    let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
    Box::pin(connect(&runtime, a, b)).await;
    let acceptance = "验收条件：必须保留原用户要求";
    let dispatch_text =
        format!("B4 user authorizes a summary for B. {} {acceptance}", "长文细节 ".repeat(4_000));
    Box::pin(turn(&runtime, a, model, &dispatch_text)).await;
    let receipt = tool_outcomes(&provider)
        .into_iter()
        .map(|value| value["outcome"]["payload"]["value"].clone())
        .find(|value| value["used"] == 1)
        .expect("delivery receipt");
    let delivery: EventId =
        serde_json::from_value(receipt["deliveries"][0]["delivery_id"].clone()).expect("id");
    let binding = services.persistence.session_model_binding_state(b).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":b,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    Box::pin(turn(&runtime, b, model, &format!("B5 discover {delivery}"))).await;
    let outcomes = tool_outcomes(&provider);
    let status = outcomes
        .iter()
        .map(|value| value["outcome"]["payload"]["value"].clone())
        .find(|value| value["deliveries"][0]["sources"].is_array())
        .expect("status source catalog");
    assert_eq!(status["deliveries"][0]["sources"].as_array().expect("source catalog").len(), 1);
    let read = outcomes
        .into_iter()
        .map(|value| value["outcome"]["payload"]["value"].clone())
        .rfind(|value| value["delivery_id"] == json!(delivery) && value["text"].is_string())
        .expect("B's source receipt");
    assert!(
        read["text"].as_str().is_some_and(|text| text.contains(acceptance)),
        "the last page still carries the original acceptance condition: {read}"
    );
}
