//! Once much of the file is free the run rebuilds it (`VACUUM`), when the disk has about twice the
//! bytes it keeps; short of them the rebuild stays owed without counting as a failed run. A run
//! that was stopped stays owed and counts, so the launcher can stop retrying one that never ends.
use aura_persistence::slimming::{self, Rebuild};

use super::*;

fn owed(store: &Store, runs: u32) {
    let marker = serde_json::json!({ "schema_version": 1, "kept_bytes": 0, "owed_runs": runs });
    std::fs::write(store.database.with_file_name(slimming::MARKER), marker.to_string()).unwrap();
}

#[test]
fn a_file_with_much_of_it_free_is_rebuilt_once_the_disk_has_room() {
    let store = Store::new();
    store.leave_free(70);
    assert!(store.free_bytes() >= 64 * MIB);
    owed(&store, 0);

    let short = store.slim(MIB);
    assert!(matches!(short.rebuild, Rebuild::NoRoom { free, .. } if free == MIB), "{short:?}");
    assert!(store.free_bytes() >= 64 * MIB);
    assert_eq!(slimming::maintenance(&store.database).unwrap().owed_runs, Some(0));

    let report = store.slim(u64::MAX / 4);
    assert_eq!(report.rebuild, Rebuild::Done);
    assert!(report.after_bytes + 60 * MIB < report.before_bytes, "{report:?}");
    assert_eq!(store.free_bytes(), 0);
    assert_eq!(store.wal_bytes(), 0, "the run leaves the log empty");
    let marker = slimming::maintenance(&store.database).unwrap();
    assert_eq!(marker.owed_runs, None);
    assert!(marker.kept_bytes > 0 && marker.kept_bytes < 8 * MIB, "{marker:?}");
    drop(store.start());
}

#[test]
fn a_run_that_was_stopped_stays_owed_and_counts() {
    let store = Store::new();
    store.leave_free(70);
    owed(&store, 1);
    let room = |_: &Path| Ok(u64::MAX / 4);
    // Stopped at its first word, before it touched anything: nothing started, nothing counts.
    let at_once =
        slimming::slim(&store.database, &store.artifacts, &room, &mut |_| ControlFlow::Break(()))
            .unwrap();
    assert!(at_once.stopped, "{at_once:?}");
    assert_eq!(slimming::maintenance(&store.database).unwrap().owed_runs, Some(1));
    // Stopped once the rebuild began: the run counts, and the file is as it was.
    let mut heard = 0;
    let report = slimming::slim(&store.database, &store.artifacts, &room, &mut |_| {
        heard += 1;
        if heard > 1 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
    })
    .unwrap();
    assert!(report.stopped, "{report:?}");
    assert_eq!(slimming::maintenance(&store.database).unwrap().owed_runs, Some(2));
    assert!(store.free_bytes() >= 64 * MIB, "a stopped rebuild leaves the file as it was");
    drop(store.start());
}

#[test]
fn a_run_that_fails_counts_too() {
    let store = Store::new();
    owed(&store, 1);
    std::fs::write(&store.database, b"not a database at all").unwrap();
    let room = |_: &Path| Ok(u64::MAX / 4);
    let mut go_on = |_| ControlFlow::Continue(());
    let failed = slimming::slim(&store.database, &store.artifacts, &room, &mut go_on);
    assert!(failed.is_err(), "{failed:?}");
    assert_eq!(
        slimming::maintenance(&store.database).unwrap().owed_runs,
        Some(2),
        "a run that always fails stops being retried after three"
    );
}

#[test]
fn a_file_with_little_free_is_left_as_it_is_and_what_it_keeps_is_noted() {
    let store = Store::new();
    let report = store.slim(u64::MAX / 4);
    assert_eq!(report.rebuild, Rebuild::NotNeeded);
    assert_eq!((report.text_index, report.catalog_versions, report.stopped), (None, 0, false));
    let marker = slimming::maintenance(&store.database).unwrap();
    assert_eq!(marker.owed_runs, None);
    assert!(marker.kept_bytes > 0);
}
