//! A record that cannot be used means the data is not found, each for its own reason, and nothing
//! is created in its place.
use crate::support;

use std::fs;

use aura_data_location::{DataMarker, InstallCopy, LocationError, Locations, write_marker};
use support::{Profile, production, test_namespace};

fn not_found(profile: &Profile, copy: &InstallCopy) -> LocationError {
    let before = profile.listing();
    let error = Locations::resolve(&profile.roots, &production(), copy)
        .expect_err("the data must not be found");
    assert_eq!(profile.listing(), before, "resolving changed the profile");
    error
}

#[test]
fn a_broken_record_is_reported() {
    let profile = Profile::new();
    let record = profile.record_path(&production());
    fs::create_dir_all(record.parent().expect("folder")).expect("folder");
    fs::write(&record, b"{ not json").expect("broken record");
    let error = not_found(&profile, &InstallCopy::NotInstalled);
    assert!(
        matches!(&error, LocationError::RecordUnreadable { path, .. } if *path == record),
        "{error}"
    );
    assert_eq!(error.code(), "record_unreadable");
}

#[test]
fn a_record_written_for_another_namespace_is_broken() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    profile.record_move(&profile.roots.local, &test_namespace(), &moved);
    let theirs = profile.record_path(&test_namespace());
    let ours = profile.record_path(&production());
    fs::create_dir_all(ours.parent().expect("folder")).expect("folder");
    fs::copy(theirs, &ours).expect("copied record");
    let error = not_found(&profile, &InstallCopy::NotInstalled);
    assert_eq!(error.code(), "record_unreadable", "{error}");
}

#[test]
fn the_two_records_disagreeing_is_reported() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    let other = profile.folder("Other");
    profile.record_move(&profile.roots.local, &production(), &moved);
    // The installation's copy names another folder.
    let error = not_found(&profile, &InstallCopy::Installed(Some(other.clone())));
    assert_eq!(
        error,
        LocationError::RecordsDisagree { record: Some(moved.clone()), install_copy: Some(other) }
    );
    // The installation records no move.
    let error = not_found(&profile, &InstallCopy::Installed(None));
    assert_eq!(error, LocationError::RecordsDisagree { record: Some(moved), install_copy: None });
}

#[test]
fn an_installation_copy_without_a_record_is_a_disagreement_not_the_default_place() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    let error = not_found(&profile, &InstallCopy::Installed(Some(moved.clone())));
    assert_eq!(error, LocationError::RecordsDisagree { record: None, install_copy: Some(moved) });
    assert_eq!(error.code(), "records_disagree");
    assert_eq!(error.missing_location(), None);
}

#[test]
fn a_missing_location_is_reported() {
    let profile = Profile::new();
    let moved = profile.folder("Unplugged");
    profile.record_move(&profile.roots.local, &production(), &moved);
    fs::remove_dir_all(&moved).expect("unplug");
    let error = not_found(&profile, &InstallCopy::NotInstalled);
    assert!(
        matches!(&error, LocationError::LocationMissing { location, .. } if *location == moved),
        "{error}"
    );
    assert_eq!(error.code(), "location_missing");
    // The desktop names the folder that is not there.
    assert_eq!(error.missing_location(), Some(moved.as_path()));
    let wrapped = LocationError::Production(Box::new(error));
    assert_eq!(wrapped.missing_location(), Some(moved.as_path()));
}

#[test]
fn a_location_without_this_installations_marker_is_reported() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    profile.record_move(&profile.roots.local, &production(), &moved);
    // Another namespace's data in the folder.
    write_marker(&moved, &DataMarker::new(test_namespace())).expect("other marker");
    let error = not_found(&profile, &InstallCopy::NotInstalled);
    assert!(
        matches!(&error, LocationError::MarkerMismatch { location, .. } if *location == moved),
        "{error}"
    );
    // No marker at all.
    fs::remove_file(moved.join("aura-data.v1.json")).expect("marker removed");
    let error = not_found(&profile, &InstallCopy::NotInstalled);
    assert_eq!(error.code(), "marker_mismatch", "{error}");
}
