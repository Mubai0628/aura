//! A restart that cuts a turn off returns it from the start, and Aura continues it by itself at
//! most twice in a row in half an hour, and not when Core failed again within two minutes of
//! continuing it.
#![allow(clippy::unwrap_used)]
#[allow(dead_code)] // Shared transaction fixture also serves other tests.
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{
        application, recovery,
        restart_continuation::{self as continuation, Decision},
    },
};

fn configure(store: &mut V2Store, f: &support::Fixture) {
    store
        .database
        .unit_of_work(|tx| {
            application::configure(
                tx,
                f.turn.agent_run_id,
                &application::RunConfiguration {
                    request: AgentStartRequest {
                        session_id: f.turn.session_id,
                        model_profile_id: ModelProfileId::new(),
                        text: "synthetic restart".into(),
                        mode: AgentTaskMode::Normal,
                        goal_budget: None,
                        explicit_backend_id: None,
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    tool_settings: aura_tool_contract::ToolSettingsSnapshotV1::default(),
                    purpose: AgentTaskPurpose::Conversation,
                },
                CommandInvocationId::new(),
                f.at,
            )
        })
        .expect("configured run");
}

fn minutes_after(at: UtcTimestamp, minutes: i64) -> UtcTimestamp {
    UtcTimestamp::from_value(at.value() + std::time::Duration::from_secs(minutes as u64 * 60))
}

#[test]
fn a_cut_off_turn_is_continued_at_most_twice_in_half_an_hour_and_not_in_a_restart_loop() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("v2");
    let mut store = V2Store::create(&path).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    configure(&mut store, &f);
    drop(store);

    let mut store = V2Store::open(&path).unwrap();
    let cut = store.database.unit_of_work(|tx| recovery::startup(tx, f.at)).unwrap();
    let turn = f.turn.turn_task_id;
    assert_eq!(
        cut,
        vec![recovery::InterruptedTurn {
            session_id: f.turn.session_id,
            turn_task_id: turn,
            agent_run_id: f.turn.agent_run_id,
        }],
        "the start returns the turn it cut off"
    );
    let decide = |store: &V2Store, minutes: i64| {
        continuation::decide(store.database.connection(), turn, minutes_after(f.at, minutes))
            .unwrap()
    };
    assert_eq!(decide(&store, 5), Decision::Continue);
    assert_eq!(continuation::view(store.database.connection(), turn).unwrap(), None);

    // The cut-off turn was itself Aura's continuation of an earlier one.
    let earlier = TurnTaskId::new();
    store
        .database
        .unit_of_work(|tx| {
            continuation::record(tx, turn, earlier, CoreRestartCause::Unresponsive, f.at, f.at)
        })
        .unwrap();
    assert_eq!(
        continuation::view(store.database.connection(), turn).unwrap(),
        Some(AgentAutoContinuedView { cause: CoreRestartCause::Unresponsive, restarted_at: f.at }),
        "the conversation can say why and when"
    );
    assert_eq!(decide(&store, 1), Decision::RestartLoop, "cut off again within two minutes");
    assert_eq!(decide(&store, 5), Decision::Continue, "once in a row is allowed again");

    // And that earlier one was a continuation too, a minute later: twice in half an hour.
    let first = TurnTaskId::new();
    store
        .database
        .unit_of_work(|tx| {
            continuation::record(
                tx,
                earlier,
                first,
                CoreRestartCause::Crash,
                minutes_after(f.at, 1),
                f.at,
            )
        })
        .unwrap();
    assert_eq!(decide(&store, 5), Decision::LimitReached);
    assert_eq!(decide(&store, 40), Decision::Continue, "half an hour later the count starts again");
}

#[test]
fn only_the_cut_off_last_turn_of_a_session_is_continued() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    // Still running: nothing cut it off.
    let decision = continuation::decide(store.database.connection(), f.turn.turn_task_id, f.at);
    assert_eq!(decision.unwrap(), Decision::Moved);
    assert!(!continuation::cut_off(store.database.connection(), f.turn.turn_task_id).unwrap());
}

#[test]
fn a_continuation_record_written_by_a_later_version_is_no_record() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    store
        .database
        .connection()
        .execute(
            "INSERT INTO events(event_id, owner, kind, version, payload, created_at)              VALUES(?1, ?2, 'agent.auto_continued', 1, ?3, ?4)",
            rusqlite::params![
                EventId::new().to_string(),
                f.turn.turn_task_id.to_string(),
                r#"{"cause":"cosmic_ray","restarted_at":"2026-09-07T00:00:00Z","after_turn_task_id":"x"}"#,
                f.at.to_string(),
            ],
        )
        .unwrap();
    assert_eq!(continuation::view(store.database.connection(), f.turn.turn_task_id).unwrap(), None);
}
