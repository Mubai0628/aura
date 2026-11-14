//! Removing takes whole items or nothing of them, and finishes what an earlier removal left.
#![allow(clippy::unwrap_used)]
use crate::common;

use std::fs::{self, File};

use aura_old_data::{Installed, Kind, Removed, Scope};
use common::{Home, current, ids, namespace, namespace_id, nothing_installed, write};

fn remove(home: &Home, installed: &Installed, ids: &[String]) -> Removed {
    aura_old_data::remove(&Scope { roots: &home.roots, current: &current(), installed }, ids)
}

/// Every name in Aura's four folders that a removal set aside.
fn leftovers(home: &Home) -> Vec<String> {
    let [local, roaming] = home.roots.namespace_folders();
    [home.local(), home.roaming(), local, roaming]
        .iter()
        .flat_map(|folder| fs::read_dir(folder).into_iter().flatten().flatten())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(aura_old_data::REMOVING_SUFFIX))
        .collect()
}

#[test]
fn the_picked_items_go_and_the_rest_stay() {
    let home = Home::new();
    home.current_version();
    home.early_layout();
    let orphan = namespace("aura.desktop.test.orphan");
    let (local, roaming) = home.namespace(&orphan);
    let removed = remove(&home, &nothing_installed(), &[namespace_id(&orphan)]);
    assert_eq!(removed, Removed { removed: vec![namespace_id(&orphan)], ..Removed::default() });
    assert!(!local.exists() && !roaming.exists());
    assert_eq!(ids(&home.find(&nothing_installed())), ["early"]);
    assert!(leftovers(&home).is_empty(), "{:?}", leftovers(&home));
    // The current version's folders and the shared log are untouched.
    assert!(home.local().join(r"logs\core.jsonl").is_file());
    let [current_local, _] = home.roots.namespace_folders();
    assert!(current_local.join(current().object_name_hash()).is_dir());

    let removed = remove(&home, &nothing_installed(), &["early".to_owned()]);
    assert_eq!(removed.removed, ["early"]);
    assert!(!home.local().join("data").exists() && !home.roaming().join("aura.toml").exists());
    assert!(home.local().join("namespaces").is_dir() && home.roaming().join("namespaces").is_dir());
    assert!(home.find(&nothing_installed()).is_empty());
}

#[test]
fn the_folder_moved_data_is_in_goes_with_its_namespace() {
    let home = Home::new();
    home.current_version();
    let orphan = namespace("aura.desktop.test.moved");
    let (local, roaming, location) = home.moved_namespace(&orphan);
    let removed = remove(&home, &nothing_installed(), &[namespace_id(&orphan)]);
    assert_eq!(removed.removed, [namespace_id(&orphan)]);
    assert!(!local.exists() && !roaming.exists() && !location.exists());
    assert!(location.parent().unwrap().is_dir(), "the folder above the moved one stays");
}

#[test]
fn an_item_with_an_open_file_is_kept_whole() {
    let home = Home::new();
    home.current_version();
    let first = namespace("aura.desktop.test.open-first");
    let (local, _) = home.namespace(&first);
    let second = namespace("aura.desktop.test.open-second");
    let (second_local, second_roaming) = home.namespace(&second);
    let _store = File::open(local.join(r"data\v2-execution\aura-v2.db")).unwrap();
    // The second item's settings are open: its data folder is renamed first and put back.
    let _settings = File::open(second_roaming.join("models.json")).unwrap();

    let removed =
        remove(&home, &nothing_installed(), &[namespace_id(&first), namespace_id(&second)]);
    assert_eq!(removed.in_use, [namespace_id(&first), namespace_id(&second)]);
    assert!(removed.removed.is_empty() && removed.failed.is_empty(), "{removed:?}");
    assert!(local.join(r"data\v2-execution\aura-v2.db").is_file());
    assert!(second_local.join(r"data\v2-execution\aura-v2.db").is_file());
    assert!(second_roaming.join("models.json").is_file());
    assert!(leftovers(&home).is_empty(), "{:?}", leftovers(&home));
}

#[test]
fn a_removal_cut_short_is_found_as_unfinished_and_finished_later() {
    let home = Home::new();
    home.current_version();
    let [local, _] = home.roots.namespace_folders();
    let cut = local.join(format!("{}.1762480000000.aura-removing", "0".repeat(32)));
    write(&cut.join(r"data\aura-v2.db"), &[3; 512]);
    let cut_early = home.roaming().join("aura.toml.1762480000000.aura-removing");
    write(&cut_early, b"a = 1");
    let found = home.find(&nothing_installed());
    assert_eq!(ids(&found), ["unfinished"]);
    assert_eq!(found[0].kind, Kind::Unfinished);
    assert_eq!(found[0].bytes, 512 + 5);

    let removed = remove(&home, &nothing_installed(), &["unfinished".to_owned()]);
    assert_eq!(removed.removed, ["unfinished"]);
    assert!(!cut.exists() && !cut_early.exists());
}

#[test]
fn an_id_that_is_not_found_now_is_left_alone() {
    let home = Home::new();
    home.current_version();
    let installed = namespace("aura.desktop.test.installed");
    let (local, roaming) = home.namespace(&installed);
    let now_installed = Installed::Known(vec![installed.object_name_hash()]);
    let removed = remove(
        &home,
        &now_installed,
        &[namespace_id(&installed), namespace_id(&current()), "early".to_owned()],
    );
    assert_eq!(removed, Removed::default());
    assert!(local.is_dir() && roaming.is_dir());
    let removed =
        remove(&home, &Installed::Unknown("entry".to_owned()), &[namespace_id(&installed)]);
    assert_eq!(removed, Removed::default());
    assert!(local.is_dir());
}

#[test]
fn read_only_files_inside_are_removed() {
    let home = Home::new();
    home.current_version();
    let orphan = namespace("aura.desktop.test.read-only");
    let (local, _) = home.namespace(&orphan);
    let file = local.join(r"data\v2-execution\aura-v2.db");
    let mut permissions = fs::metadata(&file).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&file, permissions).unwrap();
    let removed = remove(&home, &nothing_installed(), &[namespace_id(&orphan)]);
    assert_eq!(removed.removed, [namespace_id(&orphan)], "{removed:?}");
    assert!(!local.exists());
}
