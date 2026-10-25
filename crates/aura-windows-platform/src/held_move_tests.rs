//! Verifies the object moved is the same held object, with a positive non-conflicting control.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;

use crate::{
    handle_bound_move_existing, held_file_identity, open_held_for_move,
    open_verified_publish_parent,
};

#[test]
fn held_move_denies_competing_rename_and_write_but_moves_its_own_object() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let outsider = root.path().join("outsider.txt");
    fs::write(&source, b"original").unwrap();
    let held = open_held_for_move(&source).unwrap();
    let identity = held_file_identity(held.file()).unwrap();
    assert!(
        fs::rename(&source, &outsider).is_err(),
        "another actor must not remove the verified source name"
    );
    assert!(
        fs::write(&source, b"competitor").is_err(),
        "same-identity rewrites must be denied while held"
    );
    let parent = open_verified_publish_parent(root.path()).unwrap();
    handle_bound_move_existing(&held, &parent, "moved.txt".as_ref()).unwrap();
    assert_eq!(held_file_identity(held.file()).unwrap(), identity);
    assert!(!source.exists());
    drop(held);
    assert_eq!(fs::read(root.path().join("moved.txt")).unwrap(), b"original");
}

#[test]
fn held_move_refuses_an_existing_destination_without_touching_either_file() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let destination = root.path().join("destination.txt");
    fs::write(&source, b"original").unwrap();
    fs::write(&destination, b"third party").unwrap();
    let held = open_held_for_move(&source).unwrap();
    let parent = open_verified_publish_parent(root.path()).unwrap();
    assert!(handle_bound_move_existing(&held, &parent, "destination.txt".as_ref()).is_err());
    drop(held);
    assert_eq!(fs::read(&source).unwrap(), b"original");
    assert_eq!(fs::read(&destination).unwrap(), b"third party");
}
