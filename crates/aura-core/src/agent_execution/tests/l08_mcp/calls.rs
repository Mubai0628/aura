//! A trusted server's tool in the real Agent path: registry, exact operation, the session's own
//! instance of the server, result, session row and restore without replay.
use super::support::*;
use super::*;

#[tokio::test]
async fn l08_trusted_server_tool_runs_for_the_agent_and_restores_without_replay() {
    let marker = Marker::new();
    let f =
        Fixture::with_mcp(PermissionMode::WorkspaceReadWrite, &[("files", &marker.arguments())])
            .await;
    let registry = f.registry();
    let entry = registry
        .catalog_entries
        .iter()
        .find(|entry| entry.canonical_name.as_str() == "mcp.files.echo")
        .expect("the server's tool is in the catalog");
    assert_eq!(entry.load_state, ToolLoadStateV1::Ephemeral, "loaded like any discoverable tool");
    assert_eq!(entry.source, ToolSourceKindV1::Mcp);

    let batch = f.admit(&[("mcp.files.echo", json!({"text": "你好"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let content = &batch.calls[&id].operation.content;
    assert_eq!(content.source, SecuritySourceRef::McpServer { server_id: "files".into() });
    assert_eq!(content.capabilities, vec![ResolvedCapability::ExternalExtensionCall]);
    assert!(matches!(
        content.resources.as_slice(),
        [ResourceDescriptor::McpServer { server_id, .. }] if server_id == "files"
    ));
    assert_eq!(content.network, OperationNetwork::None);
    assert_eq!(content.action, json!({"text": "你好"}));
    let group = f.execute(batch).await;
    assert_eq!(f.output(id), json!(format!("{}\n你好", header("files", "echo"))));
    assert_eq!(result(&f, id).side_effect_state, SideEffectStateV1::Unassessed);
    let (summary, _) = activity(&f, id);
    assert!(summary.contains("由 MCP 服务器“files”执行，Aura 不检查它做了什么"), "{summary}");
    assert_eq!(marker.calls(), vec![r#"echo {"text":"你好"}"#.to_owned()]);

    // A restart rebuilds the batch from its records and never sends it again.
    let restored = f
        .service()
        .agent_execution_required()
        .resume_batch(batch_id)
        .expect("restored from the records stored with the snapshot");
    assert_eq!(f.execute(restored).await, group);
    assert_eq!(marker.calls().len(), 1, "the server received the call once");
    assert_eq!(f.service().mcp.running_instances("files"), 1);

    // Another session talks to its own instance of the server.
    let other = f.package_session(PermissionMode::WorkspaceReadWrite, &[]);
    let batch = other.admit(&[("mcp.files.echo", json!({"text": "二"}))]);
    let second = batch.batch.calls[0].tool_call_id;
    other.execute(batch).await;
    assert_eq!(other.output(second), json!(format!("{}\n二", header("files", "echo"))));
    assert_eq!(f.service().mcp.running_instances("files"), 2);
}

#[tokio::test]
async fn l08_workspace_and_full_access_do_not_ask_and_read_only_does() {
    for (mode, asks) in [
        (PermissionMode::ReadOnly, true),
        (PermissionMode::WorkspaceReadWrite, false),
        (PermissionMode::FullAccess, false),
    ] {
        let f = Fixture::with_mcp(mode, &[("files", &[])]).await;
        let batch = f.admit(&[("mcp.files.echo", json!({"text": "x"}))]);
        let id = batch.batch.calls[0].tool_call_id;
        if asks {
            let (_, proposal) = pause(&f, batch).await;
            assert_eq!(
                proposal.allowed_decisions,
                vec![ApprovalDecisionKind::AllowForCurrentTurn, ApprovalDecisionKind::Deny],
                "an MCP answer covers at most this turn"
            );
            assert!(!attempted(&f, id), "nothing reached the server before the answer");
        } else {
            f.execute(batch).await;
            assert_eq!(f.output(id), json!(format!("{}\nx", header("files", "echo"))));
        }
    }
}

#[tokio::test]
async fn l08_a_tool_error_is_a_failure_the_model_reads_with_the_data_header() {
    let f = Fixture::with_mcp(PermissionMode::FullAccess, &[("files", &[])]).await;
    let batch = f.admit(&[("mcp.files.fail", json!({}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    let stored = result(&f, id);
    assert!(matches!(stored.outcome, ToolResultOutcomeV1::Failed { .. }), "{stored:?}");
    assert_eq!(stored.side_effect_state, SideEffectStateV1::Unassessed, "the server answered");
    let text = error_text(&f, id);
    assert!(text.starts_with("mcp.tool_error"), "{text}");
    assert!(text.contains(&header("files", "fail")) && text.contains("fixture failure"), "{text}");
}
