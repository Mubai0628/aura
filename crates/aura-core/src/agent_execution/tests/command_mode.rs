use super::*;

/// Workspace read-write runs it without asking (`s08::workspace_runs`).
#[tokio::test]
async fn declared_shell_network_asks_before_attempt_in_read_only() {
    for name in ["shell.powershell", "shell.cmd"] {
        let f = Fixture::with_exposure(PermissionMode::ReadOnly, &[name]).await;
        let batch = f.admit(&[(
            name,
            json!({"script":"echo network required","requires_network":true,
            "working_directory":".","stdin":"","timeout_ms":1000,"max_output_bytes":4096}),
        )]);
        let call = batch.batch.calls[0].tool_call_id;
        assert!(matches!(
            f.execute_rejecting(batch).await,
            Some(BatchOutcome::AwaitingApproval { .. })
        ));
        let store = f.service().broker.lock().expect("Agent test setup and persisted assertion");
        let call = aura_persistence_v2::tool::execution::call(store.database.connection(), call)
            .expect("Agent test setup and persisted assertion");
        assert!(call.attempt_id.is_none());
        assert!(call.result.is_none(), "pending approval does not invent a result");
    }
}

#[tokio::test]
async fn full_access_network_is_core_derived_even_without_model_declaration() {
    let f = Fixture::with_exposure(PermissionMode::FullAccess, &["shell.powershell"]).await;
    let batch = f.admit(&[(
        "shell.powershell",
        json!({"script":"echo allowed","requires_network":false,
        "working_directory":".","stdin":"","timeout_ms":1000,"max_output_bytes":4096}),
    )]);
    assert_eq!(batch.calls.len(), 1);
    assert_eq!(
        batch
            .calls
            .values()
            .next()
            .expect("Agent test setup and persisted assertion")
            .operation
            .content
            .network,
        OperationNetwork::BroadProcess
    );
}
