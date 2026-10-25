//! Skills the user chose in the composer. They are checked when the message is sent,
//! loaded for the turn it starts before that turn's first step, kept with the message in its
//! history, and refused for anything that does not start a new turn.
use super::*;
use provider_fixture::Scenario;
use waiting::{prepare, wait};

const NOTE: &str = "CHOSEN-SKILL-NOTE: answer in one short line.";

/// Imports a Skill folder into Aura through the real Settings route: on, not trusted.
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

/// Imports a Skill folder and trusts it through the real Settings routes.
async fn trusted(runtime: &crate::RuntimeState, parent: &std::path::Path, name: &str) -> Value {
    let (view, listed) = imported(runtime, parent, name).await;
    crate::r03_tests::success(
        runtime,
        "agent.skills.trust",
        json!({"id": view["id"], "expected_fingerprint": view["fingerprint"], "expected_revision": listed["revision"]}),
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
async fn a_chosen_skill_is_in_the_first_step_and_stays_with_its_message() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let id = trusted(&runtime, host.root.path(), "notes").await;

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

    // The first model request already carries the instructions, labelled as the user's choice.
    let first = provider.requests.lock().expect("requests")[0].to_string();
    assert!(first.contains(NOTE), "{first}");
    assert!(first.contains("Loaded Skill \\\"notes\\\""), "{first}");
    assert!(first.contains("The user chose this Skill for this turn"), "{first}");

    // The message keeps its choice in history.
    let shown = history(&runtime, session).await;
    let users = &shown["turns"][0]["user_messages"][0];
    assert_eq!(users["skills"][0]["name"], "notes");
}

#[tokio::test]
async fn a_queued_message_takes_its_skills_into_the_turn_it_starts_and_nothing_else_does() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let id = trusted(&runtime, host.root.path(), "notes").await;

    // Appending to a running task cannot carry a Skill, and an untrusted one is refused.
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
        // One that is there but not trusted cannot be chosen.
        start_request(
            session,
            model,
            vec![imported(&runtime, host.root.path(), "draft").await.0["id"].clone()],
        ),
    )
    .await;
    assert!(format!("{refused:?}").contains("agent.skills_unavailable"), "{refused:?}");
    assert!(provider.requests.lock().expect("requests").is_empty(), "nothing was started");

    // A message sent to the idle session starts a turn with its Skill loaded. The queue starts
    // it with the session's model, which the composer chooses before it sends.
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
    assert!(first.contains(NOTE) && first.contains("The user chose this Skill"), "{first}");
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
}
