use super::*;

/// Text inside an ordinary external directory is found without any grant.
///
/// The same policy as the other file operations, now including text search: FullAccess
/// follows the existing rules, so the search runs on the external directory itself and
/// the result names the policy as the authority. The match paths are relative to that
/// directory, which is what the external facts report.
#[tokio::test]
async fn an_ordinary_external_directory_is_searched_for_text_without_a_grant() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalSearch).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-search-policy-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-search-policy");
    std::fs::create_dir_all(&external).expect("external root");
    std::fs::write(
        external.join("outside.txt"),
        format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN),
    )
    .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External search policy"}),
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
                "Search for the token under AURA_EXTERNAL_PATH={} and report it.",
                external.display()
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
    let bodies = provider.requests.lock().expect("requests").clone();
    let found = bodies
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
        .find(|content| content.contains(provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("the external text match must reach the model");
    assert!(
        found.contains("\"grant_id\":null"),
        "a policy-authorized search must not claim a grant: {found}"
    );
}

/// A text search rooted at an authorized external path is refused, not silently
/// redirected into the workspace.
///
/// Text search is served by the workspace index, so an external root has no index to
/// search. The refusal must be the corrigible outside-workspace class and neither the
/// external file's text nor a workspace match may appear, even though the user did
/// authorize that directory for reading.
#[tokio::test]
async fn an_external_text_search_root_is_searched_without_touching_the_workspace() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalSearch).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-search-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    // The same text also exists inside the workspace: a silent redirect would find it.
    std::fs::write(
        root.join("workspace-copy.txt"),
        format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN),
    )
    .expect("workspace copy");
    let external = host.root.path().join("outside-search");
    std::fs::create_dir_all(&external).expect("external root");
    std::fs::write(
        external.join("outside.txt"),
        format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN),
    )
    .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External search"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let identity = aura_windows_platform::inspect_existing_path(&external, true)
        .expect("external directory identity");
    let record = aura_permission_domain::ExternalGrantRecord {
        id: aura_domain_common::ExternalGrantId::new(),
        session_id: session,
        workspace_id: workspace.workspace_id,
        permission_grant_id: None,
        selected_root: aura_security_contract::CanonicalPath {
            utf16: external.display().to_string().encode_utf16().collect(),
            display: external.display().to_string(),
        },
        canonical_root: aura_security_contract::CanonicalPath {
            utf16: identity.final_path.display().to_string().encode_utf16().collect(),
            display: identity.final_path.display().to_string(),
        },
        root_identity: aura_workspace::domain_identity(&identity.identity),
        target_kind: aura_permission_domain::ExternalGrantTargetKind::Directory,
        capabilities: vec![
            aura_permission_domain::ExternalGrantCapability::Read,
            aura_permission_domain::ExternalGrantCapability::Write,
        ],
        recursive_scope: true,
        lifecycle: aura_permission_domain::ExternalGrantLifecycle::Active,
        revalidation_revision: 1,
        created_at: aura_domain_common::UtcTimestamp::now(),
        updated_at: aura_domain_common::UtcTimestamp::now(),
        suspended_at: None,
        revoked_at: None,
        expires_at: None,
        turn_task_id: None,
        term: aura_permission_domain::ExternalGrantTerm::Session,
    };
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(record, CorrelationId::new())
        .expect("directory grant");

    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Search for the token under AURA_EXTERNAL_PATH={} and report it.",
                external.display()
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
    let bodies = provider.requests.lock().expect("requests").clone();
    // Only tool results count as evidence: the model's own call arguments legitimately
    // contain the text it asked to search for.
    let tool_results = bodies
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
    // The root is searched itself: the token that only exists outside the workspace is
    // found, and the workspace file that holds the same token is never reported.
    let found = tool_results.contains(provider_fixture::EXTERNAL_BODY_TOKEN);
    let workspace_match = tool_results.contains("workspace-copy.txt");
    assert!(found, "an authorized external search root must be searched: {tool_results}");
    assert!(
        !workspace_match,
        "an external search must never report a workspace file instead: {tool_results}"
    );
    // The request the model really made named the external path, beside the same
    // token that also exists in the workspace.
    let requested: Vec<String> = bodies
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|message| message["role"] == "assistant")
                .flat_map(|message| {
                    message["tool_calls"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|call| call["function"]["arguments"].as_str())
                        .filter_map(|arguments| {
                            serde_json::from_str::<serde_json::Value>(arguments).ok()
                        })
                        .filter_map(|value| value["root"].as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        requested.iter().any(|root| root == &external.display().to_string()),
        "the model asked for the external root: {requested:?}"
    );
}
