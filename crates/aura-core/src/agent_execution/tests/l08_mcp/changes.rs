//! A frozen call runs only against the server and tool the user trusted: switching the server
//! off or a changed tool contract refuses it before anything reaches the server, and a server
//! that says its tools changed is listed again without breaking the unchanged tools.
use super::support::*;
use super::*;

#[tokio::test]
async fn l08_switching_the_server_off_refuses_queued_and_new_calls_before_anything_runs() {
    let marker = Marker::new();
    let mut f =
        Fixture::with_mcp(PermissionMode::FullAccess, &[("files", &marker.arguments())]).await;
    let queued = f.admit(&[("mcp.files.echo", json!({"text": "排队"}))]);
    let id = queued.batch.calls[0].tool_call_id;
    let batch_id = queued.batch.tool_batch_id;
    let expected_revision = revision(&f).await;
    route(
        &f,
        "agent.mcp.set_enabled",
        json!({"id": "files", "enabled": false, "expected_revision": expected_revision}),
    )
    .await;
    let group = f.execute(queued).await;
    let text = error_text(&f, id);
    assert!(text.contains("mcp.tool_unavailable"), "{text}");
    assert_eq!(result(&f, id).side_effect_state, SideEffectStateV1::NotStarted);
    let (summary, _) = activity(&f, id);
    assert!(!summary.contains("MCP 服务器“files”执行"), "a refused call did not run: {summary}");
    f.continue_after_tools(batch_id, &group);

    let batch = f.admit(&[("mcp.files.echo", json!({"text": "新的"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(!batch.calls.contains_key(&id), "a switched-off server's tool is not planned");
    assert!(matches!(result(&f, id).outcome, ToolResultOutcomeV1::NotExecuted { .. }));
    assert!(marker.calls().is_empty(), "nothing reached the server");
    assert_eq!(f.service().mcp.running_instances("files"), 0);
}

#[tokio::test]
async fn l08_a_changed_tool_contract_refuses_the_frozen_call() {
    let marker = Marker::new();
    let f = Fixture::with_mcp(PermissionMode::FullAccess, &[("files", &marker.arguments())]).await;
    let queued = f.admit(&[("mcp.files.echo", json!({"text": "旧契约"}))]);
    let id = queued.batch.calls[0].tool_call_id;
    // The server's tools were listed again and `echo` now has another contract; the runtime
    // assigns the stored listing its revisions, as discovery does.
    {
        let mut store = f.service().broker.lock().expect("journal");
        store
            .database
            .unit_of_work(|tx| {
                let mut catalog = aura_persistence_v2::mcp::load(tx, "files")?.expect("listed");
                let echo = catalog
                    .catalog
                    .tools
                    .iter_mut()
                    .find(|tool| tool.wire_name == "echo")
                    .expect("echo listed");
                echo.description = "Echo the text back, changed".into();
                echo.fingerprint = format!("{}-changed", echo.fingerprint);
                aura_persistence_v2::mcp::save(tx, &catalog)
            })
            .expect("the changed listing is stored");
    }
    let server = f
        .service()
        .mcp
        .configured()
        .servers
        .into_iter()
        .find(|server| server.id == "files")
        .expect("configured");
    f.service().mcp.revise(&server);
    f.execute(queued).await;
    let text = error_text(&f, id);
    assert!(text.contains("mcp.tool_changed"), "{text}");
    assert_eq!(result(&f, id).side_effect_state, SideEffectStateV1::NotStarted);
    assert!(marker.calls().is_empty(), "the changed tool was not called");
}

#[tokio::test]
async fn l08_a_tools_changed_notice_lists_the_server_again_and_unchanged_tools_keep_running() {
    let mut f = Fixture::with_mcp(PermissionMode::FullAccess, &[("files", &[])]).await;
    let batch = f.admit(&[("mcp.files.grow", json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    assert_eq!(f.output(id), json!(format!("{}\ngrown", header("files", "grow"))));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let listed = {
            let store = f.service().broker.lock().expect("journal");
            aura_persistence_v2::mcp::load(store.database.connection(), "files")
                .expect("catalog")
                .expect("listed")
        };
        if listed.catalog.tools.iter().any(|tool| tool.wire_name == "extra") {
            break;
        }
        assert!(std::time::Instant::now() < until, "the server is listed again");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    f.continue_after_tools(batch_id, &group);

    // The frozen registry's unchanged tool still runs on the same instance.
    let batch = f.admit(&[("mcp.files.echo", json!({"text": "照常"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    assert_eq!(f.output(id), json!(format!("{}\n照常", header("files", "echo"))));
    assert_eq!(f.service().mcp.running_instances("files"), 1);

    // A new registry offers the added tool.
    let other = f.package_session(PermissionMode::FullAccess, &[]);
    assert!(
        other
            .registry()
            .catalog_entries
            .iter()
            .any(|entry| entry.canonical_name.as_str() == "mcp.files.extra"),
        "the added tool is offered to the next registry"
    );
}
