//! A version's first start can be run by the launcher of the version it replaces: an update
//! applied from inside Aura restarts the launcher that was already running, and one older than the
//! slimming never runs it. The store then opens with older versions' copies still in it, so Core
//! notes the run as owed, and the next start, by the new launcher, does it.
use std::ops::ControlFlow;

use aura_persistence_v2::{
    V2Store,
    slimming::{self, Owed, Stage},
};

use super::old_store::{self, count, reader};

fn room(_: &std::path::Path) -> std::io::Result<u64> {
    Ok(u64::MAX / 4)
}

#[test]
fn opening_a_store_with_older_versions_copies_leaves_the_slimming_owed() {
    let old = old_store::build();
    assert_eq!(slimming::owed(&old.root), None);

    drop(V2Store::open(&old.root).unwrap());
    assert_eq!(
        slimming::owed(&old.root),
        Some(Owed { schema_version: 1, stage: Stage::Delete, runs: 0 })
    );

    let report = slimming::slim(&old.root, &room, &mut |_| ControlFlow::Continue(())).unwrap();
    assert_eq!(report.outbox_rows as i64, old.events);
    assert_eq!(slimming::owed(&old.root), None);
    assert_eq!(count(&reader(&old.root), "SELECT count(*) FROM projection_outbox"), 0);

    // With nothing older left in it, the store opens without owing a run.
    drop(V2Store::open(&old.root).unwrap());
    assert_eq!(slimming::owed(&old.root), None);
}
