//! Deleting an artifact makes SQLite check every row that may point at it, and deleting its stored
//! object every artifact that may share it. A column that points at an artifact without an index
//! is read whole for each one: the execution results and operation links grow with every tool
//! call, so each step's cleanup read tens of thousands of rows. Each such column that grows with
//! use has an index, and a store 0.1.9 left gains them when this version first opens it.
#![allow(clippy::unwrap_used)]
#[path = "support/release_0_1_9.rs"]
mod release_0_1_9;

use aura_domain_common::{ArtifactId, UtcTimestamp};
use aura_persistence_core::{DatabaseConnection, artifact, schema};
use aura_persistence_v2::{V2Store, migration_pending};
use rusqlite::{
    StatementStatus,
    trace::{TraceEvent, TraceEventCodes},
};
use std::cell::Cell;

#[test]
fn a_store_0_1_9_left_gains_the_indexes_when_this_version_opens_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    store.mark_clean_exit().unwrap();
    drop(store);
    let database = root.join("aura-v2.db");
    release_0_1_9::as_0_1_9(&database);
    let reader = aura_persistence_core::open_reader(&database).unwrap();
    assert!(release_0_1_9::indexes(&reader).is_empty());
    drop(reader);
    assert!(migration_pending(&database).unwrap(), "the launcher copies the store first");

    let store = V2Store::open(&root).unwrap();
    assert_eq!(release_0_1_9::indexes(store.database.connection()), release_0_1_9::INDEXES);
    drop(store);
    assert!(!migration_pending(&database).unwrap());
}

/// The launcher asks before every newer version's first start and waits 20 s for the answer, and
/// Main waits two minutes for Core to open the store. The full check of every page and key took
/// minutes on a large store, so neither runs it for a migration that only adds indexes: the answer
/// comes from the schema, and the store opens as one owed nothing does, a broken reference left
/// to the store's own checks.
#[test]
fn a_store_owed_only_indexes_is_not_checked_page_by_page_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    let db = store.database.connection();
    db.pragma_update(None, "foreign_keys", "OFF").unwrap();
    db.execute(
        "INSERT INTO operation_artifact_links VALUES('op_missing','art_missing','evidence')",
        [],
    )
    .unwrap();
    drop(store);
    let database = root.join("aura-v2.db");
    release_0_1_9::as_0_1_9(&database);
    assert!(migration_pending(&database).unwrap());
    let store = V2Store::open(&root).unwrap();
    assert_eq!(release_0_1_9::indexes(store.database.connection()), release_0_1_9::INDEXES);
}

/// A migration that adds tables still checks every page and key before it writes.
#[test]
fn a_store_owed_tables_is_still_checked_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    std::fs::create_dir(&root).unwrap();
    let agent = include_str!("../src/agent.sql");
    let (head, _) = agent.split_once("\nCREATE TABLE agent_visible_repair").unwrap();
    let older: &'static str = Box::leak(format!("{head}\n").into_boxed_str());
    let modules = aura_persistence_v2::MODULES
        .iter()
        .map(|module| match module.owner {
            "agent" => schema::SchemaModule { owner: "agent", sql: older },
            _ => *module,
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(&root.join("aura-v2.db")).unwrap();
    schema::initialize(&mut database, &modules).unwrap();
    database.connection().pragma_update(None, "foreign_keys", "OFF").unwrap();
    database
        .connection()
        .execute(
            "INSERT INTO operation_artifact_links VALUES('op_missing','art_missing','evidence')",
            [],
        )
        .unwrap();
    drop(database);
    std::fs::write(
        root.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .unwrap();
    assert!(migration_pending(&root.join("aura-v2.db")).unwrap());
    assert!(V2Store::open(&root).is_err());
}

thread_local! {
    static STEPS: Cell<i64> = const { Cell::new(0) };
}

fn count_steps(event: TraceEvent<'_>) {
    if let TraceEvent::Profile(statement, _) = event {
        let steps = i64::from(statement.get_status(StatementStatus::VmStep));
        STEPS.with(|total| total.set(total.get() + steps));
    }
}

/// Rows of every growing kind that point at artifacts other than the one deleted.
fn rows_pointing_elsewhere(store: &V2Store, rows: u64) {
    let db = store.database.connection();
    db.pragma_update(None, "foreign_keys", "OFF").unwrap();
    db.execute_batch(&format!(
        "INSERT INTO artifact_objects WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n \
         WHERE x<{rows}) SELECT 'obj_other_'||x, 0, x'' FROM n; \
         INSERT INTO artifacts SELECT 'art_other_'||substr(hash,11), hash, 'text/plain', \
         '2026-01-01T00:00:00Z' FROM artifact_objects WHERE hash LIKE 'obj_other_%'; \
         INSERT INTO execution_results SELECT 'op_other_'||rowid, 1, 'succeeded', 'no_effect', \
         id, '{{}}' FROM artifacts WHERE id LIKE 'art_other_%'; \
         INSERT INTO operation_artifact_links SELECT 'op_other_'||rowid, id, 'execution_evidence' \
         FROM artifacts WHERE id LIKE 'art_other_%'; \
         INSERT INTO terminal_output_segments SELECT 'term_other', rowid, rowid+1, id \
         FROM artifacts WHERE id LIKE 'art_other_%';"
    ))
    .unwrap();
    db.pragma_update(None, "foreign_keys", "ON").unwrap();
}

/// The SQLite steps of storing one artifact, giving it up, and collecting it with its object.
fn steps_to_delete_one(store: &mut V2Store, seed: u32) -> i64 {
    let at: UtcTimestamp = "2026-10-29T00:00:00Z".parse().unwrap();
    let id = ArtifactId::new();
    STEPS.with(|total| total.set(0));
    store.database.connection().trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(count_steps));
    let collected = store
        .database
        .unit_of_work(|tx| {
            artifact::put(tx, id, "test", "text/plain", format!("text {seed}").as_bytes(), at)?;
            artifact::release_owner(tx, id, "test")?;
            artifact::collect(tx)
        })
        .unwrap();
    store.database.connection().trace_v2(TraceEventCodes::empty(), None);
    assert_eq!(collected, 1);
    STEPS.with(Cell::get)
}

#[test]
fn deleting_an_artifact_costs_the_same_however_many_rows_point_at_others() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&dir.path().join("v2")).unwrap();
    steps_to_delete_one(&mut store, 0);
    let alone = steps_to_delete_one(&mut store, 1);
    rows_pointing_elsewhere(&store, 3_000);
    steps_to_delete_one(&mut store, 2);
    let beside = steps_to_delete_one(&mut store, 3);
    assert!(
        beside < alone + 200,
        "one delete ran {alone} SQLite steps alone and {beside} beside 3000 rows of each kind"
    );
}
