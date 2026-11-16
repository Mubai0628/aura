use super::super::provider_fixture::Scenario;
use super::super::waiting::{prepare_in_mode, start, wait};
use super::*;

const DELETE: Scenario = Scenario::Sequence {
    calls: &[(
        "shell.powershell",
        r#"{"script":"Remove-Item -LiteralPath old.txt -WhatIf; Write-Output did-dispatch"}"#,
    )],
};

#[tokio::test]
async fn a_mode_change_closes_a_pending_shell_deletion_and_resumes_the_turn() {
    for mode in [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(DELETE).await;
        let (runtime, session, model) = prepare_in_mode(&host, &provider, mode).await;
        let target = host.root.path().join("waiting-workspace/old.txt");
        std::fs::write(&target, "keep").expect("deletion fixture");
        let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
        let request =
            AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
        let pending = wait(&runtime, &request, |p| !p.pending_approval_ids.is_empty()).await;
        let approval = json!({"session_id":session,"approval_id":pending.pending_approval_ids[0]});
        let first =
            crate::r03_tests::success(&runtime, "agent.approval.get", approval.clone()).await;
        assert_eq!(first["approval"]["allowed_decisions"], json!(["allow_once", "deny"]));
        let old = crate::r03_tests::success(
            &runtime,
            "permission.get_session_state",
            json!({"session_id":session}),
        )
        .await;
        let elevated = crate::r03_tests::success(&runtime, "permission.set_session_mode", json!({
            "session_id":session,"expected_revision":old["revision"],"mode":PermissionMode::FullAccess,
        })).await;
        let closed =
            crate::r03_tests::success(&runtime, "agent.approval.get", approval.clone()).await;
        assert_eq!(closed["approval"]["state"], "invalidated", "{mode:?}: {closed}");
        crate::r03_tests::success(
            &runtime,
            "permission.set_session_mode",
            json!({
                "session_id":session,"expected_revision":elevated["revision"],"mode":mode,
            }),
        )
        .await;
        assert_resumed(&runtime, &request, &provider).await;
        assert_eq!(std::fs::read_to_string(&target).expect("fixture still exists"), "keep");
    }
}

#[tokio::test]
async fn a_mode_change_while_preparing_an_approval_does_not_leave_a_stale_wait() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(DELETE).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let hold = services.agent_required().store().arm_hold("agent.drive.approval_pause", 30_000);
    let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while !hold.was_reached() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the worker reached the approval transaction");
    change_mode_before_agent_reconciliation(&runtime, session).await;
    hold.release_all();
    assert_resumed(&runtime, &request, &provider).await;
}

#[tokio::test]
async fn a_restart_reconciles_an_approval_left_pending_by_a_mode_change() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(DELETE).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    wait(&runtime, &request, |p| !p.pending_approval_ids.is_empty()).await;
    change_mode_before_agent_reconciliation(&runtime, session).await;
    let runtime = host.restart(runtime).await;
    assert_resumed(&runtime, &request, &provider).await;
}

/// The permission database and Agent store commit separately.
async fn change_mode_before_agent_reconciliation(
    runtime: &crate::RuntimeState,
    session: SessionId,
) {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let old = services.persistence.session_permission_state(session).expect("live authority");
    services
        .persistence
        .change_session_permission_mode(
            session,
            old.revision,
            PermissionMode::FullAccess,
            aura_domain_common::CorrelationId::new(),
        )
        .await
        .expect("mode change committed");
}

async fn assert_resumed(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
    provider: &Provider,
) {
    let completed =
        wait(runtime, request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert!(completed.pending_approval_ids.is_empty());
    assert!(!completed.cancellation_requested);
    let calls = crate::r03_tests::success(
        runtime,
        "agent.tool_activity",
        json!({
            "session_id":request.session_id, "before_call_id":null, "limit":10
        }),
    )
    .await;
    assert!(calls.to_string().contains("未执行：权限模式已变"), "{calls}");
    let summaries = calls["calls"].as_array().expect("tool activity");
    assert_eq!(summaries.len(), 1, "{calls}");
    assert_eq!(summaries[0]["summary"]["side_effect_state"], "not_started", "{calls}");
    assert!(summaries[0]["summary"]["started_at"].is_null(), "{calls}");
    let requests = provider.requests.lock().expect("provider requests");
    assert_eq!(requests.len(), 2, "resume the same turn once");
    let messages =
        requests.last().expect("model resumed")["messages"].as_array().expect("provider messages");
    let result = messages.iter().find(|m| m["role"] == "tool").expect("tool result returned");
    let result: Value = serde_json::from_str(result["content"].as_str().expect("tool text"))
        .expect("tool result JSON");
    assert_eq!(result["outcome"]["type"], "not_executed", "{result}");
    assert_eq!(result["outcome"]["error"]["code"], "tool.approval.invalidated", "{result}");
}
