//! Uninstalling removes only what an installation holds of Aura's own: what else the folder holds
//! stays with it, a folder marked for another installation is refused, and an installation made
//! before markers is known by its layout.
use std::fs;

use super::{Work, namespace};
use crate::{
    marker::INSTALL_MARKER,
    uninstall::{plan_uninstall, uninstall},
};

#[test]
fn what_is_not_aura_s_stays_with_its_folder() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let root = work.root();
    fs::write(root.join("notes.txt"), b"mine").unwrap();
    let host = work.host();
    let removed = uninstall(&host, &plan_uninstall(&host, &root).unwrap(), false).unwrap();
    assert_eq!(removed.left_behind, vec![root.join("notes.txt")]);
    let left: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["notes.txt".to_owned()]);
}

#[test]
fn a_folder_marked_for_another_installation_is_not_uninstalled() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let root = work.root();
    let other = serde_json::json!({ "schema_version": 1, "namespace": "aura.desktop.test.other" });
    fs::write(root.join(INSTALL_MARKER), other.to_string()).unwrap();
    assert!(plan_uninstall(&work.host(), &root).is_err());
    assert!(root.join("versions").is_dir());
}

#[test]
fn an_installation_from_before_markers_is_known_by_its_layout() {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let root = work.root();
    fs::remove_file(root.join(INSTALL_MARKER)).unwrap();
    let host = work.host();
    let plan = plan_uninstall(&host, &root).unwrap();
    assert_eq!(plan.namespace, namespace());
    uninstall(&host, &plan, false).unwrap();
    assert!(!root.exists());
}
