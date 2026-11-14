//! The check a store owes runs while Core serves, and a quitting Core stops it
//! at once instead of waiting for every page to be read.
#![allow(clippy::unwrap_used)]
use aura_persistence_core::schema::{PendingCheck, background_check};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A database of a few megabytes with an index, so a check reads many pages.
fn database(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("store.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT NOT NULL);
             CREATE INDEX t_body ON t(body);
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<20000)
             INSERT INTO t(body) SELECT printf('%0100d', i) FROM n;",
        )
        .unwrap();
    path
}

#[test]
fn a_check_that_is_told_to_stop_ends_before_it_has_read_everything() {
    let dir = tempfile::tempdir().unwrap();
    let path = database(dir.path());
    for check in [PendingCheck::Quick, PendingCheck::Full] {
        assert!(background_check(&path, check, || false).is_ok(), "{check:?} passes a sound store");
        let asked = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&asked);
        // Told to stop the second time it asks: the store is sound, yet the check does not pass.
        let stopped =
            background_check(&path, check, move || counted.fetch_add(1, Ordering::AcqRel) >= 1);
        assert!(stopped.is_err(), "{check:?} stops");
        assert_eq!(asked.load(Ordering::Acquire), 2, "{check:?} stops the moment it is told");
    }
}
