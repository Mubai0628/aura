use super::*;
use crate::{StartupMode, r03_tests};

#[tokio::test]
async fn l02_b2_user_routes_bind_to_session_and_release_closed_records() {
    let host = r03_tests::Fixture::new();
    let runtime = host.start().await;
    let StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = r03_tests::success(
        &runtime,
        "workspace.create",
        serde_json::json!({"display_name":"B2 local","selected_root":host.root.path()}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(workspace["workspace"].clone()).unwrap();
    let session = SessionId::new();
    services
        .persistence
        .upsert_workspace_session(
            workspace.clone(),
            aura_workspace_domain::SessionSummary {
                session_id: session,
                workspace_id: workspace.workspace_id,
                title: "temporary terminal".into(),
                state: "idle".into(),
            },
            CorrelationId::new(),
        )
        .unwrap();
    let opened = r03_tests::success(
        &runtime,
        "terminal.user.open",
        serde_json::json!({"session_id":session,"shell":"cmd","columns":80,"rows":24}),
    )
    .await;
    let id = opened["terminal_id"].as_str().unwrap();
    let commands = services.commands_required();
    let terminal = commands.compat.terminals.get(session, id).unwrap();
    let weak = Arc::downgrade(&terminal);
    let root = aura_windows_platform::retain_process(terminal.identity().process_id).unwrap();
    assert_eq!(
        r03_tests::success(
            &runtime,
            "terminal.user.list",
            serde_json::json!({"session_id":session})
        )
        .await["terminals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    r03_tests::success(&runtime,"terminal.user.write",serde_json::json!({"session_id":session,"terminal_id":id,"input":"echo B2-user-interaction\r","interrupt":false})).await;
    r03_tests::success(
        &runtime,
        "terminal.user.resize",
        serde_json::json!({"session_id":session,"terminal_id":id,"columns":110,"rows":30}),
    )
    .await;
    let mut changed = terminal.subscribe_output();
    while terminal.read(0, 1).unwrap().bytes.is_empty() {
        tokio::time::timeout(std::time::Duration::from_secs(5), changed.changed())
            .await
            .unwrap()
            .unwrap();
    }
    let queue = crate::AsyncPriorityQueue::new();
    let subscribe = crate::JsonRpcRequest {
        jsonrpc: crate::JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: None,
        method: "terminal.user.subscribe".into(),
        params: serde_json::json!({"session_id":session,"terminal_id":id,"offset":0}),
    };
    let ack = commands
        .terminal_request(runtime.options.runtime_instance_id, queue.clone(), &subscribe)
        .await
        .unwrap();
    assert_eq!(
        ack["data_base64"], "",
        "P0 subscribe reply is metadata; output uses P2 notifications"
    );
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), queue.recv())
        .await
        .unwrap()
        .unwrap();
    let crate::WireFrame::Control(bytes) = frame.frame else { panic!("notification") };
    let notification: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(notification["method"], "terminal.output.v1");
    assert!(notification["params"]["next_offset"].as_u64().unwrap() > 0);
    assert!(commands.compat.terminals.get(SessionId::new(), id).is_err());
    drop(terminal);
    let closed = r03_tests::success(
        &runtime,
        "terminal.user.close",
        serde_json::json!({"session_id":session,"terminal_id":id}),
    )
    .await;
    assert_eq!(closed["end"]["stop_confirmed"], true);
    assert!(!root.is_running().unwrap());
    assert!(weak.upgrade().is_none());
    assert!(
        r03_tests::success(
            &runtime,
            "terminal.user.list",
            serde_json::json!({"session_id":session})
        )
        .await["terminals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
