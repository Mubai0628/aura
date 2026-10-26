//! What deleting a Session costs the store's writer next to the rest of the store.
//!
//! A purge deletes every event of the Session, and before deleting a row SQLite looks for rows
//! that still point at it. `projection_outbox` points at events through a column with no index,
//! so each deleted event meant reading the whole outbox. A real test deleted a Session
//! with 55 000 events next to 300 000 outbox rows: the purge would have run for about 40 minutes
//! with the execution store locked. Later the whole purge ran in one transaction of up to 14
//! seconds, and every other request to the store waited for it.
//!
//! The work is counted as SQLite virtual-machine steps, which do not depend on how busy the
//! machine is. The plan is read on a reader; on the writer, no single step of purging the same
//! Session may cost more next to a long history of another Session than next to a short one. The
//! purge must leave no row pointing at a deleted one and the schema the store verifies on open
//! unchanged.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::{ModelStreamEvent, ModelStreamEventEnvelope};
use aura_persistence_v2::{V2Store, agent::stream, model, session_purge};
use rusqlite::{
    StatementStatus,
    trace::{TraceEvent, TraceEventCodes},
};
use std::cell::Cell;
use std::time::Duration;

/// Deltas of the Session that is purged.
const PURGED: u64 = 600;

thread_local! {
    static STEPS: Cell<i64> = const { Cell::new(0) };
    static COSTLIEST: std::cell::RefCell<(i64, String)> = const { std::cell::RefCell::new((0, String::new())) };
}

fn count_steps(event: TraceEvent<'_>) {
    if let TraceEvent::Profile(statement, _) = event {
        let steps = i64::from(statement.get_status(StatementStatus::VmStep));
        STEPS.with(|total| total.set(total.get() + steps));
        COSTLIEST.with(|costliest| {
            let mut costliest = costliest.borrow_mut();
            if steps > costliest.0 {
                *costliest = (steps, statement.sql().chars().take(160).collect());
            }
        });
    }
}

/// A Session in its own Workspace whose model step streamed `deltas` deltas: each one an event
/// with its outbox row.
fn streamed(store: &mut V2Store, root: &str, deltas: u64) -> (SessionId, ModelStepId) {
    let fixture = support::Fixture::new_in_workspace(store, AgentTaskMode::Normal, root);
    let snapshot =
        model::step_snapshot(store.database.connection(), fixture.step.model_step_id).unwrap();
    let mut attempt = aura_contract_schema::canonical_fixture()
        .unwrap()
        .models
        .transport_attempt
        .expect("transport attempt fixture");
    attempt.id = TransportAttemptId::new();
    store.database.unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt)).unwrap();
    for sequence in 1..=deltas {
        let event = ModelStreamEventEnvelope {
            model_step_id: fixture.step.model_step_id,
            transport_attempt_id: attempt.id,
            sequence,
            received_at: fixture.at,
            event: ModelStreamEvent::AssistantTextDelta {
                block_id: ModelContentBlockId::new(),
                text: "tok ".into(),
            },
        };
        store.database.unit_of_work(|tx| stream::append(tx, &event)).unwrap();
    }
    (fixture.turn.session_id, fixture.step.model_step_id)
}

fn schema(store: &V2Store) -> Vec<(String, String, Option<String>)> {
    let db = store.database.connection();
    let mut statement =
        db.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name").unwrap();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    rows.collect::<Result<_, _>>().unwrap()
}

fn events_of(store: &V2Store, step: ModelStepId) -> i64 {
    store
        .database
        .connection()
        .query_row("SELECT count(*) FROM events WHERE owner=?1", [step.to_string()], |row| {
            row.get(0)
        })
        .unwrap()
}

fn dangling(store: &V2Store) -> Vec<(String, i64, String)> {
    let db = store.database.connection();
    let mut statement =
        db.prepare("SELECT \"table\", rowid, parent FROM pragma_foreign_key_check").unwrap();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    rows.collect::<Result<_, _>>().unwrap()
}

/// The SQLite steps of the costliest write step, and the number of steps, of purging a Session
/// of `PURGED` deltas next to a Session of `beside`.
fn purge_steps(beside: u64) -> (i64, usize) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("v2");
    let mut store = V2Store::create(&path).unwrap();
    let (purged, purged_step) = streamed(&mut store, "C:/purged", PURGED);
    let (_, kept_step) = streamed(&mut store, "C:/kept", beside);
    let kept_events = events_of(&store, kept_step);
    assert!(events_of(&store, purged_step) >= i64::try_from(PURGED).unwrap());
    let before = schema(&store);

    let mut plan = session_purge::plan_session(store.database.connection(), purged).unwrap();
    store.database.connection().trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(count_steps));
    let (mut costliest, mut steps) = (0, 0);
    while !plan.done() {
        STEPS.with(|total| total.set(0));
        // One delete statement per step: the smallest step the writer can be held for.
        session_purge::step(&mut store.database, &mut plan, Duration::ZERO).unwrap();
        costliest = costliest.max(STEPS.with(Cell::get));
        steps += 1;
    }
    store.database.connection().trace_v2(TraceEventCodes::empty(), None);
    eprintln!(
        "purge beside {beside} deltas: {steps} steps, the costliest {costliest} SQLite steps, {} rows;          costliest statement {:?}",
        plan.report().rows,
        COSTLIEST.with(|costliest| costliest.borrow().clone()),
    );
    assert_eq!(plan.skipped(), 0, "nothing outside the Session points at its rows");

    assert_eq!(events_of(&store, purged_step), 0, "the purged Session's events are gone");
    assert_eq!(events_of(&store, kept_step), kept_events, "the other Session keeps its events");
    assert_eq!(dangling(&store), Vec::new(), "no row points at a deleted one");
    assert_eq!(schema(&store), before, "a purge leaves the schema as it found it");
    let keys: i64 =
        store.database.connection().query_row("PRAGMA foreign_keys", [], |row| row.get(0)).unwrap();
    assert_eq!(keys, 1, "the writer enforces foreign keys again");
    drop(store);
    V2Store::open(&path).expect("the store still opens after a purge");
    (costliest, steps)
}

#[test]
fn no_purge_step_costs_more_beside_a_long_history() {
    let (short, _) = purge_steps(50);
    let (long, steps) = purge_steps(3_000);
    // A step deletes at most one chunk of rows by row ID and checks the keys that point at them;
    // what another Session has written does not enter it.
    assert!(
        long < short * 2,
        "a purge step beside 3000 deltas of another Session ran {long} SQLite steps; beside 50          the costliest ran {short}"
    );
    assert!(steps > 1, "the purge was split into steps");
}
