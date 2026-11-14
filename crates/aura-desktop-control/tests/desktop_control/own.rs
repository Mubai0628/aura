//! Which processes are the Aura that started the host: the rule alone, on snapshots made up for
//! it.
use aura_desktop_control::own::{Process, starters};
use std::collections::{BTreeSet, HashMap};

fn snapshot(rows: &[(u32, u32, &str)]) -> HashMap<u32, Process> {
    rows.iter()
        .map(|(id, parent, name)| (*id, Process { parent: *parent, name: (*name).into() }))
        .collect()
}

#[test]
fn the_host_is_started_by_core_and_core_by_auras_window() {
    let processes = snapshot(&[
        (30, 20, "aura-desktop-host.exe"),
        (20, 10, "aura-core.exe"),
        (10, 5, "Aura.exe"),
        (5, 1, "explorer.exe"),
    ]);
    assert_eq!(starters(30, &processes), BTreeSet::from([20, 10]));
}

#[test]
fn explorer_is_never_taken_for_aura() {
    // Core started from File Explorer, with no window of Aura's between.
    let processes = snapshot(&[
        (30, 20, "aura-desktop-host.exe"),
        (20, 5, "aura-core.exe"),
        (5, 1, "EXPLORER.EXE"),
    ]);
    assert_eq!(starters(30, &processes), BTreeSet::from([20]));
}

#[test]
fn a_process_that_ended_or_is_not_known_adds_nothing() {
    assert!(starters(30, &snapshot(&[])).is_empty(), "the host is not in the snapshot");
    assert!(starters(30, &snapshot(&[(30, 0, "aura-desktop-host.exe")])).is_empty(), "no parent");
    let ended = snapshot(&[(30, 20, "aura-desktop-host.exe"), (20, 10, "aura-core.exe")]);
    assert_eq!(starters(30, &ended), BTreeSet::from([20]), "Core's parent ended");
    let again = snapshot(&[(30, 20, "aura-desktop-host.exe"), (20, 30, "aura-core.exe")]);
    assert_eq!(starters(30, &again), BTreeSet::from([20]), "an id that came round to the host");
}
