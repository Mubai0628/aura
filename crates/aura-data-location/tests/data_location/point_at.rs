//! Pointing the record at the folder the data is in now, after the folder was moved by hand or its
//! drive got another letter: only this installation's data is taken, never a copy left by a move.
use crate::support;

use std::fs;

use aura_data_location::{
    InstallCopy, LocationError, LocationRecord, Locations, point_at, write_record,
};
use support::{Profile, production, test_namespace};

#[test]
fn a_folder_with_this_installation_s_marker_becomes_the_location() {
    let profile = Profile::new();
    let namespace = production();
    let moved = profile.folder("D-drive/Aura");
    profile.record_move(&profile.roots.local, &namespace, &moved);
    let renamed = profile.folder("E-drive/Aura");
    fs::remove_dir(&renamed).expect("empty");
    fs::rename(&moved, &renamed).expect("the drive got another letter");
    let lost = Locations::resolve(&profile.roots, &namespace, &InstallCopy::NotInstalled);
    assert!(matches!(lost, Err(LocationError::LocationMissing { .. })), "{lost:?}");

    point_at(&profile.roots, &namespace, &renamed).expect("pointed");
    let found =
        Locations::resolve(&profile.roots, &namespace, &InstallCopy::NotInstalled).expect("found");
    assert_eq!(found.location.as_deref(), Some(renamed.as_path()));
    assert_eq!(found.data_root, renamed.join("data"));
}

#[test]
fn another_installation_s_folder_or_an_empty_one_is_refused_and_the_record_kept() {
    let profile = Profile::new();
    let namespace = production();
    let moved = profile.folder("D-drive/Aura");
    profile.record_move(&profile.roots.local, &namespace, &moved);
    let record = fs::read(profile.record_path(&namespace)).expect("record");

    let other = profile.folder("Other/Aura");
    profile.record_move(&profile.roots.local, &test_namespace(), &other);
    let refused = point_at(&profile.roots, &namespace, &other);
    assert!(matches!(refused, Err(LocationError::MarkerMismatch { .. })), "{refused:?}");
    let empty = profile.folder("Empty");
    let refused = point_at(&profile.roots, &namespace, &empty);
    assert!(matches!(refused, Err(LocationError::MarkerMismatch { .. })), "{refused:?}");
    assert_eq!(fs::read(profile.record_path(&namespace)).expect("record"), record);
}

#[test]
fn a_copy_left_for_removal_is_refused_and_the_copies_stay_listed() {
    let profile = Profile::new();
    let namespace = production();
    let old = profile.folder("D-drive/Aura");
    profile.record_move(&profile.roots.local, &namespace, &old);
    let now = profile.folder("E-drive/Aura");
    profile.record_move(&profile.roots.local, &namespace, &now);
    write_record(
        &profile.record_path(&namespace),
        &LocationRecord::with_removals(namespace.clone(), Some(now.clone()), vec![old.clone()]),
    )
    .expect("record");

    let refused = point_at(&profile.roots, &namespace, &old);
    assert!(matches!(refused, Err(LocationError::MarkerMismatch { .. })), "{refused:?}");
    let renamed = profile.folder("F-drive/Aura");
    fs::remove_dir(&renamed).expect("empty");
    fs::rename(&now, &renamed).expect("renamed");
    point_at(&profile.roots, &namespace, &renamed).expect("pointed");
    let found =
        Locations::resolve(&profile.roots, &namespace, &InstallCopy::NotInstalled).expect("found");
    assert_eq!(found.location.as_deref(), Some(renamed.as_path()));
    assert_eq!(found.pending_removal, [old]);
}
