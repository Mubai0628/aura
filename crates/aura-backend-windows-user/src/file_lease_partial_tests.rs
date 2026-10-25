//! Regressions for partial preparation and failed passes. A pass that stopped
//! before completing its enumeration journals that halt: the launch dispatches
//! only after every grant completes, so a halted scope has no output and its
//! journal settles once intents are restored. A legacy journal without the
//! halt fact proves nothing about the objects it did not capture: pre-existing
//! files keep their original ACLs, including an account grant an earlier
//! attempt could have left, and the journal stays for reconciliation. A failed
//! apply still reports the rewrites it already did.
#![allow(clippy::unwrap_used)]
use super::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const ACCOUNT: &str = "S-1-5-21-33333-44444-55555-66666";
const WRITE: &str = "S-1-5-21-333333-444444-555555-666666";

fn acl(path: &Path) -> String {
    aura_windows_platform::ExecutionFileSecurity::open(path, false).unwrap().capture().unwrap()
}

fn signal(flag: &Arc<AtomicBool>) -> crate::file_lease::GrantSignal {
    let flag = flag.clone();
    crate::file_lease::GrantSignal::new(move || flag.load(Ordering::Acquire))
}

/// A file that already carries a grant for the attempt account, as anything an
/// earlier attempt left behind could: the grant is part of its original state,
/// never of the lease under test. Returns the path and the original SDDL.
fn pre_existing_file_with_account_grant(root: &Path) -> (PathBuf, String) {
    let path = root.join("pre-existing.txt");
    std::fs::write(&path, "pre-existing content").unwrap();
    let object = aura_windows_platform::ExecutionFileSecurity::open(&path, false).unwrap();
    let before = object.capture().unwrap();
    let planned = object.plan(&[(ACCOUNT, 0x1200a9)]).unwrap();
    object.apply_exact(&before, &planned).unwrap();
    let original = object.capture().unwrap();
    assert!(original.contains(ACCOUNT), "fixture carries the earlier grant: {original}");
    (path, original)
}

fn journal_has_released(journal: &Path) -> bool {
    std::fs::read_to_string(journal).unwrap().contains("\"entry\":\"released\"")
}

/// The v2 counterexample: a pre-existing account grant on a file this pass
/// never captured must survive a preparation that was cancelled before its
/// walk, and the lease still settles honestly.
#[test]
fn cancelled_before_walk_preserves_preexisting_account_grant() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("lease.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    let cancelled = Arc::new(AtomicBool::new(true));
    let control =
        crate::file_lease::GrantControl::with_budget(Duration::from_secs(60), signal(&cancelled));
    let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
    assert_eq!(outcome.work.objects_scanned, 0);
    assert!(outcome.finish().is_err());
    lease.restore().unwrap();
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "pre-existing content");
    assert!(journal_has_released(&journal), "nothing was touched, so the lease settles");
}

#[test]
fn expired_budget_before_walk_preserves_preexisting_account_grant() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("lease.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    let control = crate::file_lease::GrantControl::with_budget(
        Duration::ZERO,
        crate::file_lease::GrantSignal::never(),
    );
    let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
    assert_eq!(outcome.work.objects_scanned, 0);
    assert!(outcome.finish().is_err());
    lease.restore().unwrap();
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    assert!(journal_has_released(&journal));
}

#[test]
fn cancel_during_enumeration_preserves_preexisting_account_grant() {
    const FILES: usize = 64;
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    for n in 0..FILES {
        std::fs::write(root_path.join(format!("{n:04}.txt")), b"untouched").unwrap();
    }
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("lease.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let control =
        crate::file_lease::GrantControl::with_budget(Duration::from_secs(60), signal(&cancelled));
    let watched = journal.clone();
    let stop = cancelled.clone();
    let watcher = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            if let Ok(text) = std::fs::read_to_string(&watched)
                && text.matches("\"entry\":\"intent\"").count() >= 4
            {
                stop.store(true, Ordering::Release);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the walk never journaled four intents");
    });
    let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
    watcher.join().unwrap();
    assert_eq!(outcome.work.stopped, Some(crate::file_lease::GrantStop::Cancelled));
    assert_eq!(outcome.work.acl_rewrites, 0, "a walk-phase stop applies nothing");
    assert!(outcome.finish().is_err());
    // The journaled halt proves no Execute followed this scope, so the lease
    // restores its intents and settles instead of staying for reconciliation.
    lease.restore().unwrap();
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    for n in 0..FILES {
        let current = acl(&root_path.join(format!("{n:04}.txt")));
        assert!(!current.contains(ACCOUNT) && !current.contains(WRITE), "{current}");
    }
    assert!(journal_has_released(&journal), "a halted scope has no output and settles");
}

