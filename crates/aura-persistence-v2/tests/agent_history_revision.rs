//! The history poll's revision is the session's own: a turn or run of another session moving does
//! not change it, and its own run moving does.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::StoreResult;
use aura_persistence_v2::{
    V2Store,
    agent::{desktop, turns},
};

fn revision(store: &V2Store, session: SessionId) -> Revision {
    desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            session_id: session,
            before_turn_task_id: None,
            within_turn: None,
            limit: 10,
        },
        RuntimeInstanceId::new(),
    )
    .unwrap()
    .revision
}

/// One state-only transition of a run, writing no event.
fn advance(store: &mut V2Store, run: AgentRunId) {
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            let previous = turns::load_run(tx, run)?;
            let mut next = previous.clone();
            next.revision = Revision::new(previous.revision.value() + 1);
            turns::save_run(tx, &previous, &next)
        })
        .unwrap();
}

#[test]
fn another_sessions_runs_do_not_move_this_sessions_revision() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let mine = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let other = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/other");
    let before = revision(&store, mine.turn.session_id);
    advance(&mut store, other.turn.agent_run_id);
    assert_eq!(revision(&store, mine.turn.session_id), before);
    advance(&mut store, mine.turn.agent_run_id);
    assert!(revision(&store, mine.turn.session_id) > before);
}
