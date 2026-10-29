//! What the background repair of visible streams costs on a large journal.
//!
//! A long-used store holds about a million events: 711 model steps, each with close to 1 MB of
//! transport events. Steps that never streamed visible text have no coverage, and finding the
//! next stream to repair asked each of them whether it held a delta, reading every one of its
//! events. That ran on every pass, every 200 ms or 2 s, inside a write transaction on the one
//! store owner: 0.3–1.1 s a pass warm, 60 s once on a loaded machine.
//!
//! Copying one stream was quadratic in its deltas: every piece looked its delta up again by
//! scanning all of the step's events and evaluating the payload's JSON on each.
//!
//! The work is counted as SQLite virtual-machine instructions, which do not depend on how busy
//! the machine is. The ignored `measure_*` test times the same passes on a journal shaped like
//! that store.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::{ModelStreamEvent, ModelStreamEventEnvelope};
use aura_persistence_v2::{V2Store, agent::stream, model};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

/// Instructions per progress callback.
const STEP: i32 = 1_000;

fn instructions<T>(store: &mut V2Store, work: impl FnOnce(&mut V2Store) -> T) -> (T, u64) {
    let calls = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&calls);
    store
        .database
        .connection()
        .progress_handler(
            STEP,
            Some(move || {
                seen.fetch_add(1, Ordering::Relaxed);
                false
            }),
        )
        .unwrap();
    let value = work(store);
    store.database.connection().progress_handler(STEP, None::<fn() -> bool>).unwrap();
    (value, calls.load(Ordering::Relaxed) * STEP as u64)
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

/// Deltas as a version before the derived tables stored them: events only.
fn legacy_deltas(
    store: &mut V2Store,
    owner: &str,
    attempt: TransportAttemptId,
    sequences: std::ops::RangeInclusive<u64>,
    text: &str,
) {
    store
        .database
        .unit_of_work(|tx| {
            for sequence in sequences {
                let payload = serde_json::json!({
                    "attempt": attempt,
                    "sequence": sequence,
                    "text": text,
                    "reasoning": "",
                });
                aura_persistence_core::event::append(
                    tx,
                    &aura_persistence_core::event::EventDraft {
                        id: EventId::new(),
                        owner,
                        kind: "agent.visible_delta",
                        version: 1,
                        payload: &payload,
                        at: UtcTimestamp::now(),
                    },
                    "agent",
                )?;
            }
            Ok::<_, aura_persistence_core::PersistenceCoreError>(())
        })
        .unwrap();
}

/// Appends one delta the way the receiver delivers it today.
fn append(
    store: &mut V2Store,
    fixture: &support::Fixture,
    attempt: TransportAttemptId,
    sequence: u64,
    text: &str,
) {
    let event = ModelStreamEventEnvelope {
        model_step_id: fixture.step.model_step_id,
        transport_attempt_id: attempt,
        sequence,
        received_at: fixture.at,
        event: ModelStreamEvent::AssistantTextDelta {
            block_id: ModelContentBlockId::new(),
            text: text.into(),
        },
    };
    store.database.unit_of_work(|tx| stream::append(tx, &event)).unwrap();
}

/// Model steps of the fixture's run that never streamed visible text, each holding
/// `events` transport events of `pad` bytes, the shape most steps of such a store have.
fn steps_without_a_stream(
    store: &V2Store,
    fixture: &support::Fixture,
    steps: u32,
    events: u32,
    pad: usize,
) -> Vec<String> {
    let connection = store.database.connection();
    let padding = "x".repeat(pad);
    let mut ids = Vec::new();
    for ordinal in 0..steps {
        let id = ModelStepId::new().to_string();
        connection
            .execute(
                "INSERT INTO model_steps(id, agent_run_id, ordinal, snapshot_id, state, payload) \
                 VALUES(?1, ?2, ?3, NULL, 'completed', '{}')",
                rusqlite::params![
                    id,
                    fixture.turn.agent_run_id.to_string(),
                    i64::from(ordinal) + 100
                ],
            )
            .unwrap();
        connection
            .execute(
                "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<?2) \
                 INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
                 SELECT ?1||'_transport_'||x, ?1, 'model.transport', 1, json_object('pad', ?3), \
                 '2026-10-27T00:00:00Z' FROM n",
                rusqlite::params![id, i64::from(events), padding],
            )
            .unwrap();
        ids.push(id);
    }
    ids
}

