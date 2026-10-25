//! What one small save of a terminal's output costs as the store grows.
//!
//! Each save rewrites the output's last segment and collects the segment it replaced. Collecting
//! removed every object no artifact named by checking each object against every artifact, and
//! `artifacts.object_hash` has no index, so one save cost O(n²) in the store's artifacts: about
//! 70 ms beside 2 000 artifacts and 24 s beside 20 000 on a copy of the schema. A person typing
//! into a terminal the Agent opened waited for one such save per key.
//!
//! The work is counted as SQLite virtual-machine steps, as in `agent_stream_append_cost`.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_domain_common::*;
use aura_persistence_core::artifact;
use aura_persistence_v2::{V2Store, terminal::*};
use aura_windows_user_contract::*;
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

fn count(store: &V2Store, sql: &str) -> i64 {
    store.database.connection().query_row(sql, [], |row| row.get(0)).unwrap()
}

/// The SQLite steps of saving one more character of a terminal's output, in a store that holds
/// `others` artifacts of something else.
fn save_steps(others: usize) -> i64 {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, aura_agent_domain::AgentTaskMode::Normal);
    let record = AgentTerminalRecord {
        view: TerminalViewV1 {
            session_id: fixture.turn.session_id,
            terminal_id: "term_typed".into(),
            shell: ManagedShell::Cmd,
            process_id: 1,
            started_at: UtcTimestamp::now(),
            end: None,
            owner: TerminalOwnerV1::Agent,
            controller: TerminalControllerV1::User,
            title: "fixture".into(),
            tool_call_id: None,
            revision: Revision::INITIAL,
        },
        operation_id: SecurityOperationId::new(),
        runtime_instance_id: RuntimeInstanceId::new(),
        working_directory: "C:/temporary".into(),
        requires_network: false,
        tab_closed: false,
    };
    let at = UtcTimestamp::now();
    store
        .database
        .unit_of_work(|tx| {
            for n in 0..others {
                let bytes = format!("kept {n}");
                artifact::put(
                    tx,
                    ArtifactId::new(),
                    "elsewhere",
                    "text/plain",
                    bytes.as_bytes(),
                    at,
                )?;
            }
            save(tx, &record)?;
            append(tx, record.view.session_id, &record.view.terminal_id, "C:\\>")
        })
        .unwrap();
    STEPS.with(|total| total.set(0));
    store.database.connection().trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(count_steps));
    store
        .database
        .unit_of_work(|tx| append(tx, record.view.session_id, &record.view.terminal_id, "d"))
        .unwrap();
    store.database.connection().trace_v2(TraceEventCodes::empty(), None);
    let steps = STEPS.with(Cell::get);
    eprintln!("one save beside {others} artifacts: {steps} steps");
    // The replaced segment and its object are gone; nothing else is.
    let others = i64::try_from(others).unwrap();
    assert_eq!(count(&store, "SELECT count(*) FROM artifacts"), others + 1);
    assert_eq!(count(&store, "SELECT count(*) FROM artifact_objects"), others + 1);
    let output = read(store.database.connection(), &record, 0, 64).unwrap();
    assert_eq!(output.output, "C:\\>d");
    steps
}

#[test]
fn a_terminal_save_does_not_cost_the_square_of_the_stores_artifacts() {
    let small = save_steps(1_000);
    let large = save_steps(4_000);
    // Four times the artifacts: one pass over them costs about four times as much; checking every
    // object against every artifact cost sixteen times as much.
    assert!(
        large < small * 8,
        "one save ran {small} SQLite steps beside 1000 artifacts and {large} beside 4000"
    );
}

#[test]
fn an_object_two_artifacts_share_is_kept_until_neither_needs_it() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let (first, second) = (ArtifactId::new(), ArtifactId::new());
    let at = UtcTimestamp::now();
    store
        .database
        .unit_of_work(|tx| {
            artifact::put(tx, first, "one", "text/plain", b"same bytes", at)?;
            artifact::put(tx, second, "two", "text/plain", b"same bytes", at)
        })
        .unwrap();
    let collected = store
        .database
        .unit_of_work(|tx| {
            artifact::release_owner(tx, first, "one")?;
            artifact::collect(tx)
        })
        .unwrap();
    assert_eq!(collected, 1);
    assert_eq!(count(&store, "SELECT count(*) FROM artifact_objects"), 1);
    assert_eq!(artifact::read(store.database.connection(), second, "two").unwrap(), b"same bytes");
    let collected = store
        .database
        .unit_of_work(|tx| {
            artifact::release_owner(tx, second, "two")?;
            artifact::collect(tx)
        })
        .unwrap();
    assert_eq!(collected, 1);
    assert_eq!(count(&store, "SELECT count(*) FROM artifact_objects"), 0);
}
