use super::super::l03_git_writes::result;
use super::*;
use std::time::Duration;

#[tokio::test]
async fn cancelling_a_native_push_stops_its_hook_tree_and_never_replays_the_attempt() {
    interrupted_push(false).await;
}
#[tokio::test]
async fn revoking_push_session_consent_stops_its_running_native_hook_tree() {
    interrupted_push(true).await;
}
async fn interrupted_push(revoke: bool) {
    let (_temporary, url) = remote();
    let f = Fixture::with_exposure_setup(if revoke { PermissionMode::WorkspaceReadWrite } else { PermissionMode::FullAccess }, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &url]);
        std::fs::write(root.join(".git/hooks/pre-push"), b"#!/bin/sh\nprintf 'run\\n' >>push.calls\nprintf started >push.started\nsleep 30\nprintf finished >push.finished\n").expect("owned hook");
    }).await;
    let batch = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let operation = batch.calls[&id].operation.operation_id;
    let grant = revoke.then(|| approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession));
    let (sender, cancel) = tokio::sync::watch::channel(false);
    let engine = f.service().agent_execution_required().clone();
    let commands = f.service().commands_required().clone();
    let worker = tokio::spawn(async move { engine.execute_batch(batch, commands, cancel).await });
    let started = tokio::time::timeout(Duration::from_secs(90), async {
        while !f.root.join("push.started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if let Some(grant) = grant {
        crate::r03_tests::success(&f.runtime, "security.network_session_consent.revoke", json!({"session_id":f.turn.session_id,"grant_id":grant.grant_id,"expected_revision":grant.revision})).await;
    } else {
        sender.send(true).expect("cancel");
    }
    tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .expect("bounded stop")
        .expect("worker")
        .expect("durable result");
    assert!(started.is_ok(), "the real Native push hook was reached");
    assert!(matches!(result(&f, id).outcome, ToolResultOutcomeV1::Indeterminate { .. }));
    assert_eq!(result(&f, id).side_effect_state, SideEffectStateV1::Unknown);
    assert!(!f.root.join("push.finished").exists());
    let recovered =
        f.service().agent_execution_required().resume_batch(batch_id).expect("durable recovery");
    f.execute_rejecting(recovered).await;
    assert_eq!(std::fs::read(f.root.join("push.calls")).expect("one hook"), b"run\n");
    let store = f.service().broker.lock().expect("attempt ledger");
    let attempts: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM execution_attempts WHERE operation_id=?1",
            [operation.to_string()],
            |row| row.get(0),
        )
        .expect("attempts");
    assert_eq!(attempts, 1);
}
