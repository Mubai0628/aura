//! Stopping a turn, recording a failed step and settling a run after a restart all close the run's
//! unfinished model requests. Their lookup tested every request in the store against every
//! compaction's JSON, so a stop cost the product of the two: 2.8 s on a developer's store.
//!
//! A stop may read each request and each compaction of other runs once, never once per pair.
//! SQLite counts the virtual-machine instructions it runs; the stop is counted on the fixture, and
//! again after other runs' requests and compactions are added.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::AgentTaskMode;
use aura_persistence_v2::{V2Store, agent::recovery};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Requests of other runs.
const REQUESTS: u64 = 400;
/// Compactions of other runs, each naming one request.
const COMPACTIONS: u64 = 100;
/// Instructions per progress callback.
const STEP: i32 = 100;

fn instructions(store: &V2Store, work: impl FnOnce(&rusqlite::Connection)) -> u64 {
    let calls = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&calls);
    let db = store.database.connection();
    db.progress_handler(
        STEP,
        Some(move || {
            seen.fetch_add(1, Ordering::Relaxed);
            false
        }),
    )
    .unwrap();
    work(db);
    db.progress_handler(STEP, None::<fn() -> bool>).unwrap();
    calls.load(Ordering::Relaxed) * STEP as u64
}

/// Finished requests of another session's runs, and compactions of those runs.
fn other_runs(store: &V2Store) {
    let db = store.database.connection();
    db.pragma_update(None, "foreign_keys", "OFF").unwrap();
    db.execute_batch(&format!(
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{REQUESTS}) \
         INSERT INTO model_execution_snapshots(id, session_id, endpoint_id, profile_id, payload) \
         SELECT 'mes_other_'||x, 'ses_other', 'endpoint', 'profile', '{{}}' FROM n; \
         WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{REQUESTS}) \
         INSERT INTO transport_attempts(id, snapshot_id, state, revision, payload) \
         SELECT 'tra_other_'||x, 'mes_other_'||x, 'completed', 1, '{{}}' FROM n; \
         WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{COMPACTIONS}) \
         INSERT INTO context_compactions(id, session_id, state, revision, payload) \
         SELECT 'cps_other_'||x, 'ses_other', 'completed', 1, json_object( \
           'agent_run_id', 'agt_other_'||x, \
           'attempts', json_array(json_object('model_execution_snapshot_id', 'mes_other_'||x)), \
           'excluded_event_ids', json_array('evt_a_'||x, 'evt_b_'||x, 'evt_c_'||x)) \
         FROM n;"
    ))
    .unwrap();
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
}

#[test]
fn a_stop_reads_other_runs_requests_and_compactions_at_most_once_each() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let run = fixture.turn.agent_run_id;
    // Each count is taken in a transaction that is rolled back, so the run is still running for
    // the second one.
    let stop = |store: &V2Store| {
        instructions(store, |db| {
            let tx = db.unchecked_transaction().unwrap();
            recovery::interrupt(&tx, run, "agent.user_stopped", true, fixture.at).unwrap();
            tx.rollback().unwrap();
        })
    };
    let alone = stop(&store);
    other_runs(&store);
    let beside_others = stop(&store);
    // Reading every one of them once costs a few dozen instructions each; once per pair, it is
    // thousands for each request.
    let once_each = 40 * (REQUESTS + COMPACTIONS);
    assert!(
        beside_others <= alone + once_each,
        "a stop ran {alone} SQLite instructions alone and {beside_others} beside {REQUESTS} \
         requests and {COMPACTIONS} compactions of other runs"
    );
}
