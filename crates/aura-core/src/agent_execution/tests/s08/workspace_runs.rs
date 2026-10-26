//! Workspace read-write runs commands without asking, network use included; a deletion asks
//! each time, a download asks once per session (one answer covers the session's later downloads,
//! another session asks again), a write outside the workspace asks each time, and Full Access
//! runs everything without asking.
use super::support::*;
use super::*;
use aura_security_contract::NativeShell;

/// Runs one admitted call to its end and returns its output.
async fn run(f: &mut Fixture, tool: &str, arguments: serde_json::Value) -> serde_json::Value {
    let batch = f.admit(&[(tool, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    f.output(id)
}

/// Asks, and runs nothing.
async fn asks(f: &Fixture, tool: &str, arguments: serde_json::Value) {
    let batch = f.admit(&[(tool, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    no_attempt(f, id);
}

#[tokio::test]
async fn s08_workspace_mode_runs_commands_without_asking() {
    let mut f = Fixture::with_exposure(
        PermissionMode::WorkspaceReadWrite,
        &["shell.cmd", "shell.powershell"],
    )
    .await;
    f.service().commands_required().wait_until_ready().await.expect("managed runtime verified");
    assert_eq!(
        run(&mut f, "shell.cmd", arguments("echo first>effect.txt", false)).await["exit_code"],
        0
    );
    let output = run(
        &mut f,
        "shell.powershell",
        arguments("Add-Content -Path effect.txt -Value second", true),
    )
    .await;
    assert_eq!(output["exit_code"], 0, "network use declared, still unasked");
    let written = std::fs::read_to_string(f.root.join("effect.txt")).expect("both commands ran");
    assert!(written.contains("first") && written.contains("second"), "{written}");
}

#[tokio::test]
async fn s08_workspace_mode_asks_each_deletion() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, &["shell.cmd"]).await;
    f.service().commands_required().wait_until_ready().await.expect("managed runtime verified");
    std::fs::write(f.root.join("doomed.txt"), "keep me").expect("a file to delete");
    assert_eq!(run(&mut f, "shell.cmd", arguments("echo allowed", false)).await["exit_code"], 0);
    asks(&f, "shell.cmd", arguments("del doomed.txt", false)).await;
    assert!(f.root.join("doomed.txt").exists());
}

#[tokio::test]
async fn s08_workspace_mode_asks_a_download_once_per_session() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, &["shell.cmd"]).await;
    // Downloads that cannot reach the network here: the workspace is not a repository.
    let batch = f.admit(&[("shell.cmd", arguments("git fetch", false))]);
    let id = batch.batch.calls[0].tool_call_id;
    let proposal = ask(&f, &batch.calls[&id]).await;
    assert_eq!(proposal.requested_capabilities, Some(vec![ResolvedCapability::DownloadArtifact]));
    assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession));
    let offered = proposal.native_session_consent.expect("a session answer is offered");
    assert!(offered.shells.contains(&NativeShell::PowerShell));
    assert!(offered.shells.contains(&NativeShell::Cmd));
    no_attempt(&f, id);
    approve(&f, &batch.calls[&id]).await;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    assert!(f.output(id).get("exit_code").is_some(), "the approved download ran");
    let later = run(&mut f, "shell.cmd", arguments("git pull", false)).await;
    assert!(later.get("exit_code").is_some(), "the session answer covers a later download");
    let other = f.additional_session(PermissionMode::WorkspaceReadWrite, &["shell.cmd"]);
    asks(&other, "shell.cmd", arguments("git fetch", false)).await;
}

#[tokio::test]
async fn s08_workspace_mode_asks_each_write_outside_the_workspace() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, &["shell.cmd"]).await;
    f.service().commands_required().wait_until_ready().await.expect("managed runtime verified");
    // Writing inside the workspace runs.
    assert_eq!(
        run(&mut f, "shell.cmd", arguments("echo inside>inside.txt", false)).await["exit_code"],
        0
    );
    let outside = f.root.parent().expect("a parent folder").join("aura-outside-write.txt");
    let write = format!("echo x>\"{}\"", outside.display());
    let batch = f.admit(&[("shell.cmd", arguments(&write, false))]);
    let id = batch.batch.calls[0].tool_call_id;
    let proposal = ask(&f, &batch.calls[&id]).await;
    assert_eq!(
        proposal.allowed_decisions,
        [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
        "it can only be allowed once, and so is asked each time"
    );
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    no_attempt(&f, id);
    assert!(!outside.exists());
}

#[tokio::test]
async fn s08_full_access_runs_commands_without_asking() {
    let mut f = Fixture::with_exposure(PermissionMode::FullAccess, &["shell.cmd"]).await;
    let batch = f.admit(&[("shell.cmd", arguments("echo free>free.txt", false))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    assert_eq!(f.output(id)["exit_code"], 0);
    f.continue_after_tools(batch_id, &group);
    assert!(f.root.join("free.txt").exists());
}
