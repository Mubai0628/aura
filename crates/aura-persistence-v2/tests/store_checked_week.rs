//! SQLite's quick check reads every page of the store, minutes on several gigabytes, so after a
//! normal exit a store whose background check passed within the last week opens owing nothing.
//! An older check owes the quick check again, and an exit that was not normal still owes the
//! full one.
#![allow(clippy::unwrap_used)]
use aura_domain_common::UtcTimestamp;
use aura_persistence_core::schema::PendingCheck;
use aura_persistence_v2::V2Store;
use std::path::Path;

fn checked_at(root: &Path, at: UtcTimestamp) {
    std::fs::write(
        root.join("store-checked.json"),
        format!("{{\"schema_version\":1,\"at\":\"{at}\"}}\n"),
    )
    .unwrap();
}

fn days_ago(days: i64) -> UtcTimestamp {
    UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::days(days))
}

#[test]
fn a_store_checked_this_week_owes_nothing_after_a_normal_exit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    checked_at(&root, days_ago(6));
    store.mark_clean_exit().unwrap();
    drop(store);
    assert_eq!(V2Store::open(&root).unwrap().pending_check(), PendingCheck::None);
}

#[test]
fn an_older_check_owes_the_quick_check_and_an_exit_that_was_not_normal_the_full_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    checked_at(&root, days_ago(8));
    store.mark_clean_exit().unwrap();
    drop(store);
    let reopened = V2Store::open(&root).unwrap();
    assert_eq!(reopened.pending_check(), PendingCheck::Quick);
    drop(reopened);
    // A check this morning spares nothing when the process did not say it was exiting.
    checked_at(&root, UtcTimestamp::now());
    assert_eq!(V2Store::open(&root).unwrap().pending_check(), PendingCheck::Full);
}

#[test]
fn a_passed_check_is_recorded_beside_the_store_and_a_clock_set_back_does_not_trust_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    aura_persistence_v2::record_store_checked(&store.database_path()).unwrap();
    assert!(root.join("store-checked.json").exists());
    store.mark_clean_exit().unwrap();
    drop(store);
    assert_eq!(V2Store::open(&root).unwrap().pending_check(), PendingCheck::None);
    // A record from the future (the clock was set back since) proves nothing.
    let store = V2Store::open(&root).unwrap();
    checked_at(
        &root,
        UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::days(3)),
    );
    store.mark_clean_exit().unwrap();
    drop(store);
    assert_eq!(V2Store::open(&root).unwrap().pending_check(), PendingCheck::Quick);
}
