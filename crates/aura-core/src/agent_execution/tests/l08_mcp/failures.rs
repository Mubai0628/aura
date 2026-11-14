//! A call the server received and never answered is Unknown: a crash, a timeout or a stop.
//! Recovery reads the stored result and never sends it again; other servers keep working.
use super::support::*;
use super::*;
use crate::agent_execution::tests::fixture::mcp_server_config;

fn attempts(f: &Fixture, id: ToolCallId) -> i64 {
    let store = f.service().broker.lock().expect("attempt ledger");
    store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM execution_attempts WHERE operation_id=(SELECT operation_id FROM tool_calls WHERE id=?1)",
            [id.to_string()],
            |row| row.get(0),
        )
        .expect("attempt count")
}

async fn recover_without_replay(
    f: &Fixture,
    batch_id: ToolBatchId,
    id: ToolCallId,
    marker: &Marker,
) {
    let engine = f.service().agent_execution_required();
    let recovered = engine.resume_batch(batch_id).expect("query the persisted batch");
    let (_sender, cancel) = tokio::sync::watch::channel(false);
    engine
        .execute_batch(recovered, f.service().commands_required().clone(), cancel)
        .await
        .expect("recovery returns the durable result");
    assert_eq!(attempts(f, id), 1, "one durable attempt");
    assert_eq!(marker.calls().len(), 1, "the server received the call once");
}

fn record(f: &Fixture, id: ToolCallId) -> String {
    let store = f.service().broker.lock().expect("journal");
    format!("{:?}", aura_persistence_v2::tool::execution::call(store.database.connection(), id))
}

fn assert_unknown(f: &Fixture, id: ToolCallId, cause: &str) {
    let stored = result(f, id);
    assert!(matches!(stored.outcome, ToolResultOutcomeV1::Indeterminate { .. }), "{stored:?}");
    assert_eq!(stored.side_effect_state, SideEffectStateV1::Unknown);
    let text = error_text(f, id);
    assert!(text.starts_with("mcp.outcome_unknown") && text.contains(cause), "{text}");
    assert!(text.contains("Aura 不会自动重试"), "{text}");
}

#[tokio::test]
async fn l08_a_server_that_exits_mid_call_is_unknown_and_other_servers_keep_working() {
    let marker = Marker::new();
    let f = Fixture::with_mcp(
        PermissionMode::FullAccess,
        &[("files", &marker.arguments()), ("notes", &[])],
    )
    .await;
    let batch =
        f.admit(&[("mcp.files.crash", json!({})), ("mcp.notes.echo", json!({"text": "仍可用"}))]);
    let crashed = batch.batch.calls[0].tool_call_id;
    let other = batch.batch.calls[1].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    // The unknown outcome is a durable terminal: the batch completes and the model reads it.
    let outcome = f.execute_rejecting(batch).await;
    assert!(matches!(outcome, Some(BatchOutcome::Completed(_))), "{}", record(&f, crashed));
    assert_unknown(&f, crashed, "服务器在回应前就结束了");
    assert_eq!(f.output(other), json!(format!("{}\n仍可用", header("notes", "echo"))));
    recover_without_replay(&f, batch_id, crashed, &marker).await;
}

#[tokio::test]
async fn l08_a_call_past_its_timeout_is_unknown_and_never_replayed() {
    let marker = Marker::new();
    let f = Fixture::with_mcp_configs(PermissionMode::FullAccess, |exe| {
        let mut server = mcp_server_config(exe, "files", &marker.arguments());
        server["call_timeout_ms"] = json!(1_000);
        vec![server]
    })
    .await;
    let batch = f.admit(&[("mcp.files.slow", json!({"ms": 20_000}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let began = std::time::Instant::now();
    let outcome = f.execute_rejecting(batch).await;
    assert!(matches!(outcome, Some(BatchOutcome::Completed(_))), "{}", record(&f, id));
    assert!(began.elapsed() < std::time::Duration::from_secs(10), "the timeout ended the wait");
    assert_unknown(&f, id, "服务器没有在 1 秒内回应");
    recover_without_replay(&f, batch_id, id, &marker).await;
}

/// Core's part of a timed-out call is over once its Unknown result is recorded: the rest of the
/// turn goes on, including a call that takes the turn's own state for itself (the tool list).
#[tokio::test]
async fn l08_a_timed_out_call_does_not_hold_the_rest_of_the_turn() {
    let marker = Marker::new();
    let mut f =
        Fixture::with_mcp_configs_exposing(PermissionMode::FullAccess, &["tools.unload"], |exe| {
            let mut server = mcp_server_config(exe, "files", &marker.arguments());
            server["call_timeout_ms"] = json!(1_000);
            vec![server]
        })
        .await;
    let batch = f.admit(&[("mcp.files.slow", json!({"ms": 20_000}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let Some(BatchOutcome::Completed(group)) = f.execute_rejecting(batch).await else {
        panic!("the timed-out call ends the batch: {}", record(&f, id))
    };
    assert_unknown(&f, id, "服务器没有在 1 秒内回应");
    f.continue_after_tools(batch_id, &group);
    let echo = f
        .registry()
        .catalog_entries
        .iter()
        .find(|entry| entry.canonical_name.as_str() == "mcp.files.echo")
        .expect("the server's echo tool")
        .tool_id;
    let unload = f.admit(&[("tools.unload", json!({"tool_ids": [echo]}))]);
    let unloaded = unload.batch.calls[0].tool_call_id;
    let outcome =
        tokio::time::timeout(std::time::Duration::from_secs(15), f.execute_rejecting(unload))
            .await
            .expect("the turn's state is free once the timed-out call is recorded");
    assert!(matches!(outcome, Some(BatchOutcome::Completed(_))), "{}", record(&f, unloaded));
    let stored = result(&f, unloaded);
    assert!(matches!(stored.outcome, ToolResultOutcomeV1::Succeeded), "{stored:?}");
}

#[tokio::test]
async fn l08_stopping_a_running_call_leaves_the_outcome_unknown_and_never_replays() {
    let marker = Marker::new();
    let f = Fixture::with_mcp(PermissionMode::FullAccess, &[("files", &marker.arguments())]).await;
    let batch = f.admit(&[("mcp.files.slow", json!({"ms": 20_000}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let (stop, cancel) = tokio::sync::watch::channel(false);
    let engine = f.service().agent_execution_required().clone();
    let commands = f.service().commands_required().clone();
    let running = tokio::spawn(async move { engine.execute_batch(batch, commands, cancel).await });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while marker.calls().is_empty() {
        assert!(std::time::Instant::now() < until, "the call must reach the server");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    stop.send(true).expect("stop the running batch");
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), running)
        .await
        .expect("the stop ends the call long before the server's own wait")
        .expect("the batch task")
        .expect("the batch answers");
    assert!(matches!(outcome, BatchOutcome::Unknown { .. } | BatchOutcome::Completed(_)));
    assert_unknown(&f, id, "调用被停下了");
    recover_without_replay(&f, batch_id, id, &marker).await;
}
