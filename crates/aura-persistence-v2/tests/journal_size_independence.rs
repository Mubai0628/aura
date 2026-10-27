//! The owner's journal holds 1.18 million
//! events, almost all of them streamed output, and the only index on it is ``.
//! Reading a conversation after a compaction, preparing every step, finishing a run, the background
//! repair, opening a branch and the start-up checks each looked events up by kind or by a field of
//! the payload, so each of them read every event — a quarter of a second warm, seconds cold, and
//! the reads and writes of everything else waited behind them.
//!
//! What each of those reads costs must not depend on how many unrelated events the journal holds.
//! SQLite counts the virtual-machine instructions it runs; each read is counted on the fixture, and
//! again after 200,000 events of other owners are added.
//!
//! Two start-up reads still looked events up by
//! kind — the runs a restart must settle and the memory-maintenance leases it must close. On the
//! owner's 1.86 million events each read the whole 649 MiB table: 1.5 GB read before Core was ready,
//! 1.5 s warm and 13 s on a first start.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_context_domain::{AgentContextReadEventsInput, ContextEventSelector};
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    agent::{branch, desktop, inbox, recovery, stream},
    context::{manual_compaction, read},
    model::memory_maintenance,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const FILLER: u64 = 200_000;
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

/// 200,000 events of owners none of these reads is about.
fn fill(store: &V2Store) {
    store
        .database
        .connection()
        .execute_batch(&format!(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{FILLER}) \
             INSERT INTO events(event_id, owner, kind, version, payload, created_at) \
             SELECT 'evt_filler_'||x, 'tat_filler_'||(x % 997), 'model.transport', 1, '{{}}', \
             '2026-10-21T00:00:00Z' FROM n"
        ))
        .unwrap();
}

/// A completed compaction prepared before the fixture's step, so a history read asks whether it
/// was the user's.
fn compacted_before_the_step(store: &V2Store, fixture: &support::Fixture) {
    let payload = serde_json::json!({
        "agent_run_id": fixture.turn.agent_run_id,
        "source_step_id": fixture.step.model_step_id,
        "retained_recent_tokens": 1_000,
        "sequence": 1,
    });
    store
        .database
        .connection()
        .execute(
            "INSERT INTO context_compactions(id, session_id, state, revision, payload) \
             VALUES (?1, ?2, 'completed', 1, ?3)",
            rusqlite::params![
                ContextProjectionSnapshotId::new().to_string(),
                fixture.turn.session_id.to_string(),
                payload.to_string()
            ],
        )
        .unwrap();
}

struct Reads {
    history: u64,
    manual_requested: u64,
    manual_status: u64,
    repair: u64,
    inbox_startup: u64,
    runs_to_settle: u64,
    leases_to_close: u64,
    branches_unfinished: u64,
    context_events: u64,
}

fn reads(store: &mut V2Store, fixture: &support::Fixture) -> Reads {
    let session = fixture.turn.session_id;
    let run = fixture.turn.agent_run_id;
    let runtime = RuntimeInstanceId::new();
    let request = AgentHistoryRequest {
        session_id: session,
        before_turn_task_id: None,
        within_turn: None,
        limit: 10,
    };
    let (page, history) = instructions(store, |store| {
        desktop::projection(store.database.connection(), &request, runtime).unwrap()
    });
    assert!(
        page.turns[0].assistant_messages.iter().any(|step| step.context_compacted.is_some()),
        "the compaction is shown, so its kind was looked up"
    );
    let (_, manual_requested) = instructions(store, |store| {
        manual_compaction::requested(store.database.connection(), run).unwrap()
    });
    let (_, manual_status) = instructions(store, |store| {
        manual_compaction::status(
            store.database.connection(),
            &AgentRunRequest { session_id: session, agent_run_id: run },
        )
        .unwrap()
    });
    let at = fixture.at;
    let (_, repair) = instructions(store, |store| {
        store.database.unit_of_work(|tx| stream::repair_outstanding(tx, at, 262_144)).unwrap()
    });
    let (_, inbox_startup) = instructions(store, |store| {
        store.database.unit_of_work(|tx| inbox::startup(tx, at)).unwrap()
    });
    let (_, runs_to_settle) =
        instructions(store, |store| recovery::active_runs(store.database.connection()).unwrap());
    let (_, leases_to_close) = instructions(store, |store| {
        store
            .database
            .unit_of_work(|tx| memory_maintenance::interrupt_previous_process(tx, runtime, at))
            .unwrap()
    });
    let (_, branches_unfinished) =
        instructions(store, |store| branch::unfinished(store.database.connection()).unwrap());
    let input = AgentContextReadEventsInput {
        selectors: vec![ContextEventSelector::TurnTask { turn_task_id: fixture.turn.turn_task_id }],
        character_limit: 8_192,
        cursor: None,
        content_offset: None,
    };
    let (_, context_events) = instructions(store, |store| {
        store.database.unit_of_work(|tx| read::events(tx, session, run, &input, at)).unwrap()
    });
    Reads {
        history,
        manual_requested,
        manual_status,
        repair,
        inbox_startup,
        runs_to_settle,
        leases_to_close,
        branches_unfinished,
        context_events,
    }
}

#[test]
fn hot_reads_cost_the_same_whatever_else_the_journal_holds() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    compacted_before_the_step(&store, &fixture);
    let before = reads(&mut store, &fixture);
    fill(&store);
    let after = reads(&mut store, &fixture);
    let mut grew = Vec::new();
    for (what, before, after) in [
        ("conversation history", before.history, after.history),
        ("manual compaction request", before.manual_requested, after.manual_requested),
        ("manual compaction status", before.manual_status, after.manual_status),
        ("background stream repair", before.repair, after.repair),
        ("inbox start-up", before.inbox_startup, after.inbox_startup),
        ("runs a restart settles", before.runs_to_settle, after.runs_to_settle),
        (
            "memory-maintenance leases a restart closes",
            before.leases_to_close,
            after.leases_to_close,
        ),
        ("unfinished branch copies", before.branches_unfinished, after.branches_unfinished),
        ("context history read", before.context_events, after.context_events),
    ] {
        // A read of every event costs millions of instructions more; a lookup by owner, none.
        if after > before * 2 + 50_000 {
            grew.push(format!("{what}: {before} -> {after}"));
        }
    }
    assert!(grew.is_empty(), "these read the whole journal: {grew:#?}");
}
