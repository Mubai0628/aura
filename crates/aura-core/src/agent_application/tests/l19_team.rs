//! Sub-agents through the real model loop. The conversation's model starts one on a task sheet,
//! waits for its report, answers its question and stops it. The sub-agent works in a hidden,
//! read-only conversation of its own, offered only read-only tools and its report tool, is
//! refused another conversation and Aura's own data, and asks the user through the conversation
//! that started it.
use super::*;
use provider_fixture::{ANSWER, QUESTION, SUMMARY, Scenario, TeamPlan, offers};
use std::collections::BTreeSet;
#[path = "session_agent_controls.rs"]
mod session_agent_controls;
#[path = "session_agent_dispatch.rs"]
mod session_agent_dispatch;
#[path = "session_agent_graph.rs"]
mod session_agent_graph;
#[path = "session_agent_races.rs"]
mod session_agent_races;
#[path = "session_agents.rs"]
mod session_agents;
#[path = "session_profile_model.rs"]
mod session_profile_model;

/// One request to Core, boxed: Core's request futures are large.
pub(super) async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

/// A runtime whose one model the user allowed for sub-agents, and a conversation.
pub(super) async fn team(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, SessionId, ModelProfileId) {
    let (runtime, session, model) = team_with_model(host, provider).await;
    session_agent_controls::set_session_policy(&runtime, session, true).await;
    (runtime, session, model)
}

