//! The sidebar's running state
//! comes from the turns themselves: a running turn shows, one waiting for the user (an approval,
//! a question, a decision about the context) shows as waiting and what for, and a finished one
//! leaves; asked about, it says how it ended.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
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
fn a_running_turn_shows_a_waiting_one_waits_and_a_finished_one_leaves() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = fixture.turn.session_id;
    let seen = |store: &V2Store| under_way::sessions(store.database.connection()).unwrap().sessions;

    let turn = fixture.turn.turn_task_id;
    set(&store, &fixture.turn, "running", "model_running");
    assert_eq!(
        seen(&store),
        [AgentSessionActivity {
            session_id: session,
            state: AgentSessionActivityState::Running,
            turn_task_ids: vec![turn],
            waiting_for: None,
        }]
    );
    set(&store, &fixture.turn, "awaiting_approval", "awaiting_approval");
    assert_eq!(seen(&store)[0].state, AgentSessionActivityState::Waiting);
    assert_eq!(seen(&store)[0].waiting_for, Some(AgentSessionWaitingFor::Approval));
    set(&store, &fixture.turn, "awaiting_query", "awaiting_query");
    assert_eq!(seen(&store)[0].waiting_for, Some(AgentSessionWaitingFor::Question));
    // A decision about the context is the run's state; the turn itself still runs.
    set(&store, &fixture.turn, "running", "awaiting_context_remediation");
    assert_eq!(seen(&store)[0].state, AgentSessionActivityState::Waiting);
    assert_eq!(seen(&store)[0].waiting_for, Some(AgentSessionWaitingFor::Context));
    let asked =
        |store: &V2Store| under_way::activity(store.database.connection(), &[turn]).unwrap();
    assert!(asked(&store).ended.is_empty(), "a turn under way has not ended");
    set(&store, &fixture.turn, "completed", "completed");
    assert!(seen(&store).is_empty());
    assert_eq!(
        asked(&store).ended,
        [AgentTurnEnded {
            session_id: session,
            turn_task_id: turn,
            outcome: AgentTurnOutcome::Completed
        }]
    );
    set(&store, &fixture.turn, "failed", "failed");
    assert_eq!(asked(&store).ended[0].outcome, AgentTurnOutcome::Failed);
    set(&store, &fixture.turn, "recovery_required", "recovery_required");
    assert_eq!(asked(&store).ended[0].outcome, AgentTurnOutcome::RecoveryRequired);
}