/// More steps of the fixture's run on the fixture step's snapshot, so they share its transport
/// attempt and their deltas can be copied.
fn steps_sharing_the_attempt(
    store: &V2Store,
    fixture: &support::Fixture,
    count: u32,
) -> Vec<String> {
    let connection = store.database.connection();
    let snapshot = model::step_snapshot(connection, fixture.step.model_step_id).unwrap().id;
    let mut ids: Vec<String> = (0..count)
        .map(|ordinal| {
            let id = ModelStepId::new().to_string();
            connection
                .execute(
                    "INSERT INTO model_steps(id, agent_run_id, ordinal, snapshot_id, state, payload) \
                     VALUES(?1, ?2, ?3, ?4, 'completed', '{}')",
                    rusqlite::params![
                        id,
                        fixture.turn.agent_run_id.to_string(),
                        i64::from(ordinal) + 50,
                        snapshot.to_string()
                    ],
                )
                .unwrap();
            id
        })
        .collect();
    ids.sort();
    ids
}

fn turn(store: &V2Store) -> Option<String> {
    use rusqlite::OptionalExtension;
    store
        .database
        .connection()
        .query_row("SELECT owner FROM agent_visible_repair_turn WHERE id=1", [], |row| row.get(0))
        .optional()
        .unwrap()
}

fn discover(store: &V2Store, scan: &mut stream::RepairScan) {
    stream::discover_outstanding(store.database.connection(), scan, None).unwrap();
}

fn visible_rows(store: &V2Store, owner: &str) -> (i64, i64, bool) {
    let connection = store.database.connection();
    let deltas: i64 = connection
        .query_row("SELECT COUNT(*) FROM agent_visible_delta WHERE owner=?1", [owner], |row| {
            row.get(0)
        })
        .unwrap();
    let index: i64 = connection
        .query_row("SELECT COUNT(*) FROM agent_visible_index WHERE owner=?1", [owner], |row| {
            row.get(0)
        })
        .unwrap();
    let covered: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_visible_coverage WHERE owner=?1)",
            [owner],
            |row| row.get(0),
        )
        .unwrap();
    (deltas, index, covered)
}

/// One pass copies a legacy stream of `deltas` four-byte deltas, all inside the budget.
fn copy_pass(deltas: u64) -> u64 {
    let (_root, mut store, fixture, attempt) = setup();
    let owner = fixture.step.model_step_id.to_string();
    legacy_deltas(&mut store, &owner, attempt, 1..=deltas, "tok ");
    let at = fixture.at;
    let (repaired, cost) = instructions(&mut store, |store| {
        store.database.unit_of_work(|tx| stream::repair_outstanding(tx, at, 262_144)).unwrap()
    });
    assert_eq!(repaired, 1, "the pass copied the stream");
    let expected = i64::try_from(deltas).unwrap();
    assert_eq!(
        visible_rows(&store, &owner),
        (expected, expected, true),
        "every delta was copied and indexed, and the stream is covered"
    );
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 0, 64)
            .unwrap();
    assert_eq!(page.text.text, "tok ".repeat(16));
    assert_eq!(page.text.total_bytes, deltas * 4);
    cost
}

