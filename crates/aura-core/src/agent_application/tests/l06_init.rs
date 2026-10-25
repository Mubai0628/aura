//! `/init` through the real model loop: a one-off task that runs as Normal, is the only
//! turn that sees its write tool and prompt, and leaves an active Plan exactly as it was.
use super::*;
use aura_projection_contract::AgentSessionProjection;
use provider_fixture::{INIT_COMMAND, INIT_EVIDENCE_FILE, Scenario};
use waiting::{begin, prepare_in_mode, wait};

async fn start_init(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    note: &str,
) -> AgentRunProjection {
    let value = crate::r03_tests::success(
        runtime,
        "agent.init.start",
        json!(AgentInitStartRequest {
            session_id: session,
            model_profile_id: model,
            text: note.into()
        }),
    )
    .await;
    serde_json::from_value(value).expect("the init run projection")
}

async fn history(runtime: &crate::RuntimeState, session: SessionId) -> AgentSessionProjection {
    serde_json::from_value(
        crate::r03_tests::success(
            runtime,
            "agent.history",
            json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
        )
        .await,
    )
    .expect("history")
}

async fn idle(runtime: &crate::RuntimeState, run: AgentRunId) {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required().idle(run).await.expect("completion boundary");
}

fn offers_init_tool(request: &Value) -> bool {
    let tool = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("workspace_init.complete")
        .expect("published")
        .definition
        .tool_id;
    let wire = aura_model_domain::provider_wire_tool_name("workspace_init.complete", tool);
    request["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|t| t["function"]["name"] == wire))
}

#[tokio::test]
async fn init_writes_a_verified_agents_file_through_the_real_model_loop() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Init).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let root = host.root.path().join("waiting-workspace");
    std::fs::write(
        root.join(INIT_EVIDENCE_FILE),
        format!("# Project\n\nRun `{INIT_COMMAND}` before every commit.\n"),
    )
    .expect("the repository defines its test command");
    let run = start_init(&runtime, session, model, "Focus on the tests").await;
    assert_eq!(run.turn.mode, AgentTaskMode::Normal);
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let written = std::fs::read_to_string(root.join("AGENTS.md")).expect("AGENTS.md is written");
    assert!(aura_workspace_domain::standard_structure(&written), "{written}");
    assert!(written.contains("- `cargo test --workspace` — Run every Rust test"), "{written}");
    assert!(written.contains("Source: defined in `README.md`"), "{written}");
    let requests = provider.requests.lock().expect("requests").clone();
    let first = requests[0].to_string();
    assert!(first.contains("This is a /init task"), "the init prompt module is in the request");
    // The message is the command and the user's note, however the request escapes it.
    assert!(first.contains("Focus on the tests") && first.contains("/init"), "the note travels");
    assert!(offers_init_tool(&requests[0]));
}

#[tokio::test]
async fn init_runs_as_normal_and_an_active_plan_continues_afterwards() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::FinishTurn).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Plan).await;
    let session = start.turn.session_id;
    let plan = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    let done =
        wait(&runtime, &plan, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    idle(&runtime, done.run.agent_run_id).await;
    let before = history(&runtime, session).await.mode_episode.expect("the Plan stays active");
    let model = {
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
        let store = services.broker.lock().expect("store");
        aura_persistence_v2::model::step_snapshot(
            store.database.connection(),
            done.model_steps[0].model_step_id,
        )
        .expect("completed step snapshot")
        .model_profile_id
    };
    let init = start_init(&runtime, session, model, "").await;
    assert_eq!(init.turn.mode, AgentTaskMode::Normal, "an init task never runs as Plan");
    let init_request = AgentRunRequest { session_id: session, agent_run_id: init.run.agent_run_id };
    wait(&runtime, &init_request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
        .await;
    idle(&runtime, init.run.agent_run_id).await;
    let after = history(&runtime, session).await.mode_episode.expect("the Plan is still there");
    assert_eq!(after, before, "the init task neither bound to nor changed the Plan");
    let requests = provider.requests.lock().expect("requests").clone();
    assert!(!offers_init_tool(&requests[0]), "an ordinary turn never sees the init write");
    assert!(offers_init_tool(requests.last().expect("init request")));
    assert!(!requests[0].to_string().contains("This is a /init task"));

    // The next ordinary turn continues the Plan exactly as an ordinary turn would.
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.submit",
        json!(AgentSubmitRequest {
            session_id: session,
            text: "Continue the plan".into(),
            destination: AgentInputLane::NextTurn,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let next = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let view = history(&runtime, session).await;
            if let Some(latest) = view.turns.last().filter(|t| {
                t.turn.turn_task_id != done.turn.turn_task_id
                    && t.turn.turn_task_id != init.turn.turn_task_id
            }) {
                return latest.turn.mode;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the next turn starts");
    assert_eq!(next, AgentTaskMode::Plan);
}

#[tokio::test]
async fn init_waits_for_an_idle_session() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let model = {
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
        let store = services.broker.lock().expect("store");
        aura_persistence_v2::agent::application::configuration(
            store.database.connection(),
            start.run.agent_run_id,
        )
        .expect("configuration")
        .request
        .model_profile_id
    };
    let response = crate::r03_tests::request(
        &runtime,
        "agent.init.start",
        json!(AgentInitStartRequest {
            session_id: start.turn.session_id,
            model_profile_id: model,
            text: String::new()
        }),
    )
    .await;
    assert!(response.get("error").is_some(), "{response}");
    assert!(response.to_string().contains("agent.init_requires_idle"), "{response}");
}
