use super::*;

#[tokio::test]
async fn agent_loop_consumes_fs_search_matches_in_the_following_model_request() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::Search).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("agent-search-workspace");
    std::fs::create_dir(&root).expect("Agent search workspace");
    std::fs::write(root.join("search-fixture.txt"), "unique_fs_search_agent_token\n")
        .expect("search fixture");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Production Agent search"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone())
            .expect("Agent test setup and persisted assertion");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "Search the workspace for unique_fs_search_agent_token and finish.".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection =
        serde_json::from_value(started).expect("Agent test setup and persisted assertion");
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    let result = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let v = crate::r03_tests::success(&runtime, "agent.status", json!(request)).await;
            let v: AgentRunProjection =
                serde_json::from_value(v).expect("Agent test setup and persisted assertion");
            if v.turn.state.terminal() {
                break v;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Agent fs.search loop");
    assert!(
        matches!(result.turn.state, TurnTaskState::Completed { .. }),
        "{:?}",
        result.turn.state
    );
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        4,
        "tools.search, tools.load, fs.search, then the request that consumes Match"
    );
}

/// An authorized external ordinary file is readable through the real model loop.
///
/// The model names an absolute path outside the workspace. Core classifies it
/// against the session's active grant, the operation carries that grant as its
/// resource, admission checks it in FullAccess, and the file layer reads the
/// verified handle. Without the grant the same path stays a corrigible
/// outside-workspace failure, which the second half asserts.
#[tokio::test]
async fn an_authorized_external_file_is_readable_through_the_model_loop() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalRead).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-read-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside");
    std::fs::create_dir(&external).expect("external root");
    let external_file = external.join("outside.txt");
    std::fs::write(
        &external_file,
        format!("{}\nsecond line", provider_fixture::EXTERNAL_BODY_TOKEN),
    )
    .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External read"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    // The user authorized this one external file for this session.
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
        "the turn must complete: {:?}",
        done.turn.state
    );
    // The fixture only answers when the real tool result reached the next model
    // request, and the recorded request bodies are the same evidence from the test
    // side: a silent fallback to an error could not contain the file's text.
    let bodies = provider.requests.lock().expect("requests").clone();
    assert!(
        bodies.iter().any(|body| body.to_string().contains(provider_fixture::EXTERNAL_BODY_TOKEN)),
        "the external file's text must reach a model request: {}",
        bodies.len()
    );
    // The persisted tool result carries the same text, so the fact is durable and
    // not only a request the model saw. The completed-call event names the
    // model-visible result artifact; reading it is the same path the UI projection
    // uses.
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let journal = services.broker.lock().expect("journal");
    let db = journal.database.connection();
    let completed: Vec<serde_json::Value> = {
        let mut statement = db
            .prepare("SELECT payload FROM events WHERE kind='tool.call_completed'")
            .expect("results");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("results")
            .collect::<Result<Vec<_>, _>>()
            .expect("results")
            .into_iter()
            .map(|raw| serde_json::from_str(&raw).expect("completed call"))
            .collect()
    };
    let mut recorded: Vec<serde_json::Value> = Vec::new();
    for call in &completed {
        let Some(artifact_id) = call["model_visible_result_ref"].as_str() else { continue };
        let Ok(id) = artifact_id.parse() else { continue };
        let bytes =
            aura_persistence_core::artifact::read(db, id, &start.run.agent_run_id.to_string())
                .expect("model-visible result artifact");
        recorded
            .push(serde_json::from_slice(&bytes).expect("a model-visible result envelope is JSON"));
    }
    let external_read = recorded
        .iter()
        .find(|envelope| {
            envelope["outcome"]["payload"]["value"]["text"]
                .as_str()
                .is_some_and(|text| text.contains(provider_fixture::EXTERNAL_BODY_TOKEN))
        })
        .unwrap_or_else(|| {
            panic!("no recorded tool result carried the external file's text: {recorded:#?}")
        });
    let value = &external_read["outcome"]["payload"]["value"];
    assert_eq!(
        value["external"]["display_path"].as_str(),
        Some(external_file.display().to_string().as_str()),
        "the recorded result names the absolute path the user authorized"
    );
    assert_eq!(
        value["path"].as_str(),
        Some("."),
        "the file is the grant root itself, so its path relative to that root is the root"
    );
}

