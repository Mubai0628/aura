use super::*;

#[tokio::test]
async fn s07_n2_readonly_web_fetch_asks_before_attempt_or_connection() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::ReadOnly, &["web.fetch"]).await;
    let batch =
        f.admit(&[("web.fetch", json!({"url":server.url,"mode":"http_only","max_bytes":4096}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(
        matches!(f.execute_rejecting(batch).await, Some(BatchOutcome::AwaitingApproval { .. })),
        "ReadOnly network must Ask rather than hard-deny"
    );
    no_attempt(&f, id);
    assert_eq!(server.count(), 0);
}

#[tokio::test]
async fn s07_n1_n3_dedicated_http_uses_ordinary_defaults_without_provider_authority() {
    for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess] {
        let server = Server::start().await;
        let f = fixture(mode, &["http.request"]).await;
        let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
        let id = batch.batch.calls[0].tool_call_id;
        let call = batch.calls.get(&id).expect("HTTP call admitted");
        assert!(
            matches!(call.operation.content.source, SecuritySourceRef::BuiltInTool { .. }),
            "model URL cannot manufacture Provider authority"
        );
        f.execute(batch).await;
        assert_eq!(f.output(id)["status"], 200);
        assert_eq!(server.count(), 1);
    }
}

#[tokio::test]
async fn s07_n13_network_session_approval_survives_both_mode_directions() {
    let server = Server::start().await;
    let mut f = fixture(PermissionMode::ReadOnly, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let grant = approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.output(id);
    for mode in [PermissionMode::FullAccess, PermissionMode::ReadOnly] {
        let before = f
            .service()
            .persistence
            .session_permission_state(f.turn.session_id)
            .expect("current mode");
        crate::r03_tests::success(
            &f.runtime,
            "permission.set_session_mode",
            json!({"session_id":f.turn.session_id,"expected_revision":before.revision,"mode":mode}),
        )
        .await;
    }
    f.continue_after_tools(batch_id, &group);
    let next = f.admit(&[("http.request", json!({"url":format!("{}/another",server.url)}))]);
    let next_id = next.batch.calls[0].tool_call_id;
    f.execute(next).await;
    f.output(next_id);
    assert_eq!(server.count(), 2, "all ordinary targets remain covered after mode changes");
    let store = f.service().broker.lock().expect("live grant journal");
    assert!(
        aura_persistence_v2::security::grant(store.database.connection(), grant.grant_id)
            .expect("grant read")
            .expect("grant retained")
            .is_live_at(UtcTimestamp::now())
    );
}
