//! Skills the user chose in the composer. They are checked and written out whole when the message
//! is sent, follow what the user typed in that message from its turn's first step on, stay with it
//! as sent in later turns, and are refused for anything that does not start a new turn. The
//! message's history shows what was typed, with the choice beside it.
use super::*;
use provider_fixture::Scenario;
use waiting::{prepare, start, wait};

const NOTE: &str = "CHOSEN-SKILL-NOTE: answer in one short line.";
/// The message as the requests carry it (JSON text): what was typed, then the chosen Skill.
const SENT: &str = r#"Answer with the chosen Skill\n\nChosen Skill \"notes\""#;

/// Imports a Skill folder into Aura through the real Settings route: on, and usable at once.
async fn imported(
    runtime: &crate::RuntimeState,
    parent: &std::path::Path,
    name: &str,
) -> (Value, Value) {
    let folder = parent.join(name);
    std::fs::create_dir_all(&folder).expect("Skill folder");
    std::fs::write(
        folder.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Chosen {name}\n---\n\n{NOTE}\n"),
    )
    .expect("SKILL.md");
    let listed = crate::r03_tests::success(runtime, "agent.skills.list", json!({})).await;
    crate::r03_tests::success(
        runtime,
        "agent.skills.import",
        json!({"path": folder, "expected_revision": listed["revision"]}),
    )
    .await;
    let listed = crate::r03_tests::success(runtime, "agent.skills.list", json!({})).await;
    let view = listed["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .find(|skill| skill["name"] == name)
        .cloned()
        .expect("imported");
    (view, listed)
}

async fn usable(runtime: &crate::RuntimeState, parent: &std::path::Path, name: &str) -> Value {
    imported(runtime, parent, name).await.0["id"].clone()
}

/// Imports a Skill folder and switches it off through the real Settings routes.
async fn switched_off(
    runtime: &crate::RuntimeState,
    parent: &std::path::Path,
    name: &str,
) -> Value {
    let (view, listed) = imported(runtime, parent, name).await;
    crate::r03_tests::success(
        runtime,
        "agent.skills.set_enabled",
        json!({"id": view["id"], "enabled": false, "expected_revision": listed["revision"]}),
    )
    .await;
    view["id"].clone()
}

fn start_request(session: SessionId, model: ModelProfileId, skills: Vec<Value>) -> Value {
    json!({
        "session_id": session,
        "model_profile_id": model,
        "text": "Answer with the chosen Skill",
        "mode": "normal",
        "goal_budget": null,
        "skill_ids": skills,
    })
}

async fn history(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    crate::r03_tests::success(
        runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await
}

#[tokio::test]
async fn a_chosen_skill_follows_its_message_and_the_history_shows_what_was_typed() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let id = usable(&runtime, host.root.path(), "notes").await;

    let run: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(
            &runtime,
            "agent.start",
            start_request(session, model, vec![id.clone()]),
        )
        .await,
    )
    .expect("run");
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;

    // The first model request already carries the instructions, after what the user typed and
    // marked as the user's choice; nothing loads them into the task state.
    let first = provider.requests.lock().expect("requests")[0].to_string();
    assert!(first.contains(SENT) && first.contains(NOTE), "{first}");
    assert!(first.contains("The user chose this Skill with this message"), "{first}");
    assert!(!first.contains("Loaded Skill"), "{first}");

    // The message keeps its choice in history and shows what was typed.
    let shown = history(&runtime, session).await;
    let users = &shown["turns"][0]["user_messages"][0];
    assert_eq!(users["skills"][0]["name"], "notes");
    assert_eq!(users["text"], "Answer with the chosen Skill");

    // A later turn sends the message as it was sent.
    let next = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: next.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let last = provider.requests.lock().expect("requests").last().expect("a request").to_string();
    assert!(last.contains(SENT) && last.contains(NOTE), "{last}");
}

#[tokio::test]
async fn a_queued_message_takes_its_skills_into_the_turn_it_starts_and_nothing_else_does() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let id = usable(&runtime, host.root.path(), "notes").await;

    // Appending to a running task cannot carry a Skill, and one switched off is refused.
    let addition = crate::r03_tests::attempt(
        &runtime,
        "agent.submit",
        json!({"session_id": session, "text": "Add this", "destination": {"lane": "addition", "turn_task_id": TurnTaskId::new()}, "skill_ids": [id]}),
    )
    .await;
    assert!(format!("{addition:?}").contains("agent.skills_need_new_turn"), "{addition:?}");
    let refused = crate::r03_tests::attempt(
        &runtime,
        "agent.start",
        // One that is there but switched off cannot be chosen.
        start_request(
            session,
            model,
            vec![switched_off(&runtime, host.root.path(), "draft").await],
        ),
    )
    .await;
    assert!(format!("{refused:?}").contains("agent.skills_unavailable"), "{refused:?}");
    assert!(provider.requests.lock().expect("requests").is_empty(), "nothing was started");

    // A message sent to the idle session starts a turn with its Skill after what was typed. The
    // queue starts it with the session's model, which the composer chooses before it sends.
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.submit",
        json!({"session_id": session, "text": "Use the chosen Skill", "destination": {"lane": "next_turn"}, "skill_ids": [id]}),
    )
    .await;
    let first = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            if let Some(first) = provider.requests.lock().expect("requests").first() {
                break first.to_string();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the queued message started a turn");
    assert!(
        first.contains(r#"Use the chosen Skill\n\nChosen Skill \"notes\""#) && first.contains(NOTE),
        "{first}"
    );
    assert!(first.contains("The user chose this Skill with this message"), "{first}");
    let turn = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let shown = history(&runtime, session).await;
            if shown["turns"][0]["turn"]["state"]["state"] == "completed" {
                break shown["turns"][0].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the turn completed");
    assert_eq!(turn["user_messages"][0]["skills"][0]["name"], "notes");
    assert_eq!(turn["user_messages"][0]["text"], "Use the chosen Skill");
}
