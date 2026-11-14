//! A move that fails, is stopped or loses power: the data stays where it was and opens as before,
//! what the move made is taken back, and the reason is kept for the storage page.
#![allow(clippy::unwrap_used)]
use crate::support;

use std::fs;

use aura_data_location::MARKER_FILE;
use aura_data_move::{
    Fault, JOURNAL_FILE, Outcome, State, dismiss_failure, read_failure, remove_old_copies,
};
use support::{FILES, Profile, Run, ask, on_other_drive, production, write};

fn assert_data_where_it_was(profile: &Profile) {
    let now = profile.current();
    assert_eq!(now.location, None);
    assert!(now.pending_removal.is_empty());
    for (path, bytes) in FILES {
        assert_eq!(fs::read(now.data_root.join(path)).unwrap(), bytes, "{path}");
    }
    assert_eq!(fs::read(now.ui_profile.join("Preferences")).unwrap(), b"{\"window\":1}");
    assert!(!now.fixed_root.join(JOURNAL_FILE).exists());
}

#[test]
fn every_failure_before_the_records_takes_the_copy_back_and_says_why() {
    let cases = [
        (Fault::CopyFails, "复制出错"),
        (Fault::CopyDiffers, "复制后核对不上"),
        (Fault::StoresFail, "数据库检查没有通过"),
        (Fault::AccessWrong, "权限没有设好"),
    ];
    for (fault, said) in cases {
        let profile = Profile::new(production());
        let drive = on_other_drive();
        let destination = drive.path().join("AuraData");
        ask(&profile, Some(&destination));
        let run = Run::default();
        let Outcome::Failed { reason } = run.perform(&profile, Some(fault)) else {
            panic!("{fault:?} did not fail");
        };
        assert!(reason.contains(said), "{fault:?}: {reason}");
        assert!(!destination.exists(), "{fault:?} left {}", destination.display());
        assert!(run.install_copies.borrow().is_empty(), "{fault:?}");
        assert_data_where_it_was(&profile);
        let failure = read_failure(&profile.current().data_root).unwrap();
        assert_eq!(failure.destination.as_deref(), Some(destination.as_path()), "{fault:?}");
        assert_eq!(failure.reason, reason);
    }
}

#[test]
fn a_failed_check_of_the_stores_is_reported_with_its_reason() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    let run =
        Run { stores_fail: Some("database disk image is malformed".to_owned()), ..Run::default() };
    let Outcome::Failed { reason } = run.perform(&profile, None) else { panic!("moved") };
    assert_eq!(reason, "数据库检查没有通过：database disk image is malformed");
    assert_eq!(*run.stores_checked.borrow(), [destination.join("data")]);
    assert!(!destination.exists());
    assert_data_where_it_was(&profile);
}

#[test]
fn a_chosen_empty_folder_is_left_empty_and_the_failure_can_be_dismissed() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("Chosen");
    fs::create_dir(&destination).unwrap();
    ask(&profile, Some(&destination));
    assert!(matches!(
        Run::default().perform(&profile, Some(Fault::StoresFail)),
        Outcome::Failed { .. }
    ));
    assert!(destination.is_dir(), "the user's folder stays");
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    assert_data_where_it_was(&profile);
    let data_root = profile.current().data_root;
    assert!(read_failure(&data_root).is_some());
    dismiss_failure(&data_root).unwrap();
    assert_eq!(read_failure(&data_root), None);
}

#[test]
fn stopping_part_way_takes_the_copy_back_without_calling_it_a_failure() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    let run = Run { stop_after: Some(1), ..Run::default() };
    assert_eq!(run.perform(&profile, None), Outcome::Cancelled);
    assert!(!destination.exists());
    assert_data_where_it_was(&profile);
    assert_eq!(read_failure(&profile.current().data_root), None);
}

#[test]
fn a_folder_that_is_no_longer_empty_when_the_move_starts_is_not_touched() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    write(&destination.join("notes.txt"), b"the user's");
    let Outcome::Failed { reason } = Run::default().perform(&profile, None) else {
        panic!("moved")
    };
    assert!(reason.contains("不是空文件夹"), "{reason}");
    assert_eq!(fs::read(destination.join("notes.txt")).unwrap(), b"the user's");
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    assert_data_where_it_was(&profile);
}

