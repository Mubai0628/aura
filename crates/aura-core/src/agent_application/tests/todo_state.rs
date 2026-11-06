//! The task state does not repeat the to-do the conversation shows. Every to-do update changed
//! that state, and a request that reaches OpenAI's cache sends it before the conversation, so the
//! step after each update read the whole conversation again.
use super::*;

const TWO_UPDATES: Scenario = Scenario::Sequence {
    calls: &[
        (
            "todo.replace",
            r#"{"expected_revision":"1","items":[{"title":"AURA-TODO-FIRST","status":"in_progress"},{"title":"AURA-TODO-SECOND"}]}"#,
        ),
        (
            "todo.replace",
            r#"{"expected_revision":"2","items":[{"title":"AURA-TODO-FIRST","status":"completed"},{"title":"AURA-TODO-SECOND","status":"completed"}]}"#,
        ),
    ],
};

/// A model that carries the revision of an earlier turn's to-do into a new turn.
const STALE_REVISION: Scenario = Scenario::Sequence {
    calls: &[(
        "todo.replace",
        r#"{"expected_revision":"5","items":[{"title":"AURA-TODO-STALE","status":"in_progress"}]}"#,
    )],
};

/// The task's current state: the request's last message.
fn task_state(request: &Value) -> String {
    let state = request["messages"].as_array().and_then(|messages| messages.last());
    let text = state.and_then(|state| state["content"].as_str()).expect("the last message");
    assert!(text.contains("Current-Task-State"), "{text}");
    text.to_owned()
}

#[tokio::test]
async fn the_task_state_stays_the_same_while_the_conversation_shows_the_todo() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(TWO_UPDATES).await;
    let (runtime, run) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: run.turn.session_id, agent_run_id: run.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    assert!(requests.len() >= 3, "two to-do updates and the answer: {}", requests.len());
    let states = requests.iter().map(task_state).collect::<Vec<_>>();
    assert!(
        states.iter().all(|state| *state == states[0]),
        "the task state changed with the to-do: {states:#?}"
    );
    assert!(!states[0].contains("AURA-TODO-FIRST"), "{}", states[0]);
    // The model reads the to-do in its own results, and Core keeps it.
    let last = requests.last().expect("the answer's request");
    let results = last["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| message["content"].as_str().expect("result text").to_owned())
        .collect::<Vec<_>>();
    let newest = results.last().expect("the second update's result");
    assert!(
        newest.contains("AURA-TODO-SECOND") && newest.contains(r#""revision":"3""#),
        "{newest}"
    );
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let journal = services.broker.lock().expect("V2 store");
    let todo = aura_persistence_v2::agent::work::todo(
        journal.database.connection(),
        run.turn.turn_task_id,
    )
    .expect("the turn's to-do");
    assert_eq!(todo.revision.to_string(), "3");
    assert_eq!(todo.items.len(), 2);
}

#[tokio::test]
async fn a_todo_change_against_another_revision_is_told_the_turns_own() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(STALE_REVISION).await;
    let (runtime, run) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: run.turn.session_id, agent_run_id: run.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let last = requests.last().expect("the answer's request");
    let result = last["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| message["content"].as_str().expect("result text").to_owned())
        .next_back()
        .expect("the update's result");
    // A correctable argument, not a failed dispatch: the model is told the revision to send.
    assert!(result.contains("tool.schema.arguments_invalid"), "{result}");
    assert!(!result.contains("dispatch_failed"), "{result}");
    assert!(result.contains(r#"Send expected_revision \"1\""#), "{result}");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let journal = services.broker.lock().expect("V2 store");
    let todo = aura_persistence_v2::agent::work::todo(
        journal.database.connection(),
        run.turn.turn_task_id,
    )
    .expect("the turn's to-do");
    assert_eq!(todo.revision.to_string(), "1");
    assert!(todo.items.is_empty());
}
