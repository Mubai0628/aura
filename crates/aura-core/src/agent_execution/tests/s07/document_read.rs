use super::*;

#[tokio::test]
async fn s07_n2_once_fetch_allows_repeated_session_document_reads_without_more_network() {
    let server = Server::start().await;
    let mut f = fixture(PermissionMode::ReadOnly, &["web.fetch", "web.read"]).await;
    let batch =
        f.admit(&[("web.fetch", json!({"url":format!("{}/page",server.url),"mode":"http_only"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&call], ApprovalDecisionKind::AllowOnce);
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let artifact = f.output(call)["document_artifact_id"].clone();
    assert!(artifact.is_string());
    f.continue_after_tools(batch_id, &group);
    for _ in 0..2 {
        let batch = f.admit(&[("web.read", json!({"document_artifact_id":artifact}))]);
        let call = batch.batch.calls[0].tool_call_id;
        let batch_id = batch.batch.tool_batch_id;
        let group = f.execute(batch).await;
        f.output(call);
        f.continue_after_tools(batch_id, &group);
    }
    assert_eq!(server.count(), 1, "reading a cached page does not connect");
    let foreign = fixture(PermissionMode::FullAccess, &["web.read"]).await;
    let read = foreign.admit(&[("web.read", json!({"document_artifact_id":artifact}))]);
    let foreign_id = read.batch.calls[0].tool_call_id;
    foreign.execute_rejecting(read).await;
    no_attempt(&foreign, foreign_id);
    assert_eq!(server.count(), 1, "another session cannot import the cached document");
    let next =
        f.admit(&[("web.fetch", json!({"url":format!("{}/page",server.url),"mode":"http_only"}))]);
    let id = next.batch.calls[0].tool_call_id;
    assert!(matches!(f.execute_rejecting(next).await, Some(BatchOutcome::AwaitingApproval { .. })));
    no_attempt(&f, id);
    assert_eq!(server.count(), 1, "Once never authorizes another fetch");
}

#[tokio::test]
async fn s07_n5_only_registered_control_endpoint_is_blocked_and_unregister_releases_it() {
    let server = Server::start().await;
    let address = server.url.trim_start_matches("http://").parse().expect("bound test endpoint");
    let registration = aura_network_control::ordinary_tools::register_control_endpoint(
        aura_domain_common::RuntimeInstanceId::new(),
        address,
    )
    .expect("synthetic Core registration");
    for url in [server.url.clone(), server.url.replace("127.0.0.1", "localhost")] {
        let f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
        let batch = f.admit(&[("http.request", json!({"url":url}))]);
        let id = batch.batch.calls[0].tool_call_id;
        f.execute_rejecting(batch).await;
        no_attempt(&f, id);
        let store = f.service().broker.lock().expect("named control refusal");
        let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("refused call");
        assert!(
            serde_json::to_string(&call.result)
                .expect("safe refusal")
                .contains("tool.runtime.http_control_endpoint_denied")
        );
    }
    assert_eq!(server.count(), 0, "registered listener is never contacted");
    let ordinary = Server::start().await;
    let f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":ordinary.url}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    f.output(id);
    assert_eq!(ordinary.count(), 1, "other local listeners remain normal destinations");
    drop(registration);
    let f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    f.output(id);
    assert_eq!(server.count(), 1, "dropped runtime registration does not poison port reuse");
}
