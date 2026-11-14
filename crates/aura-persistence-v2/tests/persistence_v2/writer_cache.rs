//! Core writes the store from one connection, and no other connection writes behind it, so the
//! pages that connection has read stay valid from one transaction to the next. With SQLite's
//! default cache of 2 MB it read them from the file again and again: every text a conversation's
//! step releases is deleted with a check of each table that may refer to it, two tables of 15,000
//! rows each on a long-used store, 280 MB of reading a turn.
#![allow(clippy::unwrap_used)]
use aura_persistence_v2::V2Store;

fn cache_kib(store: &V2Store) -> i64 {
    store.database.connection().query_row("PRAGMA cache_size", [], |row| row.get(0)).unwrap()
}

#[test]
fn the_writer_keeps_up_to_64_mib_of_pages_however_the_store_was_opened() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    assert_eq!(cache_kib(&store), -65_536, "a new store");
    store.mark_clean_exit().unwrap();
    drop(store);
    let store = V2Store::open(&root).unwrap();
    assert_eq!(cache_kib(&store), -65_536, "an existing store");
}
