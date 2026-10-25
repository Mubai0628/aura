use super::support::*;
use super::*;

#[tokio::test]
async fn s08_readonly_running_answer_allows_real_writes_and_survives_mode_changes() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, &["shell.cmd"]).await;
    let batch = f.admit(&[("shell.cmd", arguments("echo first>effect.txt", false))]);
    let id = batch.batch.calls[0].tool_call_id;
    no_attempt(&f, id);
    let grants = approve(&f, &batch.calls[&id]).await;
    assert_eq!(grants.len(), 1);
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    assert_eq!(f.output(id)["exit_code"], 0);
    assert!(
        std::fs::read_to_string(f.root.join("effect.txt"))
            .expect("actual compat effect")
            .contains("first")
    );
    f.continue_after_tools(batch_id, &group);
    for current in [PermissionMode::WorkspaceReadWrite, PermissionMode::ReadOnly] {
        mode(&f, current).await;
        let batch = f.admit(&[("shell.cmd", arguments("echo reused>>effect.txt", false))]);
        let id = batch.batch.calls[0].tool_call_id;
        let batch_id = batch.batch.tool_batch_id;
        let group = f.execute(batch).await;
        assert_eq!(f.output(id)["exit_code"], 0);
        f.continue_after_tools(batch_id, &group);
    }
    let other = f.additional_session(PermissionMode::ReadOnly, &["shell.cmd"]);
    let batch = other.admit(&[("shell.cmd", arguments("echo other>forbidden.txt", false))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(matches!(
        other.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    no_attempt(&other, id);
    assert!(!f.root.join("forbidden.txt").exists());
}

#[tokio::test]
async fn s08_shell_running_and_network_answers_are_independent_and_separately_revocable() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["shell.cmd"]).await;
    let batch = f.admit(&[("shell.cmd", arguments("echo split>split.txt", true))]);
    let id = batch.batch.calls[0].tool_call_id;
    let grants = approve(&f, &batch.calls[&id]).await;
    assert_eq!(grants.len(), 2, "one question stores two independently revocable answers");
    let network = grants
        .iter()
        .find(|grant| grant.scope.network_consent.is_some())
        .expect("Shell network answer");
    let running = grants.iter().find(|grant| grant.scope.consent.is_some()).expect("run answer");
    assert_eq!(network.scope.capability_limit, Some(vec![ResolvedCapability::OrdinaryNetwork]));
    assert_eq!(running.scope.capability_limit, Some(vec![ResolvedCapability::ProcessExecute]));
    crate::r03_tests::success(&f.runtime,"security.network_session_consent.revoke",json!({"session_id":f.turn.session_id,"grant_id":network.grant_id,"expected_revision":network.revision})).await;
    match f.execute_rejecting(batch).await.expect("real Core answers") {
        BatchOutcome::AwaitingApproval { proposal, .. } => assert_eq!(
            proposal.requested_capabilities,
            Some(vec![ResolvedCapability::OrdinaryNetwork])
        ),
        _ => panic!("only the revoked network capability needs another answer"),
    }
    no_attempt(&f, id);
    assert!(!f.root.join("split.txt").exists());
    let listed = crate::r03_tests::success(
        &f.runtime,
        "security.native_session_consent.list",
        json!({"session_id":f.turn.session_id,"include_inactive":true}),
    )
    .await;
    assert!(listed.to_string().contains("in_force"), "withdrawing networking preserves running");
}
