use super::*;

pub(super) async fn external_read_within_non_recursive_grant(
    relative: &str,
    recursive: bool,
    refused: bool,
) -> String {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let scenario = if refused {
        provider_fixture::Scenario::ExternalReadRefused
    } else {
        provider_fixture::Scenario::ExternalRead
    };
    let provider = Provider::scenario(scenario).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-scope-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-scope");
    let target = external.join(relative);
    std::fs::create_dir_all(target.parent().expect("parent")).expect("external root");
    std::fs::write(&target, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External scope"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let mut record = external_directory_grant(
        session,
        workspace.workspace_id,
        &external,
        vec![aura_permission_domain::ExternalGrantCapability::Read],
    );
    record.recursive_scope = recursive;
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(record, CorrelationId::new())
        .expect("non-recursive directory grant");

    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Read AURA_EXTERNAL_PATH={} and report its first line.",
                target.display()
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
    let done = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    // The decoded message contents, the way the Provider fixture reads them: a joined
    // serialized body would escape the quotes inside a tool result.
    provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|message| message["content"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runs one external-read turn under `mode` with a read grant and returns every
/// request body the Provider saw, joined for inspection.
pub(super) async fn external_read_outcome(mode: PermissionMode) -> String {
    external_read_outcome_for(mode, true).await
}

/// The same without attaching a grant.
pub(super) async fn external_read_outcome_unmoved(mode: PermissionMode) -> String {
    external_read_outcome_for(mode, false).await
}

/// The same read in a mode that must ask: the run waits for the approval, so this
/// returns what the model was sent within a bounded wait instead of a terminal state.
pub(super) async fn external_read_asking_for_approval(mode: PermissionMode) -> String {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalReadMatrix).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-ask-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-ask");
    std::fs::create_dir(&external).expect("external root");
    let external_file = external.join("ask.txt");
    std::fs::write(&external_file, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External ask"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, mode).await;
    crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Read AURA_EXTERNAL_PATH={} and report its first line.",
                external_file.display()
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
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    // The decoded message contents, the way the Provider fixture reads them: a joined
    // serialized body would escape the quotes inside a tool result.
    provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|message| message["content"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) async fn external_read_outcome_for(mode: PermissionMode, granted: bool) -> String {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalReadMatrix).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-matrix-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-matrix");
    std::fs::create_dir(&external).expect("external root");
    let external_file = external.join("matrix.txt");
    std::fs::write(&external_file, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External matrix"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, mode).await;
    if granted {
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
            panic!("Core ready")
        };
        services
            .agent_required()
            .execution
            .external_grants
            .attach_read_only(
                aura_permission_domain::AttachExternalReadOnlyInput {
                    session_id: session,
                    workspace_id: workspace.workspace_id,
                    selected_path: external_file.display().to_string(),
                },
                CorrelationId::new(),
            )
            .expect("attach read-only external grant");
    }
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Read AURA_EXTERNAL_PATH={} and report its first line.",
                external_file.display()
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
    let done = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    assert!(
        matches!(done.turn.state, TurnTaskState::Completed { .. }),
        "{mode:?}: the turn must complete: {:?}",
        done.turn.state
    );
    // The decoded message contents, the way the Provider fixture reads them: a joined
    // serialized body would escape the quotes inside a tool result.
    provider
        .requests
        .lock()
        .expect("requests")
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|message| message["content"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
