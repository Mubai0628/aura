use super::*;
#[path = "l05_maintenance.rs"]
mod l05_maintenance;

#[tokio::test]
async fn configured_provider_real_core_registry_file_loop_respects_all_three_modes() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        run(mode, true).await;
    }
}

#[tokio::test]
async fn text_response_uses_core_continuation_and_real_completion_group() {
    run(PermissionMode::ReadOnly, false).await;
}

/// The C1 reminder is model-visible protocol text: it must state the route that
/// can really end the turn in the current effective mode.
#[test]
fn the_protocol_reminder_states_the_route_of_the_effective_mode() {
    let normal = super::context::protocol_completion_reminder(AgentTaskMode::Normal, false);
    let text = normal["instruction"].as_str().expect("reminder text");
    assert!(text.contains("a complete normal answer ends the turn"), "{text}");
    assert!(!text.contains("normal.complete with the user-visible answer"), "{text}");

    for mode in [AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let reminder = super::context::protocol_completion_reminder(mode, false);
        let text = reminder["instruction"].as_str().expect("reminder text");
        assert!(text.contains("stays active across turns"), "{mode:?}: {text}");
        assert!(text.contains("normal.complete with the user-visible answer"), "{mode:?}: {text}");
        assert!(!text.contains("a complete normal answer ends the turn"), "{mode:?}: {text}");
    }
    let plan_tool = super::context::protocol_completion_reminder(AgentTaskMode::Plan, false);
    assert!(
        plan_tool["instruction"]
            .as_str()
            .expect("reminder text")
            .contains("plan.complete only if the entire Plan mode is finished")
    );
    let goal_tool = super::context::protocol_completion_reminder(AgentTaskMode::Goal, false);
    assert!(
        goal_tool["instruction"]
            .as_str()
            .expect("reminder text")
            .contains("goal.complete only if the entire Goal mode is finished")
    );

    let disposition = super::context::protocol_completion_reminder(AgentTaskMode::Goal, true);
    let text = disposition["instruction"].as_str().expect("reminder text");
    assert!(text.contains("mode.disposition"), "{text}");
    assert!(text.contains("skip marks it skipped and answers as Normal"), "{text}");
}

#[tokio::test]
async fn a_normal_greeting_completes_from_its_own_final_response() {
    let greeting = greeting(provider_fixture::Scenario::Text).await;
    let TurnTaskState::Completed { final_report } = &greeting.state else {
        panic!("greeting must complete: {:?}", greeting.state);
    };
    assert_eq!(greeting.requests, 1, "a final answer needs no extra completion request");
    assert_eq!(greeting.explicit_completions, 0, "no fabricated normal.complete call");
    assert!(!greeting.reminders_sent.iter().any(|sent| *sent), "no completion reminder");
    let record = greeting.natural.first().expect("one natural completion record");
    assert_eq!(record.basis, aura_agent_domain::NaturalCompletionBasis::EndTurn);
    assert_eq!(record.final_report, *final_report, "the answer is persisted once, verbatim");
    assert_eq!(
        greeting.final_text_source,
        Some(aura_agent_domain::AgentFinalTextSource::AssistantStep {
            model_step_id: record.model_step_id
        }),
        "the projection attributes the final text to the answering ModelStep"
    );
    // The frozen request the model really received must carry one completion route:
    // this is the composed prompt, not a single resource file.
    let request = greeting.first_request_body.as_deref().expect("one real model request");
    assert!(
        request.contains(
            "a complete end of your response that carries the user-visible answer finishes the turn"
        ),
        "the Normal request states that its own answer ends the turn"
    );
    assert!(
        !request.contains("submit your final answer through normal.complete"),
        "the Normal request must not require the completion Tool"
    );
    assert!(
        !request.contains("the next user message continues Goal"),
        "no request may promise that the next message resumes Goal"
    );
}

#[tokio::test]
async fn an_answer_that_never_arrived_keeps_the_protocol_protection() {
    // Reasoning is not an answer: the turn is reminded once, then completes only
    // through the model's explicit decision.
    let greeting = greeting(provider_fixture::Scenario::ThinkingOnly).await;
    assert!(greeting.natural.is_empty(), "reasoning alone never completes a turn");
    assert_eq!(
        greeting.requests,
        2,
        "Core reminds once instead of completing: state={:?} natural={:?} reminders={:?} diagnostics={:?}",
        greeting.state,
        greeting.natural.len(),
        greeting.reminders_sent,
        greeting.diagnostics
    );
    assert!(greeting.reminders_sent.iter().any(|sent| *sent), "one reminder is recorded");
    assert!(
        greeting
            .second_request_body
            .as_deref()
            .is_some_and(|body| body.contains("ended without a usable final answer")),
        "the reminder states the real reason: {:?}",
        greeting.second_request_body
    );
    assert!(
        greeting
            .second_request_body
            .as_deref()
            .is_some_and(|body| !body.contains("reached the output-token limit")),
        "a response that ended on its own was not cut off"
    );
    assert!(matches!(greeting.state, TurnTaskState::Completed { .. }));
    assert_eq!(greeting.explicit_completions, 1, "the reminder answer completes explicitly");
}

