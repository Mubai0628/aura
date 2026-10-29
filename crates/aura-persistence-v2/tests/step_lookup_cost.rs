//! Every step looks up the conversation's newest counted request (to measure the next request,
//! and to keep its snapshot) and the context its newest step was sent with (for the step's
//! context details). Both matched every execution snapshot's JSON against the conversation's
//! context snapshots, so each lookup read all of them: 30 MB on a store of a thousand requests,
//! three to seven times a step.
//!
//! What they cost must not depend on how many requests other conversations sent. SQLite counts
//! the virtual-machine instructions it runs; the lookups are counted on the fixture, and again
//! after other conversations' requests are added.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::AgentTaskMode;
use aura_persistence_v2::{
    V2Store,
    agent::{counted_request, inspector},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Requests of other conversations.
const OTHERS: u64 = 2_000;
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

/// Steps of another conversation, each sent with its own execution snapshot.
fn other_conversations_requests(store: &V2Store) {
    let db = store.database.connection();
    db.pragma_update(None, "foreign_keys", "OFF").unwrap();
    db.execute_batch(&format!(
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{OTHERS}) \
         INSERT INTO model_execution_snapshots(id, session_id, endpoint_id, profile_id, payload) \
         SELECT 'mes_other_'||x, 'ses_other', 'endpoint', 'profile', \
         json_object('context_snapshot_id', 'ctx_other_'||x, 'padding', hex(zeroblob(2000))) \
         FROM n; \
         WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{OTHERS}) \
         INSERT INTO model_steps(id, agent_run_id, ordinal, snapshot_id, state, payload) \
         SELECT 'stp_other_'||x, 'agt_other', x, 'mes_other_'||x, 'completed', '{{}}' FROM n;"
    ))
    .unwrap();
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
}

#[test]
fn a_steps_lookups_do_not_read_other_conversations_requests() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = fixture.turn.session_id;
    // A context snapshot of the conversation that no request names: a lookup that starts from the
    // conversation's snapshots and matches every request against them finds nothing, at a cost.
    store
        .database
        .connection()
        .execute(
            "INSERT INTO context_snapshots VALUES(?1,?2,NULL,?3)",
            rusqlite::params![
                aura_domain_common::ContextProjectionSnapshotId::new().to_string(),
                session.to_string(),
                serde_json::json!({ "model_step_id": fixture.step.model_step_id }).to_string()
            ],
        )
        .unwrap();
    let lookups = |store: &V2Store| {
        instructions(store, |db| {
            counted_request::latest(db, session).unwrap();
            counted_request::latest_counted_snapshot(db, session).unwrap();
            inspector::previous_context(db, session).unwrap();
        })
    };
    let alone = lookups(&store);
    other_conversations_requests(&store);
    let beside_others = lookups(&store);
    assert!(
        beside_others <= alone + 1_000,
        "the lookups ran {alone} SQLite instructions alone and {beside_others} beside {OTHERS} \
         requests of other conversations"
    );
}
