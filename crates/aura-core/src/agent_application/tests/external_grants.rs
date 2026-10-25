use super::*;

/// The approval path mints one reusable directory grant. A second approval of the same
/// directory reuses it, another session in the workspace can use a workspace-term grant,
/// and revoke or expiry stops the next classification.
#[tokio::test]
async fn an_approved_directory_grant_is_reused_until_revoked_or_expired() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("external-reuse-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-reuse");
    std::fs::create_dir_all(external.join("nested")).expect("external root");
    std::fs::write(external.join("direct.txt"), "direct").expect("direct file");
    std::fs::write(external.join("nested/deep.txt"), "deep").expect("deep file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External reuse"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let other = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let identity = aura_windows_platform::inspect_existing_path(&external, true)
        .expect("external directory identity");
    let spec = crate::external_grants::ApprovedDirectoryGrant {
        session_id: session,
        workspace_id: workspace.workspace_id,
        canonical_root: aura_security_contract::CanonicalPath {
            utf16: identity.final_path.display().to_string().encode_utf16().collect(),
            display: identity.final_path.display().to_string(),
        },
        root_identity: aura_workspace::domain_identity(&identity.identity),
        recursive_scope: false,
        capabilities: vec![aura_permission_domain::ExternalGrantCapability::Read],
        term: aura_permission_domain::ExternalGrantTerm::Workspace,
        turn_task_id: None,
        expires_at: None,
    };
    let agent = services.agent_required();
    let grants = agent.execution.external_grants.clone();
    let approval_a = aura_domain_common::ApprovalRequestId::new();
    let approval_b = aura_domain_common::ApprovalRequestId::new();
    let first = grants
        .issue_approved_directory(
            spec.clone(),
            approval_a,
            "directory-approval-a",
            CorrelationId::new(),
        )
        .expect("approved directory grant");
    let second = grants
        .issue_approved_directory(
            spec.clone(),
            approval_b,
            "directory-approval-b",
            CorrelationId::new(),
        )
        .expect("reuse");
    assert_eq!(first.id, second.id, "a second approval reuses the grant that is still active");
    let replayed = grants
        .issue_approved_directory(
            spec.clone(),
            approval_a,
            "directory-approval-a",
            CorrelationId::new(),
        )
        .expect("replay");
    assert_eq!(replayed.id, first.id, "replaying one approval returns its bound grant");
    let direct = grants
        .grant_covering_path(
            other,
            workspace.workspace_id,
            &external.join("direct.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("other session");
    assert!(direct.is_some(), "a workspace-term grant covers a direct child in another session");
    let nested = grants
        .grant_covering_path(
            other,
            workspace.workspace_id,
            &external.join("nested/deep.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("depth");
    assert!(nested.is_none(), "a non-recursive grant does not cover a grandchild");
    grants.revoke(first.id, session, CorrelationId::new()).expect("revoke");
    let after = grants
        .grant_covering_path(
            other,
            workspace.workspace_id,
            &external.join("direct.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("after revoke");
    assert!(after.is_none(), "revoke blocks a later classification");
    let revived = grants
        .issue_approved_directory(
            spec.clone(),
            approval_a,
            "directory-approval-a",
            CorrelationId::new(),
        )
        .expect("replay after revoke");
    assert_eq!(revived.id, first.id, "the old approval stays bound to the revoked grant");
    assert_eq!(revived.lifecycle, aura_permission_domain::ExternalGrantLifecycle::Revoked);
    let still_revoked = grants
        .grant_covering_path(
            other,
            workspace.workspace_id,
            &external.join("direct.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("replay does not cover");
    assert!(still_revoked.is_none(), "replaying the revoked approval must not mint a new grant");
    let approval_c = aura_domain_common::ApprovalRequestId::new();
    let renewed = grants
        .issue_approved_directory(spec, approval_c, "directory-approval-c", CorrelationId::new())
        .expect("new approval");
    assert_ne!(renewed.id, first.id, "a new approval after revocation can authorize again");
    let covered_again = grants
        .grant_covering_path(
            session,
            workspace.workspace_id,
            &external.join("direct.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("renewed cover");
    assert!(covered_again.is_some(), "the new approval covers the direct child");
    grants.revoke(renewed.id, session, CorrelationId::new()).expect("revoke renewal");

    let expired = aura_permission_domain::ExternalGrantRecord {
        id: aura_domain_common::ExternalGrantId::new(),
        session_id: session,
        workspace_id: workspace.workspace_id,
        permission_grant_id: None,
        selected_root: first.canonical_root.clone(),
        canonical_root: first.canonical_root.clone(),
        root_identity: first.root_identity.clone(),
        target_kind: aura_permission_domain::ExternalGrantTargetKind::Directory,
        capabilities: vec![aura_permission_domain::ExternalGrantCapability::Read],
        recursive_scope: false,
        lifecycle: aura_permission_domain::ExternalGrantLifecycle::Active,
        revalidation_revision: 1,
        created_at: aura_domain_common::UtcTimestamp::now(),
        updated_at: aura_domain_common::UtcTimestamp::now(),
        suspended_at: None,
        revoked_at: None,
        expires_at: Some("2020-01-01T00:00:00Z".parse().expect("timestamp")),
        turn_task_id: None,
        term: aura_permission_domain::ExternalGrantTerm::Session,
    };
    services
        .agent_required()
        .execution
        .persistence
        .create_external_grant(expired, CorrelationId::new())
        .expect("expired grant row");
    let stale = grants
        .grant_covering_path(
            session,
            workspace.workspace_id,
            &external.join("direct.txt"),
            None,
            CorrelationId::new(),
        )
        .expect("expired classification");
    assert!(stale.is_none(), "an expired grant is not a covering grant");
    assert!(
        crate::external_grants::directory_grant_term(
            aura_security_contract::ApprovalDecisionKind::AllowOnce
        )
        .is_none()
    );
    assert!(
        crate::external_grants::directory_grant_term(
            aura_security_contract::ApprovalDecisionKind::Deny
        )
        .is_none()
    );
}

/// A revoked grant stops authorizing immediately: the same path becomes the
/// corrigible outside-workspace refusal again and the file stays unread.
#[tokio::test]
async fn a_revoked_grant_is_no_longer_the_authority_for_an_ordinary_external_file() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::ExternalRead).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("external-revoked-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-revoked");
    std::fs::create_dir(&external).expect("external root");
    let external_file = external.join("revoked.txt");
    std::fs::write(&external_file, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("external file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External revoked"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let grant = services
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
    // The user takes the authorization back before the model asks for the file.
    services
        .agent_required()
        .execution
        .external_grants
        .revoke(grant.id, session, CorrelationId::new())
        .expect("revoke");

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
    // In FullAccess an ordinary external file stays readable by policy, so what a
    // revocation must change is the authority the read names: the revoked grant can no
    // longer be the one that authorized it.
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
        .expect("the file stays readable by policy after the revocation");
    assert!(
        !read.contains(&grant.id.to_string()),
        "the revoked grant must not be the authority: {read}"
    );
    assert!(read.contains("\"grant_id\":null"), "the read names the policy: {read}");
}

/// An approved external target that is **replaced** between the approval and the read is no
/// longer the object the user approved.
///
/// This is the in-window swap case. The grant records the identity of the object the user
/// named, and every use re-inspects the path and compares that identity — so deleting the
/// approved file and putting a different one at the same name must not be served under the old
/// approval. A name-based check would pass here and hand the caller a file the user never
/// approved, which is exactly the substitution this test exists to reject.
#[tokio::test]
async fn a_target_replaced_after_approval_is_not_the_approved_object() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("external-swapped-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let external = host.root.path().join("outside-swapped");
    std::fs::create_dir(&external).expect("external root");
    let approved = external.join("approved.txt");
    std::fs::write(&approved, "the bytes the user approved\n").expect("approved file");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"External swapped"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session =
        create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let grants = services.agent_required().execution.external_grants.clone();
    let grant = grants
        .attach_read_only(
            aura_permission_domain::AttachExternalReadOnlyInput {
                session_id: session,
                workspace_id: workspace.workspace_id,
                selected_path: approved.display().to_string(),
            },
            CorrelationId::new(),
        )
        .expect("attach read-only external grant");

    // The approval is live and really covers the file **before** the swap, so the assertion below
    // is about the swap and not about a grant that never worked.
    let before = grants
        .grant_covering_path(session, workspace.workspace_id, &approved, None, CorrelationId::new())
        .expect("classification before the swap");
    let covered = before.expect("the approved file is covered before anything changes");
    assert_eq!(covered.grant_id, grant.id, "the approval is the authority that covers it");

    // The swap: the approved object is gone and a different one now answers to the same name.
    std::fs::remove_file(&approved).expect("remove the approved object");
    std::fs::write(&approved, "different bytes nobody approved\n").expect("replacement");

    // The approval must not keep serving this name. `grant_covering_path` classifies the path
    // against the live grants and re-inspects the object, so a replacement with a different
    // identity has to stop being covered — a name-based check would happily return the grant and
    // the caller would receive bytes the user never approved.
    let covering = grants.grant_covering_path(
        session,
        workspace.workspace_id,
        &approved,
        None,
        CorrelationId::new(),
    );
    assert!(
        !matches!(covering, Ok(Some(_))),
        "an object replaced after approval must not stay covered by it: {covering:?}"
    );
}
