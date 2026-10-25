//! What one more delta of a long live stream costs to store.
//!
//! A real 8192-token reasoning response arrives as about 8000 deltas. Each append used to
//! re-read the step's whole stream: two JSON scans of every earlier delta event and a gap
//! check that walks every index row, twice. The n-th append cost O(n), the response O(n²),
//! and Core needed four to five minutes to store a stream the Provider had finished in 40 s.
//!
//! The work is counted as SQLite virtual-machine steps, which do not depend on how busy the
//! machine is. A late append of a long stream must stay close to an early one; before the
//! fix one append after 3000 deltas executed about 380 000 steps.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::{ModelStreamEvent, ModelStreamEventEnvelope};
use aura_persistence_v2::{V2Store, agent::stream, model};
use rusqlite::{
    StatementStatus,
    trace::{TraceEvent, TraceEventCodes},
};
use std::{cell::Cell, time::Instant};

thread_local! {
    static STEPS: Cell<i64> = const { Cell::new(0) };
}

fn count_steps(event: TraceEvent<'_>) {
    if let TraceEvent::Profile(statement, _) = event {
        let steps = i64::from(statement.get_status(StatementStatus::VmStep));
        STEPS.with(|total| total.set(total.get() + steps));
    }
}

fn setup() -> (tempfile::TempDir, V2Store, support::Fixture, TransportAttemptId) {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let snapshot =
        model::step_snapshot(store.database.connection(), fixture.step.model_step_id).unwrap();
    let attempt = aura_contract_schema::canonical_fixture()
        .unwrap()
        .models
        .transport_attempt
        .expect("transport attempt fixture");
    store.database.unit_of_work(|tx| model::insert_transport(tx, snapshot.id, &attempt)).unwrap();
    (root, store, fixture, attempt.id)
}

fn append(store: &mut V2Store, fixture: &support::Fixture, attempt: TransportAttemptId, n: u64) {
    let event = ModelStreamEventEnvelope {
        model_step_id: fixture.step.model_step_id,
        transport_attempt_id: attempt,
        sequence: n,
        received_at: fixture.at,
        event: ModelStreamEvent::ReasoningDelta {
            block_id: ModelContentBlockId::new(),
            text: "tok ".into(),
        },
    };
    store.database.unit_of_work(|tx| stream::append(tx, &event)).unwrap();
}

/// The average SQLite steps of `count` appends starting at sequence `from`.
fn steps_per_append(
    store: &mut V2Store,
    fixture: &support::Fixture,
    attempt: TransportAttemptId,
    from: u64,
    count: u64,
) -> i64 {
    STEPS.with(|total| total.set(0));
    store.database.connection().trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(count_steps));
    let started = Instant::now();
    for n in from..from + count {
        append(store, fixture, attempt, n);
    }
    let elapsed = started.elapsed();
    store.database.connection().trace_v2(TraceEventCodes::empty(), None);
    let steps = STEPS.with(Cell::get) / i64::try_from(count).unwrap();
    eprintln!("appends {from}..{}: {steps} steps each, {elapsed:?} in total", from + count);
    steps
}

#[test]
fn a_late_delta_of_a_long_stream_costs_about_the_same_as_an_early_one() {
    let (_root, mut store, fixture, attempt) = setup();
    let early = steps_per_append(&mut store, &fixture, attempt, 1, 100);
    for n in 101..3_000 {
        append(&mut store, &fixture, attempt, n);
    }
    let late = steps_per_append(&mut store, &fixture, attempt, 3_000, 100);
    // Counting the index rows to detect a removed one is the only work that still grows,
    // by a few steps per row; re-reading the stream grew by more than a hundred per row.
    assert!(
        late < early * 4 + 20_000,
        "an append after 3000 deltas ran {late} SQLite steps; an early one ran {early}"
    );
    let body = stream::partial(store.database.connection(), fixture.step.model_step_id).unwrap();
    assert_eq!(body.reasoning.len(), 3_099 * 4, "the stored stream is still the whole body");
    assert_eq!(body.sequence, Some(3_099));
}
