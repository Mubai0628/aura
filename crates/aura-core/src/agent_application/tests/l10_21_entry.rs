//! The unified entry: the request's tool list
//! stays the same for a whole turn so the Provider can reuse its cached prefix; another tool of the
//! step's registry runs through `tools.call` with that tool's own checks, and a load applies from
//! the next turn.
use super::*;
use aura_core_broker::agent_store;
use aura_model_domain::provider_wire_tool_name;

fn listed(request: &Value) -> Vec<String> {
    request["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .collect()
}

fn wire(name: &str) -> String {
    let release = aura_tool_registry::agent_release::release().expect("release");
    provider_wire_tool_name(name, release.entry(name).expect(name).definition.tool_id)
}

fn messages(request: &Value) -> Vec<Value> {
    request["messages"].as_array().cloned().unwrap_or_default()
}

/// The text of the tool result that answers `call_id` in `request`.
fn result_for(request: &Value, call_id: &str) -> String {
    messages(request)
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == call_id)
        .and_then(|message| message["content"].as_str().map(str::to_owned))
        .unwrap_or_else(|| panic!("the result of {call_id} reached the request"))
}

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    services
}

fn completed(view: &AgentRunProjection) -> bool {
    matches!(view.turn.state, TurnTaskState::Completed { .. })
}

#[tokio::test]
async fn a_tool_outside_the_list_runs_through_the_entry_and_a_load_waits_for_the_next_turn() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::UnifiedEntry).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let run = started.run.agent_run_id;
    waiting::wait(&runtime, &AgentRunRequest { session_id: session, agent_run_id: run }, completed)
        .await;
    let turn = provider.requests.lock().expect("requests").clone();
    assert_eq!(turn.len(), 3, "the entry call, the load and the answer");
    // One list for the whole turn: the entry is in it, the queried tool is not, and the load
    // changed nothing in it.
    let list = listed(&turn[0]);
    assert!(list.contains(&wire("tools.call")));
    assert!(!list.contains(&wire("agent.activity.query")));
    for later in &turn[1..] {
        assert_eq!(listed(later), list, "the list stays the same during the turn");
    }
    // The model is sent back its own call - tools.call with its own arguments - and the query's
    // real result.
    let call = messages(&turn[1])
        .iter()
        .filter_map(|message| message["tool_calls"].as_array().cloned())
        .flatten()
        .find(|call| call["id"] == "fixture_0_0")
        .expect("the entry call is in the history");
    assert_eq!(call["function"]["name"], wire("tools.call"));
    let arguments: Value =
        serde_json::from_str(call["function"]["arguments"].as_str().expect("args"))
            .expect("arguments json");
    assert_eq!(arguments["tool"], "agent.activity.query");
    let result = result_for(&turn[1], "fixture_0_0");
    assert!(result.contains("contract_version") && result.contains("page"), "{result}");
    // History names the result by the tool that ran, so its data class is the query's.
    let items = services(&runtime)
        .agent_required()
        .store()
        .read(agent_store::StoreStep::run(run, "l10_21.entry_history"), move |db| {
            Ok(aura_persistence_v2::agent::model_context::history_for_model(db, run, false)?.items)
        })
        .await
        .expect("history");
    assert!(items.iter().any(|item| matches!(item,
        LogicalConversationItem::ToolResult { tool_name, .. } if tool_name == "agent.activity.query")));
    assert!(items.iter().any(|item| matches!(item,
        LogicalConversationItem::Assistant { tool_calls, .. }
            if tool_calls.iter().any(|call| call.canonical_name == "tools.call"))));
    // The next turn starts with the loaded tool in its list.
    let next = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    waiting::wait(
        &runtime,
        &AgentRunRequest { session_id: session, agent_run_id: next.run.agent_run_id },
        completed,
    )
    .await;
    let all = provider.requests.lock().expect("requests").clone();
    let next_list = listed(all.last().expect("the next turn's request"));
    assert!(next_list.contains(&wire("agent.activity.query")), "loaded from the next turn");
}

#[tokio::test]
async fn the_entry_runs_no_turn_control_tool() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::EntryRefused).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    waiting::wait(&runtime, &request, completed).await;
    let requests = provider.requests.lock().expect("requests").clone();
    let result = result_for(&requests[1], "fixture_0_0");
    assert!(result.contains("tools.call did not run `goal.complete`"), "{result}");
    assert!(result.contains("turn-control tools"), "{result}");
}

#[tokio::test]
async fn a_write_through_the_entry_asks_exactly_as_the_write_itself() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::EntryExternalWrite).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let external = host.root.path().join("outside-entry");
    std::fs::create_dir_all(&external).expect("external root");
    let target = external.join("through-entry.txt");
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!("Create AURA_EXTERNAL_PATH={} now.", target.display()),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let started: AgentRunProjection = serde_json::from_value(started).expect("start");
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    // A lower mode asks before an external write, whichever way the model called it.
    let asked = |view: &AgentRunProjection| {
        !view.pending_approval_ids.is_empty()
            || matches!(view.run.state, AgentRunState::AwaitingApproval { .. })
    };
    let view = waiting::wait(&runtime, &request, asked).await;
    assert!(asked(&view));
    assert!(!target.exists(), "nothing is written while the write waits for approval");
}

/// A trusted `tool.before_call` Hook that blocks every call of `tool`.
async fn blocking_hook(runtime: &crate::RuntimeState, root: &std::path::Path, tool: &str) {
    let directory = root.join("hooks").join("guard");
    std::fs::create_dir_all(directory.join("bin")).expect("package folder");
    let debug = std::env::current_exe()
        .expect("test image")
        .parent()
        .expect("deps")
        .parent()
        .expect("debug")
        .to_owned();
    std::fs::copy(debug.join("aura-hook-fixture.exe"), directory.join("bin/hook.exe"))
        .expect("build current aura-tool-fixtures --bins first");
    std::fs::write(
        directory.join("hook.toml"),
        format!(
            "schema_version = 1\nname = \"guard\"\nversion = \"1.0.0\"\n\
             events = ['tool.before_call']\n[command]\nexecutable = \"bin/hook.exe\"\n\
             arguments = ['answer', '{{\"decision\":\"block\",\"reason\":\"不许查询\"}}']\n\
             [match]\ntools = [\"{tool}\"]\n"
        ),
    )
    .expect("manifest");
    let path = directory.display().to_string();
    let preview =
        crate::r03_tests::success(runtime, "agent.hooks.preview", json!({"path": path})).await;
    let list = crate::r03_tests::success(runtime, "agent.hooks.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.hooks.trust",
        json!({"path": path, "expected_fingerprint": preview["fingerprint"], "enabled": true, "expected_revision": list["revision"]}),
    )
    .await;
}

#[tokio::test]
async fn a_hook_on_the_named_tool_sees_and_stops_a_call_made_through_the_entry() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::EntryHookQuery).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    // The Hook listens to the query, which is not in the request's list: only the entry
    // reaches it.
    blocking_hook(&runtime, host.root.path(), "agent.activity.query").await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    waiting::wait(&runtime, &request, completed).await;
    let requests = provider.requests.lock().expect("requests").clone();
    assert!(!listed(&requests[0]).contains(&wire("agent.activity.query")));
    let seen = result_for(&requests[1], "fixture_0_0");
    assert!(seen.contains("被 Hook guard 阻止：不许查询"), "{seen}");
    assert!(!seen.contains("contract_version"), "the blocked query never ran: {seen}");
}