/// A pass over a step with thousands of deltas is linear in them. Each piece used to look its
/// delta up by scanning the step's events, so four times the deltas cost about sixteen times
/// the work.
#[test]
fn a_repair_pass_over_thousands_of_deltas_stays_linear() {
    let small = copy_pass(500);
    let large = copy_pass(2_000);
    assert!(
        large < small * 6,
        "four times the deltas must cost about four times the work: 500 -> {small}, \
         2000 -> {large} instructions"
    );
}

/// A covered stream whose first locatable row went missing is listed by the repair. Its deltas
/// are already in the locatable table, so nothing is copied; the index is published again.
/// The copy used to insert those deltas a second time and failed on their key every round.
#[test]
fn a_covered_stream_that_lost_its_first_index_row_is_published_again() {
    let (_root, mut store, fixture, attempt) = setup();
    let owner = fixture.step.model_step_id.to_string();
    for (sequence, text) in [(1, "AAA"), (2, "BBB"), (3, "CCC")] {
        append(&mut store, &fixture, attempt, sequence, text);
    }
    store
        .database
        .connection()
        .execute("DELETE FROM agent_visible_index WHERE owner=?1 AND sequence=1", [&owner])
        .unwrap();
    let at = fixture.at;
    store
        .database
        .unit_of_work(|tx| stream::repair_outstanding(tx, at, 262_144))
        .expect("the repair publishes the index instead of copying the deltas again");
    assert_eq!(visible_rows(&store, &owner), (3, 3, true));
    let page =
        stream::partial_windows(store.database.connection(), fixture.step.model_step_id, 0, 64)
            .unwrap();
    assert_eq!(page.text.text, "AAABBBCCC");
}

fn steady_discovery_cost(events: u32) -> u64 {
    let (_root, mut store, fixture, _attempt) = setup();
    steps_without_a_stream(&store, &fixture, 20, events, 16);
    let mut scan = stream::RepairScan::default();
    discover(&store, &mut scan);
    assert!(!scan.sweeping() && !scan.has_outstanding(), "the sweep finished and found nothing");
    let (_, cost) = instructions(&mut store, |store| discover(store, &mut scan));
    cost
}

/// Once the sweep has asked every step without coverage, finding what is left reads the events
/// added since, so it costs the same however many events those steps hold.
#[test]
fn once_swept_finding_streams_reads_none_of_the_steps_events() {
    let few = steady_discovery_cost(100);
    let many = steady_discovery_cost(2_000);
    assert!(
        many <= few + 5_000,
        "an idle pass read the steps' events again: 100 events each -> {few}, \
         2000 -> {many} instructions"
    );
}

/// With its deadline gone the sweep asks one step and lets go, so a store with many steps is
/// swept over many short reads. It still finds a stream stored before the index.
#[test]
fn the_sweep_runs_in_slices_and_finds_a_stream_stored_before_the_index() {
    let (_root, mut store, fixture, attempt) = setup();
    let steps = steps_without_a_stream(&store, &fixture, 30, 50, 16);
    let owner = fixture.step.model_step_id.to_string();
    legacy_deltas(&mut store, &owner, attempt, 1..=3, "abc ");
    let mut scan = stream::RepairScan::default();
    let mut calls = 0;
    while scan.sweeping() {
        stream::discover_outstanding(store.database.connection(), &mut scan, Some(Instant::now()))
            .unwrap();
        calls += 1;
        assert!(calls <= steps.len() + 3, "the sweep ends");
    }
    assert!(calls > steps.len(), "one step per slice: {calls} calls for {} steps", steps.len());
    assert!(scan.has_outstanding(), "the legacy stream was found");
    let at = fixture.at;
    assert_eq!(stream::repair_pass(&mut store.database, &mut scan, at, 262_144).unwrap(), 1);
    assert_eq!(visible_rows(&store, &owner), (3, 3, true));
    assert!(!scan.has_outstanding(), "a finished stream leaves the scan");
}