/// Reasoning cut off by the output limit is not a model that chose to stop. The reminder
/// names the limit and asks for shorter reasoning; the generic one alone let a thinking model
/// reason to the same limit again and again.
#[tokio::test]
async fn reasoning_cut_off_at_the_output_limit_is_named_in_the_reminder() {
    let greeting = greeting(provider_fixture::Scenario::ThinkingCutOff).await;
    assert!(greeting.natural.is_empty(), "cut-off reasoning never completes a turn");
    assert_eq!(greeting.requests, 2, "{:?}", greeting.diagnostics);
    let second = greeting.second_request_body.as_deref().expect("the reminder request");
    assert!(
        second.contains("reached the output-token limit before it finished"),
        "the reminder states why the response was empty: {second}"
    );
    assert!(second.contains("ended without a usable final answer"), "{second}");
    assert!(matches!(greeting.state, TurnTaskState::Completed { .. }));
}

#[tokio::test]
async fn an_empty_response_is_a_provider_failure_not_a_completion() {
    let greeting = greeting(provider_fixture::Scenario::EmptyText).await;
    assert!(greeting.natural.is_empty(), "an empty response is never an answer");
    assert_eq!(greeting.requests, 1, "an empty output is not retried as a completion");
    assert!(
        !matches!(greeting.state, TurnTaskState::Completed { .. }),
        "an empty Provider response must not complete the turn: {:?}",
        greeting.state
    );
}

#[tokio::test]
async fn a_truncated_answer_never_completes_the_turn() {
    let greeting = greeting(provider_fixture::Scenario::TruncatedText).await;
    assert!(greeting.natural.is_empty(), "a truncated answer is not a final answer");
    assert_eq!(greeting.requests, 2, "truncation is not silently accepted");
    assert!(matches!(greeting.state, TurnTaskState::Completed { .. }));
}

/// A Provider can stream a tool argument fragment that is not a complete JSON
/// value. The call must then fail as an invalid Tool input - not turn the whole
/// conversation into history that no later step can read.
#[tokio::test]
async fn unparseable_provider_tool_arguments_do_not_poison_the_conversation() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::MalformedArguments).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("malformed-arguments-workspace");
    std::fs::create_dir(&root).expect("malformed arguments workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Malformed provider arguments"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace projection");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "Ask the provider for a malformed tool call, then finish.".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection =
        serde_json::from_value(started).expect("started run projection");
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    let result = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let value = crate::r03_tests::success(&runtime, "agent.status", json!(request)).await;
            let projection: AgentRunProjection =
                serde_json::from_value(value).expect("run projection");
            if projection.turn.state.terminal() {
                break projection;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("a malformed provider call must still reach a terminal state");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable")
    };
    let store = services.broker.lock().expect("V2 store");
    let db = store.database.connection();
    let diagnostics = {
        let mut q = db
            .prepare(
                "SELECT payload FROM events WHERE kind IN('agent.application_error','model.failure')",
            )
            .expect("diagnostics");
        q.query_map([], |row| row.get::<_, String>(0))
            .expect("diagnostics")
            .collect::<Result<Vec<_>, _>>()
            .expect("diagnostics")
    };
    assert!(
        matches!(result.turn.state, TurnTaskState::Completed { .. }),
        "{:?}: {diagnostics:?}",
        result.turn.state
    );
    // The next model step could read this history at all: every call is one
    // complete JSON value, and the Provider's own bytes stay in the response
    // artifact rather than in the conversation.
    let history = aura_persistence_v2::agent::model_context::history_for_model(
        db,
        result.run.agent_run_id,
        false,
    )
    .expect("malformed provider arguments must not poison history");
    let mut marked = 0;
    for item in &history.items {
        if let LogicalConversationItem::Assistant { tool_calls, .. } = item {
            for call in tool_calls {
                serde_json::from_str::<serde_json::Value>(&call.arguments_json)
                    .unwrap_or_else(|error| panic!("history kept unparseable arguments: {error}"));
                if call.arguments_json.contains("_aura_invalid_tool_arguments") {
                    marked += 1;
                }
            }
        }
    }
    assert_eq!(marked, 1, "the unparseable call is marked exactly once");
    // The model is told why, so a correct retry stays possible in the same turn.
    let requests = provider.requests.lock().expect("requests");
    assert_eq!(requests.len(), 2, "the step after the malformed call ran");
    assert!(requests[1]["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|m| {
            m["role"] == "tool"
                && m["content"].as_str().is_some_and(|content| content.contains("invalid"))
        })
    }));
}
