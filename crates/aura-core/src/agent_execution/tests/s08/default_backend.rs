use super::*;

#[tokio::test]
async fn s08_all_modes_offer_only_the_current_user_compat_backend_by_default() {
    let f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, &["shell.cmd"]).await;
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let candidates = f.service().commands_required().candidates_for(mode);
        assert_eq!(candidates.len(), 1, "one explicit default, independent of R06 installation");
        assert_eq!(candidates[0].descriptor().id.0, "windows-user-compat-v1");
        assert_eq!(candidates[0].descriptor().class, BackendClass::Native);
    }
}

#[tokio::test]
async fn s08_shell_requests_only_the_missing_slices_before_an_attempt() {
    // Read-only asks to run and to use the network; workspace mode runs both and asks only for
    // the download.
    for (mode, script, required) in [
        (
            PermissionMode::ReadOnly,
            "echo pending",
            vec![ResolvedCapability::ProcessExecute, ResolvedCapability::OrdinaryNetwork],
        ),
        (
            PermissionMode::WorkspaceReadWrite,
            "git fetch",
            vec![ResolvedCapability::DownloadArtifact],
        ),
    ] {
        let f = Fixture::with_exposure(mode, &["shell.cmd"]).await;
        let batch = f.admit(&[("shell.cmd", json!({"script":script,"requires_network":true,"working_directory":".","stdin":"","timeout_ms":1000,"max_output_bytes":4096}))]);
        let id = batch.batch.calls[0].tool_call_id;
        match f.execute_rejecting(batch).await.expect("real Core answers") {
            BatchOutcome::AwaitingApproval { proposal, .. } => {
                assert_eq!(proposal.requested_capabilities, Some(required))
            }
            _ => panic!("missing Shell authority must Ask before any process or Attempt"),
        }
        let store = f.service().broker.lock().expect("read the real ledger");
        assert!(
            aura_persistence_v2::tool::execution::call(store.database.connection(), id)
                .expect("admitted call")
                .attempt_id
                .is_none()
        );
    }
}
