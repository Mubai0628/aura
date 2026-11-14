//! Each layout older versions left is found as one item; what is in use or not plainly Aura's is
//! never found.
#![allow(clippy::unwrap_used)]
use crate::common;

use std::fs;

use aura_data_location::{LocationRecord, RECORD_FILE, write_record};
use aura_old_data::{Installed, Kind};
use common::{Home, current, ids, namespace, namespace_id, nothing_installed, production, write};

#[test]
fn the_layout_before_namespaces_is_one_item_with_both_sides_and_not_the_shared_log() {
    let home = Home::new();
    home.current_version();
    home.early_layout();
    let found = home.find(&nothing_installed());
    assert_eq!(ids(&found), ["early"]);
    let early = &found[0];
    assert_eq!(early.kind, Kind::EarlyLayout);
    let mut names: Vec<String> = early
        .paths
        .iter()
        .map(|path| {
            let side = if path.starts_with(home.local()) { "local" } else { "roaming" };
            format!("{side}:{}", path.file_name().unwrap().to_string_lossy())
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "local:config-history",
            "local:data",
            "local:desktop",
            "local:e2e-runs",
            "local:launcher",
            "local:plugins",
            "local:runtime",
            "local:skills",
            "local:tools",
            "local:web",
            "local:worktrees",
            "roaming:aura.toml",
            "roaming:commands.json",
            "roaming:hooks.toml",
            "roaming:mcp.json",
            "roaming:models.json",
            "roaming:settings",
        ]
    );
    assert_eq!(early.bytes, 4096 + 2 + 2 + 5 + 5 * 2);
    assert!(early.modified.is_some());
}

#[test]
fn a_namespace_no_installation_uses_is_found_with_its_settings() {
    let home = Home::new();
    home.current_version();
    let orphan = namespace("aura.desktop.test.orphan");
    let (local, roaming) = home.namespace(&orphan);
    let found = home.find(&nothing_installed());
    assert_eq!(ids(&found), [namespace_id(&orphan)]);
    assert_eq!(found[0].kind, Kind::Namespace);
    assert_eq!(found[0].paths, [local, roaming]);
    assert_eq!(found[0].bytes, 8192 + 3 + 2 + 15 + 2);
}

#[test]
fn a_namespace_with_only_its_settings_left_is_found() {
    let home = Home::new();
    home.current_version();
    let orphan = namespace("aura.desktop.test.settings-only");
    let (local, roaming) = home.namespace(&orphan);
    fs::remove_dir_all(&local).unwrap();
    let found = home.find(&nothing_installed());
    assert_eq!(ids(&found), [namespace_id(&orphan)]);
    assert_eq!(found[0].paths, [roaming]);
}

#[test]
fn a_namespace_whose_data_was_moved_takes_the_folder_it_was_moved_to_along() {
    let home = Home::new();
    home.current_version();
    let orphan = namespace("aura.desktop.test.moved");
    let (local, roaming, location) = home.moved_namespace(&orphan);
    let found = home.find(&nothing_installed());
    assert_eq!(ids(&found), [namespace_id(&orphan)]);
    assert_eq!(found[0].paths, [local, location, roaming]);
}

#[test]
fn the_current_the_released_and_the_installed_namespaces_are_never_found() {
    let home = Home::new();
    home.current_version();
    let installed = namespace("aura.desktop.test.installed");
    home.namespace(&current());
    home.namespace(&production());
    home.namespace(&installed);
    let orphan = namespace("aura.desktop.test.orphan");
    home.namespace(&orphan);
    let found = home.find(&Installed::Known(vec![installed.object_name_hash()]));
    assert_eq!(ids(&found), [namespace_id(&orphan)]);
}

#[test]
fn no_namespace_is_found_while_an_installation_cannot_be_read() {
    let home = Home::new();
    home.current_version();
    home.early_layout();
    home.namespace(&namespace("aura.desktop.test.orphan"));
    let found = home.find(&Installed::Unknown("D:\\Aura: active.json is missing".to_owned()));
    assert_eq!(ids(&found), ["early"]);
}

