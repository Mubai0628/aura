//! Production model-loop coverage for legacy non-recursive directory approvals.
use super::*;

/// What one `fs.list` of an external path produced, as the model saw it.
pub(in crate::agent_application::tests) struct ExternalListOutcome {
    /// Everything the Provider saw in tool results, joined for inspection.
    pub(in crate::agent_application::tests) results: String,
    /// The turn reached a terminal state.
    pub(in crate::agent_application::tests) finished: bool,
    pub(in crate::agent_application::tests) asked: bool,
    pub(in crate::agent_application::tests) attempts: i64,
    pub(in crate::agent_application::tests) requested_target:
        Option<aura_security_contract::FileConsentTarget>,
}

/// Lists one external path through the real model loop under a non-recursive grant.
///
/// The grant is written the way the approval flow writes it. The listing target is given
/// by the test, so the same walk can be asked for the grant root (a control) and for one
/// of its direct children (the boundary).
pub(in crate::agent_application::tests) async fn external_list_under_non_recursive_grant(
    external: &std::path::Path,
    target: &std::path::Path,
    mode: PermissionMode,
) -> ExternalListOutcome {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalListOutcome).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-scope-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External scope"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, mode).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let identity = aura_windows_platform::inspect_existing_path(external, true)
        .expect("external directory identity");
    let mut record = external_directory_grant(
        session,
        workspace.workspace_id,
        external,
        vec![aura_permission_domain::ExternalGrantCapability::Read],
    );
    // The confirmed boundary: the directory and its direct children, nothing deeper.
    record.recursive_scope = false;
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(record, CorrelationId::new())
        .expect("non-recursive directory grant");
    let _ = identity;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!("List AURA_EXTERNAL_PATH={} and report its entries.", target.display()),
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
    let done = super::super::waiting::wait(&runtime, &request, |view| {
        view.turn.state.terminal() || !view.pending_approval_ids.is_empty()
    })
    .await;
    let asked = !done.pending_approval_ids.is_empty();
    let requested_target = if let Some(id) = done.pending_approval_ids.first() {
        let view = crate::r03_tests::success(
            &runtime,
            "agent.approval.get",
            json!({"session_id":session,"approval_id":id}),
        )
        .await;
        let view: aura_execution_contract::ExecutionApprovalView =
            serde_json::from_value(view).expect("the real pending file approval is readable");
        view.operation
            .content
            .resources
            .iter()
            .find_map(aura_security_contract::file_consent_target_of)
    } else {
        None
    };
    let attempts = services
        .broker
        .lock()
        .expect("inspect this isolated Core's journal")
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM execution_attempts a JOIN operations o ON o.id=a.operation_id WHERE json_extract(o.payload,'$.content.action_kind')='fs.list'", [], |row| row.get::<_, i64>(0))
        .expect("count file enumeration attempts separately from registry control calls");
    let results = provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|message| message["role"] == "tool")
                .filter_map(|message| message["content"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    ExternalListOutcome {
        results,
        finished: done.turn.state.terminal(),
        asked,
        attempts,
        requested_target,
    }
}
