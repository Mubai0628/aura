//! The rebuild needs about twice the bytes the store keeps; short of them, only the deletion is
//! done and the rebuild waits for a later run. A store with nothing older in it is left alone.
use std::ops::ControlFlow;

use aura_persistence_v2::{
    V2Store,
    slimming::{self, Owed, Rebuild, Stage},
};

use super::old_store::{self, count, reader};

const MIB: u64 = 1024 * 1024;

#[test]
fn without_room_the_deletion_is_done_and_the_rebuild_waits_for_a_later_run() {
    let old = old_store::build();
    let short = |_: &std::path::Path| Ok(MIB);
    let first = slimming::slim(&old.root, &short, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(
        matches!(first.rebuild, Rebuild::NoRoom { free, needed } if free == MIB && needed > 2 * MIB)
    );
    assert!(!first.stopped);
    let db = reader(&old.root);
    assert_eq!(count(&db, "SELECT count(*) FROM projection_outbox"), 0);
    assert_eq!(
        count(&db, "SELECT count(*) FROM events WHERE version=1 AND kind='model.transport'"),
        0
    );
    // The deleted pages wait in the file for reuse.
    assert!(count(&db, "PRAGMA freelist_count") > 0);
    drop(db);
    // Short of room is not a failed run.
    assert_eq!(
        slimming::owed(&old.root),
        Some(Owed { schema_version: 1, stage: Stage::Rebuild, runs: 0 })
    );

    let room = |_: &std::path::Path| Ok(u64::MAX / 4);
    let second = slimming::slim(&old.root, &room, &mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(second.rebuild, Rebuild::Done);
    assert_eq!(
        (second.outbox_rows, second.transport_events, second.snapshots, second.raw_streams),
        (0, 0, 0, 0)
    );
    assert!(second.after_bytes * 3 < first.after_bytes, "{first:?} then {second:?}");
    assert_eq!(slimming::owed(&old.root), None);
    assert_eq!(count(&reader(&old.root), "PRAGMA freelist_count"), 0);
}

#[test]
fn an_older_version_run_while_the_rebuild_waits_has_its_copies_cleared_too() {
    let old = old_store::build();
    let short = |_: &std::path::Path| Ok(MIB);
    slimming::slim(&old.root, &short, &mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(slimming::owed(&old.root).map(|owed| owed.stage), Some(Stage::Rebuild));
    // Going back to the older version: it copies every event it writes into the outbox again.
    let writer = rusqlite::Connection::open(old.root.join("aura-v2.db")).unwrap();
    writer
        .execute(
            "INSERT INTO projection_outbox(event_id,topic,revision,payload) \
             SELECT event_id,'again',sequence,payload FROM events",
            [],
        )
        .unwrap();
    drop(writer);
    let room = |_: &std::path::Path| Ok(u64::MAX / 4);
    let again = slimming::slim(&old.root, &room, &mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(again.outbox_rows as i64, old.events);
    assert_eq!(again.rebuild, Rebuild::Done);
    assert_eq!(count(&reader(&old.root), "SELECT count(*) FROM projection_outbox"), 0);
    assert_eq!(slimming::owed(&old.root), None);
}

#[test]
fn a_store_with_nothing_older_in_it_is_left_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("v2");
    let store = V2Store::create(&root).unwrap();
    store.mark_clean_exit().unwrap();
    drop(store);
    let mut heard = Vec::new();
    let room = |_: &std::path::Path| Ok(u64::MAX / 4);
    let report = slimming::slim(&root, &room, &mut |step| {
        heard.push(step);
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(
        report,
        slimming::Report {
            before_bytes: report.before_bytes,
            after_bytes: report.before_bytes,
            ..slimming::Report::default()
        }
    );
    assert!(heard.is_empty(), "{heard:?}");
    assert_eq!(slimming::owed(&root), None);
    assert!(root.join("clean-exit.json").exists());
}
