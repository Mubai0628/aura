use super::*;

#[tokio::test]
async fn s09_changed_credential_requirements_explain_rebinding_before_approval_or_attempt() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::ReadOnly, &["http.request"]).await;
    let entry = aura_tool_registry::agent_release::release()
        .expect("current release")
        .entry("http.request")
        .expect("HTTP entry")
        .clone();
    let db = rusqlite::Connection::open(f.service().persistence.database_path())
        .expect("isolated configuration store");
    db.execute(
        "INSERT INTO builtin_credential_rebind_required VALUES(?1,?2)",
        rusqlite::params![entry.instance.tool_instance_id.to_string(), "credential_slot_changed"],
    )
    .expect("isolated persisted requirement-change marker");
    drop(db);
    let batch = f.admit(&[("http.request", json!({"url":server.url,"credential":"api_key"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(batch.calls.is_empty(), "the refusal precedes permission approval");
    f.execute_rejecting(batch).await;
    no_attempt(&f, id);
    assert_eq!(server.count(), 0, "no request with an incompatible binding");
    let store = f.service().broker.lock().expect("durable refusal");
    let result = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("call")
        .result
        .expect("paired refusal");
    let ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } = result.outcome else {
        panic!("expected a pre-dispatch refusal explaining recovery");
    };
    assert!(
        error.message.contains("凭据要求")
            && error.message.contains("工具设置")
            && error.message.contains("重新绑定"),
        "the model must receive the cause and required user action: {}",
        error.message
    );
}
