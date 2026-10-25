#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::*;
use aura_persistence_core::PersistenceCoreError;
use aura_persistence_v2::{
    V2Store,
    agent::{desktop, turns},
};

fn completed(store: &mut V2Store, fixture: &support::Fixture, report: String) {
    store
        .database
        .unit_of_work(|tx| {
            let before = turns::load(tx, fixture.turn.turn_task_id)?;
            let mut done = before.clone();
            done.transition(
                before.revision,
                TurnTaskState::Completed { final_report: report },
                fixture.at,
            )
            .expect("complete");
            turns::save_completed(tx, &before, &done)?;
            tx.execute(
                "DELETE FROM turn_poll WHERE id=?1",
                [fixture.turn.turn_task_id.to_string()],
            )?;
            tx.execute(
                "DELETE FROM agent_visible_body WHERE owner=?1 AND source='completion'",
                [fixture.turn.turn_task_id.to_string()],
            )?;
            Ok::<_, PersistenceCoreError>(())
        })
        .expect("legacy report");
}

fn exercise_failure(captured: bool) {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let failed = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    completed(&mut store, &failed, "x".repeat(1_048_576));
    let mut cursor = desktop::PollBackfillCursor::default();
    if captured {
        store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut cursor))
            .expect("first slice");
    }
    let healthy =
        support::Fixture::preflight_in_workspace(&mut store, AgentTaskMode::Normal, "C:/second");
    completed(&mut store, &healthy, "healthy report".into());
    let bad = failed.turn.turn_task_id.to_string();
    store.database.connection().execute_batch(&format!(
        "CREATE TRIGGER fail_one_owner BEFORE INSERT ON agent_visible_body WHEN NEW.owner='{bad}' AND NEW.source='completion' BEGIN SELECT RAISE(ABORT,'per-owner temporary failure'); END;"
    )).expect("fault injection");
    // A restart resets the scheduler, including when the faulty owner is first.
    cursor = desktop::PollBackfillCursor::default();
    let mut failures = 0;
    for _ in 0..6 {
        if store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut cursor))
            .is_err()
        {
            failures += 1;
            assert_eq!(cursor.current().expect("failed owner").0, bad);
        }
    }
    assert!(failures > 0, "the real write failure must have been reached");
    let present: bool = store
        .database
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM turn_poll WHERE id=?1)",
            [healthy.turn.turn_task_id.to_string()],
            |r| r.get(0),
        )
        .expect("healthy poll");
    assert!(present, "one failing owner must not starve a not-yet-captured turn");
    store
        .database
        .connection()
        .execute_batch("DROP TRIGGER fail_one_owner")
        .expect("restore writes");
    for _ in 0..8 {
        if store
            .database
            .unit_of_work(|tx| desktop::backfill_turn_poll(tx, 262_144, &mut cursor))
            .expect("recovered")
            == 0
        {
            break;
        }
    }
    let present: bool = store
        .database
        .connection()
        .query_row("SELECT EXISTS(SELECT 1 FROM turn_poll WHERE id=?1)", [&bad], |r| r.get(0))
        .expect("recovered poll");
    assert!(present, "the skipped owner must be retried after recovery");
    assert!(
        matches!(turns::load(store.database.connection(), failed.turn.turn_task_id).expect("authority").state,
        TurnTaskState::Completed { final_report } if final_report.len() == 1_048_576)
    );
}

#[test]
fn failed_existing_capture_does_not_starve_a_new_owner() {
    exercise_failure(true);
}
#[test]
fn failed_initial_capture_does_not_starve_a_new_owner() {
    exercise_failure(false);
}
