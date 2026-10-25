//! A step's backup connection is recorded while the
//! step is prepared, so it is found among the step's first events however much the step streamed
//! afterwards — the history read no longer reads every streamed piece of a step to learn it has none.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::event;
use aura_persistence_v2::{V2Store, model::connections};
use rusqlite::{
    StatementStatus,
    trace::{TraceEvent, TraceEventCodes},
};
use std::cell::Cell;

thread_local! {
    static STEPS: Cell<i64> = const { Cell::new(0) };
}

fn count_steps(event: TraceEvent<'_>) {
    if let TraceEvent::Profile(statement, _) = event {
        let steps = i64::from(statement.get_status(StatementStatus::VmStep));
        STEPS.with(|total| total.set(total.get() + steps));
    }
}

/// The SQLite virtual-machine steps of one lookup, which do not depend on how busy the machine is.
fn lookup_steps(store: &V2Store, step: ModelStepId) -> i64 {
    STEPS.with(|total| total.set(0));
    store.database.connection().trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(count_steps));
    connections::backup_of(store.database.connection(), step).unwrap();
    store.database.connection().trace_v2(TraceEventCodes::empty(), None);
    STEPS.with(Cell::get)
}

fn streamed(store: &mut V2Store, step: ModelStepId, count: usize) {
    let at = UtcTimestamp::now();
    store
        .database
        .unit_of_work(|tx| {
            for n in 0..count {
                event::append(
                    tx,
                    &event::EventDraft {
                        id: EventId::new(),
                        owner: &step.to_string(),
                        kind: "test.streamed",
                        version: 1,
                        payload: &serde_json::json!({ "n": n }),
                        at,
                    },
                    "agent",
                )?;
            }
            Ok::<_, aura_persistence_core::PersistenceCoreError>(())
        })
        .unwrap();
}

#[test]
fn a_backup_is_found_among_the_steps_first_events_however_much_it_streamed_after() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let step = fixture.step.model_step_id;
    streamed(&mut store, step, 3);
    assert_eq!(connections::backup_of(store.database.connection(), step).unwrap(), None);
    store
        .database
        .unit_of_work(|tx| connections::record_backup(tx, step, "备用连接", UtcTimestamp::now()))
        .unwrap();
    streamed(&mut store, step, 500);
    assert_eq!(
        connections::backup_of(store.database.connection(), step).unwrap().as_deref(),
        Some("备用连接")
    );
}

#[test]
fn looking_for_a_backup_costs_the_same_however_much_the_step_streamed() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let step = fixture.step.model_step_id;
    streamed(&mut store, step, 100);
    let early = lookup_steps(&store, step);
    streamed(&mut store, step, 5_000);
    let late = lookup_steps(&store, step);
    // Looking from the newest down walked every streamed event: about 50 times the work here.
    assert!(
        late < early * 2,
        "after 5,100 streamed events the lookup ran {late} SQLite steps; after 100 it ran {early}"
    );
}
