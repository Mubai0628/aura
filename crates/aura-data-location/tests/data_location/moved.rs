//! With a location record the data, Core's log and the Electron profile are in the moved folder;
//! the settings, the Chrome files and the record stay in the default places.
use crate::support;

use aura_data_location::{InstallCopy, Locations};
use support::{Profile, production, test_namespace};

#[test]
fn a_moved_installation_finds_its_data_in_the_new_folder() {
    let profile = Profile::new();
    let namespace = production();
    let moved = profile.folder("D-drive/Aura");
    profile.record_move(&profile.roots.local, &namespace, &moved);
    let copy = InstallCopy::Installed(Some(moved.clone()));
    let locations = Locations::resolve(&profile.roots, &namespace, &copy).expect("locations");
    let fixed = profile.roots.local.join("Aura/namespaces").join(namespace.object_name_hash());
    assert_eq!(locations.data_root, moved.join("data"));
    assert_eq!(locations.core_log, moved.join("logs/core.jsonl"));
    assert_eq!(locations.ui_profile, moved.join("ui-profile"));
    assert_eq!(locations.location.as_deref(), Some(moved.as_path()));
    assert_eq!(locations.fixed_root, fixed);
    assert_eq!(locations.chrome_root, fixed.join("chrome"));
    assert_eq!(locations.record, fixed.join("location.v1.json"));
    assert_eq!(
        locations.settings_root,
        profile.roots.roaming.join("Aura/namespaces").join(namespace.object_name_hash())
    );
    assert_eq!(
        locations.control_files(),
        [fixed.join("location.v1.json"), moved.join("aura-data.v1.json")]
    );
}

#[test]
fn the_installation_copy_may_spell_the_folder_differently() {
    let profile = Profile::new();
    let namespace = test_namespace();
    let moved = profile.folder("Moved");
    profile.record_move(&profile.roots.local, &namespace, &moved);
    let spelled = format!("{}\\", moved.display().to_string().to_uppercase());
    let copy = InstallCopy::Installed(Some(spelled.into()));
    let locations = Locations::resolve(&profile.roots, &namespace, &copy).expect("locations");
    assert_eq!(locations.data_root, moved.join("data"));
}

#[test]
fn a_run_without_an_installation_folder_reads_the_record_alone() {
    let profile = Profile::new();
    let namespace = test_namespace();
    let moved = profile.folder("Moved");
    profile.record_move(&profile.roots.local, &namespace, &moved);
    let locations = Locations::resolve(&profile.roots, &namespace, &InstallCopy::NotInstalled)
        .expect("locations");
    assert_eq!(locations.data_root, moved.join("data"));
}

#[test]
fn the_protected_folders_cover_the_moved_data_but_not_global_memory() {
    let profile = Profile::new();
    let namespace = production();
    let moved = profile.folder("Moved");
    profile.record_move(&profile.roots.local, &namespace, &moved);
    let locations = Locations::resolve(&profile.roots, &namespace, &InstallCopy::NotInstalled)
        .expect("locations")
        .with_global_memory(&profile.roots, &namespace)
        .expect("global memory");
    let protected = locations.protected_folders();
    for folder in [moved.join("data"), moved.join("logs"), moved.join("ui-profile")] {
        assert!(protected.contains(&folder), "{folder:?} is not protected: {protected:?}");
    }
    let memory = locations.global_memory.expect("global memory");
    assert!(
        protected.iter().all(|folder| !memory.starts_with(folder)),
        "global memory {memory:?} is inside a protected folder: {protected:?}"
    );
}
