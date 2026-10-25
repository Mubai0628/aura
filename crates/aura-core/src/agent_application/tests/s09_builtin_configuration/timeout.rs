use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn s09_saved_timeout_override_bounds_a_real_http_execution() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { panic!("Core ready") };
    expose_http(s).await;
    set_instance(s, true, Some(1500));
    let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("HTTP tool target");
    let target_url = format!("http://{}", target.local_addr().expect("target address"));
    let started = Arc::new(tokio::sync::Notify::new());
    let seen = Arc::clone(&started);
    let target_task = tokio::spawn(async move {
        let (mut socket, _) = target.accept().await.expect("actual tool request");
        let mut buf = [0; 8192];
        let received = socket.read(&mut buf).await.expect("target request bytes");
        assert!(received > 0, "the target received an actual request");
        seen.notify_one();
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await;
    });
    let provider = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("model provider");
    let provider_url = format!("http://{}/v1", provider.local_addr().expect("provider address"));
    let model_task = tokio::spawn(async move {
        for ordinal in 0..2 {
            let (mut socket, _) = provider.accept().await.expect("real model request");
            let request = read_request(&mut socket).await;
            let call = |name: &str, args: Value, index: usize| {
                let entry = aura_tool_registry::agent_release::release()
                    .expect("release")
                    .entry(name)
                    .expect("requested tool");
                json!({"index":index,"id":format!("s09-timeout-{index}"),"type":"function","function":{"name":aura_model_domain::provider_wire_tool_name(name,entry.definition.tool_id),"arguments":crate::agent_application::tests::provider_fixture::flat_call("Test saved HTTP timeout", args)}})
            };
            let delta = if ordinal == 0 {
                json!({"role":"assistant","tool_calls":[call("http.request",json!({"url":target_url}),0)]})
            } else {
                provider_fixture::recovery_response(&provider_fixture::state(&request), &call)
            };
            provider_fixture::send_with(&mut socket, ordinal, delta, None).await;
        }
    });
    let model = profile(&runtime, &provider_url).await;
    let root = host.root.path().join("timeout-override");
    std::fs::create_dir(&root).expect("workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"S09 timeout"}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace, PermissionMode::FullAccess).await;
    let start: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(
            &runtime,
            "agent.start",
            json!(AgentStartRequest {
                session_id: session,
                model_profile_id: model,
                text: "Make the local HTTP request.".into(),
                mode: AgentTaskMode::Normal,
                goal_budget: None,
                explicit_backend_id: None,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                attachments: Vec::new(),
            }),
        )
        .await,
    )
    .expect("Agent start");
    wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
        .await
        .expect("the tool really started its request");
    let store = s.broker.lock().expect("execution evidence");
    let db = store.database.connection();
    let http = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP")
        .definition
        .tool_id;
    let mut query = db.prepare("SELECT id FROM tool_calls").expect("call identities");
    let ids = query
        .query_map([], |r| r.get::<_, String>(0))
        .expect("call rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("call ids");
    let calls = ids
        .into_iter()
        .map(|id| {
            aura_persistence_v2::tool::execution::call(db, id.parse().expect("call id"))
                .expect("execution record")
        })
        .collect::<Vec<_>>();
    let call = calls
        .iter()
        .find(|call| call.call.definition_ref.as_ref().is_some_and(|r| r.tool_id == http))
        .expect("HTTP call");
    assert!(call.attempt_id.is_some(), "the test exercised execution rather than planning");
    let result = call.result.as_ref().expect("HTTP result");
    assert!(
        matches!(
            result.outcome,
            aura_tool_contract::ToolResultOutcomeV1::Cancelled {
                reason: aura_tool_contract::ToolCancellationReasonV1::Timeout
            }
        ),
        "the saved override must time out instead of succeeding after four seconds: {result:?}"
    );
    model_task.abort();
    target_task.abort();
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = socket.read(&mut buf).await.expect("provider request bytes");
        assert!(n > 0, "request must contain a full body");
        bytes.extend_from_slice(&buf[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .expect("body length")
                .trim()
                .parse()
                .expect("body length number");
            if bytes.len() >= end + 4 + length {
                return serde_json::from_slice(&bytes[end + 4..end + 4 + length])
                    .expect("compiled model request");
            }
        }
    }
}
