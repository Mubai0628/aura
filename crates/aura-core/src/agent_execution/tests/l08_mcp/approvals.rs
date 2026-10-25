//! Read-only answers for MCP calls: allowing a server covers its tools for the rest
//! of the turn; denying it refuses its later calls in the turn without asking again. Neither
//! reaches another session or another turn.
use super::support::*;
use super::*;

#[tokio::test]
async fn l08_a_turn_answer_covers_the_servers_tools_for_this_turn_only() {
    let marker = Marker::new();
    let mut f =
        Fixture::with_mcp(PermissionMode::ReadOnly, &[("files", &marker.arguments())]).await;
    let batch = f.admit(&[("mcp.files.echo", json!({"text": "一"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let approved = batch.calls[&id].operation.clone();
    let (approval_id, _) = pause(&f, batch).await;
    answer(&f, approval_id, ApprovalDecisionKind::AllowForCurrentTurn);
    let group = resume(&f, batch_id).await;
    assert_eq!(f.output(id), json!(format!("{}\n一", header("files", "echo"))));
    {
        let store = f.service().broker.lock().expect("journal");
        let grants = aura_persistence_v2::security::grants_for_operation(
            store.database.connection(),
            &approved.fingerprint,
        )
        .expect("grants");
        let [grant] = grants.as_slice() else { panic!("one grant: {grants:?}") };
        let consent = grant.scope.mcp_consent.as_ref().expect("a turn answer for the server");
        assert_eq!(consent.server_id, "files");
        assert_eq!(consent.turn_task_id, f.turn.turn_task_id);
        assert!(matches!(grant.scope.lifetime, AuthorizationLifetime::TurnTask { .. }));
        assert_eq!(grant.expires_at, None, "it ends with the turn, not a clock");
    }
    f.continue_after_tools(batch_id, &group);

    // Another tool of the same server in the same turn does not ask again.
    let again = f.admit(&[("mcp.files.slow", json!({"ms": 10}))]);
    let second = again.batch.calls[0].tool_call_id;
    f.execute(again).await;
    assert_eq!(f.output(second), json!(format!("{}\nslept 10", header("files", "slow"))));
    assert!(pending(&f).is_empty());
    assert_eq!(marker.calls().len(), 2);

    // Another session's turn asks again.
    let other = f.package_session(PermissionMode::ReadOnly, &[]);
    let batch = other.admit(&[("mcp.files.echo", json!({"text": "三"}))]);
    let third = batch.batch.calls[0].tool_call_id;
    assert!(matches!(
        other.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    assert!(!attempted(&other, third));
    assert_eq!(marker.calls().len(), 2, "nothing reached the server without an answer");
}

#[tokio::test]
async fn l08_a_denial_refuses_the_servers_later_calls_in_the_turn_without_asking() {
    let marker = Marker::new();
    let mut f = Fixture::with_mcp(
        PermissionMode::ReadOnly,
        &[("files", &marker.arguments()), ("notes", &[])],
    )
    .await;
    let batch = f.admit(&[("mcp.files.echo", json!({"text": "不"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let (approval_id, _) = pause(&f, batch).await;
    answer(&f, approval_id, ApprovalDecisionKind::Deny);
    let group = resume(&f, batch_id).await;
    assert!(matches!(
        result(&f, id).outcome,
        ToolResultOutcomeV1::NotExecuted { reason: ToolNotExecutedReasonV1::PermissionDenied, .. }
    ));
    f.continue_after_tools(batch_id, &group);

    // The same server again in this turn: refused at once, no new question.
    let again = f.admit(&[("mcp.files.slow", json!({"ms": 10}))]);
    let second = again.batch.calls[0].tool_call_id;
    let second_batch = again.batch.tool_batch_id;
    let Some(BatchOutcome::Completed(group)) = f.execute_rejecting(again).await else {
        panic!("the call is refused without a question");
    };
    assert!(matches!(
        result(&f, second).outcome,
        ToolResultOutcomeV1::NotExecuted { reason: ToolNotExecutedReasonV1::PermissionDenied, .. }
    ));
    assert!(pending(&f).is_empty(), "the denial is not asked again");
    assert!(marker.calls().is_empty(), "nothing reached the denied server");
    f.continue_after_tools(second_batch, &group);

    // Another server is a separate question.
    let other_server = f.admit(&[("mcp.notes.echo", json!({"text": "可以问"}))]);
    assert!(matches!(
        f.execute_rejecting(other_server).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
}
