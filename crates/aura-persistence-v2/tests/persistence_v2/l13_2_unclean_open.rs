//! On the owner's 7.1 GB store the full check
//! took 154 s, past the two minutes Main waits for Core, so a start after an exit nobody announced
//! never finished: Main restarted Core over and over. The store now opens with its structure
//! checked and owes the full check to the background. And a normal exit is recorded within the
//! three seconds Main allows even while a reader still holds the write-ahead log.
#![allow(clippy::unwrap_used)]
use aura_persistence_core::schema::PendingCheck;
use aura_persistence_v2::V2Store;
use std::time::{Duration, Instant};

#[test]
fn an_unannounced_end_opens_with_the_full_check_owed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    drop(V2Store::create(&root).unwrap());
    // A broken key the structure check cannot see, written past the store's own checks.
    let raw = rusqlite::Connection::open(root.join("aura-v2.db")).unwrap();
    raw.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO artifact_gc_queue VALUES('art_gone');")
        .unwrap();
    drop(raw);

    let store = V2Store::open(&root).expect("the full check does not keep Core from starting");
    assert_eq!(store.pending_check(), PendingCheck::Full);
    let found = aura_persistence_core::schema::background_check(
        &store.database_path(),
        PendingCheck::Full,
        || false,
    );
    assert!(found.is_err(), "the check it owes finds the broken key");
}

#[test]
fn a_reader_holding_the_log_does_not_hold_up_the_normal_exit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    let database = store.database_path();
    // Something in the log, and a reader (the background check) on a snapshot that includes it.
    let writer = rusqlite::Connection::open(&database).unwrap();
    writer
        .execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO artifact_gc_queue VALUES('x');")
        .unwrap();
    let reader = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    reader.execute_batch("BEGIN").unwrap();
    let _: i64 =
        reader.query_row("SELECT count(*) FROM artifact_gc_queue", [], |r| r.get(0)).unwrap();

    let started = Instant::now();
    store.mark_clean_exit().unwrap();
    assert!(started.elapsed() < Duration::from_millis(2_000), "took {:?}", started.elapsed());
    assert!(root.join("clean-exit.json").exists());
    drop(reader);
}

#[test]
fn a_store_made_now_owes_nothing_and_a_normal_exit_owes_the_quick_check() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    assert_eq!(store.pending_check(), PendingCheck::None);
    store.mark_clean_exit().unwrap();
    drop(store);
    assert_eq!(V2Store::open(&root).unwrap().pending_check(), PendingCheck::Quick);
}
