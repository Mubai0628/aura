//! A run can be ended at any point: each batch is its own transaction and the marker says what is
//! left, so the next run finishes the work. A run that never ended counts as an exit that was not
//! normal, so the next open owes the full check.
use std::ops::ControlFlow;

use aura_persistence_core::schema::{self, PendingCheck};
use aura_persistence_v2::{
    V2Store,
    slimming::{self, Owed, Rebuild, Stage, Step},
};

use super::old_store::{self, count, reader};

fn room(_: &std::path::Path) -> std::io::Result<u64> {
    Ok(u64::MAX / 4)
}

#[test]
fn a_run_stopped_between_two_batches_is_finished_by_the_next() {
    let old = old_store::build();
    let mut batches = 0;
    let first = slimming::slim(&old.root, &room, &mut |step| {
        if matches!(step, Step::Deleting { .. }) {
            batches += 1;
        }
        if batches == 3 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
    })
    .unwrap();
    assert!(first.stopped);
    assert_eq!(first.rebuild, Rebuild::NotNeeded);
    assert_eq!(
        slimming::owed(&old.root),
        Some(Owed { schema_version: 1, stage: Stage::Delete, runs: 1 })
    );
    let db = reader(&old.root);
    schema::integrity(&db).unwrap();
    // Some snapshots are still there for the next run.
    assert!(count(&db, "SELECT count(*) FROM context_snapshots") > 2);
    drop(db);

    let second = slimming::slim(&old.root, &room, &mut |_| ControlFlow::Continue(())).unwrap();
    assert!(!second.stopped);
    assert_eq!(second.rebuild, Rebuild::Done);
    assert_eq!(slimming::owed(&old.root), None);
    let db = reader(&old.root);
    assert_eq!(count(&db, "SELECT count(*) FROM projection_outbox"), 0);
    assert_eq!(
        count(&db, "SELECT count(*) FROM events WHERE version=1 AND kind='model.transport'"),
        0
    );
    assert_eq!(count(&db, "SELECT count(*) FROM context_snapshots"), 2);
    assert_eq!(
        count(&db, &format!("SELECT count(*) FROM artifacts WHERE id='{}'", old.completed.stream)),
        0
    );
    assert_eq!(
        first.snapshots + second.snapshots,
        ((old_store::SNAPSHOTS - 1) * old.conversations.len()) as u64
    );
}

#[test]
fn a_rebuild_stopped_midway_leaves_the_file_as_it_was_and_is_owed() {
    let old = old_store::build();
    let first = slimming::slim(&old.root, &room, &mut |step| {
        if matches!(step, Step::Rebuilding { .. }) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .unwrap();
    assert!(first.stopped);
    assert_eq!(
        slimming::owed(&old.root),
        Some(Owed { schema_version: 1, stage: Stage::Rebuild, runs: 1 })
    );
    let db = reader(&old.root);
    schema::integrity(&db).unwrap();
    assert!(count(&db, "PRAGMA freelist_count") > 0);
    drop(db);
    let second = slimming::slim(&old.root, &room, &mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(second.rebuild, Rebuild::Done);
    assert_eq!(slimming::owed(&old.root), None);
}

#[test]
fn a_run_that_never_ended_leaves_the_full_check_owed() {
    let old = old_store::build();
    // As the run that died left it: its marker, and the normal exit's marker it had taken away.
    std::fs::write(
        old.root.join(slimming::MARKER),
        "{\"schema_version\":1,\"stage\":\"delete\",\"runs\":1}\n",
    )
    .unwrap();
    std::fs::remove_file(old.root.join("clean-exit.json")).unwrap();
    let report = slimming::slim(&old.root, &room, &mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(report.rebuild, Rebuild::Done);
    assert_eq!(slimming::owed(&old.root), None);
    assert!(!old.root.join("clean-exit.json").exists());
    assert_eq!(V2Store::open(&old.root).unwrap().pending_check(), PendingCheck::Full);
}
