//! The speed mode a conversation chose goes with its turn, in the requests the model is sent,
//! when the model's settings allow speed modes and its catalog entry offers that mode; a turn
//! with none, a mode the entry does not offer and a model that does not allow speed modes go in
//! the standard mode. A message waiting in the queue keeps its mode for the turn it starts, a
//! turn Core continues after a restart keeps the cut-off turn's mode, and the session's model
//! choices list the modes a conversation can choose.
use super::*;
use provider_fixture::Scenario;
use std::time::Duration;

async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

/// The startup model, linked to a catalog entry that offers `fast` as `service_tier: priority`,
/// its settings allowing speed modes or not.
fn configure(host: &crate::r03_tests::Fixture, url: &str, allowed: bool) -> ModelProfileId {
    let model = super::activity_provider::configure_startup(
        host,
        url,
        ModelApiFormat::OpenAiChatCompletions,
    );
    let entry: aura_model_domain::ProviderCatalogModelEntry = serde_json::from_value(json!({
        "catalog_model_id": "fixture", "display_name": "Fixture", "family": null,
        "description": null, "aliases": [], "context_window_tokens": null,
        "input_limit_tokens": null, "output_limit_tokens": null, "input_modalities": ["text"],
        "output_modalities": ["text"], "capabilities": {"attachment": false, "reasoning": false,
        "tool_call": true, "structured_output": false, "temperature": true, "interleaved": null},
        "reasoning_options": [], "knowledge_cutoff": null, "release_date": null,
        "last_updated": null, "open_weights": null, "status": null, "price": null,
        "modes": {"fast": {"price": null, "body": {"service_tier": "priority"}}}
    }))
    .expect("a catalog entry");
    let link = aura_model_domain::ModelCatalogMetadata {
        catalog_provider_id: "openai".to_owned(),
        catalog_model_id: "fixture".to_owned(),
        catalog_revision: Revision::INITIAL,
        catalog_content_hash: aura_domain_common::ContentHash::blake3(b"catalog"),
        catalog_object_hash: aura_domain_common::ContentHash::blake3(b"fixture"),
        snapshot: entry,
        matched_at: Some(UtcTimestamp::now()),
        match_state: aura_model_domain::CatalogMatchState::Matched,
    };
    let path = host.root.path().join("config").join("models.json");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("models.json")).expect("JSON");
    let key = serde_json::to_value(model).expect("JSON");
    // The document's values sit beside its version and revision.
    let profile = &mut document["models"][key.as_str().expect("an ID")];
    assert!(profile.is_object(), "the startup model is in models.json");
    profile["catalog"] = serde_json::to_value(link).expect("JSON");
    profile["request_defaults"]["allow_speed_modes"] = json!(allowed);
    std::fs::write(&path, serde_json::to_vec(&document).expect("JSON")).expect("models.json");
    model
}

/// Core started, with a workspace and a conversation in it.
async fn open(host: &crate::r03_tests::Fixture) -> (crate::RuntimeState, SessionId) {
    let runtime = host.start().await;
    let root = host.root.path().join("speed-workspace");
    std::fs::create_dir_all(&root).expect("workspace folder");
    let created =
        call(&runtime, "workspace.create", json!({"selected_root": root, "display_name": "Speed"}))
            .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace, PermissionMode::WorkspaceReadWrite).await;
    (runtime, session)
}

async fn start(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    text: &str,
    mode: Option<&str>,
) -> AgentRunProjection {
    serde_json::from_value(
        call(
            runtime,
            "agent.start",
            json!(AgentStartRequest {
                computer_mode: None,
                session_id: session,
                model_profile_id: model,
                text: text.to_owned(),
                mode: AgentTaskMode::Normal,
                goal_budget: None,
                explicit_backend_id: None,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                speed_mode: mode.map(|word| TurnSpeedMode::parse(word).expect("a mode")),
                attachments: Vec::new(),
                references: Vec::new(),
            }),
        )
        .await,
    )
    .expect("run projection")
}

async fn finish(runtime: &crate::RuntimeState, run: &AgentRunProjection) {
    let request =
        AgentRunRequest { session_id: run.turn.session_id, agent_run_id: run.run.agent_run_id };
    waiting::wait(runtime, &request, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
}

/// The `service_tier` each request the model was sent named, oldest first.
fn sent(provider: &Provider) -> Vec<Value> {
    provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .map(|body| body.get("service_tier").cloned().unwrap_or(Value::Null))
        .collect()
}

async fn requests_reach(provider: &Provider, count: usize) {
    tokio::time::timeout(Duration::from_secs(45), async {
        while provider.requests.lock().expect("requests").len() < count {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the model requests arrive");
}

/// The speed modes the session's model choices offer for the model.
fn choices(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
) -> Vec<String> {
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
    option.speed_modes.clone()
}

#[tokio::test]
async fn a_conversations_mode_goes_with_its_turns_when_the_model_allows_it() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let model = configure(&host, &provider.url, true);
    let (runtime, session) = open(&host).await;
    assert_eq!(choices(&runtime, session, model), ["fast"]);

    let fast = start(&runtime, session, model, "AURA-L186-FAST", Some("fast")).await;
    finish(&runtime, &fast).await;
    let standard = start(&runtime, session, model, "AURA-L186-STANDARD", None).await;
    finish(&runtime, &standard).await;
    // The entry offers no `flex`: the turn goes in the standard mode.
    let unoffered = start(&runtime, session, model, "AURA-L186-FLEX", Some("flex")).await;
    finish(&runtime, &unoffered).await;

    // A message sent to the idle conversation starts its turn from the queue, with its mode.
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
            speed_mode: TurnSpeedMode::parse("fast"),
            ..AgentSubmitRequest::new(session, "AURA-L186-QUEUED", AgentInputLane::NextTurn)
        }),
    )
    .await;
    requests_reach(&provider, 4).await;

    let priority = json!("priority");
    assert_eq!(sent(&provider), [priority.clone(), Value::Null, Value::Null, priority]);
}

#[tokio::test]
async fn a_model_that_does_not_allow_speed_modes_goes_in_the_standard_mode() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let model = configure(&host, &provider.url, false);
    let (runtime, session) = open(&host).await;
    assert_eq!(choices(&runtime, session, model), Vec::<String>::new());

    let fast = start(&runtime, session, model, "AURA-L186-NOT-ALLOWED", Some("fast")).await;
    finish(&runtime, &fast).await;
    assert_eq!(sent(&provider), [Value::Null]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_continued_after_a_restart_keeps_the_cut_off_turns_mode() {
    let host = Arc::new(crate::r03_tests::Fixture::new());
    let provider = Arc::new(Provider::scenario(Scenario::RecoveredSession).await);
    let model = configure(&host, &provider.url, true);
    let (first, held) = (host.clone(), provider.clone());
    let cut = recovery::auto_continue::crashed(move || {
        Box::pin(async move {
            let (runtime, session) = open(&first).await;
            let run = start(&runtime, session, model, "AURA-L186-CUT", Some("fast")).await;
            requests_reach(&held, 1).await;
            run.turn.turn_task_id
        })
    })
    .await;

    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let decisions =
        services.agent_required().continue_after_restart(CoreRestartCause::Unresponsive).await;
    assert_eq!(
        decisions,
        vec![(cut, aura_persistence_v2::agent::restart_continuation::Decision::Continue)]
    );
    requests_reach(&provider, 2).await;
    assert_eq!(sent(&provider), [json!("priority"), json!("priority")]);
}