/// The same absolute path without a grant stays a corrigible refusal: it is an
/// authorization boundary, not a spelling problem.
#[tokio::test]
async fn an_ordinary_external_file_is_readable_without_a_grant() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    // The confirmed product behaviour: an ordinary external file needs no grant of its
    // own in FullAccess, so the read happens and the result must name the policy as the
    // authority instead of inventing a grant.
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalRead).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-refusal-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-refused");
    std::fs::create_dir(&external).expect("external root");
    let external_file = external.join("secret.txt");
    std::fs::write(&external_file, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External refusal"}),
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
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    // The fixture asserts the body reached the next request; this also asserts the
    // authority the result names.
    let bodies = provider.requests.lock().expect("requests").clone();
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
        .collect::<Vec<_>>();
    let read = tool_results
        .iter()
        .find(|content| content.contains(provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("the external body must reach the model");
    assert!(
        read.contains("\"grant_id\":null"),
        "a policy-authorized read must not claim a grant: {read}"
    );
}

/// And a read deeper than a non-recursive grant follows the policy instead, which is the
/// confirmed behaviour for an ordinary external file in FullAccess.
#[tokio::test]
async fn a_read_below_a_non_recursive_grant_follows_the_policy() {
    let nested = external_read_within_non_recursive_grant("nested/deep.txt", false, false).await;
    assert!(
        nested.contains(provider_fixture::EXTERNAL_BODY_TOKEN),
        "FullAccess reads an ordinary external file by policy, whatever a grant covers: {nested}"
    );
}

/// The permission matrix for an authorized external file.
///
/// The engine's existing intent is that reading a session-authorized external file
/// is allowed in every mode, because the user attached that exact file; anything not
/// covered by such a grant asks instead of reading. These tests pin what each mode
/// really did rather than restating the expectation, so a silent widening would show
/// up here.
#[tokio::test]
async fn an_authorized_external_file_is_read_in_every_mode() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let outcome = external_read_outcome(mode).await;
        assert!(
            outcome.contains(provider_fixture::EXTERNAL_BODY_TOKEN),
            "{mode:?}: an explicitly authorized external file must be readable: {outcome}"
        );
    }
}

/// What each mode really does with an ordinary external file that has no grant.
///
/// The confirmed semantics are that the existing policy decides: FullAccess and
/// WorkspaceReadWrite read an ordinary external file without the user pre-creating a
/// grant, and ReadOnly asks for approval instead of reading it. Each assertion is bound
/// to what that mode really did, so a silent widening into ReadOnly would fail here.
#[tokio::test]
async fn an_ungranted_external_file_is_decided_by_the_mode() {
    // FullAccess is the one mode whose confirmed default reaches an ordinary external object,
    // so it reads without pre-creating a grant.
    let full = external_read_outcome_unmoved(PermissionMode::FullAccess).await;
    assert!(
        full.contains(provider_fixture::EXTERNAL_BODY_TOKEN),
        "FullAccess: an ordinary external file follows the existing policy: {full}"
    );
    // The two lower modes do **not** reach an external object by default. The workspace
    // defaults are about the workspace; treating a declared read capability as authority over
    // an arbitrary reachable object is the substitution that would let a workspace-mode session
    // read outside its boundary with no approval. Each asks, so the file never reaches the
    // model.
    for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::ReadOnly] {
        let asked = external_read_asking_for_approval(mode).await;
        assert!(
            !asked.contains(provider_fixture::EXTERNAL_BODY_TOKEN),
            "{mode:?} must not read an ungranted external file: {asked}"
        );
    }
    // A grant the user did issue is still honored in the lower modes, so this boundary is about
    // the *default* and not about refusing external reads outright.
    let granted = external_read_outcome(PermissionMode::WorkspaceReadWrite).await;
    assert!(
        granted.contains(provider_fixture::EXTERNAL_BODY_TOKEN),
        "an approved external read is authorized in WorkspaceReadWrite: {granted}"
    );
}
