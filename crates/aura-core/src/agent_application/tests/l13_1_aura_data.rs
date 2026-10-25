//! A model reaching into Aura's own data folders asks
//! every time, like the user's credential stores - even in FullAccess, where an ordinary external
//! file is read by the policy.
use super::*;

#[tokio::test]
async fn a_file_in_auras_own_data_asks_even_in_full_access() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalReadMatrix).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("aura-data-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    // The fixture's Core keeps its data here (`AuraPaths::from_roots(.., root/local)`).
    let logs = host.root.path().join("local").join("logs");
    std::fs::create_dir_all(&logs).expect("Aura's log folder");
    let inside = logs.join("note.txt");
    std::fs::write(&inside, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("a file in Aura's data");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Aura data"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Read AURA_EXTERNAL_PATH={} and report its first line.",
                inside.display()
            ),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("start");
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    let waiting = wait(&runtime, &request, |p| !p.pending_approval_ids.is_empty()).await;
    let approval = crate::r03_tests::success(
        &runtime,
        "agent.approval.get",
        json!({"session_id": session, "approval_id": waiting.pending_approval_ids[0]}),
    )
    .await;
    assert!(
        approval.to_string().contains("sensitive_user_path"),
        "it asks as a protected path: {approval}"
    );
    let read = provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .any(|body| body.to_string().contains(provider_fixture::EXTERNAL_BODY_TOKEN));
    assert!(!read, "nothing in Aura's data reaches the model before the user answers");
}
