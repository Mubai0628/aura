//! Another conversation of the workspace referred to with "@": the model reads its title and a
//! recap of where it was by its handle, never its id, and reads more of it with the history tools
//! by that handle until it is archived or deleted. Deleting it leaves every record of the
//! conversation that referred to it.
use super::*;
use aura_domain_common::ConversationHandle;
use provider_fixture::Scenario;
use std::collections::BTreeSet;

/// One request to Core, boxed: Core's request futures are large, and a test that awaits many of
/// them inline outgrows the test thread's stack in a debug build.
async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services
}

/// Every event and operation in the execution store.
fn records(runtime: &crate::RuntimeState) -> BTreeSet<String> {
    let store = services(runtime).broker.lock().expect("V2 store");
    let mut statement = store
        .database
        .connection()
        .prepare("SELECT event_id FROM events UNION ALL SELECT id FROM operations")
        .expect("query");
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).expect("rows");
    rows.map(|row| row.expect("row")).collect()
}

fn referring(session: SessionId, model: ModelProfileId, text: &str, to: SessionId) -> Value {
    json!({
        "session_id": session,
        "model_profile_id": model,
        "text": text,
        "mode": "normal",
        "goal_budget": null,
        "references": [{"kind": "session", "session_id": to}],
    })
}

/// The texts of a request's messages of one role.
fn texts(request: &Value, role: &str) -> Vec<String> {
    request["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == role)
        .filter_map(|message| message["content"].as_str().map(str::to_owned))
        .collect()
}

/// The last two tool results the model read: the turn's two history calls.
fn last_two_read(request: &Value) -> Vec<String> {
    let read = texts(request, "tool");
    read[read.len().saturating_sub(2)..].to_vec()
}

async fn finished(runtime: &crate::RuntimeState, session: SessionId, run: &AgentRunProjection) {
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    wait(runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
}

async fn started(runtime: &crate::RuntimeState, request: Value) -> AgentRunProjection {
    serde_json::from_value(call(runtime, "agent.start", request).await).expect("run")
}

/// The last request the model was sent.
fn last(provider: &Provider) -> Value {
    provider.requests.lock().expect("requests").last().expect("a request").clone()
}

#[tokio::test]
async fn another_conversation_is_named_by_its_handle_and_read_until_archived_or_deleted() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ReferredHistory).await;
    let (runtime, earlier, model) = prepare(&host, &provider).await;
    let first = start(&runtime, earlier, model, AgentTaskMode::Normal).await;
    finished(&runtime, earlier, &first).await;
    let projection = call(
        &runtime,
        "projection.snapshot",
        json!({"projection_kind": "workspace_session", "projection_scope": {"type": "global"}}),
    )
    .await;
    let workspace: WorkspaceId = serde_json::from_value(
        projection["payload"]["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .find(|s| s["session_id"] == json!(earlier))
            .expect("the earlier conversation")["workspace_id"]
            .clone(),
    )
    .expect("its workspace");
    let before = records(&runtime);
    let theirs = provider.requests.lock().expect("requests").len();
    let session = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    let handle = ConversationHandle::of(earlier);

    // Named by its title and handle with a recap of its round and reply; the model reads its
    // rounds and its first message by that handle.
    let run = started(&runtime, referring(session, model, "接着那个会话做", earlier)).await;
    finished(&runtime, session, &run).await;
    let request = last(&provider);
    let sent = texts(&request, "user");
    let message = sent.iter().find(|t| t.starts_with("接着那个会话做")).expect("the message");
    let named = format!(
        "接着那个会话做\n\nReferred to in this message:\n- conversation \"Agent\" (`{handle}`), not \
         included: read more of it with history.list_turns, history.search and history.read, \
         passing \"conversation\": \"{handle}\", if the task needs more than its recent state:\n  \
         > T1 ("
    );
    assert!(message.starts_with(&named), "{message}");
    assert!(
        message.ends_with("): Check the answer and finish\n  > Last reply: Done."),
        "{message}"
    );
    let read = last_two_read(&request);
    assert_eq!(read.len(), 2, "{read:?}");
    assert!(read.iter().all(|result| result.contains("Check the answer and finish")), "{read:?}");
    for request in &provider.requests.lock().expect("requests")[theirs..] {
        assert!(!request.to_string().contains(&earlier.to_string()), "never by its id: {request}");
    }
    let shown = call(
        &runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await;
    assert_eq!(
        shown["turns"][0]["user_messages"][0]["references"],
        json!([{"kind": "session", "label": "Agent"}])
    );

    // Archived, it cannot be referred to, and the tools say why they do not read it.
    call(&runtime, "session.archive", json!({"session_id": earlier})).await;
    let refused = crate::r03_tests::attempt(
        &runtime,
        "agent.start",
        referring(session, model, "again", earlier),
    )
    .await;
    assert!(format!("{refused:?}").contains("引用的会话不在这个工作区里，或已归档、删除"));
    let again = start(&runtime, session, model, AgentTaskMode::Normal).await;
    finished(&runtime, session, &again).await;
    let read = last_two_read(&last(&provider));
    assert!(
        read.len() == 2 && read.iter().all(|r| r.contains("history.conversation_archived")),
        "{read:?}"
    );

    // Deleted, the tools say it is gone; nothing the referring conversation keeps went with it.
    let mine: BTreeSet<String> = records(&runtime).difference(&before).cloned().collect();
    call(&runtime, "session.delete", json!({"session_id": earlier})).await;
    crate::session_lifecycle::deleted(&runtime).await;
    let after = records(&runtime);
    let lost: Vec<&String> = mine.iter().filter(|id| !after.contains(*id)).collect();
    assert!(lost.is_empty(), "the referring conversation lost {lost:?}");
    let more = start(&runtime, session, model, AgentTaskMode::Normal).await;
    finished(&runtime, session, &more).await;
    let read = last_two_read(&last(&provider));
    assert!(
        read.len() == 2 && read.iter().all(|r| r.contains("history.conversation_unavailable")),
        "{read:?}"
    );
}
