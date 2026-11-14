//! A sub-agent's turn is the work of the conversation that started it: that conversation runs
//! while it runs and waits for the user while it waits, and the sub-agent's own turns are never
//! listed, so no notice speaks of them.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_domain::*;
use aura_domain_common::SessionId;
use aura_persistence_v2::{V2Store, agent::under_way};

fn set(store: &V2Store, turn: &TurnTask, turn_state: &str, run_state: &str) {
    let db = store.database.connection();
    db.execute(
        "UPDATE turn_tasks SET state=?2 WHERE id=?1",
        rusqlite::params![turn.turn_task_id.to_string(), turn_state],
    )
    .unwrap();
    db.execute(
        "UPDATE agent_runs SET state=?2 WHERE turn_task_id=?1 AND parent_id IS NULL",
        rusqlite::params![turn.turn_task_id.to_string(), run_state],
    )
    .unwrap();
}

#[test]
fn a_sub_agent_waiting_for_an_approval_shows_on_the_conversation_that_started_it() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let (child, parent) = (fixture.turn.session_id, SessionId::new());
    store
        .database
        .connection()
        .execute(
            "INSERT INTO agent_team_members(child_session_id, parent_session_id, handle, \
             spawn_call, state, revision, payload, created_at, updated_at) \
             VALUES(?1, ?2, 'A1', 'call-a1', 'running', 1, '{}', '2026-11-06T00:00:00Z', \
             '2026-11-06T00:00:00Z')",
            rusqlite::params![child.to_string(), parent.to_string()],
        )
        .unwrap();
    let seen = |store: &V2Store| under_way::sessions(store.database.connection()).unwrap().sessions;

    set(&store, &fixture.turn, "running", "model_running");
    assert_eq!(
        seen(&store),
        [AgentSessionActivity {
            session_id: parent,
            state: AgentSessionActivityState::Running,
            turn_task_ids: vec![],
            waiting_for: None,
        }]
    );
    set(&store, &fixture.turn, "awaiting_approval", "awaiting_approval");
    assert_eq!(
        seen(&store),
        [AgentSessionActivity {
            session_id: parent,
            state: AgentSessionActivityState::Waiting,
            turn_task_ids: vec![],
            waiting_for: Some(AgentSessionWaitingFor::Approval),
        }]
    );
    set(&store, &fixture.turn, "completed", "completed");
    assert!(seen(&store).is_empty());
}
