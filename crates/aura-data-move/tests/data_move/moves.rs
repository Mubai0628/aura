//! A move to another drive and back: what is copied, what stays, the records, the access, and the
//! old copy going once the new place is ready.
#![allow(clippy::unwrap_used)]
use crate::support;

use std::fs;

use aura_data_location::{MARKER_FILE, RECORD_FILE};
use aura_data_move::{
    JOURNAL_FILE, MAX_LOCATION_CHARS, Outcome, Stage, read_failure, remove_old_copies,
};
use support::{FILES, Profile, Run, ask, files, on_other_drive, production, test_namespace, write};

#[test]
fn a_move_to_another_drive_copies_everything_but_the_caches_and_points_the_record_at_it() {
    let profile = Profile::new(production());
    let before = profile.default_place();
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    let run = Run::default();
    let outcome = run.perform(&profile, None);
    assert_eq!(outcome, Outcome::Moved { from: None, to: Some(destination.clone()) });

    let now = profile.current();
    assert_eq!(now.location.as_deref(), Some(destination.as_path()));
    assert_eq!(now.data_root, destination.join("data"));
    for (path, bytes) in FILES {
        assert_eq!(fs::read(destination.join("data").join(path)).unwrap(), bytes, "{path}");
        assert_eq!(
            fs::read(before.data_root.join(path)).unwrap(),
            bytes,
            "{path} stays until removed"
        );
    }
    assert_eq!(
        fs::read(destination.join("logs/core.jsonl")).unwrap(),
        b"{\"event\":\"core_started\"}\n"
    );
    let window = files(&destination.join("ui-profile"));
    let names: Vec<&str> = window.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["Local Storage/leveldb/000003.log", "Partitions/aura-browser/Cookies", "Preferences"],
        "Chromium's caches are rebuilt, not copied"
    );
    assert_eq!(fs::read(destination.join("memories/MEMORY.md")).unwrap(), b"- remembered");
    assert!(destination.join(MARKER_FILE).is_file());
    assert!(!destination.join("data/chrome").exists(), "the Chrome folder stays");
    assert!(before.chrome_root.join("manifest.json").is_file());
    assert_eq!(aura_windows_platform::private_folder_problem(&destination).unwrap(), None);

    assert_eq!(
        *run.stages.borrow(),
        [
            Stage::Copying,
            Stage::Verifying,
            Stage::CheckingStores,
            Stage::SettingAccess,
            Stage::Recording
        ]
    );
    assert_eq!(*run.stores_checked.borrow(), [destination.join("data")]);
    assert_eq!(*run.install_copies.borrow(), [Some(destination.clone())]);
    assert!(!before.fixed_root.join(JOURNAL_FILE).exists());
    assert_eq!(read_failure(&before.data_root), None);

    let mut pending = now.pending_removal.clone();
    pending.sort();
    let mut expected = vec![
        before.data_root.join("backups"),
        before.data_root.join("data"),
        before.data_root.join("launcher"),
        before.data_root.join("logs"),
        before.core_log.clone(),
        before.ui_profile.clone(),
        before.global_memory.clone().unwrap(),
    ];
    expected.sort();
    assert_eq!(pending, expected);
}

#[test]
fn the_old_copy_goes_once_the_new_place_is_ready_and_only_what_the_move_left() {
    let profile = Profile::new(production());
    let before = profile.default_place();
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    assert!(matches!(Run::default().perform(&profile, None), Outcome::Moved { .. }));

    // A path that is not Aura's, written into the record by someone else, is never touched.
    let stranger = profile.here("someone-else");
    write(&stranger.join("keep.txt"), b"keep");
    let record_path = before.fixed_root.join(RECORD_FILE);
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    record["pending_removal"].as_array_mut().unwrap().push(stranger.display().to_string().into());
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();

    let removal =
        remove_old_copies(&profile.roots, &profile.namespace, &profile.current()).unwrap();
    assert_eq!(removal.refused, std::slice::from_ref(&stranger));
    assert!(removal.failed.is_empty(), "{removal:?}");
    assert_eq!(removal.removed.len(), 7, "{removal:?}");
    assert_eq!(fs::read(stranger.join("keep.txt")).unwrap(), b"keep");
    let mut left: Vec<String> = fs::read_dir(&before.fixed_root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["chrome", RECORD_FILE]);
    assert!(!before.core_log.exists() && !before.ui_profile.exists());
    assert!(!before.global_memory.unwrap().exists());
    let now = profile.current();
    assert!(now.pending_removal.is_empty());
    assert_eq!(now.location.as_deref(), Some(destination.as_path()));
    for (path, bytes) in FILES {
        assert_eq!(fs::read(now.data_root.join(path)).unwrap(), bytes, "{path}");
    }
}