/// A failed apply must report the rewrites that already succeeded. The fault
/// is a foreign DACL change on one child, injected once the walk journaled
/// every intent; afterwards the fault is undone and the production restore
/// returns every object — including the pre-existing grant — to its original.
#[test]
fn apply_failure_reports_rewrites_already_applied() {
    const FILES: usize = 512;
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    for n in 0..FILES {
        std::fs::write(root_path.join(format!("{n:04}.txt")), b"untouched").unwrap();
    }
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let mut originals = BTreeMap::new();
    originals.insert(root_path.clone(), acl(&root_path));
    for n in 0..FILES {
        let path = root_path.join(format!("{n:04}.txt"));
        originals.insert(path.clone(), acl(&path));
    }
    originals.insert(file.clone(), original.clone());
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("lease.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    let control = crate::file_lease::GrantControl::unbounded();
    // Reverse enumeration applies the root first and `0000.txt` last: once the
    // root carries the grant, flip that child's DACL so its apply refuses.
    let target = root_path.join("0000.txt");
    let flipped = {
        let object = aura_windows_platform::ExecutionFileSecurity::open(&target, false).unwrap();
        object.plan(&[("S-1-5-21-333333-444444-555555-777777", 0x1200a9)]).unwrap()
    };
    let watched_root = root_path.clone();
    let watched_target = target.clone();
    let watched_flipped = flipped.clone();
    let watcher = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            let current = acl(&watched_root);
            if current.contains(ACCOUNT) {
                let object =
                    aura_windows_platform::ExecutionFileSecurity::open(&watched_target, false)
                        .unwrap();
                let before = object.capture().unwrap();
                object.apply_exact(&before, &watched_flipped).unwrap();
                return before;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the apply phase never rewrote the workspace root");
    });
    let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
    let target_before_flip = watcher.join().unwrap();
    let work = outcome.work;
    assert_eq!(outcome.verdict, crate::file_lease::GrantVerdict::Failed);
    assert!(outcome.finish().is_err());
    assert_eq!(work.objects_scanned, (FILES + 2) as u64, "root, numbered files and the fixture");
    assert_eq!(
        work.journal_records,
        (FILES + 5) as u64,
        "one scope record, every intent, the finished enumeration fact and the halt record are durable"
    );
    let durable_records = std::fs::read_to_string(&journal).unwrap().lines().count();
    assert_eq!(
        work.journal_records as usize, durable_records,
        "the reported count is the durable journal records themselves"
    );
    let changed = originals
        .iter()
        .filter(|(path, before)| acl(path) != **before)
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    assert!(changed.contains(&root_path), "the root applied before the failure");
    assert_eq!(
        work.acl_rewrites as usize,
        changed.len() - 1,
        "every rewrite before the failed object is counted; the flipped child is not a rewrite"
    );
    assert!(work.acl_rewrites >= 1);
    // Undo the injected change; the production restore then settles everything.
    let object = aura_windows_platform::ExecutionFileSecurity::open(&target, false).unwrap();
    object.apply_exact(&flipped, &target_before_flip).unwrap();
    lease.restore().unwrap();
    for (path, before) in &originals {
        let current = acl(path);
        assert!(
            aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(&current, before),
            "restored: {path:?} now {current}"
        );
    }
    assert!(journal_has_released(&journal));
}