#[test]
fn power_lost_while_copying_the_next_start_takes_the_copy_back() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    assert_eq!(
        Run::default().perform(&profile, Some(Fault::StopBeforeRecords)),
        Outcome::Interrupted
    );
    assert!(destination.join("data").is_dir(), "the half-made copy is there until the next start");
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(profile.default_place().fixed_root.join(JOURNAL_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["state"], serde_json::to_value(State::Copying).unwrap());

    let next = Run::default();
    let Outcome::Failed { reason } = next.resume(&profile) else { panic!("resumed into a move") };
    assert_eq!(reason, "上次搬数据没有做完，数据留在原处");
    assert!(!destination.exists());
    assert!(next.install_copies.borrow().is_empty());
    assert_data_where_it_was(&profile);
    assert_eq!(read_failure(&profile.current().data_root).unwrap().reason, reason);
    assert_eq!(Run::default().resume(&profile), Outcome::NothingAsked);
}

#[test]
fn power_lost_while_writing_the_records_the_next_start_finishes_them() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    let first = Run::default();
    assert_eq!(
        first.perform(&profile, Some(Fault::StopBeforeLocationRecord)),
        Outcome::Interrupted
    );
    assert!(destination.join(MARKER_FILE).is_file());
    assert_eq!(*first.install_copies.borrow(), [Some(destination.clone())]);
    assert_eq!(profile.current().location, None, "no location record yet");

    let next = Run::default();
    assert_eq!(next.resume(&profile), Outcome::Moved { from: None, to: Some(destination.clone()) });
    assert_eq!(*next.install_copies.borrow(), [Some(destination.clone())]);
    assert!(next.stores_checked.borrow().is_empty(), "the copy checked out before the power loss");
    let now = profile.current();
    assert_eq!(now.location.as_deref(), Some(destination.as_path()));
    for (path, bytes) in FILES {
        assert_eq!(fs::read(now.data_root.join(path)).unwrap(), bytes, "{path}");
    }
    assert_eq!(now.pending_removal.len(), 7);
    assert!(!now.fixed_root.join(JOURNAL_FILE).exists());
    assert_eq!(Run::default().resume(&profile), Outcome::NothingAsked);
}

#[test]
fn moving_back_while_the_default_places_still_hold_the_old_copy_is_refused() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    assert!(matches!(Run::default().perform(&profile, None), Outcome::Moved { .. }));

    ask(&profile, None);
    let run = Run::default();
    let Outcome::Failed { reason } = run.perform(&profile, None) else { panic!("moved back") };
    assert!(reason.contains("默认位置还有旧数据"), "{reason}");
    assert!(run.install_copies.borrow().is_empty());
    let now = profile.current();
    assert_eq!(now.location.as_deref(), Some(destination.as_path()));
    for (path, bytes) in FILES {
        assert_eq!(fs::read(now.data_root.join(path)).unwrap(), bytes, "{path}");
    }
    assert_eq!(read_failure(&now.data_root).unwrap().destination, None);

    remove_old_copies(&profile.roots, &profile.namespace, &now).unwrap();
    ask(&profile, None);
    assert!(matches!(Run::default().perform(&profile, None), Outcome::Moved { to: None, .. }));
    assert_eq!(read_failure(&profile.current().data_root), None, "a later move clears it");
}

#[test]
fn a_request_can_be_read_back_and_withdrawn_before_it_starts() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    let current = profile.current();
    assert_eq!(aura_data_move::requested(&profile.namespace, &current).unwrap(), None);
    ask(&profile, Some(&destination));
    assert_eq!(
        aura_data_move::requested(&profile.namespace, &current).unwrap(),
        Some(Some(destination.clone()))
    );
    assert_eq!(
        Run::default().resume(&profile),
        Outcome::NothingAsked,
        "a request waits for perform"
    );
    aura_data_move::withdraw(&profile.namespace, &current).unwrap();
    assert_eq!(aura_data_move::requested(&profile.namespace, &current).unwrap(), None);
    assert_eq!(Run::default().perform(&profile, None), Outcome::NothingAsked);
    assert!(!destination.exists());
}
