use super::*;

#[tokio::test]
async fn missing_file_returns_a_normal_operation_rejection() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.read"]).await;
    let batch =
        f.admit(&[("fs.read", json!({"path":"missing.txt","range":{"range_kind":"full"}}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let result = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("Agent test setup and persisted assertion")
        .result
        .expect("Agent test setup and persisted assertion");
    let ToolResultOutcomeV1::OperationRejected { error } = result.outcome else {
        panic!("normal read refusal required: {:?}", result.outcome)
    };
    assert_eq!(error.category, ErrorCategory::NotFound);
    assert_eq!(error.code.as_str(), "fs.not_found");
    assert!(!error.retryable);
}

#[tokio::test]
async fn access_denied_returns_rejection_without_reading_or_reporting_infrastructure_failure() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.read"]).await;
    let path = f.root.join("denied.txt");
    std::fs::write(&path, "PRIVATE-READ-CANARY").expect("Agent test setup and persisted assertion");
    let object = aura_windows_platform::ExecutionFileSecurity::open(&path, false)
        .expect("Agent test setup and persisted assertion");
    let before = object.capture().expect("Agent test setup and persisted assertion");
    let owner = aura_windows_platform::current_user_token_identity()
        .expect("Agent test setup and persisted assertion")
        .0;
    let control =
        if before.split('(').next().is_some_and(|v| v.contains("AI")) { "PAI" } else { "P" };
    let proposed = format!("D:{control}(D;;FR;;;{owner})(D;;FR;;;WD)(A;;WD;;;{owner})(A;;FA;;;SY)");
    let denied = apply_denial(&object, &before, &proposed);
    let batch = f.admit(&[("fs.read", json!({"path":"denied.txt","range":{"range_kind":"full"}}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    object.apply_exact(&denied, &before).expect("Agent test setup and persisted assertion");
    let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
    let result = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("Agent test setup and persisted assertion")
        .result
        .expect("Agent test setup and persisted assertion");
    let ToolResultOutcomeV1::OperationRejected { error } = result.outcome else {
        panic!("normal permission refusal required: {:?}", result.outcome)
    };
    assert_eq!(error.category, ErrorCategory::Permission);
    assert!(!error.message.contains("PRIVATE-READ-CANARY"));
    assert_eq!(
        std::fs::read_to_string(path).expect("Agent test setup and persisted assertion"),
        "PRIVATE-READ-CANARY"
    );
}

fn apply_denial(
    object: &aura_windows_platform::ExecutionFileSecurity,
    before: &str,
    proposed: &str,
) -> String {
    match object.apply_exact(before, proposed) {
        Ok(()) => proposed.to_string(),
        Err(error) => {
            let actual = object.capture().expect("Agent test setup and persisted assertion");
            assert_ne!(
                actual, before,
                "synthetic denial setup: {error}; expected {proposed}; actual {actual}"
            );
            actual
        }
    }
}