#[test]
fn a_side_of_the_early_layout_holding_a_name_aura_does_not_write_is_left_out() {
    let home = Home::new();
    home.current_version();
    home.early_layout();
    write(&home.local().join("profile.dat"), b"another program's");
    let found = home.find(&nothing_installed());
    assert_eq!(ids(&found), ["early"]);
    assert!(
        found[0].paths.iter().all(|path| path.starts_with(home.roaming())),
        "{:?}",
        found[0].paths
    );

    write(&home.roaming().join("notes.txt"), b"the user's");
    assert_eq!(ids(&home.find(&nothing_installed())), Vec::<&str>::new());
}

#[test]
fn a_namespace_holding_a_name_aura_does_not_write_is_never_found() {
    let home = Home::new();
    home.current_version();
    let in_local = namespace("aura.desktop.test.local-extra");
    let (local, _) = home.namespace(&in_local);
    write(&local.join("notes.txt"), b"the user's");
    let in_roaming = namespace("aura.desktop.test.roaming-extra");
    let (_, roaming) = home.namespace(&in_roaming);
    write(&roaming.join("keep.md"), b"the user's");
    let in_moved = namespace("aura.desktop.test.moved-extra");
    let (_, _, location) = home.moved_namespace(&in_moved);
    write(&location.join("photo.jpg"), b"the user's");
    assert_eq!(ids(&home.find(&nothing_installed())), Vec::<&str>::new());
}

#[test]
fn a_working_copy_or_a_link_inside_keeps_a_folder_from_being_found() {
    let home = Home::new();
    home.current_version();
    home.early_layout();
    write(&home.local().join(r"worktrees\project\README.md"), b"work");
    let with_copy = namespace("aura.desktop.test.worktree");
    let (local, _) = home.namespace(&with_copy);
    write(&local.join(r"worktrees\feature\main.rs"), b"fn main() {}");
    let with_link = namespace("aura.desktop.test.link");
    let (local, _) = home.namespace(&with_link);
    let elsewhere = home.folder.path().join("Projects");
    write(&elsewhere.join("keep.txt"), b"the user's");
    junction::create(&elsewhere, local.join(r"data\linked")).expect("junction");

    let found = home.find(&nothing_installed());
    // The early layout's roaming side holds nothing of the working copy and is still found.
    assert_eq!(ids(&found), ["early"]);
    assert!(found[0].paths.iter().all(|path| path.starts_with(home.roaming())));
}

#[test]
fn a_namespace_with_a_move_waiting_or_under_way_is_not_found() {
    let home = Home::new();
    home.current_version();
    let moving = namespace("aura.desktop.test.moving");
    let (local, _) = home.namespace(&moving);
    write(&local.join(aura_data_move::JOURNAL_FILE), b"{}");
    assert_eq!(ids(&home.find(&nothing_installed())), Vec::<&str>::new());
}

#[test]
fn a_record_that_names_another_namespace_or_a_folder_that_is_gone_keeps_it_from_being_found() {
    let home = Home::new();
    home.current_version();
    let other = namespace("aura.desktop.test.other");
    let (local, _, _) = home.moved_namespace(&other);
    let impostor = namespace("aura.desktop.test.impostor");
    let (impostor_local, _) = home.namespace(&impostor);
    fs::copy(local.join(RECORD_FILE), impostor_local.join(RECORD_FILE)).unwrap();
    let gone = namespace("aura.desktop.test.gone");
    let (gone_local, _) = home.namespace(&gone);
    write_record(
        &gone_local.join(RECORD_FILE),
        &LocationRecord::new(gone.clone(), home.folder.path().join("Unplugged")),
    )
    .unwrap();
    assert_eq!(ids(&home.find(&nothing_installed())), [namespace_id(&other)]);
}

#[test]
fn a_folder_whose_name_is_not_a_namespace_is_never_found() {
    let home = Home::new();
    home.current_version();
    let [local, roaming] = home.roots.namespace_folders();
    write(&local.join(r"Some Folder\data\aura.db"), b"x");
    write(&local.join(r"0123456789ABCDEF0123456789ABCDEF\data\aura.db"), b"x");
    write(&roaming.join("0123456789abcdef0123456789abcdef.json"), b"{}");
    assert_eq!(ids(&home.find(&nothing_installed())), Vec::<&str>::new());
}
