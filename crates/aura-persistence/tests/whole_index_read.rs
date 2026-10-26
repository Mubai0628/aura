//! Reading a whole workspace index is bounded by the index lane's budget rather than the five
//! seconds an interactive read gets: a large index read on a busy start is read in full instead of
//! being cut off, which used to fail the workspace's open.
mod index_support;

use std::time::Duration;

use aura_domain_common::CorrelationId;
use aura_persistence::barrier::Point;
use index_support::{entries, partition, start, workspace};

/// Longer than an interactive read's budget.
const PARKED: Duration = Duration::from_secs(6);

#[test]
fn a_whole_index_read_outlasts_the_interactive_read_budget() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let indexed = workspace(&persistence, "large");
    persistence
        .replace_workspace_catalog(indexed, entries(100), partition(100), CorrelationId::new())
        .expect("the index");

    let hold = persistence.arm_read_hold(Point::ReadConnection, 0);
    let reading = persistence.clone();
    let reader = std::thread::spawn(move || reading.load_workspace_catalog(indexed));
    assert!(hold.wait_for_arrivals(1, Duration::from_secs(10)), "the read reached a connection");
    std::thread::sleep(PARKED);
    hold.release_all();
    let (stored, _, _) = reader.join().expect("reader").expect("the whole index is read");
    assert_eq!(stored.len(), 100);
}
