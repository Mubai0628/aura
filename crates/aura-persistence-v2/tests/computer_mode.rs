#[allow(dead_code)]
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{application, inbox},
};

#[test]
fn the_run_and_each_queued_message_keep_their_mode_after_reopening_the_store() {
    let temp = tempfile::tempdir().expect("fixture");
    let path = temp.path().join("v2");
    let mut store = V2Store::create(&path).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let request: AgentStartRequest = serde_json::from_value(serde_json::json!({
        "session_id":f.turn.session_id, "model_profile_id":ModelProfileId::new(),
        "text":"fixture", "mode":"normal", "goal_budget":null, "computer_mode":"child_session"
    }))
    .expect("request");
    store
        .database
        .unit_of_work(|tx| {
            application::configure(
                tx,
                f.turn.agent_run_id,
                &application::RunConfiguration {
                    request,
                    tool_settings: Default::default(),
                    purpose: AgentTaskPurpose::Conversation,
                },
                CommandInvocationId::new(),
                f.at,
            )
        })
        .expect("configure");
    for mode in [ComputerMode::Foreground, ComputerMode::ChildSession] {
        let mut request =
            AgentSubmitRequest::new(f.turn.session_id, "queued fixture", AgentInputLane::NextTurn);
        request.computer_mode = Some(mode);
        store
            .database
            .unit_of_work(|tx| inbox::submit(tx, CommandInvocationId::new(), &request, f.at))
            .expect("submit");
    }
    drop(store);
    let reopened = V2Store::open(&path).expect("reopen");
    let stored = application::configuration(reopened.database.connection(), f.turn.agent_run_id)
        .expect("configuration");
    assert_eq!(stored.request.computer_mode, Some(ComputerMode::ChildSession));
    let projected = application::projection(
        reopened.database.connection(),
        &AgentRunRequest { session_id: f.turn.session_id, agent_run_id: f.turn.agent_run_id },
    )
    .expect("projection");
    assert_eq!(
        serde_json::to_value(projected).expect("projection value")["computer_mode"],
        "child_session",
        "the UI and Continue must use the stored turn's desktop"
    );
    let history = aura_persistence_v2::agent::desktop::projection(
        reopened.database.connection(),
        &serde_json::from_value(serde_json::json!({"session_id":f.turn.session_id,"limit":10}))
            .expect("history request"),
        RuntimeInstanceId::new(),
    )
    .expect("history");
    assert_eq!(
        serde_json::to_value(history).expect("history value")["turns"][0]["computer_mode"],
        "child_session"
    );
    let queued = inbox::load(reopened.database.connection(), f.turn.session_id).expect("inbox");
    assert_eq!(
        queued.messages.iter().map(|message| message.computer_mode).collect::<Vec<_>>(),
        vec![Some(ComputerMode::Foreground), Some(ComputerMode::ChildSession)]
    );
}

#[test]
fn changing_the_mode_cannot_reuse_an_old_input_receipt() {
    let temp = tempfile::tempdir().expect("fixture");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let command = CommandInvocationId::new();
    let mut request =
        AgentSubmitRequest::new(f.turn.session_id, "fixture", AgentInputLane::NextTurn);
    request.computer_mode = Some(ComputerMode::ChildSession);
    store.database.unit_of_work(|tx| inbox::submit(tx, command, &request, f.at)).expect("submit");
    request.computer_mode = Some(ComputerMode::Foreground);
    assert!(store.database.unit_of_work(|tx| inbox::submit(tx, command, &request, f.at)).is_err());
    let queued = inbox::load(store.database.connection(), f.turn.session_id).expect("inbox");
    assert_eq!(queued.messages.len(), 1);
    assert_eq!(queued.messages[0].computer_mode, Some(ComputerMode::ChildSession));
}
