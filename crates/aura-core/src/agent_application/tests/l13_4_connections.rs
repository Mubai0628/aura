//! A provider's default connection takes its models' requests;
//! a request that fails before any answer goes at once to the next backup, and the step says so.
#![allow(clippy::unwrap_used)]
use super::*;
use aura_agent_domain::AgentTaskMode;
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelDiscoveryMode, ModelEndpointCreateRequest, ModelEndpointTimeouts,
};

/// A second connection of the model's own provider, at `url`.
async fn second_connection(
    runtime: &crate::RuntimeState,
    model: ModelProfileId,
    url: &str,
) -> (Value, Value) {
    let success = crate::r03_tests::success;
    let snapshot = success(runtime, "models.settings.get_snapshot", json!({})).await;
    let registry = &snapshot["registry"];
    let endpoint = registry["models"][model.to_string()]["endpoint_id"].clone();
    let provider = registry["endpoints"][endpoint.as_str().unwrap()]["provider_id"].clone();
    let created = success(
        runtime,
        "models.endpoint.create",
        json!(ModelEndpointCreateRequest {
            expected_revision: serde_json::from_value(snapshot["models_revision"].clone()).unwrap(),
            provider_id: serde_json::from_value(provider.clone()).unwrap(),
            display_name: "Backup connection".into(),
            api_format: ModelApiFormat::OpenAiChatCompletions,
            base_url: url.into(),
            discovery_mode: ModelDiscoveryMode::Disabled,
            chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
            chat_instruction_role: Some(ChatInstructionRole::System),
            auth_scheme: EndpointAuthScheme::None,
            openai_organization_id: None,
            openai_project_id: None,
            anthropic_workspace_id: None,
            timeouts: ModelEndpointTimeouts {
                connect_ms: 5000,
                first_event_ms: 10000,
                stream_idle_ms: 10000,
                total_ms: Some(30000),
            },
            max_parallel_model_requests: 2,
        }),
    )
    .await;
    (provider, created["value"]["id"].clone())
}

async fn set_connections(
    runtime: &crate::RuntimeState,
    provider: &Value,
    default: Value,
    backups: Value,
) {
    let success = crate::r03_tests::success;
    let snapshot = success(runtime, "models.settings.get_snapshot", json!({})).await;
    success(
        runtime,
        "models.provider.set_connections",
        json!({"expected_revision": snapshot["models_revision"], "provider_id": provider,
            "default_endpoint_id": default, "backup_endpoint_ids": backups}),
    )
    .await;
}

fn completed(view: &AgentRunProjection) -> bool {
    view.turn.state.terminal()
}

async fn assistant(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    let history = crate::r03_tests::success(
        runtime,
        "agent.history",
        json!({"session_id":session,"before_turn_task_id":null,"within_turn":null,"limit":10}),
    )
    .await;
    history["turns"][0]["assistant_messages"].clone()
}

#[tokio::test]
async fn a_request_that_fails_before_answering_goes_to_the_backup_and_the_step_says_so() {
    let host = crate::r03_tests::Fixture::new();
    let failing = Provider::scenario(Scenario::ProviderUnavailable).await;
    let working = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = waiting::prepare(&host, &failing).await;
    // A second connection is a
    // backup without being made one.
    second_connection(&runtime, model, &working.url).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let view = waiting::wait(&runtime, &request, completed).await;
    assert!(
        matches!(view.turn.state, aura_agent_domain::TurnTaskState::Completed { .. }),
        "{:?}",
        view.turn.state
    );
    assert_eq!(failing.requests.lock().unwrap().len(), 1, "no wait, no second try on it");
    assert_eq!(working.requests.lock().unwrap().len(), 1);
    let messages = assistant(&runtime, session).await;
    let last = messages.as_array().unwrap().last().unwrap();
    assert_eq!(last["backup_connection"], "Backup connection", "{messages}");
}

#[tokio::test]
async fn the_default_connection_takes_the_requests_of_the_providers_models() {
    let host = crate::r03_tests::Fixture::new();
    let own = Provider::scenario(Scenario::Text).await;
    let chosen = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = waiting::prepare(&host, &own).await;
    let (provider, other) = second_connection(&runtime, model, &chosen.url).await;
    set_connections(&runtime, &provider, other, json!([])).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    waiting::wait(&runtime, &request, completed).await;
    assert_eq!(own.requests.lock().unwrap().len(), 0);
    assert_eq!(chosen.requests.lock().unwrap().len(), 1);
    let messages = assistant(&runtime, session).await;
    assert!(messages.as_array().unwrap().iter().all(|m| m.get("backup_connection").is_none()));
}