async fn team_with_model(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, SessionId, ModelProfileId) {
    let (runtime, session, model) = prepare(host, provider).await;
    let snapshot = call(&runtime, "models.settings.get_snapshot", json!({})).await;
    call(
        &runtime,
        "models.profile.set_subagent_availability",
        json!({
            "expected_revision": snapshot["models_revision"],
            "model_profile_id": model,
            "available_for_subagents": true,
        }),
    )
    .await;
    // The desktop binds the conversation's model before it sends; the team list names it.
    call(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    (runtime, session, model)
}

/// Runs one turn of the conversation to its end.
pub(super) async fn turn(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    text: &str,
) {
    let request = begin(runtime, session, model, text).await;
    wait(runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
}

/// Starts one turn of the conversation.
pub(super) async fn begin(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    text: &str,
) -> AgentRunRequest {
    let started = call(
        runtime,
        "agent.start",
        json!(AgentStartRequest {
            computer_mode: None,
            session_id: session,
            model_profile_id: model,
            text: text.into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            speed_mode: None,
            attachments: Vec::new(),
            references: Vec::new(),
        }),
    )
    .await;
    let run: AgentRunProjection = serde_json::from_value(started).expect("run");
    AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id }
}

/// The requests the model was sent: the sub-agent's (they offer `agent.report`) and the
/// conversation's.
pub(super) fn requests(provider: &Provider) -> (Vec<Value>, Vec<Value>) {
    provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .cloned()
        .partition(|r| offers(r, "agent.report"))
}

/// The tool results a request carries, in order.
pub(super) fn results(request: &Value) -> Vec<String> {
    request["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == "tool")
        .filter_map(|m| m["content"].as_str().map(str::to_owned))
        .collect()
}

pub(super) fn texts(request: &Value, role: &str) -> Vec<String> {
    request["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == role)
        .filter_map(|m| m["content"].as_str().map(str::to_owned))
        .collect()
}

/// The released tools a request offers, by name.
pub(super) fn offered(request: &Value) -> BTreeSet<String> {
    let release = aura_tool_registry::agent_release::release().expect("release");
    release
        .manifest()
        .entries
        .iter()
        .map(|entry| entry.handler.name().to_owned())
        .filter(|name| offers(request, name))
        .collect()
}

pub(super) async fn members(runtime: &crate::RuntimeState, session: SessionId) -> Vec<Value> {
    let view = call(runtime, "agent.team.list", json!({"session_id": session})).await;
    view["members"].as_array().cloned().unwrap_or_default()
}

/// Asks for the team's list until `ready` holds for it.
pub(super) async fn listed_until(
    runtime: &crate::RuntimeState,
    session: SessionId,
    what: &str,
    ready: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let view = call(runtime, "agent.team.list", json!({"session_id": session})).await;
            if ready(&view) {
                return view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{what}"))
}

/// The conversations of the workspace/session projection.
async fn sessions(runtime: &crate::RuntimeState) -> Vec<Value> {
    let projection = call(
        runtime,
        "projection.snapshot",
        json!({"projection_kind": "workspace_session", "projection_scope": {"type": "global"}}),
    )
    .await;
    projection["payload"]["sessions"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn a_sub_agent_reports_through_wait_and_is_offered_the_main_agents_tools() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Report }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    turn(&runtime, session, model, "看看入口在哪").await;

    let (theirs, ours) = requests(&provider);
    assert!(!theirs.is_empty(), "the sub-agent worked");
    // The conversation waited for the report and read it.
    let read = results(ours.last().expect("the conversation's last request"));
    assert!(read.last().is_some_and(|wait| wait.contains(SUMMARY)), "{read:?}");
    // The sub-agent began on its task sheet, under its own role.
    let first = &theirs[0];
    assert!(
        texts(first, "user").iter().any(|t| t.starts_with("来自主 Agent 的任务单：读代码")),
        "{first}"
    );
    assert!(texts(first, "system").iter().any(|t| t.contains("You are a sub-agent.")));
    assert!(!texts(&ours[0], "system").iter().any(|t| t.contains("You are a sub-agent.")));
    let guide = texts(&ours[0], "system").join("\n");
    assert!(guide.contains("Use agents.spawn for a bounded independent part"), "{guide}");
    assert!(guide.contains("\"temporary_slots_available\":3"), "{guide}");
    // Its sheet says it starts read-only in the shared workspace; the role it shares with a
    // writing sub-agent says nothing either way.
    assert!(
        texts(first, "user").iter().any(|t| t.contains("\n## 权限\n你的权限档位是「只读」")),
        "{first}"
    );
    assert!(!texts(first, "system").iter().any(|t| t.contains("change no file")), "{first}");
    // The conversation is told which sub-agents may change files, and that a sub-agent loads the
    // user's Skills itself.
    assert!(guide.contains("With worktree: true"), "{guide}");
    assert!(guide.contains("finds and loads the user's Skills itself"), "{guide}");
    // What the conversation is offered but the main agent's own tools, and its report; the
    // conversation never has the report tool.
    let parent_tools = offered(&ours[0]);
    for request in &theirs {
        let tools = offered(request);
        assert!(tools.contains("agent.report"), "{tools:?}");
        let missing: Vec<&String> = parent_tools
            .iter()
            .filter(|t| !aura_agent_team::policy::main_agent_only(t) && !tools.contains(*t))
            .collect();
        assert!(missing.is_empty(), "the sub-agent lacks {missing:?}");
        let main_only: Vec<&String> =
            tools.iter().filter(|t| aura_agent_team::policy::main_agent_only(t)).collect();
        assert!(main_only.is_empty(), "the sub-agent was offered {main_only:?}");
        for tool in ["fs.write", "tools.search", "tools.load"] {
            assert!(tools.contains(tool), "{tool} not offered: {tools:?}");
        }
    }
    assert!(!parent_tools.contains("agent.report"));
    // agents.approve only while the settings let the main agent answer first; they do not here.
    let coordinating = aura_agent_team::policy::COORDINATOR_TOOLS.iter();
    assert!(coordinating.filter(|t| **t != "agents.approve").all(|t| parent_tools.contains(*t)));
    assert!(!parent_tools.contains("agents.approve"), "{parent_tools:?}");
    // The team shows the main agent first, then the reported sub-agent with its report; each
    // with what its answers used. Its conversation is kept out of the sidebar.
    let view = call(&runtime, "agent.team.list", json!({"session_id": session})).await;
    assert_eq!(view["lead"]["model_profile_id"], json!(model));
    assert!(view["lead"]["model"].is_string(), "{view}");
    assert_eq!(view["lead"]["working"], false);
    assert!(view["lead"]["tokens"].as_u64().is_some_and(|t| t > 0), "{view}");
    let team = view["members"].as_array().cloned().unwrap_or_default();
    assert_eq!(team.len(), 1, "{team:?}");
    assert_eq!(team[0]["agent"], "A1");
    assert_eq!(team[0]["state"], "reported");
    assert_eq!(team[0]["last_report"], "ready");
    assert_eq!(team[0]["summary"], SUMMARY);
    assert!(team[0]["tokens"].as_u64().is_some_and(|t| t > 0), "{team:?}");
    let child = sessions(&runtime)
        .await
        .into_iter()
        .find(|s| s["session_id"] == team[0]["session_id"])
        .expect("the sub-agent's conversation");
    assert_eq!(child["state"], "subagent");
    assert_eq!(child["title"], "子 Agent：读代码");
}

#[tokio::test]
async fn a_question_in_a_report_is_answered_with_send_and_the_sub_agent_goes_on() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Send }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    turn(&runtime, session, model, "看看入口在哪").await;

    let (theirs, ours) = requests(&provider);
    let read = results(ours.last().expect("the conversation's last request"));
    assert_eq!(read.len(), 4, "spawn, wait, send, wait: {read:?}");
    assert!(read[1].contains(QUESTION), "the first wait brings the question: {}", read[1]);
    assert!(read[3].contains(SUMMARY), "the second wait brings the report: {}", read[3]);
    let answered = theirs
        .iter()
        .any(|r| texts(r, "user").iter().any(|t| *t == format!("来自主 Agent：{ANSWER}")));
    assert!(answered, "the answer reached the sub-agent as a message of its own");
    let team = members(&runtime, session).await;
    assert_eq!((&team[0]["state"], &team[0]["last_report"]), (&json!("reported"), &json!("ready")));
}

#[tokio::test]
async fn stopping_a_sub_agent_cancels_its_turn_and_it_reports_nothing() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Stop }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    turn(&runtime, session, model, "看看入口在哪").await;

    let (theirs, _) = requests(&provider);
    assert_eq!(theirs.len(), 1, "its one request was held until it stopped");
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let team = members(&runtime, session).await;
            if team[0]["state"] == "stopped" && team[0]["working"] == false {
                return team;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the sub-agent's turn ends");
    assert!(stopped[0].get("last_report").is_none(), "{stopped:?}");
}

#[tokio::test]
async fn the_user_stops_a_sub_agent_from_the_page_and_the_waiting_main_agent_is_told() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::UserStop }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    let request = begin(&runtime, session, model, "看看入口在哪").await;
    // A1 works on its held request while the main agent waits for it in agents.wait.
    let working = listed_until(&runtime, session, "A1 works and the main agent waits", |view| {
        view["members"][0]["working"] == true && view["lead"]["tool"] == "agents.wait"
    })
    .await;
    assert_eq!(working["lead"]["working"], true);
    let stopped =
        call(&runtime, "agent.team.stop", json!({"session_id": session, "agent": "A1"})).await;
    assert_eq!(stopped["members"][0]["state"], "stopped");
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;

    let (theirs, ours) = requests(&provider);
    assert_eq!(theirs.len(), 1, "its one request was held until it stopped");
    let waited = results(ours.last().expect("the conversation's last request"));
    assert!(
        waited.last().is_some_and(|wait| wait.contains("用户在多 Agent 页停止了这个子 Agent")),
        "{waited:?}"
    );
    let again =
        call(&runtime, "agent.team.stop", json!({"session_id": session, "agent": "A1"})).await;
    assert_eq!(again["members"][0]["state"], "stopped");
    assert_eq!(again["members"][0]["last_report"], "failed");
}

