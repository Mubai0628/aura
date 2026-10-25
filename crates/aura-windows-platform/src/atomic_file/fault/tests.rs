use super::*;
use crate::{
    HandleBoundReplaceContent, HandlePublishError, handle_bound_replace_with_backup_checked,
    inspect_existing_path, open_verified_publish_parent,
};
use std::sync::{Arc, atomic::AtomicUsize};

#[test]
fn an_armed_destination_does_not_fail_another_real_publish() {
    let root = tempfile::tempdir().expect("new real root");
    let first = root.path().join("first.txt");
    let second = root.path().join("second.txt");
    std::fs::write(&first, b"old first").expect("seed first");
    std::fs::write(&second, b"old second").expect("seed second");
    let first_id = inspect_existing_path(&first, false).expect("first ID").identity;
    let second_id = inspect_existing_path(&second, false).expect("second ID").identity;
    let parent = open_verified_publish_parent(root.path()).expect("same real parent");
    let armed = arm(Stage::QueryMovedBackup, &first);
    handle_bound_replace_with_backup_checked(
        &parent,
        "second.txt".as_ref(),
        &second_id,
        HandleBoundReplaceContent::Unchecked,
        b"new second",
        ".second.backup".as_ref(),
    )
    .expect("another destination must not consume this fault")
    .commit()
    .expect("unrelated real commit");
    let error = match handle_bound_replace_with_backup_checked(
        &parent,
        "first.txt".as_ref(),
        &first_id,
        HandleBoundReplaceContent::Unchecked,
        b"new first",
        ".first.backup".as_ref(),
    ) {
        Ok(_) => panic!("the exact selected destination must fail after moving the old object"),
        Err(error) => error,
    };
    assert!(matches!(error, HandlePublishError::RolledBackAfterPublish { .. }), "{error}");
    assert_eq!(std::fs::read(first).expect("restored first"), b"old first");
    assert_eq!(std::fs::read(second).expect("committed second"), b"new second");
    drop(armed);
}

#[test]
fn dropping_one_arm_keeps_the_other_registration() {
    let root = tempfile::tempdir().expect("root");
    let destination = root.path().join("absent.txt");
    let first = arm(Stage::RestoreBackup, &destination);
    let second = arm(Stage::RestoreBackup, &destination);
    assert!(armed_now(Stage::RestoreBackup, &destination));
    drop(first);
    assert!(armed_now(Stage::RestoreBackup, &destination));
    assert!(!armed_now(Stage::RestoreBackup, &root.path().join("unrelated.txt")));
    drop(second);
    assert!(!armed_now(Stage::RestoreBackup, &destination));
}

#[test]
fn condition_and_commit_guards_clean_only_their_own_hooks() {
    let root = tempfile::tempdir().expect("root");
    let destination = root.path().join("name.txt");
    let first_count = Arc::new(AtomicUsize::new(0));
    let second_count = Arc::new(AtomicUsize::new(0));
    let first = {
        let count = Arc::clone(&first_count);
        observe_replace_condition(
            &destination,
            Box::new(move |_, _| {
                count.fetch_add(1, Ordering::Relaxed);
            }),
        )
    };
    let second = {
        let count = Arc::clone(&second_count);
        observe_replace_condition(
            &destination,
            Box::new(move |_, _| {
                count.fetch_add(1, Ordering::Relaxed);
            }),
        )
    };
    drop(first);
    consumed_replace_condition(&destination, "sha256", "measured");
    assert_eq!(first_count.load(Ordering::Relaxed), 0);
    assert_eq!(second_count.load(Ordering::Relaxed), 1);
    drop(second);
    consumed_replace_condition(&destination, "sha256", "measured again");
    assert_eq!(second_count.load(Ordering::Relaxed), 1, "a dropped condition observer is removed");

    let first = observe_commit(&destination, Box::new(|_| panic!("removed observer")));
    let second = {
        let count = Arc::clone(&second_count);
        observe_commit(
            &destination,
            Box::new(move |_| {
                count.fetch_add(1, Ordering::Relaxed);
            }),
        )
    };
    drop(first);
    reached_commit(&destination);
    assert_eq!(second_count.load(Ordering::Relaxed), 2);
    drop(second);
    reached_commit(&destination);
    assert_eq!(second_count.load(Ordering::Relaxed), 2);
}

#[test]
fn interference_ownership_and_directory_boundaries_are_independent() {
    let root = tempfile::tempdir().expect("root");
    let first_root = root.path().join("one");
    let sibling = root.path().join("one-extra");
    std::fs::create_dir(&first_root).expect("first directory");
    std::fs::create_dir(&sibling).expect("sibling directory");
    let first = interfere_at(
        Stage::CreateAbsentBeforeRename,
        &first_root,
        Box::new(|_| panic!("removed hook")),
    );
    let count = Arc::new(AtomicUsize::new(0));
    let second = {
        let count = Arc::clone(&count);
        interfere_at(
            Stage::CreateAbsentBeforeRename,
            &first_root,
            Box::new(move |_| {
                count.fetch_add(1, Ordering::Relaxed);
            }),
        )
    };
    drop(first);
    reached_interference(Stage::CreateAbsentBeforeRename, &sibling.join("name.txt"));
    assert_eq!(count.load(Ordering::Relaxed), 0, "a string prefix is not directory containment");
    reached_interference(Stage::CreateAbsentBeforeRename, &first_root.join("name.txt"));
    assert_eq!(count.load(Ordering::Relaxed), 1, "dropping another hook did not remove this one");
    drop(second);
}
