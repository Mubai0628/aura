//! DeepSeek patched a file it had just written with a
//! hash copied from the write result's envelope - the registry snapshot's or the result
//! artifact's, the only hashes it showed - and the patch was refused twice. A tool result now
//! shows the model none of the envelope's own hashes and identities.
use super::*;

/// The write's result as the request after it shows it to the model.
fn write_result(requests: &[Value]) -> String {
    requests
        .iter()
        .flat_map(|request| request["messages"].as_array().cloned().unwrap_or_default())
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str().map(str::to_owned))
        .find(|content| content.contains("affected_paths"))
        .expect("the write's result reached the next request")
}

/// The model sees none of them, and the file's own hash after the write instead.
#[tokio::test]
async fn a_write_result_shows_the_model_its_files_hash_and_none_of_the_envelopes() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Write).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &request, done).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let result = write_result(&requests);
    for noise in ["registry_snapshot_ref", "content_hash", "completed_at", "schema_version"] {
        assert!(!result.contains(noise), "{noise} is not shown to the model: {result}");
    }
    assert!(result.contains("\"outcome\""), "the outcome stays: {result}");
    let written =
        std::fs::read(host.root.path().join("waiting-workspace").join("agent-result.txt"))
            .expect("the written file");
    let result: Value = serde_json::from_str(&result).expect("result json");
    assert_eq!(
        result["outcome"]["payload"]["value"]["logical_hash"],
        serde_json::to_value(aura_domain_common::ContentHash::blake3(&written)).expect("hash"),
        "{result}"
    );
}

/// A step the provider failed for a passing
/// reason is asked again as a new step, twice at most in a row; the third failure stops the turn
/// and names its cause.
#[tokio::test]
async fn a_passing_provider_failure_is_asked_again_twice_at_most() {
    for (failures, completes) in [(2, true), (3, false)] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(Scenario::OverloadedThenText { failures }).await;
        let (runtime, session, model) = waiting::prepare(&host, &provider).await;
        let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
        let request =
            AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
        let ended = |v: &AgentRunProjection| {
            matches!(
                v.turn.state,
                TurnTaskState::Completed { .. } | TurnTaskState::RecoveryRequired { .. }
            )
        };
        let view = waiting::wait(&runtime, &request, ended).await;
        let asked = provider.requests.lock().expect("provider requests").len();
        if completes {
            assert!(
                matches!(view.turn.state, TurnTaskState::Completed { .. }),
                "{:?}",
                view.turn.state
            );
            assert_eq!(asked, 3, "two failed steps and the answer");
        } else {
            assert!(
                matches!(view.turn.state, TurnTaskState::RecoveryRequired { .. }),
                "{:?}",
                view.turn.state
            );
            assert_eq!(asked, 3, "the first step and two retries, then the turn stops");
        }
    }
}

/// The turn's first step sends its message the way the second
/// step does, so everything before the first request's task state starts the second request.
#[tokio::test]
async fn the_first_step_sends_its_message_the_way_the_second_does() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Write).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &request, done).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let messages = |request: &Value| request["messages"].as_array().cloned().unwrap_or_default();
    let (first, second) = (messages(&requests[0]), messages(&requests[1]));
    let shared = first.len() - 1;
    assert_eq!(second[..shared], first[..shared], "the second request starts with the first's");
}

/// Once a call has run, its long text is shown as a line
/// naming its size, from the very next request on.
#[tokio::test]
async fn a_written_texts_length_is_named_and_not_repeated() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::LongWrite).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &request, done).await;
    let written = host.root.path().join("waiting-workspace").join("agent-result.txt");
    assert_eq!(std::fs::read_to_string(written).expect("the file").len(), 40_000, "the full text");
    let requests = provider.requests.lock().expect("provider requests").clone();
    let after = requests.last().expect("the request after the write").to_string();
    assert!(after.contains("Long argument not repeated: 40000 characters"), "the line is sent");
    assert!(!after.contains(&"x".repeat(40_000)), "the text is not sent again");
}