#[tokio::test]
async fn a_task_sheet_naming_another_conversation_starts_nothing() {
    let host = crate::r03_tests::Fixture::new();
    let provider =
        Provider::scenario(Scenario::Team { plan: TeamPlan::SheetNamesConversation }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    turn(&runtime, session, model, "看看那个会话").await;

    let (theirs, ours) = requests(&provider);
    assert!(theirs.is_empty());
    let read = results(ours.last().expect("the conversation's last request"));
    assert!(read[0].contains("agents.task_names_conversation"), "{read:?}");
    assert!(members(&runtime, session).await.is_empty());
    assert!(sessions(&runtime).await.iter().all(|s| s["state"] != "subagent"));
    // The call's own task sheet was refused: a request the model can correct, not a tool that is
    // unavailable.
    let rows = call(
        &runtime,
        "agent.tool_activity",
        json!({"session_id": session, "before_call_id": null, "limit": 10, "call_ids": null}),
    )
    .await;
    let spawn = rows["calls"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["summary"]["display_name"] == "agents.spawn")
        .unwrap_or_else(|| panic!("the spawn has its row: {rows}"));
    assert_eq!(spawn["summary"]["outcome"]["type"], "operation_rejected", "{spawn}");
    assert_eq!(spawn["summary"]["error_class"], "invalid_request", "{spawn}");
    assert_eq!(spawn["summary"]["error_code"], "agents.task_names_conversation", "{spawn}");
    assert_eq!(spawn["display"]["summary"], "未执行：请求无效", "{spawn}");
}

#[tokio::test]
async fn a_sub_agent_is_refused_another_conversation_and_auras_data_without_asking() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Refusals }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    // The fixture's Core keeps its data here (`AuraPaths::from_roots(.., root/local)`).
    let logs = host.root.path().join("local").join("logs");
    std::fs::create_dir_all(&logs).expect("Aura's log folder");
    let inside = logs.join("note.txt");
    std::fs::write(&inside, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("a file in Aura's data");
    turn(&runtime, session, model, &format!("先看 AURA_EXTERNAL_PATH={}", inside.display())).await;

    let (theirs, ours) = requests(&provider);
    let read = results(theirs.last().expect("the sub-agent's last request"));
    // Its report is its final answer, not a tool result.
    assert_eq!(read.len(), 2, "history and the data file: {read:?}");
    // It reads its own history, never another conversation's; nothing runs.
    assert!(
        read[0].contains("agents.other_conversation"),
        "another conversation's history: {}",
        read[0]
    );
    assert!(read[1].contains("agents.aura_data"), "Aura's data: {}", read[1]);
    for request in theirs.iter().chain(&ours) {
        assert!(!request.to_string().contains(provider_fixture::EXTERNAL_BODY_TOKEN));
    }
    // Refused before anything ran: nobody was asked, and the sub-agent went on to report.
    let team = members(&runtime, session).await;
    assert_eq!(team[0]["approvals"], json!([]));
    assert_eq!(team[0]["last_report"], "ready");
}

