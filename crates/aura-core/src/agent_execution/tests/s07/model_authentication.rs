use super::*;

async fn refuses_authentication(mode: PermissionMode, body: bool) {
    let server = Server::start().await;
    let f = fixture(mode, &["http.request"]).await;
    let canary = "sk-s07-authentication-canary-0123456789";
    let inputs = if body {
        vec![
            json!({"url":server.url,"method":"POST","body":{"kind":"text","text":canary}}),
            json!({"url":server.url,"method":"POST","body":{"kind":"json","value":{"copied":canary}}}),
        ]
    } else {
        [
            "Authorization",
            "Proxy-Authorization",
            "Cookie",
            "Set-Cookie",
            "Cookie2",
            "X-API-Key",
            "Api-Key",
            "X-Auth-Token",
            "X-Access-Token",
        ]
        .into_iter()
        .map(|name| json!({"url":server.url,"headers":[{"name":name,"value":"synthetic-auth"}]}))
        .collect()
    };
    let calls = inputs.into_iter().map(|input| ("http.request", input)).collect::<Vec<_>>();
    let batch = f.admit(&calls);
    let ids = batch.batch.calls.iter().map(|call| call.tool_call_id).collect::<Vec<_>>();
    let outcome = f.execute_rejecting(batch).await.expect("real batch outcome");
    assert!(
        matches!(outcome, BatchOutcome::Completed(_)),
        "authentication refusal must finish without requesting approval"
    );
    assert_eq!(server.count(), 0, "the local service receives no request or authentication");
    let store = f.service().broker.lock().expect("inspect the real preflight journal");
    let db = store.database.connection();
    let approvals: i64 = db
        .query_row("SELECT count(*) FROM approval_requests", [], |row| row.get(0))
        .expect("count durable approval requests");
    assert_eq!(approvals, 0, "no approval card is created");
    for id in ids {
        let call = aura_persistence_v2::tool::execution::call(db, id).expect("durable refusal");
        assert!(call.attempt_id.is_none(), "model authentication cannot reserve an Attempt");
        let ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } =
            call.result.expect("paired result").outcome
        else {
            panic!("authentication must be a preflight refusal")
        };
        assert_eq!(error.code.as_str(), "tool.resource.http_model_authentication_forbidden");
        assert!(
            error.message.contains("认证信息只能通过工具凭据提供（工具设置里配置凭据与允许网站）")
        );
        assert!(!serde_json::to_string(&error).expect("safe error envelope").contains(canary));
    }
}

#[tokio::test]
async fn s07_f5_readonly_authentication_headers_never_ask_or_attempt() {
    refuses_authentication(PermissionMode::ReadOnly, false).await;
}

#[tokio::test]
async fn s07_f5_readonly_secret_bodies_never_ask_or_attempt() {
    refuses_authentication(PermissionMode::ReadOnly, true).await;
}

#[tokio::test]
async fn s07_f5_full_access_cannot_bypass_model_authentication_refusal() {
    refuses_authentication(PermissionMode::FullAccess, false).await;
}

#[tokio::test]
async fn s07_f5_a_named_api_is_refused_before_approval_and_before_dns() {
    // Real APIs use host names. `.invalid` never resolves (RFC 6761), so the refusal must come
    // from planning, not from the address check that runs early only for literal addresses.
    let f = fixture(PermissionMode::ReadOnly, &["http.request"]).await;
    let headers = json!([{"name":"Authorization","value":"synthetic-auth"}]);
    let batch = f.admit(&[(
        "http.request",
        json!({"url":"https://s07-model-auth.invalid/v1/items","headers":headers}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(
        matches!(f.execute_rejecting(batch).await, Some(BatchOutcome::Completed(_))),
        "the user is not asked to approve a request that cannot be sent"
    );
    let store = f.service().broker.lock().expect("inspect the real preflight journal");
    let db = store.database.connection();
    let approvals: i64 = db
        .query_row("SELECT count(*) FROM approval_requests", [], |row| row.get(0))
        .expect("count durable approval requests");
    assert_eq!(approvals, 0, "no approval card is created");
    let call = aura_persistence_v2::tool::execution::call(db, id).expect("durable refusal");
    assert!(call.attempt_id.is_none(), "no Attempt is reserved");
    let ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } =
        call.result.expect("paired result").outcome
    else {
        panic!("a named target must be refused in planning like a literal address")
    };
    assert_eq!(error.code.as_str(), "tool.resource.http_model_authentication_forbidden");
}
