use super::*;

async fn started(server: &Server) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while server.count() == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the local request starts before changing its authority");
}

#[tokio::test]
async fn s07_n11_explicit_session_approval_keeps_an_inflight_request_across_both_mode_changes() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::ReadOnly, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":format!("{}/slow-body",server.url)}))]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let changes = async {
        started(&server).await;
        for mode in [PermissionMode::FullAccess, PermissionMode::ReadOnly] {
            let before = f
                .service()
                .persistence
                .session_permission_state(f.turn.session_id)
                .expect("current default");
            crate::r03_tests::success(&f.runtime,"permission.set_session_mode",json!({"session_id":f.turn.session_id,"expected_revision":before.revision,"mode":mode})).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(f.execute(batch), changes);
    })
    .await
    .expect("approved request finishes normally");
    assert_eq!(f.output(id)["text"], "partdone");
    assert_eq!(server.count(), 1);
}

#[tokio::test]
async fn s07_n11_revocation_stops_only_the_request_depending_on_the_network_approval() {
    let server = Server::start().await;
    let other = Server::start().await;
    let f = fixture(PermissionMode::ReadOnly, &["http.request"]).await;
    let independent = f.additional_session(PermissionMode::WorkspaceReadWrite, &["http.request"]);
    assert!(Arc::ptr_eq(&f.runtime, &independent.runtime), "N11 sessions must share one real Core");
    assert_eq!(f.turn.workspace_id, independent.turn.workspace_id);
    assert_ne!(f.turn.session_id, independent.turn.session_id);
    let batch = f.admit(&[("http.request", json!({"url":format!("{}/slow",server.url)}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let grant = approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let other_batch =
        independent.admit(&[("http.request", json!({"url":format!("{}/slow-body",other.url)}))]);
    let other_id = other_batch.batch.calls[0].tool_call_id;
    let revoke = async {
        started(&server).await;
        started(&other).await;
        crate::r03_tests::success(&f.runtime,"security.network_session_consent.revoke",json!({"session_id":f.turn.session_id,"grant_id":grant.grant_id,"expected_revision":grant.revision})).await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(f.execute_rejecting(batch), independent.execute(other_batch), revoke);
    })
    .await
    .expect("revocation confirms the local request stopped within its budget");
    let store = f.service().broker.lock().expect("terminal stopped result");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("call record");
    assert!(!matches!(call.result.expect("paired result").outcome, ToolResultOutcomeV1::Succeeded));
    assert_eq!(other.count(), 1, "another session remains runnable");
    drop(store);
    assert_eq!(
        independent.output(other_id)["text"],
        "partdone",
        "the other session's already-running request finishes normally"
    );
}