#[tokio::test]
async fn a_sub_agents_approval_is_listed_for_the_conversation_and_decided_there() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::Approval }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    let outside = host.root.path().join("outside-team");
    std::fs::create_dir_all(&outside).expect("a folder outside the workspace");
    let file = outside.join("note.txt");
    std::fs::write(&file, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("a file outside the workspace");
    let request =
        begin(&runtime, session, model, &format!("先看 AURA_EXTERNAL_PATH={}", file.display()))
            .await;

    // Its read waits for the user, and the conversation that started it lists the approval, with
    // the tool the sub-agent waits to run.
    let (child, approval, tool) = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let team = members(&runtime, session).await;
            if let Some(member) = team.first()
                && let Some(id) = member["approvals"].as_array().and_then(|ids| ids.first())
            {
                return (member["session_id"].clone(), id.clone(), member["tool"].clone());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the sub-agent's approval is listed");
    assert_eq!(tool, "fs.read");
    let asked =
        call(&runtime, "agent.approval.get", json!({"session_id": child, "approval_id": approval}))
            .await;
    call(
        &runtime,
        "agent.approval.resolve",
        json!({
            "session_id": child,
            "approval_id": approval,
            "expected_revision": asked["approval"]["revision"],
            "decision": "allow_session",
        }),
    )
    .await;
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;

    // The approved read reached the sub-agent, and its report the conversation.
    let (theirs, ours) = requests(&provider);
    let read = results(theirs.last().expect("the sub-agent's last request"));
    assert!(read[0].contains(provider_fixture::EXTERNAL_BODY_TOKEN), "{read:?}");
    let waited = results(ours.last().expect("the conversation's last request"));
    assert!(waited.last().is_some_and(|wait| wait.contains(SUMMARY)), "{waited:?}");
    let team = members(&runtime, session).await;
    assert_eq!(team[0]["approvals"], json!([]));
    assert_eq!(team[0]["last_report"], "ready");
}

/// A task sheet the model wrote as a string of JSON, with the tools field inside the string,
/// still starts the sub-agent with those tools, and the conversation is told how to write it.
#[tokio::test]
async fn a_task_sheet_written_as_a_string_still_starts_the_sub_agent() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::StringTask }).await;
    let (runtime, session, model) = team(&host, &provider).await;
    turn(&runtime, session, model, "看看入口在哪").await;

    let (theirs, ours) = requests(&provider);
    assert!(!theirs.is_empty(), "the sub-agent worked");
    let spawned = results(&ours[1]);
    let spawned = spawned.first().expect("the spawn's result");
    assert!(spawned.contains("A1"), "{spawned}");
    assert!(spawned.contains("JSON") && spawned.contains("task"), "the note: {spawned}");
    // The tools that were inside the string are the ones it was given.
    let tools = offered(&theirs[0]);
    assert!(tools.contains("fs.read"), "{tools:?}");
    assert!(!tools.contains("fs.search"), "{tools:?}");
    let read = results(ours.last().expect("the conversation's last request"));
    assert!(read.last().is_some_and(|wait| wait.contains(SUMMARY)), "{read:?}");
}