#[test]
fn moving_back_to_the_default_places_brings_everything_home_and_then_removes_the_folder() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    assert!(matches!(Run::default().perform(&profile, None), Outcome::Moved { .. }));
    remove_old_copies(&profile.roots, &profile.namespace, &profile.current()).unwrap();

    ask(&profile, None);
    let run = Run::default();
    assert_eq!(
        run.perform(&profile, None),
        Outcome::Moved { from: Some(destination.clone()), to: None }
    );
    let home = profile.current();
    assert_eq!(home.location, None);
    assert_eq!(home.pending_removal, std::slice::from_ref(&destination));
    assert_eq!(*run.install_copies.borrow(), [None]);
    for (path, bytes) in FILES {
        assert_eq!(fs::read(home.data_root.join(path)).unwrap(), bytes, "{path}");
    }
    assert_eq!(fs::read(&home.core_log).unwrap(), b"{\"event\":\"core_started\"}\n");
    assert_eq!(fs::read(home.ui_profile.join("Preferences")).unwrap(), b"{\"window\":1}");
    assert_eq!(
        fs::read(home.global_memory.as_ref().unwrap().join("MEMORY.md")).unwrap(),
        b"- remembered"
    );

    let removal = remove_old_copies(&profile.roots, &profile.namespace, &home).unwrap();
    assert_eq!(removal.removed, std::slice::from_ref(&destination));
    assert!(!destination.exists());
    assert!(!home.record.exists(), "no record is what the default places mean");
    assert_eq!(profile.current().location, None);
}

#[test]
fn another_namespace_moves_its_data_and_window_profile_but_not_global_memory_or_the_shared_log() {
    let profile = Profile::new(test_namespace());
    let before = profile.default_place();
    assert!(before.ui_profile.starts_with(&before.fixed_root));
    // The log at the default places is the user's: the released product writes it too.
    let shared_log = before.core_log.clone();
    assert!(!shared_log.starts_with(&before.fixed_root));
    let destination = profile.here("Moved");
    ask(&profile, Some(&destination));
    assert!(matches!(Run::default().perform(&profile, None), Outcome::Moved { .. }));
    assert!(!destination.join("memories").exists());
    assert!(!destination.join("logs").exists(), "the shared log is not copied");
    assert!(!destination.join("data/ui-profile").exists(), "the window profile is its own part");
    assert_eq!(fs::read(destination.join("ui-profile/Preferences")).unwrap(), b"{\"window\":1}");
    let now = profile.current();
    assert!(now.pending_removal.contains(&before.ui_profile));
    assert!(!now.pending_removal.contains(&shared_log));

    // Not even a record that lists it removes it.
    let record_path = before.fixed_root.join(RECORD_FILE);
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    record["pending_removal"].as_array_mut().unwrap().push(shared_log.display().to_string().into());
    fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    let removal =
        remove_old_copies(&profile.roots, &profile.namespace, &profile.current()).unwrap();
    assert_eq!(removal.refused, std::slice::from_ref(&shared_log));
    assert_eq!(fs::read(&shared_log).unwrap(), b"{\"event\":\"core_started\"}\n");
    assert!(!before.ui_profile.exists());
}

#[test]
fn the_deepest_file_aura_writes_fits_below_the_longest_location() {
    let profile = Profile::new(production());
    // A memory vault's maintenance file: 174 characters below the data folder.
    let deep = format!(
        "data/memory-vaults/{}.records/maintenance/{}.json",
        "3b2a76af0317953bd5c179f0efd6cbb692058f3333dd8adf98e71aa6f24ab0ef",
        "6336a984571e8cc26deca88a220a3a101c79833228660f3880e8895790751b4f0"
    );
    assert_eq!(deep.len(), 174);
    write(&profile.default_place().data_root.join(&deep), b"deep");
    let drive = on_other_drive();
    let base = drive.path().as_os_str().len();
    let destination = drive.path().join("d".repeat(MAX_LOCATION_CHARS - base - 1));
    assert_eq!(destination.as_os_str().len(), MAX_LOCATION_CHARS);
    ask(&profile, Some(&destination));
    assert!(matches!(Run::default().perform(&profile, None), Outcome::Moved { .. }));
    let moved = destination.join("data").join(&deep);
    assert_eq!(moved.as_os_str().len(), 259, "within MAX_PATH with its NUL");
    assert_eq!(fs::read(&moved).unwrap(), b"deep");
}