/// Each pass takes the next outstanding owner after the cursor, and wraps around.
#[test]
fn the_round_robin_serves_each_outstanding_owner_in_turn() {
    let (_root, mut store, fixture, attempt) = setup();
    let owners = steps_sharing_the_attempt(&store, &fixture, 3);
    for owner in &owners {
        legacy_deltas(&mut store, owner, attempt, 1..=2, "four");
    }
    let mut scan = stream::RepairScan::default();
    discover(&store, &mut scan);
    let mut served = Vec::new();
    // Four bytes a pass: one delta of one owner.
    while scan.has_outstanding() {
        stream::repair_pass(&mut store.database, &mut scan, fixture.at, 4).unwrap();
        served.push(scan.current().unwrap().to_owned());
        assert_eq!(turn(&store).as_deref(), scan.current(), "the cursor row follows the pass");
        assert!(served.len() <= 6, "served {served:?}");
    }
    let rounds: Vec<String> = owners.iter().chain(&owners).cloned().collect();
    assert_eq!(served, rounds);
    for owner in &owners {
        assert_eq!(visible_rows(&store, owner), (2, 2, true));
    }
}

/// A copy that fails rolls back, the caller moves the cursor past its owner, and the next pass
/// serves the next owner. The failed owner comes round again.
#[test]
fn a_failed_copy_is_moved_past_and_the_next_owner_is_served() {
    let (_root, mut store, fixture, attempt) = setup();
    let owners = steps_sharing_the_attempt(&store, &fixture, 2);
    for owner in &owners {
        legacy_deltas(&mut store, owner, attempt, 1..=1, "text");
    }
    store
        .database
        .connection()
        .execute_batch(&format!(
            "CREATE TEMP TRIGGER refuse_first BEFORE INSERT ON agent_visible_delta \
             WHEN NEW.owner='{}' BEGIN SELECT RAISE(ABORT,'synthetic copy fault'); END;",
            owners[0]
        ))
        .unwrap();
    let at = fixture.at;
    let mut scan = stream::RepairScan::default();
    discover(&store, &mut scan);
    assert!(stream::repair_pass(&mut store.database, &mut scan, at, 262_144).is_err());
    assert_eq!(scan.current(), Some(owners[0].as_str()), "the failed owner is known");
    assert_eq!(turn(&store), None, "the failed pass rolled its cursor back");
    let failed = scan.current().unwrap().to_owned();
    store.database.unit_of_work(|tx| stream::advance_repair_cursor(tx, &failed)).unwrap();
    assert_eq!(stream::repair_pass(&mut store.database, &mut scan, at, 262_144).unwrap(), 1);
    assert_eq!(scan.current(), Some(owners[1].as_str()), "the next owner is served");
    assert_eq!(visible_rows(&store, &owners[1]), (1, 1, true));
    store.database.connection().execute_batch("DROP TRIGGER refuse_first").unwrap();
    assert_eq!(stream::repair_pass(&mut store.database, &mut scan, at, 262_144).unwrap(), 1);
    assert_eq!(scan.current(), Some(owners[0].as_str()), "the failed owner comes round again");
    assert_eq!(visible_rows(&store, &owners[0]), (1, 1, true));
    assert!(!scan.has_outstanding());
}

/// A stream whose deltas all belong to an earlier attempt has nothing a copy of the latest
/// attempt can take. It no longer costs a write section on every pass: the scan lets it go
/// until a delta of its own appears.
#[test]
fn a_stream_with_nothing_to_copy_waits_for_its_own_next_delta() {
    let (_root, mut store, fixture, attempt) = setup();
    let owner = fixture.step.model_step_id.to_string();
    legacy_deltas(&mut store, &owner, TransportAttemptId::new(), 1..=2, "old ");
    let at = fixture.at;
    let mut scan = stream::RepairScan::default();
    discover(&store, &mut scan);
    assert!(scan.has_outstanding());
    assert_eq!(stream::repair_pass(&mut store.database, &mut scan, at, 262_144).unwrap(), 0);
    assert!(!scan.has_outstanding(), "nothing to copy: the scan lets it go");
    discover(&store, &mut scan);
    assert!(!scan.has_outstanding(), "and does not find it again without a new delta");
    legacy_deltas(&mut store, &owner, attempt, 1..=1, "new ");
    discover(&store, &mut scan);
    assert!(scan.has_outstanding(), "its own next delta brings it back");
    assert_eq!(stream::repair_pass(&mut store.database, &mut scan, at, 262_144).unwrap(), 1);
    assert_eq!(visible_rows(&store, &owner), (1, 1, true));
}

