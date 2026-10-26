use super::*;

/// An authorized external create goes through the real model loop.
///
/// The model names an absolute path outside the workspace and asks for a file that
/// does not exist yet, so the call exercises the create target rather than a
/// replacement: a verified parent directory plus the name to publish. The bytes must
/// land on disk, the result must reach the next model request, and the workspace
/// index must not move — the file is not part of it.
#[tokio::test]
async fn an_authorized_external_file_is_created_through_the_model_loop() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalWrite).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-write-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-write");
    std::fs::create_dir_all(&external).expect("external root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External write"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    // The workspace's first index walk commits after the create returns; compare from after it.
    services
        .p03_required()
        .workspace
        .wait_until_indexed(std::time::Duration::from_secs(60))
        .expect("the first walk");
    let revision_before = services
        .p03_required()
        .workspace
        .catalog(workspace.workspace_id)
        .expect("catalog")
        .revision()
        .expect("revision");
    // A create needs both: `write` covers the directory's contents and `create` the
    // new name inside it.
    let record = external_directory_grant(
        session,
        workspace.workspace_id,
        &external,
        vec![
            aura_permission_domain::ExternalGrantCapability::Write,
            aura_permission_domain::ExternalGrantCapability::Create,
        ],
    );
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(record, CorrelationId::new())
        .expect("directory grant");
    let target = external.join("written.txt");
    assert!(!target.exists(), "the fixture creates the file, not the test");

    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Create the file AURA_EXTERNAL_PATH={} with the body you were given.",
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

    // The real bytes are outside the workspace, which is where the model asked for
    // them: nothing here is a workspace path with a grant painted on it.
    let written = std::fs::read_to_string(&target).expect("the external file must exist");
    assert_eq!(written, provider_fixture::EXTERNAL_WRITE_TOKEN);
    let revision_after = services
        .p03_required()
        .workspace
        .catalog(workspace.workspace_id)
        .expect("catalog")
        .revision()
        .expect("revision");
    assert_eq!(
        revision_before, revision_after,
        "an external write must not move the workspace revision"
    );
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
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        tool_results.contains("\"created\":true"),
        "the create result must reach the next model request: {tool_results}"
    );
    assert!(
        tool_results.contains("written.txt"),
        "the result must name the external file it wrote: {tool_results}"
    );
}

/// An ordinary external file is created without any grant, and a lower mode asks instead.
///
/// The confirmed policy: FullAccess follows the existing rules for an ordinary external
/// file, so a create needs no grant of its own and the result names the policy. Outside
/// FullAccess the same create asks for approval, so the file must not appear.
#[tokio::test]
async fn an_external_create_without_a_grant_follows_the_policy() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalWrite).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-write-refused-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-refused");
    std::fs::create_dir_all(&external).expect("external root");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External write refused"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let target = external.join("written.txt");

    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!(
                "Create the file AURA_EXTERNAL_PATH={} with the body you were given.",
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
    assert_eq!(
        std::fs::read_to_string(&target).expect("the policy allowed the create"),
        provider_fixture::EXTERNAL_WRITE_TOKEN
    );
    let bodies = provider.requests.lock().expect("requests").clone();
    let written = bodies
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
        .find(|content| content.contains("\"created\":true"))
        .expect("the create result must reach the model");
    assert!(
        written.contains("\"grant_id\":null"),
        "a policy-authorized create must not claim a grant: {written}"
    );

    // The same create outside FullAccess asks for approval instead, so nothing appears.
    let asked = external_create_asking_for_approval().await;
    assert!(!asked.exists(), "a mode that must ask leaves the file uncreated");
}

/// The depth boundary, asserted where the grant is the authority.
///
/// The confirmed semantics: a non-recursive directory grant covers the directory itself
/// and its direct children and nothing deeper, while a recursive one covers everything
/// below. Because FullAccess follows the existing policy for an ordinary external file,
/// a model-loop create is no longer the place this shows: policy allows it at any depth
/// there. The boundary is therefore asserted on the grant service itself, which is the
/// component every path asks — classification (`grant_covering_path`), resolution
/// (`target_path`/`within_record`) and creation (`validate_create_parent`) all call the
/// same depth rule.
#[tokio::test]
async fn a_non_recursive_directory_grant_covers_direct_children_only() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("external-depth-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-depth");
    std::fs::create_dir_all(external.join("nested")).expect("external root");
    std::fs::write(external.join("direct.txt"), "direct").expect("direct file");
    std::fs::write(external.join("nested/deep.txt"), "deep").expect("deep file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External depth"}),
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
        vec![
            aura_permission_domain::ExternalGrantCapability::Read,
            aura_permission_domain::ExternalGrantCapability::Write,
            aura_permission_domain::ExternalGrantCapability::Create,
        ],
    );
    record.recursive_scope = false;
    let grant_id = record.id;
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(record, CorrelationId::new())
        .expect("non-recursive directory grant");
    let agent = services.agent_required();
    let grants = agent.execution.external_grants.clone();

    // Classification: a direct child is inside the grant, a grandchild is not.
    let direct = grants
        .grant_covering_path(
            session,
            workspace.workspace_id,
            &external.join("direct.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("classification");
    assert!(direct.is_some(), "a direct child of a non-recursive grant is in scope");
    let nested = grants
        .grant_covering_path(
            session,
            workspace.workspace_id,
            &external.join("nested/deep.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("classification");
    assert!(nested.is_none(), "a grandchild is outside a non-recursive grant");

    // Creation agrees with classification: in scope for a direct child, refused deeper.
    let direct_relative = WorkspacePath::parse("direct.txt").expect("path");
    assert!(
        grants
            .validate_create_parent(grant_id, session, &direct_relative, CorrelationId::new())
            .is_ok(),
        "creating a direct child is authorized"
    );
    let nested_relative = WorkspacePath::parse("nested/deep.txt").expect("path");
    assert!(
        grants
            .validate_create_parent(grant_id, session, &nested_relative, CorrelationId::new())
            .is_err(),
        "creating below the authorized depth is refused"
    );
}
