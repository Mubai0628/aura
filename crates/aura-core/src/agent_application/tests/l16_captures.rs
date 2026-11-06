//! What a terminal printed or a page of the built-in browser showed, sent with a message: Core
//! keeps it as a file in Aura's data, attached like a file from outside the workspace. The model
//! reads it by its path without being asked; another conversation's model reaching it asks, as for
//! anything else in Aura's data; it goes when its conversation is deleted.
use super::*;
use aura_domain_common::ConversationHandle;
use provider_fixture::Scenario;

/// One request to Core, boxed: Core's request futures are large, and a test that awaits many of
/// them inline outgrows the test thread's stack in a debug build.
async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

const PRINTED: &str =
    "PS C:\\work> cargo test\nerror[E0425]: cannot find value `total` in this scope\n";

fn tool_results(request: &Value) -> String {
    request["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str())
        .collect()
}

#[tokio::test]
async fn a_capture_is_a_file_of_its_conversation_its_model_reads_and_others_ask_for() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ReadAttachedFile).await;
    let runtime = host.start().await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("captures-workspace");
    std::fs::create_dir(&root).expect("workspace root");
    let created = call(
        &runtime,
        "workspace.create",
        json!({"selected_root": root, "display_name": "Captures"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;

    let started: AgentRunProjection = serde_json::from_value(
        call(
            &runtime,
            "agent.start",
            json!({
                "session_id": session,
                "model_profile_id": model,
                "text": "这个报错怎么修",
                "mode": "normal",
                "goal_budget": null,
                "attachments": [{"kind": "capture", "name": "terminal-1.txt", "text": PRINTED}],
            }),
        )
        .await,
    )
    .expect("run");
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;

    // Kept in Aura's data under the conversation's handle, never its id.
    let requests = provider.requests.lock().expect("requests").clone();
    let message = requests[0]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .find(|text| text.starts_with("这个报错怎么修"))
        .expect("the message")
        .to_owned();
    let line = "- external file `";
    let path = {
        let rest = &message[message.find(line).expect("its file") + line.len()..];
        rest[..rest.find('`').expect("its path ends")].to_owned()
    };
    assert!(path.contains("v2-message-captures"), "{path}");
    assert!(path.contains(ConversationHandle::of(session).as_str()), "{path}");
    assert!(!message.contains(&session.to_string()), "{message}");
    assert!(path.ends_with("terminal-1.txt"), "{path}");
    assert_eq!(std::fs::read_to_string(&path).expect("kept"), PRINTED);
    // Its model read it by that path in ReadOnly without being asked.
    let read = tool_results(requests.last().expect("the answer's request"));
    assert!(read.contains("error[E0425]"), "{read}");

    // The conversation shows it as its file, opened through its authorization.
    let shown = call(
        &runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await;
    let attached = &shown["turns"][0]["user_messages"][0]["attachments"][0];
    assert_eq!(attached["kind"], "external_file", "{attached}");
    assert_eq!(attached["name"], "terminal-1.txt", "{attached}");
    let located = call(
        &runtime,
        "agent.attachments.locate",
        json!({"session_id": session, "external_grant_id": attached["external_grant_id"]}),
    )
    .await;
    assert_eq!(
        std::fs::canonicalize(located["path"].as_str().expect("where")).expect("there"),
        std::fs::canonicalize(&path).expect("kept")
    );

    // Another conversation's model reaching the same file asks, as anywhere in Aura's data, and
    // reads nothing before the answer.
    let stranger = Provider::scenario(Scenario::ExternalReadMatrix).await;
    let stranger_model = profile_after_existing(&runtime, &stranger.url).await;
    let other = create_session(&runtime, workspace.workspace_id, PermissionMode::FullAccess).await;
    let reaching: AgentRunProjection = serde_json::from_value(
        call(
            &runtime,
            "agent.start",
            json!({
                "session_id": other,
                "model_profile_id": stranger_model,
                "text": format!("Read AURA_EXTERNAL_PATH={path} and report its first line."),
                "mode": "normal",
                "goal_budget": null,
            }),
        )
        .await,
    )
    .expect("run");
    let asking = AgentRunRequest { session_id: other, agent_run_id: reaching.run.agent_run_id };
    let waiting = wait(&runtime, &asking, |p| !p.pending_approval_ids.is_empty()).await;
    let approval = call(
        &runtime,
        "agent.approval.get",
        json!({"session_id": other, "approval_id": waiting.pending_approval_ids[0]}),
    )
    .await;
    assert!(approval.to_string().contains("sensitive_user_path"), "{approval}");
    let leaked = stranger
        .requests
        .lock()
        .expect("requests")
        .iter()
        .any(|body| tool_results(body).contains("error[E0425]"));
    assert!(!leaked, "nothing of the capture reached the other model");

    // Deleting the conversation deletes its captures.
    let folder = std::path::Path::new(&path)
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == ConversationHandle::of(session).as_str()))
        .expect("the conversation's folder")
        .to_path_buf();
    call(&runtime, "session.archive", json!({"session_id": session})).await;
    call(&runtime, "session.delete", json!({"session_id": session})).await;
    crate::session_lifecycle::deleted(&runtime).await;
    assert!(!folder.exists(), "{}", folder.display());
}
