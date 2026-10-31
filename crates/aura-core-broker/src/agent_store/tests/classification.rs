//! Which store failures are facts about the database and which are one operation's answer.
use super::super::*;

#[test]
fn the_classification_is_by_type_and_never_by_message() {
    let corrupt = classify(
        "agent.step",
        PersistenceCoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            Some("database disk image is malformed".into()),
        )),
    );
    match corrupt {
        StoreError::Unavailable { snapshot, .. } => {
            assert_eq!(snapshot.code, None, "the snapshot is refilled from live health");
        }
        other => panic!("a damaged store is a dependency fact: {other}"),
    }
    assert_eq!(
        shared_failure(&PersistenceCoreError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".into()),
        )))
        .map(|(code, _)| code),
        Some("sqlite.busy")
    );
    for local in [
        PersistenceCoreError::Conflict,
        PersistenceCoreError::Invalid("agent.mode_switch_requires_idle"),
    ] {
        assert!(
            shared_failure(&local).is_none(),
            "an operation-level answer must not become a dependency fact"
        );
        assert!(matches!(classify("agent.step", local), StoreError::Failed { .. }));
    }
}

#[test]
fn a_write_success_clears_a_write_fault_and_a_read_success_does_not_hide_it() {
    let health = health::StoreHealth::new();
    health.observe_unavailable(JOURNAL, StoreAccess::Write, "sqlite.busy", "locked".into());
    assert!(!health.snapshot(JOURNAL).available);
    // In WAL a reader is served while the writer is blocked: that must not clear the fact.
    health.observe_success(JOURNAL, StoreAccess::Read);
    let still_failed = health.snapshot(JOURNAL);
    assert!(!still_failed.available, "{still_failed:?}");
    assert_eq!(still_failed.cleared, 0);
    // A write that really committed does clear it, with the text that produced it.
    health.observe_success(JOURNAL, StoreAccess::Write);
    let cleared = health.snapshot(JOURNAL);
    assert!(cleared.available);
    assert_eq!(cleared.cleared, 1);
    assert_eq!(cleared.code, None, "a recovered dependency is not described by its old error");
    assert!(cleared.revision > still_failed.revision);
}
