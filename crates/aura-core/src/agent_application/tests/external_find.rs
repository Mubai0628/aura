use super::*;

/// An authorized external directory is searched by name through the real model
/// loop, and the recursive walk reaches a file inside a subdirectory.
#[tokio::test]
async fn an_authorized_external_directory_is_searched_through_the_model_loop() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalFind).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-find-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-find");
    std::fs::create_dir_all(external.join("nested")).expect("external root");
    // The match is one level down, so only a recursive walk finds it.
    let token_file =
        external.join("nested").join(format!("{}.txt", provider_fixture::EXTERNAL_LIST_TOKEN));
    std::fs::write(&token_file, "found body").expect("token file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External find"}),
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
        capabilities: vec![aura_permission_domain::ExternalGrantCapability::Read],
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
            text: format!("Find the file AURA_EXTERNAL_PATH={} and report it.", external.display()),
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
    let joined = bodies.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    assert!(
        joined.contains(provider_fixture::EXTERNAL_LIST_TOKEN),
        "the external match must reach a model request"
    );
    assert!(joined.contains("nested/"), "the match reports the path inside the authorized root");
}

/// An ordinary external directory is searched by name without any grant.
///
/// The same policy as for a listing: FullAccess follows the existing rules, so the
/// lookup happens without the user pre-creating a grant, and the result names the policy
/// as the authority.
#[tokio::test]
async fn an_ordinary_external_directory_is_searched_without_a_grant() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalFind).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-find-policy-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-find-policy");
    std::fs::create_dir_all(external.join("nested")).expect("external root");
    // The match is one level down, so the walk really descends.
    std::fs::write(
        external.join("nested").join(format!("{}.txt", provider_fixture::EXTERNAL_LIST_TOKEN)),
        "found body",
    )
    .expect("token file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External find policy"}),
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
            text: format!("Find the file AURA_EXTERNAL_PATH={} and report it.", external.display()),
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
        .find(|content| content.contains(provider_fixture::EXTERNAL_LIST_TOKEN))
        .expect("the external match must reach the model");
    assert!(
        found.contains("\"grant_id\":null"),
        "a policy-authorized lookup must not claim a grant: {found}"
    );
}

/// A recursive request cannot turn a non-recursive directory grant into a wider one.
///
/// The model asks for the same recursive lookup as above, against the same directory
/// layout, and the only difference is that the grant does not include descendants. The
/// request expresses what the model wants to look at; the grant decides how far it may
/// look, and Core intersects the two before the walk starts. So the file one level down
/// must not be reported, and the answer must be an honest empty page — not an error, and
/// not a silent full-depth search that treats the request as an authorization.
#[tokio::test]
async fn a_recursive_external_lookup_cannot_widen_a_non_recursive_grant() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalFindScopeCapped).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-find-cap-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-find-cap");
    std::fs::create_dir_all(external.join("nested")).expect("external root");
    // The only file with the requested name sits below the authorized depth.
    std::fs::write(
        external.join("nested").join(format!("{}.txt", provider_fixture::EXTERNAL_LIST_TOKEN)),
        "hidden body",
    )
    .expect("token file");
    std::fs::write(external.join("visible.txt"), "visible body").expect("visible file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External find capped"}),
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
    // The confirmed boundary: the directory and its direct children, nothing deeper.
    record.recursive_scope = false;
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
            text: format!("Find the file AURA_EXTERNAL_PATH={} and report it.", external.display()),
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
    // The fixture asserts the capped-walk facts on the tool result the model saw: the
    // nested name is absent and the page is an empty match list. Only tool-role
    // messages count, because the model's own arguments name the file it asked for.
    let bodies = provider.requests.lock().expect("requests").clone();
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
                        .filter_map(|value| {
                            value["recursive"].as_bool().map(|recursive| {
                                format!(
                                    "{}:{recursive}",
                                    value["path"].as_str().unwrap_or_default()
                                )
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        requested.iter().any(|call| call.ends_with(":true")),
        "the model really asked for a recursive lookup: {requested:?}"
    );
}
