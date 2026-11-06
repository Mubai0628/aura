//! An MCP resource referred to with "@": read when the message is sent and kept as a file of the
//! conversation, which the model reads by its path like any other capture.
use super::*;
use provider_fixture::Scenario;

async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

/// The repository's test MCP server, offering resources, saved and trusted through the routes.
async fn notes_server(host: &crate::r03_tests::Fixture, runtime: &crate::RuntimeState) {
    let debug = std::env::current_exe()
        .expect("test image")
        .parent()
        .and_then(std::path::Path::parent)
        .expect("debug")
        .to_owned();
    let exe = host.root.path().join("mcp-servers").join("fixture-server.exe");
    std::fs::create_dir_all(exe.parent().expect("folder")).expect("server folder");
    std::fs::copy(debug.join("aura-mcp-fixture.exe"), &exe)
        .expect("build current aura-tool-fixtures --bins first");
    let revision = |listed: &Value| listed["revision"].clone();
    let listed = call(runtime, "agent.mcp.list", json!({})).await;
    let server = json!({
        "id": "notes",
        "display_name": "笔记",
        "transport": {"type": "stdio", "command": exe.display().to_string(), "arguments": ["--resources"]},
        "startup_timeout_ms": 10_000,
        "call_timeout_ms": 10_000,
    });
    call(
        runtime,
        "agent.mcp.save",
        json!({"server": server, "replaces": null, "expected_revision": revision(&listed)}),
    )
    .await;
    let listed = call(runtime, "agent.mcp.list", json!({})).await;
    let fingerprint = listed["servers"][0]["fingerprint"].clone();
    call(
        runtime,
        "agent.mcp.trust",
        json!({"id": "notes", "expected_fingerprint": fingerprint, "expected_revision": revision(&listed)}),
    )
    .await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let state = call(runtime, "agent.mcp.list", json!({})).await["servers"][0]["state"].clone();
        if state != "starting" || std::time::Instant::now() >= deadline {
            assert_eq!(state, "ready");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn an_mcp_resource_is_read_when_the_message_is_sent_and_kept_for_the_model_to_read() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ReadAttachedFile).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    notes_server(&host, &runtime).await;
    let listed = call(&runtime, "agent.mcp.resources", json!({"session_id": session})).await;
    assert_eq!(listed["resources"][0]["uri"], "fixture://notes/readme", "{listed}");

    let started: AgentRunProjection = serde_json::from_value(
        call(
            &runtime,
            "agent.start",
            json!({
                "session_id": session,
                "model_profile_id": model,
                "text": "按这份笔记做",
                "mode": "normal",
                "goal_budget": null,
                "attachments": [{"kind": "mcp_resource", "server_id": "notes", "uri": "fixture://notes/readme", "name": "readme"}],
            }),
        )
        .await,
    )
    .expect("run");
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("requests").clone();
    let first = requests[0].to_string();
    assert!(first.contains("v2-message-captures"), "{first}");
    assert!(first.contains("readme.txt"), "{first}");
    // What the read gave the model: the text inside each tool result.
    let read: String = requests.last().expect("the answer's request")["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|m| m["role"] == "tool")
        .filter_map(|m| m["content"].as_str())
        .filter_map(|content| serde_json::from_str::<Value>(content).ok())
        .filter_map(|result| {
            result["outcome"]["payload"]["value"]["text"].as_str().map(str::to_owned)
        })
        .collect();
    assert!(
        read.contains("MCP resource \"readme\" (fixture://notes/readme) of server \"笔记\""),
        "{read}"
    );
    assert!(read.contains("The first line."), "{read}");

    // A resource of a server that is not there stops the message and says so.
    let refused = crate::r03_tests::attempt(
        &runtime,
        "agent.start",
        json!({
            "session_id": session,
            "model_profile_id": model,
            "text": "again",
            "mode": "normal",
            "goal_budget": null,
            "attachments": [{"kind": "mcp_resource", "server_id": "gone", "uri": "fixture://x", "name": "x"}],
        }),
    )
    .await;
    assert!(format!("{refused:?}").contains("mcp.not_found"), "{refused:?}");
}
