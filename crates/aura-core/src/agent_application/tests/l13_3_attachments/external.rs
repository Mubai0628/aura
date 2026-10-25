use super::*;

#[tokio::test]
async fn external_video_is_attached_by_location_and_its_single_file_read_grant_reaches_the_fs_tool()
{
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ExternalRead).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let selected = host.root.path().join("outside-video.mp4");
    // B2 tests file authority, not decoding: B3 generates the actual video in Chromium.
    std::fs::write(&selected, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN))
        .expect("file");
    let agent = services(&runtime).agent_required();
    let started = agent
        .start(
            CommandInvocationId::new(),
            start_request(
                session,
                model,
                &selected,
                &format!(
                    "Read AURA_EXTERNAL_PATH={} and report its first line.",
                    selected.display()
                ),
            ),
        )
        .await
        .expect("external non-text file is accepted");
    finish(&runtime, &started).await;
    let grants = agent.execution.persistence.list_session_external_grants(session).expect("grants");
    assert_eq!(grants.len(), 1);
    let grant = &grants[0];
    assert_eq!(grant.target_kind, ExternalGrantTargetKind::File);
    assert_eq!(grant.capabilities, vec![ExternalGrantCapability::Read]);
    assert_eq!(grant.term, ExternalGrantTerm::Session);
    assert!(!grant.recursive_scope);
    let text = request_text(&provider);
    assert!(text.contains(selected.file_name().unwrap().to_str().unwrap()), "{text}");
    assert!(text.contains(&grant.id.to_string()), "{text}");
    let history = crate::r03_tests::success(
        &runtime,
        "agent.history",
        json!({"session_id":session,
        "before_turn_task_id":null,"within_turn":null,"limit":10}),
    )
    .await;
    let attached = &history["turns"][0]["user_messages"][0]["attachments"][0];
    assert_eq!(attached["kind"], "external_file");
    assert_eq!(attached["path"], selected.to_str().unwrap());
    assert_eq!(attached["external_grant_id"], grant.id.to_string());
    assert!(
        provider
            .requests
            .lock()
            .expect("requests")
            .iter()
            .any(|request| request.to_string().contains(provider_fixture::EXTERNAL_BODY_TOKEN))
    );
    let other = host.root.path().join("other-video.mp4");
    std::fs::write(&other, b"not selected").expect("other");
    assert!(
        agent
            .execution
            .external_grants
            .grant_covering_path(session, grant.workspace_id, &other, None, CorrelationId::new())
            .expect("classify")
            .is_none()
    );
    agent
        .execution
        .external_grants
        .revoke(grant.id, session, CorrelationId::new())
        .expect("revoke");
    assert!(
        agent
            .execution
            .external_grants
            .resolve_existing(
                grant.id,
                session,
                &WorkspacePath::parse(WorkspacePath::ROOT).unwrap(),
                ExternalGrantCapability::Read,
                NativeFileAccess::Read,
                CorrelationId::new()
            )
            .is_err(),
        "revocation must refuse the read"
    );
}

#[tokio::test]
async fn replay_of_a_submitted_external_file_does_no_io_and_never_revives_its_revoked_grant() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({
        "expected_revision":"0","session_id":session,"model_profile_id":model}),
    )
    .await;
    let path = host.root.path().join("selected.pdf");
    std::fs::write(&path, b"%PDF fixture").expect("selected file");
    let request: AgentSubmitRequest = serde_json::from_value(json!({"session_id":session,
        "text":"look","destination":{"lane":"next_turn"},"attachments":[{"kind":"file","path":path}]}))
        .expect("existing submit contract");
    let command = CommandInvocationId::new();
    let agent = services(&runtime).agent_required();
    agent.submit(command, request.clone()).await.expect("external file accepted");
    let grants = agent.execution.persistence.list_session_external_grants(session).expect("grants");
    assert_eq!(grants.len(), 1);
    agent
        .execution
        .external_grants
        .revoke(grants[0].id, session, CorrelationId::new())
        .expect("revoke");
    std::fs::remove_file(&path).expect("remove selected file after acceptance");
    agent
        .submit(command, request)
        .await
        .expect("same command replays before reading a deleted file");
    let after = agent.execution.persistence.list_session_external_grants(session).expect("grants");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].lifecycle, aura_permission_domain::ExternalGrantLifecycle::Revoked);
}

