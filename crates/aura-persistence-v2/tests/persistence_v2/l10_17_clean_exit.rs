//! A store whose previous process said it was exiting
//! opens owing only the quick check, and spends the marker as it opens; without the marker (a
//! crash, or a process ended without saying so) it owes every page, index and key, checked while
//! it serves. The quick check that follows such an open still finds a damaged page.
#![allow(clippy::unwrap_used)]
use aura_persistence_core::schema::{PendingCheck, quick_check};
use aura_persistence_v2::V2Store;
use std::io::{Seek, SeekFrom, Write};

#[test]
fn a_normal_exit_lets_the_next_open_check_only_the_structure_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    store.mark_clean_exit().unwrap();
    drop(store);

    let reopened = V2Store::open(&root).unwrap();
    assert_eq!(reopened.pending_check(), PendingCheck::Quick);
    assert!(!root.join("clean-exit.json").exists(), "the marker is spent as the store opens");
    drop(reopened);
    // This process never said it was exiting: the next open owes everything.
    assert_eq!(V2Store::open(&root).unwrap().pending_check(), PendingCheck::Full);
}

#[test]
fn the_quick_check_passes_a_sound_store_and_finds_a_damaged_page() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    // Everything in the database file itself, nothing left in the write-ahead log.
    store.mark_clean_exit().unwrap();
    drop(store);
    let database = root.join("aura-v2.db");
    quick_check(&database).unwrap();

    let mut file = std::fs::OpenOptions::new().write(true).open(&database).unwrap();
    file.seek(SeekFrom::Start(4096)).unwrap();
    file.write_all(&[0xFF; 64]).unwrap();
    drop(file);
    assert!(quick_check(&database).is_err(), "a damaged page is found");
}
