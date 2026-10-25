use super::*;

#[tokio::test]
async fn a_b2_selected_external_file_is_readable_but_ungranted_files_are_refused_even_in_full_access()
 {
    for granted in [false, true] {
        let host = crate::r03_tests::Fixture::new();
        let selected = host.root.path().join("external-media.webm");
        std::fs::write(&selected, SOURCE).unwrap();
        let provider = Provider::scenario(scenario("media.probe", json!({"path":selected}))).await;
        let mode = if granted { PermissionMode::ReadOnly } else { PermissionMode::FullAccess };
        let (runtime, session, model) = prepare_in_mode(&host, &provider, mode).await;
        let (seen, task) = play_host(&runtime);
        let input: AgentStartRequest =
            serde_json::from_value(json!({"session_id":session,"model_profile_id":model,
            "text":"Probe the selected file","mode":"normal","goal_budget":null,
            "attachments":if granted {json!([{"kind":"file","path":selected}])}else{json!([])}}))
            .unwrap();
        let started = services(&runtime)
            .agent_required()
            .start(CommandInvocationId::new(), input)
            .await
            .unwrap();
        finish(&runtime, &started).await;
        assert_eq!(
            seen.lock().unwrap().len(),
            usize::from(granted),
            "{mode:?}: {}",
            tool_text(&provider)
        );
        let text = tool_text(&provider);
        if granted {
            assert!(text.contains("\"duration\":2.0"), "{text}");
        } else {
            assert!(
                text.contains("outside this session's authorized workspace")
                    || text.contains("explicitly authorized"),
                "{text}"
            );
        }
        task.abort();
    }
}

#[tokio::test]
async fn revoked_replaced_or_directory_authority_never_reaches_the_decoder_in_full_access() {
    for boundary in ["revoked", "replaced", "directory"] {
        let host = crate::r03_tests::Fixture::new();
        let accepted = Provider::scenario(Scenario::Text).await;
        let (runtime, session, model) =
            prepare_in_mode(&host, &accepted, PermissionMode::FullAccess).await;
        let directory = host.root.path().join("outside-media");
        std::fs::create_dir(&directory).unwrap();
        let selected = directory.join("selected.webm");
        std::fs::write(&selected, SOURCE).unwrap();
        let agent = services(&runtime).agent_required();
        if boundary == "directory" {
            let record = external_directory_grant(
                session,
                agent.session_workspace(session).unwrap(),
                &directory,
                vec![aura_permission_domain::ExternalGrantCapability::Read],
            );
            agent
                .execution
                .persistence
                .create_external_grant(record, CorrelationId::new())
                .unwrap();
        } else {
            let input: AgentStartRequest = serde_json::from_value(json!({"session_id":session,
                "model_profile_id":model,"text":"The selected file is attached.","mode":"normal",
                "goal_budget":null,"attachments":[{"kind":"file","path":selected}]}))
            .unwrap();
            let started = agent.start(CommandInvocationId::new(), input).await.unwrap();
            finish(&runtime, &started).await;
            let grant = agent
                .execution
                .persistence
                .list_session_external_grants(session)
                .unwrap()
                .remove(0);
            if boundary == "revoked" {
                agent
                    .execution
                    .external_grants
                    .revoke(grant.id, session, CorrelationId::new())
                    .unwrap();
            } else {
                std::fs::rename(&selected, directory.join("original.webm")).unwrap();
                std::fs::write(&selected, SOURCE).unwrap();
                let replacement =
                    aura_windows_platform::inspect_existing_path(&selected, true).unwrap();
                assert_ne!(
                    grant.root_identity,
                    aura_workspace::domain_identity(&replacement.identity)
                );
            }
        }
        let provider = Provider::scenario(scenario("media.probe", json!({"path":selected}))).await;
        let model = profile_after_existing(&runtime, &provider.url).await;
        let (seen, task) = play_host(&runtime);
        let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
        finish(&runtime, &started).await;
        assert!(
            seen.lock().unwrap().is_empty(),
            "{boundary}: unauthorized media reached Main: {}",
            tool_text(&provider)
        );
        assert!(
            !provider
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.to_string().contains(std::str::from_utf8(SOURCE).unwrap())),
            "{boundary}: file bytes reached the provider"
        );
        task.abort();
    }
}