/// Coverage is dropped where an index repair finds a hole, in the transaction that appends the
/// stream's epoch event and no delta. After the sweep, that epoch is how the stream is found;
/// its deltas are already stored, so the pass publishes the index again without copying.
#[test]
fn a_stream_whose_coverage_was_dropped_is_found_from_its_epoch() {
    let (_root, mut store, fixture, attempt) = setup();
    let owner = fixture.step.model_step_id.to_string();
    for (sequence, text) in [(1, "AAA"), (2, "BBB")] {
        append(&mut store, &fixture, attempt, sequence, text);
    }
    let mut scan = stream::RepairScan::default();
    discover(&store, &mut scan);
    assert!(!scan.sweeping() && !scan.has_outstanding(), "a covered stream is not outstanding");
    let epoch =
        serde_json::json!({ "attempt": attempt, "through_sequence": 2, "unit": "utf8_bytes" });
    store
        .database
        .unit_of_work(|tx| {
            tx.execute("DELETE FROM agent_visible_coverage WHERE owner=?1", [&owner])?;
            aura_persistence_core::event::append(
                tx,
                &aura_persistence_core::event::EventDraft {
                    id: EventId::new(),
                    owner: &owner,
                    kind: "agent.visible_delta_epoch",
                    version: 1,
                    payload: &epoch,
                    at: fixture.at,
                },
                "agent",
            )
        })
        .unwrap();
    discover(&store, &mut scan);
    assert!(scan.has_outstanding(), "the epoch event brought the stream back");
    assert_eq!(
        stream::repair_pass(&mut store.database, &mut scan, fixture.at, 262_144).unwrap(),
        1
    );
    assert_eq!(visible_rows(&store, &owner), (2, 2, true));
    assert!(!scan.has_outstanding());
}

fn env_or(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|value| value.parse().ok()).unwrap_or(default)
}

fn millis(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000.0
}

/// Times the passes on a journal shaped like a long-used store. Run with
/// `cargo test -p aura-persistence-v2 --test agent_stream_repair_cost -- --ignored --nocapture`;
/// `AURA_MEASURE_STEPS`, `AURA_MEASURE_EVENTS`, `AURA_MEASURE_PAD` and `AURA_MEASURE_DELTAS`
/// change its size.
#[test]
#[ignore = "measurement on a large journal; run explicitly"]
fn measure_one_shot_repair_on_a_large_journal() {
    let steps = u32::try_from(env_or("AURA_MEASURE_STEPS", 711)).unwrap();
    let events = u32::try_from(env_or("AURA_MEASURE_EVENTS", 1_450)).unwrap();
    let pad = usize::try_from(env_or("AURA_MEASURE_PAD", 600)).unwrap();
    let deltas = env_or("AURA_MEASURE_DELTAS", 2_000);
    let (_root, mut store, fixture, attempt) = setup();
    let built = Instant::now();
    store.database.connection().execute_batch("BEGIN").unwrap();
    steps_without_a_stream(&store, &fixture, steps, events, pad);
    store.database.connection().execute_batch("COMMIT").unwrap();
    let total: i64 = store
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    println!(
        "MEASURE fixture steps={steps} events_per_step={events} pad={pad} events_total={total} \
         built_ms={:.0}",
        millis(built)
    );
    let at = fixture.at;
    for round in 0..3 {
        let started = Instant::now();
        let (repaired, cost) = instructions(&mut store, |store| {
            store.database.unit_of_work(|tx| stream::repair_outstanding(tx, at, 262_144)).unwrap()
        });
        println!(
            "MEASURE one_shot_idle_pass round={round} repaired={repaired} ms={:.1} \
             instructions={cost}",
            millis(started)
        );
    }
    let owner = fixture.step.model_step_id.to_string();
    legacy_deltas(&mut store, &owner, attempt, 1..=deltas, "tok ");
    let started = Instant::now();
    let (repaired, cost) = instructions(&mut store, |store| {
        store.database.unit_of_work(|tx| stream::repair_outstanding(tx, at, 262_144)).unwrap()
    });
    println!(
        "MEASURE one_shot_copy_pass deltas={deltas} repaired={repaired} ms={:.1} \
         instructions={cost} rows={:?}",
        millis(started),
        visible_rows(&store, &owner)
    );
}

