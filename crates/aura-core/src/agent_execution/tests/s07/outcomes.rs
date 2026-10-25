use super::*;

#[tokio::test]
async fn s07_n9_post_disconnect_is_not_automatically_replayed() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":format!("{}/disconnect",server.url),"method":"POST","body":{"kind":"text","text":"change"}}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute_rejecting(batch).await;
    assert_eq!(server.count(), 1, "possibly sent mutation must not retry");
    let store = f.service().broker.lock().expect("durable mutation outcome");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("mutation call");
    assert!(call.attempt_id.is_some());
    let result = call.result.expect("paired unknown remote effect");
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Indeterminate { .. }));
    assert_eq!(result.side_effect_state, aura_execution_contract::SideEffectStateV1::Unknown);
}

#[tokio::test]
async fn s07_n9_post_redirect_is_the_http_result_and_is_never_followed() {
    let first = Server::start().await;
    let second = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch = f.admit(&[(
        "http.request",
        json!({
            "url": format!("{}/redirect/{}", first.url, second.url),
            "method": "POST",
            "body": {"kind": "text", "text": "one change"}
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    assert_eq!(f.output(id)["status"], 302, "a mutating request reports the actual redirect");
    assert_eq!(first.count(), 1);
    assert_eq!(second.count(), 0, "a POST is never re-sent to the redirect target");
}

#[tokio::test]
async fn s07_n9_response_limit_refuses_the_body_and_the_session_can_continue() {
    let server = Server::start().await;
    let mut f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch =
        f.admit(&[("http.request", json!({"url":format!("{}/binary/9437184",server.url)}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    {
        let store = f.service().broker.lock().expect("bounded response refusal");
        let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("refused call");
        assert!(call.attempt_id.is_some());
        assert!(!matches!(
            call.result.expect("limit result").outcome,
            ToolResultOutcomeV1::Succeeded
        ));
    }
    f.continue_after_tools(batch_id, &group);
    let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    f.output(id);
    assert_eq!(server.count(), 2);
}

#[tokio::test]
async fn s07_n10_unbound_credential_refuses_before_attempt_and_request() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":server.url,"credential":"not-bound"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute_rejecting(batch).await;
    no_attempt(&f, id);
    assert_eq!(server.count(), 0);
}

#[tokio::test]
async fn s07_n11_tightening_stops_a_request_without_explicit_approval() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":format!("{}/slow",server.url)}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let execution = f.execute_rejecting(batch);
    let tighten = async {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while server.count() == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("request starts before mode transition");
        let state =
            f.service().persistence.session_permission_state(f.turn.session_id).expect("live mode");
        crate::r03_tests::success(&f.runtime,"permission.set_session_mode",json!({"session_id":f.turn.session_id,"expected_revision":state.revision,"mode":"read_only"})).await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::join!(execution, tighten);
    })
    .await
    .expect("lost authority cancels the slow request within its stop budget");
    let store = f.service().broker.lock().expect("stopped call journal");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("stopped call");
    assert!(!matches!(call.result.map(|v| v.outcome), Some(ToolResultOutcomeV1::Succeeded)));
}
