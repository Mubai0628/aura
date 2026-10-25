//! Real ConPTY input does not consume settled command history.
use super::*;
use crate::{StartupMode, r03_tests};

#[tokio::test]
async fn l03_b0_five_thousand_keys_preserve_command_outcome_and_input_order() {
    let host = r03_tests::Fixture::new();
    let runtime = host.start().await;
    let StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = r03_tests::success(
        &runtime,
        "workspace.create",
        serde_json::json!({"display_name":"B0 input","selected_root":host.root.path()}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(workspace["workspace"].clone()).unwrap();
    let remembered = {
        let outcomes = runtime.command_outcomes.lock().unwrap();
        let (id, value) = outcomes.entries.iter().next().expect("real workspace command result");
        (*id, serde_json::to_value(value).unwrap())
    };
    let session = SessionId::new();
    services
        .persistence
        .upsert_workspace_session(
            workspace.clone(),
            aura_workspace_domain::SessionSummary {
                session_id: session,
                workspace_id: workspace.workspace_id,
                title: "temporary input".into(),
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
    let suffix = ">ordered-keys.txt\r";
    let expected = "0123456789".repeat(500);
    let expected = &expected[..5000 - "echo ".len() - suffix.len()];
    let command = format!("echo {expected}{suffix}");
    assert_eq!(command.len(), 5000);
    for key in command.chars() {
        r03_tests::success(
            &runtime,
            "terminal.user.write",
            serde_json::json!({"session_id":session,"terminal_id":id,"input":key.to_string(),"interrupt":false}),
        ).await;
    }
    let path = host.root.path().join("ordered-keys.txt");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(std::fs::read_to_string(path).unwrap().trim_end(), expected);
    let outcome = r03_tests::success(
        &runtime,
        "runtime.command_outcome",
        serde_json::json!({"command_invocation_id":remembered.0}),
    )
    .await;
    assert_eq!(outcome, remembered.1, "keys must not evict the remembered command");
    assert!(!crate::command_claims::needs_command_claim("terminal.user.write"));
    let denied = r03_tests::request(
        &runtime,
        "terminal.user.write",
        serde_json::json!({"session_id":SessionId::new(),"terminal_id":id,"input":"X","interrupt":false}),
    ).await;
    assert!(denied["error"].is_object(), "session identity remains authoritative");
    r03_tests::success(
        &runtime,
        "terminal.user.close",
        serde_json::json!({"session_id":session,"terminal_id":id}),
    )
    .await;
}