/// The same journal through a [`stream::RepairScan`], as the Core task runs it: the first sweep
/// in 50 ms slices, an idle pass after it, and a pass after a stream appears.
#[test]
#[ignore = "measurement on a large journal; run explicitly"]
fn measure_scan_passes_on_a_large_journal() {
    let steps = u32::try_from(env_or("AURA_MEASURE_STEPS", 711)).unwrap();
    let events = u32::try_from(env_or("AURA_MEASURE_EVENTS", 1_450)).unwrap();
    let pad = usize::try_from(env_or("AURA_MEASURE_PAD", 600)).unwrap();
    let deltas = env_or("AURA_MEASURE_DELTAS", 2_000);
    let (_root, mut store, fixture, attempt) = setup();
    store.database.connection().execute_batch("BEGIN").unwrap();
    steps_without_a_stream(&store, &fixture, steps, events, pad);
    store.database.connection().execute_batch("COMMIT").unwrap();
    let mut scan = stream::RepairScan::default();
    let started = Instant::now();
    let mut slices = 0_u32;
    let mut slowest = 0.0_f64;
    while scan.sweeping() {
        let slice = Instant::now();
        stream::discover_outstanding(
            store.database.connection(),
            &mut scan,
            Some(Instant::now() + Duration::from_millis(50)),
        )
        .unwrap();
        slices += 1;
        slowest = slowest.max(millis(slice));
    }
    println!(
        "MEASURE scan_sweep steps={steps} events_per_step={events} slices={slices} \
         total_ms={:.1} slowest_slice_ms={slowest:.1} outstanding={}",
        millis(started),
        scan.has_outstanding()
    );
    for round in 0..3 {
        let started = Instant::now();
        let (_, cost) = instructions(&mut store, |store| discover(store, &mut scan));
        println!(
            "MEASURE scan_idle_pass round={round} ms={:.3} instructions={cost} outstanding={}",
            millis(started),
            scan.has_outstanding()
        );
    }
    let owner = fixture.step.model_step_id.to_string();
    legacy_deltas(&mut store, &owner, attempt, 1..=deltas, "tok ");
    let started = Instant::now();
    let (_, cost) = instructions(&mut store, |store| discover(store, &mut scan));
    println!(
        "MEASURE scan_after_new_stream deltas={deltas} ms={:.3} instructions={cost} outstanding={}",
        millis(started),
        scan.has_outstanding()
    );
    let at = fixture.at;
    let started = Instant::now();
    let (copied, cost) = instructions(&mut store, |store| {
        stream::repair_pass(&mut store.database, &mut scan, at, 262_144).unwrap()
    });
    println!(
        "MEASURE scan_copy_pass deltas={deltas} progressed={copied} ms={:.1} instructions={cost} \
         rows={:?} outstanding_after={}",
        millis(started),
        visible_rows(&store, &owner),
        scan.has_outstanding()
    );
}
