use super::support::*;
use super::*;
use aura_windows_user_contract::{BackgroundState, ShellExecutionView, ShellQueryInput};
use std::time::Duration;

#[tokio::test]
async fn s08_stop_without_a_live_job_or_durable_confirmation_reports_unknown() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["shell.cmd"]).await;
    assert!(
        f.service().commands_required().stop_operation(SecurityOperationId::new()).await.is_err(),
        "absence of an owned Job is not evidence that a process tree has stopped"
    );
}

fn view(f: &Fixture, id: ToolCallId) -> ShellExecutionView {
    aura_persistence_v2::execution::shell::query(
        f.service().broker.lock().expect("read the execution journal").database.connection(),
        f.turn.session_id,
        &ShellQueryInput { tool_call_id: id, stdout_offset: 0, stderr_offset: 0, max_bytes: 4096 },
    )
    .expect("the real Shell execution projection")
}

async fn background(f: &mut Fixture, network: bool) -> ToolCallId {
    let batch = f.admit(&[("shell.cmd", arguments(
        "start \"\" /b cmd /d /c \"echo child>child-start.txt & ping -n 20 127.0.0.1 >nul & echo late>late.txt\"\necho foreground",
        network))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    if f.service().persistence.session_permission_state(f.turn.session_id).expect("default").mode
        == PermissionMode::ReadOnly
    {
        approve(f, &batch.calls[&id]).await;
    }
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if view(f, id).background == Some(BackgroundState::Running)
                && f.root.join("child-start.txt").exists()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a descendant is registered after its foreground completes");
    id
}

async fn stopped(f: &Fixture, id: ToolCallId) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while view(f, id).background != Some(BackgroundState::Interrupted) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Job emptiness produces the durable stopped background state");
    assert!(
        f.service()
            .commands_required()
            .managed_operations(f.turn.session_id)
            .expect("real active Job owners")
            .is_empty()
    );
    assert!(
        !f.root.join("late.txt").exists(),
        "the stopped descendant cannot publish its later effect"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_user_stop_works_in_readonly_after_the_foreground_has_completed() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, &["shell.cmd"]).await;
    let id = background(&mut f, false).await;
    let result = f.service().commands_required().stop_call(f.turn.session_id, id).await;
    assert!(result.is_ok(), "the user's stop needs no model approval: {result:?}");
    stopped(&f, id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_tightening_stops_only_a_background_job_without_explicit_running_authority() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, &["shell.cmd"]).await;
    let mut approved = f.additional_session(PermissionMode::ReadOnly, &["shell.cmd"]);
    let approved_id = background(&mut approved, false).await;
    let default_id = background(&mut f, false).await;
    mode(&f, PermissionMode::ReadOnly).await;
    stopped(&f, default_id).await;
    let still_running = view(&approved, approved_id).background;
    approved
        .service()
        .commands_required()
        .stop_call(approved.turn.session_id, approved_id)
        .await
        .expect("clean up the explicitly approved Job");
    assert_eq!(
        still_running,
        Some(BackgroundState::Running),
        "another session's approval remains effective in the same Core"
    );
    stopped(&approved, approved_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_a_running_session_answer_survives_modes_and_revocation_stops_its_job() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, &["shell.cmd"]).await;
    let id = background(&mut f, false).await;
    mode(&f, PermissionMode::WorkspaceReadWrite).await;
    mode(&f, PermissionMode::ReadOnly).await;
    assert_eq!(
        view(&f, id).background,
        Some(BackgroundState::Running),
        "D2 protects the explicitly covered running Job"
    );
    let rows = crate::r03_tests::success(
        &f.runtime,
        "security.native_session_consent.list",
        json!({"session_id":f.turn.session_id,"include_inactive":true}),
    )
    .await;
    let row = &rows.as_array().expect("run approvals")[0];
    crate::r03_tests::success(&f.runtime,"security.native_session_consent.revoke",json!({"session_id":f.turn.session_id,"grant_id":row["grant_id"],"expected_revision":row["grant_revision"],"correlation_id":CorrelationId::new()})).await;
    stopped(&f, id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_revoking_network_stops_its_job_and_keeps_the_running_answer() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, &["shell.cmd"]).await;
    let id = background(&mut f, true).await;
    let rows = crate::r03_tests::success(
        &f.runtime,
        "security.network_session_consent.list",
        json!({"session_id":f.turn.session_id,"include_inactive":true}),
    )
    .await;
    let row = &rows.as_array().expect("independent network approvals")[0];
    crate::r03_tests::success(
        &f.runtime,
        "security.network_session_consent.revoke",
        json!({"session_id":f.turn.session_id,"grant_id":row["grant_id"],"expected_revision":row["grant_revision"]}),
    )
    .await;
    stopped(&f, id).await;
    let running = crate::r03_tests::success(
        &f.runtime,
        "security.native_session_consent.list",
        json!({"session_id":f.turn.session_id,"include_inactive":true}),
    )
    .await;
    assert_eq!(
        running[0]["availability"], "in_force",
        "network revocation does not revoke running"
    );
}
