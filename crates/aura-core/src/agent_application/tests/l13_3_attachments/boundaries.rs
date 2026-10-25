//! Real scripted model requests go through discovery, permission admission and fs.read.
use super::*;

async fn read_boundary(
    runtime: &crate::RuntimeState,
    started: &AgentRunProjection,
    boundary: &str,
) -> AgentRunProjection {
    let request = AgentRunRequest {
        session_id: started.turn.session_id,
        agent_run_id: started.run.agent_run_id,
    };
    // Three model steps (search, read, then the refusal or approval); in a loaded parallel test
    // run each can take several seconds.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let projection = services(runtime).agent_required().status(&request).await.unwrap();
        if projection.turn.state.terminal() || !projection.pending_approval_ids.is_empty() {
            return projection;
        }
        assert!(
            !matches!(projection.turn.state, TurnTaskState::AwaitingQuery { .. })
                && !matches!(
                    projection.run.state,
                    AgentRunState::AwaitingContextRemediation { .. }
                ),
            "{boundary}: unexpected user intervention: {}",
            serde_json::to_string_pretty(&projection).unwrap()
        );
        assert!(
            std::time::Instant::now() < deadline,
            "{boundary}: fs.read produced neither a refusal nor an approval: {}",
            serde_json::to_string_pretty(&projection).unwrap()
        );
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
}

#[tokio::test]
async fn fs_read_cannot_use_a_selected_attachment_to_read_another_revoked_or_replaced_file() {
    for boundary in ["other", "revoked", "replaced"] {
        let host = crate::r03_tests::Fixture::new();
        let accepted = Provider::scenario(Scenario::Text).await;
        let (runtime, session, model) = prepare(&host, &accepted).await;
        let selected = host.root.path().join("selected-for-boundary.pdf");
        std::fs::write(&selected, b"original selected object").unwrap();
        let agent = services(&runtime).agent_required();
        let started = agent
            .start(
                CommandInvocationId::new(),
                start_request(session, model, &selected, "The selected file is attached."),
            )
            .await
            .unwrap();
        finish(&runtime, &started).await;
        let grant =
            agent.execution.persistence.list_session_external_grants(session).unwrap().remove(0);
        let target = if boundary == "other" {
            let other = host.root.path().join("never-selected.pdf");
            std::fs::write(&other, provider_fixture::EXTERNAL_BODY_TOKEN).unwrap();
            other
        } else {
            if boundary == "revoked" {
                agent
                    .execution
                    .external_grants
                    .revoke(grant.id, session, CorrelationId::new())
                    .unwrap();
                std::fs::write(&selected, provider_fixture::EXTERNAL_BODY_TOKEN).unwrap();
            } else {
                // Keep the original object alive at a different name: NTFS cannot reuse its ID.
                let original = selected.with_extension("original");
                std::fs::rename(&selected, &original).unwrap();
                std::fs::write(&selected, provider_fixture::EXTERNAL_BODY_TOKEN).unwrap();
                let replacement =
                    aura_windows_platform::inspect_existing_path(&selected, true).unwrap();
                assert_ne!(
                    grant.root_identity,
                    aura_workspace::domain_identity(&replacement.identity)
                );
            }
            selected
        };
        let refused = Provider::scenario(Scenario::ExternalReadMatrix).await;
        let model = profile_after_existing(&runtime, &refused.url).await;
        let mut request = start_request(
            session,
            model,
            &target,
            &format!("Read AURA_EXTERNAL_PATH={} and report its first line.", target.display()),
        );
        request.attachments.clear();
        let started = agent.start(CommandInvocationId::new(), request).await.unwrap();
        let pending = read_boundary(&runtime, &started, boundary).await;
        if let Some(approval_id) = pending.pending_approval_ids.first() {
            // An ungranted external read asks in ReadOnly. Deny that real request, retaining
            // the original permission semantics while proving no file bytes were delivered.
            let approval = crate::r03_tests::success(
                &runtime,
                "agent.approval.get",
                json!({
                "session_id":session,"approval_id":approval_id}),
            )
            .await;
            assert!(approval.to_string().contains("fs.read"), "{boundary}: {approval}");
            assert!(
                !refused.requests.lock().unwrap().iter().any(|request| request
                    .to_string()
                    .contains(provider_fixture::EXTERNAL_BODY_TOKEN)),
                "{boundary}: file bytes escaped before approval: {approval}"
            );
            crate::r03_tests::success(
                &runtime,
                "agent.approval.resolve",
                json!({
                "session_id":session,"approval_id":approval_id,
                "expected_revision":approval["approval"]["revision"],"decision":"deny"}),
            )
            .await;
            let completed = read_boundary(&runtime, &started, boundary).await;
            assert!(
                matches!(completed.turn.state, TurnTaskState::Completed { .. }),
                "{boundary}: after deny, projection={}, approval={approval}",
                serde_json::to_string_pretty(&completed).unwrap()
            );
        } else {
            assert!(
                matches!(pending.turn.state, TurnTaskState::Completed { .. }),
                "{boundary}: {}",
                serde_json::to_string_pretty(&pending).unwrap()
            );
        }
        let requests = refused.requests.lock().unwrap();
        let results = requests
            .iter()
            .flat_map(|request| request["messages"].as_array().into_iter().flatten())
            .filter(|message| message["role"] == "tool")
            // The existing ExternalReadMatrix fixture issues fs.read at ordinal 2. Exclude
            // state.snapshot/tools.load: their ordinary identity fields are not a refusal.
            .filter(|message| message["tool_call_id"] == "fixture_2_0")
            .filter_map(|message| message["content"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!results.contains(provider_fixture::EXTERNAL_BODY_TOKEN), "{boundary}: {results}");
        assert!(
            results.contains("outside this session's authorized workspace")
                || results.contains("identity")
                || results.contains("revoked")
                || results.to_lowercase().contains("denied"),
            "{boundary}: fs.read must receive a real authorization refusal: {results}"
        );
    }
}
