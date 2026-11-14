//! Global memory is the released product's, wherever its data is, for every namespace.
use crate::support;

use aura_data_location::{InstallCopy, LocationError, Locations};
use support::{Profile, production, test_namespace};

fn global_memory(
    profile: &Profile,
    namespace: &aura_runtime_contract::BundleNamespace,
) -> Result<std::path::PathBuf, LocationError> {
    Locations::resolve(&profile.roots, namespace, &InstallCopy::NotInstalled)
        .expect("locations")
        .with_global_memory(&profile.roots, namespace)
        .map(|locations| locations.global_memory.expect("global memory"))
}

#[test]
fn without_a_move_global_memory_is_in_the_profile_folder() {
    let profile = Profile::new();
    let memory = profile.roots.profile.join(".aura/memories");
    assert_eq!(global_memory(&profile, &production()).expect("released"), memory);
    assert_eq!(global_memory(&profile, &test_namespace()).expect("other"), memory);
}

#[test]
fn global_memory_moves_with_the_released_products_data() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    // The released product runs with the real LOCALAPPDATA, which is the known folder.
    let released = Profile {
        roots: aura_data_location::Roots {
            local: profile.roots.known_local.clone(),
            ..profile.roots.clone()
        },
        folder: tempfile::tempdir().expect("unused"),
    };
    profile.record_move(&profile.roots.known_local, &production(), &moved);
    assert_eq!(global_memory(&released, &production()).expect("released"), moved.join("memories"));
}

#[test]
fn a_development_run_with_its_own_localappdata_follows_the_released_products_move() {
    let profile = Profile::new();
    let moved = profile.folder("Moved");
    // The record is in the known local folder; this run's LOCALAPPDATA is elsewhere.
    profile.record_move(&profile.roots.known_local, &production(), &moved);
    assert_eq!(global_memory(&profile, &test_namespace()).expect("other"), moved.join("memories"));
}

#[test]
fn a_released_products_record_that_cannot_be_used_is_not_replaced_by_the_profile_folder() {
    let profile = Profile::new();
    let moved = profile.folder("Unplugged");
    profile.record_move(&profile.roots.known_local, &production(), &moved);
    std::fs::remove_dir_all(&moved).expect("unplug");
    let error = global_memory(&profile, &test_namespace()).expect_err("must not be found");
    assert!(matches!(error, LocationError::Production(_)), "{error}");
    assert_eq!(error.code(), "location_missing");
}