/// With a connection chosen, web.search is planned for its origin,
/// sent there as DeepSeek's search, and answers in the tool's own shape.
#[tokio::test]
async fn web_search_goes_to_the_chosen_deepseek_connection() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::DeepSeekWebSearch).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let agent = services.agent_required();
    let settings = agent.tool_settings().expect("view");
    let endpoint = {
        let config = services.p05().expect("p05").model_config.snapshot().expect("config");
        config.registry.models.get(&model).expect("profile").endpoint_id
    };
    // The fixture's loopback connection stands in for DeepSeek's API here; one that is not a
    // DeepSeek connection with a key is refused, never saved.
    assert!(settings.web_service_connections.iter().any(|c| c.endpoint_id == endpoint));
    let choose = |endpoint_id| aura_tool_contract::AgentToolSettingsUpdate {
        expected_revision: settings.revision,
        fixed_tool_ids: settings.fixed_tool_ids.clone(),
        max_dynamic_tool_schemas: settings.max_dynamic_tool_schemas,
        max_concurrent_tool_calls: settings.max_concurrent_tool_calls,
        default_timeout_ms: settings.default_timeout_ms,
        max_retries: settings.max_retries,
        use_recommended_fixed_tool_ids: true,
        network_proxy: None,
        web_service: Some(aura_tool_contract::WebServiceSettingV1::DeepSeek { endpoint_id }),
    };
    let refused = agent
        .update_tool_settings(
            CommandInvocationId::new(),
            choose(aura_domain_common::EndpointId::new()),
        )
        .await
        .expect_err("not a usable connection");
    assert!(refused.to_string().contains("tools.web_service_connection_unusable"), "{refused}");
    let saved = agent
        .update_tool_settings(CommandInvocationId::new(), choose(endpoint))
        .await
        .expect("saved");
    assert_eq!(
        saved.web_service,
        aura_tool_contract::WebServiceSettingV1::DeepSeek { endpoint_id: endpoint }
    );
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &request, done).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let searched = requests
        .iter()
        .find(|body| body.get("query").is_some())
        .expect("DeepSeek's search was asked");
    assert_eq!(searched["query"], "aura fixture query");
    assert_eq!(searched["count"], 3);
    let result = requests
        .iter()
        .flat_map(|body| body["messages"].as_array().cloned().unwrap_or_default())
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str().map(str::to_owned))
        .find(|content| content.contains("deepseek-search-v1"))
        .expect("the result reached the model");
    assert!(result.contains("A longer summary the model reads."), "{result}");
}

/// The call is
/// rejected as a page that has no content, and the model reads Core's sentence, not a bare code.
#[tokio::test]
async fn a_page_deepseek_cannot_fetch_is_a_rejection_the_model_reads() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::DeepSeekWebFetchMissing).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let agent = services.agent_required();
    let settings = agent.tool_settings().expect("view");
    let endpoint = {
        let config = services.p05().expect("p05").model_config.snapshot().expect("config");
        config.registry.models.get(&model).expect("profile").endpoint_id
    };
    agent
        .update_tool_settings(
            CommandInvocationId::new(),
            aura_tool_contract::AgentToolSettingsUpdate {
                expected_revision: settings.revision,
                fixed_tool_ids: settings.fixed_tool_ids.clone(),
                max_dynamic_tool_schemas: settings.max_dynamic_tool_schemas,
                max_concurrent_tool_calls: settings.max_concurrent_tool_calls,
                default_timeout_ms: settings.default_timeout_ms,
                max_retries: settings.max_retries,
                use_recommended_fixed_tool_ids: true,
                network_proxy: None,
                web_service: Some(aura_tool_contract::WebServiceSettingV1::DeepSeek {
                    endpoint_id: endpoint,
                }),
            },
        )
        .await
        .expect("saved");
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &request, done).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let result: Value = requests
        .iter()
        .flat_map(|body| body["messages"].as_array().cloned().unwrap_or_default())
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str().map(str::to_owned))
        .find_map(|content| serde_json::from_str(&content).ok())
        .expect("the fetch's result reached the model");
    let outcome = &result["outcome"];
    assert_eq!(outcome["type"], "operation_rejected", "{result}");
    assert_eq!(outcome["error"]["code"], "web.deepseek_fetch_empty", "{result}");
    assert_eq!(outcome["error"]["category"], "not_found", "{result}");
    let message = outcome["error"]["message"].as_str().expect("message");
    assert!(message.contains("没能从这个网址取到") && message.contains("HTTP 404"), "{message}");
    assert!(!message.contains("page not found"), "DeepSeek's own detail stays in the receipt");
}
