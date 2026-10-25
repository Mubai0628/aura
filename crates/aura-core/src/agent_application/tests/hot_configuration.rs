use super::*;

/// Only the remote model is a fixture. This intentionally uses the production
/// models.json setter and is not runnable while host AtomicReplace is denied.
#[tokio::test]
async fn query_resume_uses_new_model_and_budget_without_mutating_old_snapshots() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::Query).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let paused = waiting::wait(&runtime, &request, |view| {
        matches!(view.turn.state, TurnTaskState::AwaitingQuery { .. })
    })
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required().idle(request.agent_run_id).await.expect("safe Query boundary");
    let old = {
        let store = services.broker.lock().expect("store");
        paused
            .model_steps
            .iter()
            .map(|s| {
                aura_persistence_v2::model::step_snapshot(
                    store.database.connection(),
                    s.model_step_id,
                )
                .expect("old snapshot")
            })
            .collect::<Vec<_>>()
    };
    let settings =
        crate::r03_tests::success(&runtime, "models.settings.get_snapshot", json!({})).await;
    let model = crate::r03_tests::success(&runtime, "models.profile.create_manual", json!({
        "expected_revision":settings["models_revision"],
        "input":{"endpoint_id":old[0].endpoint_id,"provider_model_name":"fixture-hot","display_name":"Hot model","notes":null,"context_window_tokens":128000}
    })).await;
    let configured = crate::r03_tests::success(&runtime, "models.profile.update", json!({
        "expected_revision":model["revision"],"model_profile_id":model["value"]["id"],"display_name":"Hot model","notes":null,"enabled":true,
        "request_defaults":{"max_output_tokens":2048,"temperature":null,"top_p":null,"chat_output_token_field":null,"chat_instruction_role":null},"historical_snapshot_max_tokens":1024
    })).await;
    let profile: ModelProfileId =
        serde_json::from_value(configured["value"]["id"].clone()).expect("new profile");
    let binding =
        services.persistence.session_model_binding_state(request.session_id).expect("binding");
    crate::r03_tests::success(&runtime, "models.session.set_binding", json!({
        "expected_revision":binding.revision,"session_id":request.session_id,"model_profile_id":profile
    })).await;
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        3,
        "binding change is not a Query answer"
    );
    let query = paused.pending_query.expect("formal Query");
    crate::r03_tests::success(
        &runtime,
        "agent.query.answer",
        json!(AgentQueryAnswerRequest {
            session_id: request.session_id,
            agent_run_id: request.agent_run_id,
            query_id: query.query_id,
            expected_revision: query.revision,
            answers: vec![QueryAnswer {
                question_id: "choice".into(),
                value: QueryAnswerValue::Confirm { accepted: true }
            }],
        }),
    )
    .await;
    let done = waiting::wait(&runtime, &request, |view| {
        matches!(view.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    let store = services.broker.lock().expect("store");
    for before in &old {
        assert_eq!(
            aura_persistence_v2::model::step_snapshot(
                store.database.connection(),
                before.model_step_id
            )
            .expect("unchanged historical snapshot"),
            *before
        );
    }
    assert!(done.model_steps.len() > old.len());
    for step in done.model_steps.iter().skip(old.len()) {
        let snapshot = aura_persistence_v2::model::step_snapshot(
            store.database.connection(),
            step.model_step_id,
        )
        .expect("new snapshot");
        assert_eq!(snapshot.model_profile_id, profile);
    }
    let requests = provider.requests.lock().expect("actual compiled requests");
    for body in &requests[3..] {
        assert_eq!(body["model"], "fixture-hot");
        assert_eq!(body["max_tokens"], 2048);
        assert!(
            body["messages"].as_array().expect("full history").iter().any(|m| m["role"] == "tool")
        );
    }
}

#[tokio::test]
async fn clearing_a_previously_set_binding_never_reuses_the_start_profile() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(provider_fixture::Scenario::Query).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let paused = waiting::wait(&runtime, &request, |view| {
        matches!(view.turn.state, TurnTaskState::AwaitingQuery { .. })
    })
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let profile = {
        let journal = services.broker.lock().expect("store");
        aura_persistence_v2::model::step_snapshot(
            journal.database.connection(),
            paused.model_steps[0].model_step_id,
        )
        .expect("actual model")
        .model_profile_id
    };
    let binding = services
        .persistence
        .session_model_binding_state(request.session_id)
        .expect("never-bound state");
    assert_eq!(binding.revision.value(), 0);
    assert!(binding.binding.is_none());
    let bound = crate::r03_tests::success(&runtime, "models.session.set_binding", json!({
        "session_id":request.session_id,"model_profile_id":profile,"expected_revision":binding.revision
    })).await;
    crate::r03_tests::success(&runtime, "models.session.set_binding", json!({
        "session_id":request.session_id,"model_profile_id":null,"expected_revision":bound["revision"]
    })).await;
    let before = provider.requests.lock().expect("requests").len();
    let query = paused.pending_query.expect("formal question");
    crate::r03_tests::success(
        &runtime,
        "agent.query.answer",
        json!(AgentQueryAnswerRequest {
            session_id: request.session_id,
            agent_run_id: request.agent_run_id,
            query_id: query.query_id,
            expected_revision: query.revision,
            answers: vec![QueryAnswer {
                question_id: "choice".into(),
                value: QueryAnswerValue::Confirm { accepted: true }
            }],
        }),
    )
    .await;
    let done = waiting::wait(&runtime, &request, |view| view.turn.state.terminal()).await;
    assert!(matches!(done.turn.state, TurnTaskState::RecoveryRequired { .. }));
    assert_eq!(provider.requests.lock().expect("no fallback request").len(), before);
    let journal = services.broker.lock().expect("store");
    let reason: String = journal.database.connection().query_row(
        "SELECT json_extract(payload,'$.error') FROM events WHERE owner=?1 AND kind='agent.application_error' ORDER BY sequence DESC LIMIT 1",
        [request.agent_run_id.to_string()], |row| row.get(0),
    ).expect("interruption reason");
    assert_eq!(reason, "agent.model_binding_required");
}
