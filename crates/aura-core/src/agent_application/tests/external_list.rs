use super::s06::non_recursive_reads::external_list_under_non_recursive_grant;
use super::*;

/// An ordinary external directory is listed without any grant.
///
/// The same confirmed policy as for a file: FullAccess follows the existing rules for an
/// ordinary external directory, so the listing happens without the user pre-creating a
/// grant, and the result names the policy as the authority rather than inventing one.
#[tokio::test]
async fn an_ordinary_external_directory_is_listed_without_a_grant() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalList).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-list-policy-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-list-policy");
    std::fs::create_dir_all(&external).expect("external root");
    std::fs::write(
        external.join(format!("{}.txt", provider_fixture::EXTERNAL_LIST_TOKEN)),
        "listed",
    )
    .expect("token file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External list policy"}),
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
            text: format!("List AURA_EXTERNAL_PATH={} and report its entries.", external.display()),
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
    // The fixture asserts the entry reached the next request; this also asserts the
    // authority the result names.
    let bodies = provider.requests.lock().expect("requests").clone();
    let listed = bodies
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
        .expect("the external entry must reach the model");
    assert!(
        listed.contains("\"grant_id\":null"),
        "a policy-authorized listing must not claim a grant: {listed}"
    );
}

/// ReadOnly has no directory grant, so the first list asks. Approving that exact
/// call must run it, not reject the frozen operation because a grant now exists.
#[tokio::test]
async fn approving_an_external_directory_runs_the_original_call() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalList).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-approve-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-approve");
    std::fs::create_dir_all(&external).expect("external root");
    std::fs::write(
        external.join(format!("{}.txt", provider_fixture::EXTERNAL_LIST_TOKEN)),
        "listed",
    )
    .expect("token file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External approve"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!("List AURA_EXTERNAL_PATH={} and report its entries.", external.display()),
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
    let mut approval_id = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let view: AgentRunProjection = serde_json::from_value(
            crate::r03_tests::success(&runtime, "agent.status", json!(&request)).await,
        )
        .expect("status");
        if let Some(id) = view.pending_approval_ids.first().copied() {
            approval_id = Some(id);
            break;
        }
        if view.turn.state.terminal() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    let approval_id = approval_id.expect("ReadOnly must ask before listing an external directory");
    let approval = crate::r03_tests::success(
        &runtime,
        "agent.approval.get",
        json!({"session_id": session, "approval_id": approval_id}),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.approval.resolve",
        json!({
            "session_id": session,
            "approval_id": approval_id,
            "expected_revision": approval["approval"]["revision"],
            "decision": "allow_session",
        }),
    )
    .await;
    let done = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }), "{:?}", done.turn.state);
    let bodies = provider.requests.lock().expect("requests").clone();
    let listed = bodies
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
        .expect("the approved list must reach the model");
    assert!(
        !listed.contains("source_or_scope_changed"),
        "adopting the new directory grant must not reject the frozen call: {listed}"
    );
}

/// An authorized external directory is listed through the real model loop.
///
/// The grant here is a directory grant, which the product creates through the
/// approval flow; the test writes the record the same way that flow does, so the
/// listing path itself is the real one: classification, the grant resource, the
/// live scope and identity check, one bounded level of the real directory, and the
/// external facts in the model-visible result.
#[tokio::test]
async fn an_authorized_external_directory_is_listed_through_the_model_loop() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalList).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-list-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-dir");
    std::fs::create_dir_all(external.join("nested")).expect("external root");
    let token_file = external.join(format!("{}.txt", provider_fixture::EXTERNAL_LIST_TOKEN));
    std::fs::write(&token_file, "listed body").expect("token file");
    std::fs::write(external.join("nested/deeper.txt"), "deeper").expect("nested file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External list"}),
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
            text: format!("List AURA_EXTERNAL_PATH={} and report its entries.", external.display()),
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
    // The fixture only answers when the real listing reached the next request.
    let bodies = provider.requests.lock().expect("requests").clone();
    let joined = bodies.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    assert!(
        joined.contains(provider_fixture::EXTERNAL_LIST_TOKEN),
        "the external directory's entries must reach a model request"
    );
    assert!(
        joined.contains("nested"),
        "the listing reports a child directory without descending into it"
    );
    assert!(
        !joined.contains("deeper.txt"),
        "one level is listed, so a nested child's entries are not read"
    );
}

/// A non-recursive grant bounds every object the walk enumerates, not just its target.
///
/// One level of the grant root is authorized, so listing the root really lists its direct
/// children — and stops there. Listing one of those direct children is a different act:
/// the child is an authorized object, but its own entries are grandchildren of the grant
/// root. Aura asks for that directory before dispatch instead of reading through the
/// child's name; no grandchild name or execution Attempt may appear while waiting.
#[tokio::test]
async fn a_non_recursive_grant_bounds_the_objects_it_enumerates() {
    let host = crate::r03_tests::Fixture::new();
    let external = host.root.path().join("outside-scope");
    std::fs::create_dir_all(external.join("sub")).expect("external root");
    std::fs::write(external.join("top.txt"), "top body").expect("top file");
    std::fs::write(external.join("sub").join("inner.txt"), "inner body").expect("inner file");

    // The control: the grant root lists its direct children, and nothing deeper.
    let listed =
        external_list_under_non_recursive_grant(&external, &external, PermissionMode::ReadOnly)
            .await;
    assert!(listed.finished, "a listing of the authorized root completes");
    assert!(listed.results.contains("top.txt"), "direct children are listed: {}", listed.results);
    assert!(
        listed.results.contains("sub"),
        "a direct child directory is an object: {}",
        listed.results
    );
    assert!(
        !listed.results.contains("inner.txt"),
        "a non-recursive grant does not read below its direct children: {}",
        listed.results
    );

    // The boundary: naming the direct child does not authorize its contents.
    let refused = external_list_under_non_recursive_grant(
        &external,
        &external.join("sub"),
        PermissionMode::ReadOnly,
    )
    .await;
    assert!(refused.asked, "the child directory must ask before dispatch: {}", refused.results);
    assert!(!refused.finished, "the run waits for the new directory approval");
    assert_eq!(refused.attempts, 0, "no Attempt is reserved before the child is approved");
    assert!(
        !refused.results.contains("inner.txt"),
        "contents below a direct child are outside the authorized scope: {}",
        refused.results
    );
    assert!(!refused.results.contains("dispatch_failed"), "an approval is not a runtime failure");
    let target = refused.requested_target.expect("the pending approval previews the actual child");
    let child = aura_windows_platform::inspect_existing_path(&external.join("sub"), true)
        .expect("verify the exact child directory identity");
    assert_eq!(
        target.identity_key,
        aura_security_contract::file_object_key(&aura_workspace::domain_identity(&child.identity))
    );
    assert!(target.recursive, "a new directory approval previews its default recursive contents");
}