/// A journal whose intents exist but that carries no enumeration fact — a
/// legacy journal, or one that died inside its first walk — cannot prove its
/// remaining objects are attempt output. Recovery restores the intents but
/// refuses to publish a cleanup receipt: the journal stays for reconciliation.
#[test]
fn journal_without_enumeration_proof_is_restored_but_not_released() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let info = aura_windows_platform::inspect_existing_path(&file, false).unwrap();
    let journal = temp.path().join("legacy.jsonl");
    let scope = serde_json::json!({
        "entry": "scope",
        "root": root,
        "account": ACCOUNT,
        "write_sid": WRITE,
    });
    let intent = serde_json::json!({
        "entry": "intent",
        "path": file,
        "volume": info.identity.volume_serial_number,
        "id": info.identity.file_id,
        "before": original,
        "after": original,
    });
    std::fs::write(&journal, format!("{scope}\n{intent}\n")).unwrap();
    let error = file_lease::recover(&journal).expect_err("an unproven journal is not released");
    assert_eq!(format!("{error:#}"), "runner.lease_enumeration_unproven");
    assert!(!journal_has_released(&journal));
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
}

/// A scope record without a single intent proves the walk never touched
/// anything, whatever version wrote the journal: recovery settles and releases
/// it without retiring any object.
#[test]
fn journal_with_scope_but_no_intent_recovers_and_releases() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("cancelled.jsonl");
    let scope = serde_json::json!({
        "entry": "scope",
        "root": root,
        "account": ACCOUNT,
        "write_sid": WRITE,
    });
    std::fs::write(&journal, format!("{scope}\n")).unwrap();
    file_lease::recover(&journal).unwrap();
    assert!(journal_has_released(&journal));
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "pre-existing content");
}

/// The R1 counterexample: a file the walk refused to touch because of its
/// extra hard link is seen, not absent. Deleting the second link afterwards
/// leaves the same file identity, and the restore must not reclassify it as
/// attempt output — its original account grant survives and the lease settles.
#[test]
fn skipped_hard_link_keeps_preexisting_grant_after_link_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let second = temp.path().join("second-link.txt");
    std::fs::hard_link(&file, &second).unwrap();
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("lease.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    let outcome = lease.grant_tree(
        &root,
        ACCOUNT,
        Some(WRITE),
        &crate::file_lease::GrantControl::unbounded(),
    );
    assert_eq!(outcome.verdict, crate::file_lease::GrantVerdict::Complete);
    assert_eq!(outcome.work.objects_skipped, 1, "the linked file is seen but untouched");
    outcome.finish().unwrap();
    // The probe: the second link goes away after the walk, leaving the same
    // identity behind a single name.
    std::fs::remove_file(&second).unwrap();
    lease.restore().unwrap();
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "pre-existing content");
    assert!(journal_has_released(&journal));
}

/// The same seen proof holds while the extra link stays: the skipped object
/// is never visited by the new-output retirement at all.
#[test]
fn skipped_hard_link_is_never_retired_while_links_remain() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let second = temp.path().join("second-link.txt");
    std::fs::hard_link(&file, &second).unwrap();
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("lease.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    let outcome = lease.grant_tree(
        &root,
        ACCOUNT,
        Some(WRITE),
        &crate::file_lease::GrantControl::unbounded(),
    );
    assert_eq!(outcome.verdict, crate::file_lease::GrantVerdict::Complete);
    assert_eq!(outcome.work.objects_skipped, 1);
    outcome.finish().unwrap();
    lease.restore().unwrap();
    assert_eq!(
        std::fs::read_to_string(&second).unwrap(),
        "pre-existing content",
        "the second name of the same identity survives"
    );
    // The alias guard keeps the ACL API away from a multi-link name; once the
    // second link is gone the original DACL proves the object was never
    // retired.
    std::fs::remove_file(&second).unwrap();
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    assert!(journal_has_released(&journal));
}

