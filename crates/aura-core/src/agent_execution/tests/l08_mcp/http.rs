//! An HTTP server's tool through the real Agent path. The question is the same as for a
//! program on this computer (read-only: once per turn and server; read-write: none); the
//! request goes through Core's network gateway to exactly the trusted address, and an answer
//! that broke off is unknown and never sent again.
use super::support::*;
use super::*;
use crate::mcp::tests::http_server::HttpServer;

fn http_config(id: &str, url: &str) -> serde_json::Value {
    json!({
        "id": id,
        "display_name": format!("测试 {id}"),
        "transport": {"type": "http", "url": url},
        "startup_timeout_ms": 10_000,
        "call_timeout_ms": 10_000,
    })
}

#[tokio::test]
async fn l08_http_server_tool_is_asked_once_per_turn_in_read_only() {
    let folder = tempfile::tempdir().expect("server folder");
    let server = HttpServer::start(folder.path());
    let url = server.url.clone();
    let mut f =
        Fixture::with_mcp_configs(PermissionMode::ReadOnly, |_| vec![http_config("web", &url)])
            .await;
    let batch = f.admit(&[("mcp.web.echo", json!({"text": "一"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let content = &batch.calls[&id].operation.content;
    assert_eq!(content.source, SecuritySourceRef::McpServer { server_id: "web".into() });
    assert_eq!(content.capabilities, vec![ResolvedCapability::ExternalExtensionCall]);
    assert_eq!(content.network, OperationNetwork::None, "the trusted address is the server");
    let (approval_id, _) = pause(&f, batch).await;
    assert_eq!(server.count("call"), 0, "nothing reached the server before the answer");
    answer(&f, approval_id, ApprovalDecisionKind::AllowForCurrentTurn);
    let group = resume(&f, batch_id).await;
    assert_eq!(f.output(id), json!(format!("{}\n一", header("web", "echo"))));
    f.continue_after_tools(batch_id, &group);

    // The server's other tool in the same turn: no new question, an event-stream answer.
    let again = f.admit(&[("mcp.web.stream", json!({"text": "二"}))]);
    let second = again.batch.calls[0].tool_call_id;
    f.execute(again).await;
    assert_eq!(
        f.output(second),
        json!(format!("{}\nstreamed 二 (ping answered: true)", header("web", "stream")))
    );
    assert!(pending(&f).is_empty());
    assert_eq!(server.count("call"), 2);
}

#[tokio::test]
async fn l08_http_server_tool_runs_without_a_question_in_read_write_and_a_broken_answer_is_unknown()
{
    let folder = tempfile::tempdir().expect("server folder");
    let server = HttpServer::start(folder.path());
    let url = server.url.clone();
    let f = Fixture::with_mcp_configs(PermissionMode::WorkspaceReadWrite, |_| {
        vec![http_config("web", &url)]
    })
    .await;
    let batch = f.admit(&[("mcp.web.echo", json!({"text": "直接"})), ("mcp.web.drop", json!({}))]);
    let (echo, dropped) = (batch.batch.calls[0].tool_call_id, batch.batch.calls[1].tool_call_id);
    f.execute(batch).await;
    assert_eq!(f.output(echo), json!(format!("{}\n直接", header("web", "echo"))));
    assert!(pending(&f).is_empty());
    let stored = result(&f, dropped);
    assert!(matches!(stored.outcome, ToolResultOutcomeV1::Indeterminate { .. }), "{stored:?}");
    assert_eq!(stored.side_effect_state, SideEffectStateV1::Unknown);
    let text = error_text(&f, dropped);
    assert!(text.starts_with("mcp.outcome_unknown") && text.contains("不会自动重试"), "{text}");
    assert_eq!(server.count("call drop"), 1, "never sent again");
}
