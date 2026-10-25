//! What a
//! message carried, read again for the conversation's 文件 tab — a text file from where the message
//! keeps it, an attached file outside the workspace through its read-only authorization — and for
//! no other conversation.
#![allow(clippy::unwrap_used)]
use super::*;
use aura_agent_domain::AgentStartRequest;
use aura_domain_common::CommandInvocationId;

const PNG_BASE64: &str = "iVBORw0KGgoAAAAN";

/// The refusal a route gave, as text; a route that answered is a failed test.
async fn refused(runtime: &crate::RuntimeState, method: &str, params: Value) -> String {
    let response = crate::r03_tests::attempt(runtime, method, params).await.unwrap();
    assert!(response.get("error").is_some(), "{method} answered: {response}");
    response["error"].to_string()
}

/// A conversation of another workspace.
async fn other_session(
    runtime: &crate::RuntimeState,
    host: &crate::r03_tests::Fixture,
) -> SessionId {
    let root = host.root.path().join("other-workspace");
    std::fs::create_dir(&root).unwrap();
    let created = crate::r03_tests::success(
        runtime,
        "workspace.create",
        json!({"selected_root": root, "display_name": "Other"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).unwrap();
    create_session(runtime, workspace.workspace_id, PermissionMode::ReadOnly).await
}

#[tokio::test]
async fn a_text_file_a_message_carried_reads_back_for_its_conversation_only() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.submit",
        json!({"session_id": session, "text": "看看", "destination": {"lane": "next_turn"},
            "attachments": [
                {"kind": "text", "name": "notes.md", "text": "第一行\nsecond line"},
                {"kind": "image", "name": "shot.png", "media_type": "image/png", "data_base64": PNG_BASE64}
            ]}),
    )
    .await;
    let attachments = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let history = crate::r03_tests::success(
                &runtime,
                "agent.history",
                json!({"session_id":session,"before_turn_task_id":null,"within_turn":null,"limit":10}),
            )
            .await;
            let attached = history["turns"][0]["user_messages"][0]["attachments"].clone();
            if attached.as_array().is_some_and(|items| items.len() == 2) {
                break attached;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the message keeps its attachments");
    let text_id = attachments[0]["artifact_id"].clone();
    let picture_id = attachments[1]["artifact_id"].clone();
    let read = crate::r03_tests::success(
        &runtime,
        "agent.attachments.text",
        json!({"session_id": session, "artifact_id": text_id}),
    )
    .await;
    assert_eq!(read["text"], "第一行\nsecond line");
    let picture = refused(
        &runtime,
        "agent.attachments.text",
        json!({"session_id": session, "artifact_id": picture_id}),
    )
    .await;
    assert!(picture.contains("agent.attachment_unavailable"), "{picture}");
    let stranger = other_session(&runtime, &host).await;
    let elsewhere = refused(
        &runtime,
        "agent.attachments.text",
        json!({"session_id": stranger, "artifact_id": text_id}),
    )
    .await;
    assert!(elsewhere.contains("agent.attachment_unavailable"), "{elsewhere}");
}

#[tokio::test]
async fn an_attached_outside_file_is_found_again_only_while_it_is_the_file_attached() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ExternalRead).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let selected = host.root.path().join("SaveTik.io_7638307602505436479.mp4");
    std::fs::write(&selected, format!("{}\n", provider_fixture::EXTERNAL_BODY_TOKEN)).unwrap();
    let agent = match &runtime.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required(),
        _ => panic!("ready"),
    };
    let request: AgentStartRequest = serde_json::from_value(json!({
        "session_id": session, "model_profile_id": model, "mode": "normal", "goal_budget": null,
        "text": format!("Read AURA_EXTERNAL_PATH={} and report its first line.", selected.display()),
        "attachments": [{"kind": "file", "path": selected}]}))
    .unwrap();
    let started = agent.start(CommandInvocationId::new(), request).await.unwrap();
    wait(
        &runtime,
        &AgentRunRequest {
            session_id: started.turn.session_id,
            agent_run_id: started.run.agent_run_id,
        },
        |view| view.turn.state.terminal(),
    )
    .await;
    let grant = agent.execution.persistence.list_session_external_grants(session).unwrap()[0].id;
    let located = crate::r03_tests::success(
        &runtime,
        "agent.attachments.locate",
        json!({"session_id": session, "external_grant_id": grant}),
    )
    .await;
    let path = std::path::PathBuf::from(located["path"].as_str().unwrap());
    assert!(!located["path"].as_str().unwrap().starts_with(r"\\?\"), "{located}");
    assert_eq!(std::fs::canonicalize(&path).unwrap(), std::fs::canonicalize(&selected).unwrap());
    assert_eq!(located["byte_length"], std::fs::metadata(&selected).unwrap().len());
    let stranger = other_session(&runtime, &host).await;
    let elsewhere = refused(
        &runtime,
        "agent.attachments.locate",
        json!({"session_id": stranger, "external_grant_id": grant}),
    )
    .await;
    assert!(elsewhere.contains("agent.attachment_missing"), "{elsewhere}");
    // Another file put where the attached one was is not the file the user attached.
    std::fs::remove_file(&selected).unwrap();
    std::fs::write(&selected, b"a different file").unwrap();
    let replaced = refused(
        &runtime,
        "agent.attachments.locate",
        json!({"session_id": session, "external_grant_id": grant}),
    )
    .await;
    assert!(replaced.contains("agent.attachment_missing"), "{replaced}");
}