/// The replay half of the seen proof: a lease whose process died after the
/// walk settles through recovery from the durable skipped identity, never
/// through a re-scan that could misread the link-count change.
#[test]
fn recovery_of_skipped_hard_link_keeps_preexisting_grant() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let second = temp.path().join("second-link.txt");
    std::fs::hard_link(&file, &second).unwrap();
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let journal = temp.path().join("lease.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    let outcome = lease.grant_tree(
        &root,
        ACCOUNT,
        Some(WRITE),
        &crate::file_lease::GrantControl::unbounded(),
    );
    assert_eq!(outcome.verdict, crate::file_lease::GrantVerdict::Complete);
    outcome.finish().unwrap();
    std::fs::remove_file(&second).unwrap();
    // Simulate the attempt deferring to a process boundary: the lease drops
    // without an in-process restore and the journal is all recovery gets.
    lease.defer_recovery();
    drop(lease);
    file_lease::recover(&journal).unwrap();
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "pre-existing content");
    assert!(journal_has_released(&journal));
}

/// A halted scope in a journal proves no Execute followed it: recovery
/// restores the intents and settles instead of holding the journal for a
/// reconciliation that can never produce new evidence.
#[test]
fn journal_with_halted_scope_recovers_and_releases() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let info = aura_windows_platform::inspect_existing_path(&file, false).unwrap();
    let journal = temp.path().join("halted.jsonl");
    let scope = serde_json::json!({
        "entry": "scope",
        "root": root,
        "account": ACCOUNT,
        "write_sid": WRITE,
    });
    let intent = serde_json::json!({
        "entry": "intent",
        "path": file,
        "volume": info.identity.volume_serial_number,
        "id": info.identity.file_id,
        "before": original,
        "after": original,
    });
    let halted = serde_json::json!({
        "entry": "scope_halted",
        "root": root,
        "reason": "cancelled",
    });
    std::fs::write(&journal, format!("{scope}\n{intent}\n{halted}\n")).unwrap();
    file_lease::recover(&journal).unwrap();
    assert!(journal_has_released(&journal));
    assert_eq!(acl(&file), original, "the pre-existing DACL is byte-identical");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "pre-existing content");
}

/// Scratch finished its enumeration while the workspace walk halted: the two
/// durable facts combine and recovery settles the whole journal.
#[test]
fn journal_with_enumerated_and_halted_scopes_recovers_and_releases() {
    let temp = tempfile::tempdir().unwrap();
    let scratch_path = temp.path().join("install/execution-scratch/nonce");
    std::fs::create_dir_all(&scratch_path).unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (scratch_file, scratch_original) = pre_existing_file_with_account_grant(&scratch_path);
    let (file, original) = pre_existing_file_with_account_grant(&root_path);
    let (scratch, _) = file_binding::capture(&scratch_path, false).unwrap();
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let scratch_info = aura_windows_platform::inspect_existing_path(&scratch_file, false).unwrap();
    let info = aura_windows_platform::inspect_existing_path(&file, false).unwrap();
    let journal = temp.path().join("mixed.jsonl");
    let records = [
        serde_json::json!({
            "entry": "scope",
            "root": scratch,
            "account": ACCOUNT,
            "write_sid": WRITE,
        }),
        serde_json::json!({
            "entry": "intent",
            "path": scratch_file,
            "volume": scratch_info.identity.volume_serial_number,
            "id": scratch_info.identity.file_id,
            "before": scratch_original,
            "after": scratch_original,
        }),
        serde_json::json!({ "entry": "scope_enumerated", "root": scratch }),
        serde_json::json!({
            "entry": "scope",
            "root": root,
            "account": ACCOUNT,
            "write_sid": WRITE,
        }),
        serde_json::json!({
            "entry": "intent",
            "path": file,
            "volume": info.identity.volume_serial_number,
            "id": info.identity.file_id,
            "before": original,
            "after": original,
        }),
        serde_json::json!({
            "entry": "scope_halted",
            "root": root,
            "reason": "cancelled",
        }),
    ];
    let text = records.iter().map(|record| format!("{record}\n")).collect::<String>();
    std::fs::write(&journal, text).unwrap();
    file_lease::recover(&journal).unwrap();
    assert!(journal_has_released(&journal));
    assert_eq!(acl(&file), original, "the workspace DACL is byte-identical");
    assert_eq!(acl(&scratch_file), scratch_original, "the scratch DACL is byte-identical");
}