#[tokio::test]
async fn all_file_drafts_are_checked_before_any_external_authority_is_issued() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let selected = host.root.path().join("valid.pdf");
    std::fs::write(&selected, b"valid selected file").expect("file");
    let missing = host.root.path().join("missing.pdf");
    let mut request = start_request(session, model, &selected, "look");
    request
        .attachments
        .push(serde_json::from_value(json!({"kind":"file","path":missing})).unwrap());
    let agent = services(&runtime).agent_required();
    let failure = agent
        .start(CommandInvocationId::new(), request)
        .await
        .expect_err("missing selection")
        .to_string();
    assert!(failure.contains("「missing.pdf」"), "{failure}");
    assert!(
        agent
            .execution
            .persistence
            .list_session_external_grants(session)
            .expect("grants")
            .is_empty()
    );
    assert!(provider.requests.lock().expect("requests").is_empty());
}

#[tokio::test]
async fn a_failed_message_revokes_its_new_grant_and_never_revives_or_revokes_existing_authority() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, _) = prepare(&host, &provider).await;
    let path = host.root.path().join("selected-for-rejected-message.pdf");
    std::fs::write(&path, b"file").expect("selected file");
    let request: AgentSubmitRequest = serde_json::from_value(json!({"session_id":session,
        "text":"x".repeat(256 * 1024 + 1),"destination":{"lane":"next_turn"},
        "attachments":[{"kind":"file","path":path}]}))
    .unwrap();
    let command = CommandInvocationId::new();
    let agent = services(&runtime).agent_required();
    let previous_file = host.root.path().join("previously-authorized.pdf");
    std::fs::write(&previous_file, b"previous authority").unwrap();
    let previous = agent
        .execution
        .external_grants
        .attach_read_only(
            aura_permission_domain::AttachExternalReadOnlyInput {
                session_id: session,
                workspace_id: agent.session_workspace(session).unwrap(),
                selected_path: previous_file.display().to_string(),
            },
            CorrelationId::new(),
        )
        .unwrap();
    assert!(agent.submit(command, request.clone()).await.is_err(), "the message is too long");
    let grants = agent.execution.persistence.list_session_external_grants(session).expect("grants");
    assert_eq!(grants.len(), 2, "the grant transaction ran before the known message failure");
    assert_eq!(
        grants.iter().find(|g| g.id == previous.id).unwrap().lifecycle,
        aura_permission_domain::ExternalGrantLifecycle::Active
    );
    assert_eq!(
        grants.iter().find(|g| g.id != previous.id).unwrap().lifecycle,
        aura_permission_domain::ExternalGrantLifecycle::Revoked
    );
    assert!(
        agent.submit(command, request).await.is_err(),
        "a failed command must not revive its authority"
    );
    let after = agent.execution.persistence.list_session_external_grants(session).expect("grants");
    assert_eq!(after, grants);
    assert!(provider.requests.lock().expect("requests").is_empty());
}

#[tokio::test]
async fn a_directory_selection_names_the_file_in_chinese_and_issues_no_authority() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let path = host.root.path().join("selected-folder");
    std::fs::create_dir(&path).unwrap();
    let agent = services(&runtime).agent_required();
    let error = agent
        .start(CommandInvocationId::new(), start_request(session, model, &path, "look"))
        .await
        .expect_err("a directory is not a selected single file")
        .to_string();
    assert!(error.contains("「selected-folder」") && error.contains("文件夹"), "{error}");
    assert!(agent.execution.persistence.list_session_external_grants(session).unwrap().is_empty());
}
