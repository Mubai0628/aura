#![allow(clippy::unwrap_used)]
use super::*;
const ACCOUNT: &str = "S-1-5-21-33333-44444-55555-66666";
const WRITE: &str = "S-1-5-21-333333-444444-555555-666666";

#[test]
fn overlapping_leases_restore_in_reverse_without_removing_written_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("workspace");
    std::fs::create_dir(&path).unwrap();
    let (root, _) = file_binding::capture(&path, false).unwrap();
    let acl = || {
        aura_windows_platform::ExecutionFileSecurity::open(&path, false).unwrap().capture().unwrap()
    };
    let before = acl();
    let mut first = file_lease::FileLease::create(&temp.path().join("first.jsonl")).unwrap();
    first
        .grant_tree(&root, ACCOUNT, Some(WRITE), &crate::file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    std::fs::write(path.join("first.txt"), "retained first").unwrap();
    let mut second = file_lease::FileLease::create(&temp.path().join("second.jsonl")).unwrap();
    second.share_roots_from(&first);
    let second_sid = "S-1-5-21-333333-444444-555555-777777";
    second
        .grant_tree(&root, ACCOUNT, Some(second_sid), &crate::file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    std::fs::write(path.join("second.txt"), "retained second").unwrap();
    second.restore_existing().unwrap();
    first.restore_existing().unwrap();
    second.finish_restore().unwrap();
    first.finish_restore().unwrap();
    assert!(aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(&acl(), &before));
    for name in ["first.txt", "second.txt"] {
        let file = path.join(name);
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("retained"));
        let remaining = aura_windows_platform::ExecutionFileSecurity::open(&file, false)
            .unwrap()
            .capture()
            .unwrap();
        assert!(
            !remaining.contains(ACCOUNT)
                && !remaining.contains(WRITE)
                && !remaining.contains(second_sid)
        );
    }
}

#[test]
fn deleted_files_and_renamed_files_do_not_leave_attempt_authority() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    std::fs::write(root_path.join("delete.txt"), "delete").unwrap();
    std::fs::write(root_path.join("move.txt"), "move").unwrap();
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&root, ACCOUNT, Some(WRITE), &crate::file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    std::fs::remove_file(root_path.join("delete.txt")).unwrap();
    std::fs::rename(root_path.join("move.txt"), root_path.join("renamed.txt")).unwrap();
    lease.restore().unwrap();
    assert!(!root_path.join("delete.txt").exists());
    assert!(
        !aura_windows_platform::ExecutionFileSecurity::open(&root_path.join("renamed.txt"), false)
            .unwrap()
            .capture()
            .unwrap()
            .contains(WRITE)
    );
}

#[test]
fn ntfs_volume_identity_is_exact_in_canonical_plan_content() {
    let identity = aura_windows_user_contract::ExecutionFileBinding {
        path: "C:/fixture".into(),
        volume_serial: u64::MAX.to_string(),
        file_id: [255; 16],
        sha256: None,
    };
    let bytes = serde_json::to_vec(&identity).unwrap();
    assert!(aura_domain_common::jcs::canonical_sha256(&identity).is_ok());
    let decoded: aura_windows_user_contract::ExecutionFileBinding =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded.volume_serial, "18446744073709551615");
}

#[test]
fn restore_preserves_content_and_retires_new_file_authority() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    std::fs::write(root_path.join("existing.txt"), "before").unwrap();
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let before = aura_windows_platform::ExecutionFileSecurity::open(&root_path, false)
        .unwrap()
        .capture()
        .unwrap();
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&root, ACCOUNT, Some(WRITE), &crate::file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    std::fs::create_dir(root_path.join("created")).unwrap();
    let output = root_path.join("created/new.txt");
    std::fs::write(&output, "command output").unwrap();
    let running = aura_windows_platform::ExecutionFileSecurity::open(&output, false)
        .unwrap()
        .capture()
        .unwrap();
    assert!(running.contains(WRITE), "new files inherit per-attempt access");
    lease.restore().unwrap();
    assert!(aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(
        &aura_windows_platform::ExecutionFileSecurity::open(&root_path, false)
            .unwrap()
            .capture()
            .unwrap(),
        &before
    ));
    let restored = aura_windows_platform::ExecutionFileSecurity::open(&output, false)
        .unwrap()
        .capture()
        .unwrap();
    assert!(!restored.contains(WRITE) && !restored.contains(ACCOUNT));
    assert_eq!(std::fs::read_to_string(output).unwrap(), "command output");
}

/// Controller-owned output must return to the ACL Windows gives an ordinary
/// file under the original parent. Its creation ACL can contain explicit
/// entries; their flags alone cannot prove retirement added authority.
#[test]
fn retiring_controller_owned_files_never_adds_authority() {
    assert_controller_owned_retirement(false);
}

/// A limited creation ACL distinguishes the old unconditional Controller
/// FullControl grant even on a host where ordinary new files already have it.
#[test]
fn controller_owned_retirement_preserves_limited_creation_authority() {
    assert_controller_owned_retirement(true);
}

fn assert_controller_owned_retirement(limited: bool) {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let capture = |path: &std::path::Path| {
        aura_windows_platform::ExecutionFileSecurity::open(path, false).unwrap().capture().unwrap()
    };
    let (controller, _) = aura_windows_platform::current_user_token_identity().unwrap();
    if limited {
        // Only this test's new directory is changed. The owner retains implicit
        // WRITE_DAC, while inherited data access deliberately excludes FullControl.
        let parent = aura_windows_platform::ExecutionFileSecurity::open(&root_path, false).unwrap();
        let original = parent.capture().unwrap();
        parent.apply_exact(&original, &format!("D:P(A;OICI;0x1301bf;;;{controller})")).unwrap();
    }
    let parent_before = capture(&root_path);
    let clean = root_path.join("clean-before.txt");
    std::fs::write(&clean, "untouched creation control").unwrap();
    let clean_before = capture(&clean);
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&root, ACCOUNT, Some(WRITE), &crate::file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    let output = root_path.join("new.txt");
    std::fs::write(&output, "command output").unwrap();
    let running = aura_windows_platform::ExecutionFileSecurity::open(&output, false)
        .unwrap()
        .capture()
        .unwrap();
    assert!(running.contains(WRITE), "new files inherit per-attempt access");
    let parent_running = capture(&root_path);
    lease.restore_existing().unwrap();
    let parent_restored = capture(&root_path);
    let before_retirement = capture(&output);
    lease.finish_restore().unwrap();
    let restored = aura_windows_platform::ExecutionFileSecurity::open(&output, false)
        .unwrap()
        .capture()
        .unwrap();
    let clean_after = root_path.join("clean-after.txt");
    std::fs::write(&clean_after, "untouched creation control").unwrap();
    let clean_after = capture(&clean_after);
    eprintln!(
        "controller_owned_retirement {}",
        serde_json::json!({
            "limited": limited,
            "parent_before": parent_before,
            "parent_running": parent_running,
            "parent_restored": parent_restored,
            "clean_before": clean_before,
            "clean_after": clean_after,
            "output_running": running,
            "before_retirement": before_retirement,
            "output_restored": restored,
        })
    );
    assert!(
        aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(
            &parent_before,
            &parent_restored
        ),
        "the original parent ACL must be restored"
    );
    assert!(
        aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(&clean_before, &clean_after),
        "untouched creation controls must agree: before={clean_before}; after={clean_after}"
    );
    assert!(!restored.contains(WRITE) && !restored.contains(ACCOUNT));
    assert!(
        aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(&restored, &clean_before),
        "retirement changed creation authority: expected={clean_before}; actual={restored}"
    );
    if limited {
        assert!(!clean_before.contains(";FA;"), "limited control must distinguish FullControl");
        assert!(
            !aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(
                &format!("{clean_before}(A;;FA;;;{controller})"),
                &clean_before
            ),
            "the comparator must reject the old unconditional Controller grant"
        );
    }
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "command output");
}

#[test]
fn interrupted_lease_is_recovered_without_replaying_command() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("workspace");
    std::fs::create_dir(&root_path).unwrap();
    let (root, _) = file_binding::capture(&root_path, false).unwrap();
    let before = aura_windows_platform::ExecutionFileSecurity::open(&root_path, false)
        .unwrap()
        .capture()
        .unwrap();
    let journal = temp.path().join("journal.jsonl");
    let mut lease = file_lease::FileLease::create(&journal).unwrap();
    lease
        .grant_tree(&root, ACCOUNT, Some(WRITE), &crate::file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    std::fs::write(root_path.join("output.txt"), "already ran").unwrap();
    lease.defer_recovery();
    drop(lease);
    file_lease::recover(&journal).unwrap();
    file_lease::recover(&journal).unwrap();
    assert!(aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(
        &aura_windows_platform::ExecutionFileSecurity::open(&root_path, false)
            .unwrap()
            .capture()
            .unwrap(),
        &before
    ));
    assert!(
        !aura_windows_platform::ExecutionFileSecurity::open(&root_path.join("output.txt"), false)
            .unwrap()
            .capture()
            .unwrap()
            .contains(WRITE)
    );
    assert_eq!(std::fs::read_to_string(root_path.join("output.txt")).unwrap(), "already ran");
}

#[test]
fn ephemeral_scratch_hardlinks_do_not_block_restore() {
    let temp = tempfile::tempdir().unwrap();
    let scratch_parent = temp.path().join("execution-scratch");
    std::fs::create_dir(&scratch_parent).unwrap();
    let scratch = scratch_parent.join("0123456789abcdef0123456789abcdef");
    std::fs::create_dir(&scratch).unwrap();
    let (root, _) = file_binding::capture(&scratch, false).unwrap();
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&root, ACCOUNT, Some(WRITE), &crate::file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    let created = scratch.join("command.ps1");
    std::fs::write(&created, "script").unwrap();
    std::fs::hard_link(&created, scratch.join("alias.ps1")).unwrap();
    lease.restore().unwrap();
    assert!(created.exists());
}

/// The four production-fact stops from the independent review, as durable
/// in-repo regressions: a stopped pass reports its partial work, refuses
/// `finish()`, and still restores every touched ACL.
mod stopped_pass_regressions {
    use super::*;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    const FILES: usize = 512;

    fn signal(flag: &Arc<AtomicBool>) -> crate::file_lease::GrantSignal {
        let flag = flag.clone();
        crate::file_lease::GrantSignal::new(move || flag.load(Ordering::Acquire))
    }

    fn tree(
        temp: &tempfile::TempDir,
    ) -> (std::path::PathBuf, aura_windows_user_contract::ExecutionFileBinding) {
        let root_path = temp.path().join("workspace");
        std::fs::create_dir(&root_path).unwrap();
        for n in 0..FILES {
            std::fs::write(root_path.join(format!("{n:04}.txt")), b"untouched").unwrap();
        }
        let (root, _) = file_binding::capture(&root_path, false).unwrap();
        (root_path, root)
    }

    fn assert_no_grant_remains(root_path: &std::path::Path) {
        let root_acl = aura_windows_platform::ExecutionFileSecurity::open(root_path, false)
            .unwrap()
            .capture()
            .unwrap();
        assert!(!root_acl.contains(ACCOUNT) && !root_acl.contains(WRITE), "{root_acl}");
        for n in 0..FILES {
            let acl = aura_windows_platform::ExecutionFileSecurity::open(
                &root_path.join(format!("{n:04}.txt")),
                false,
            )
            .unwrap()
            .capture()
            .unwrap();
            assert!(!acl.contains(ACCOUNT) && !acl.contains(WRITE), "{acl}");
        }
    }

    #[test]
    fn already_cancelled_pass_is_a_stop_never_a_success() {
        let temp = tempfile::tempdir().unwrap();
        let (root_path, root) = tree(&temp);
        let mut lease = file_lease::FileLease::create(&temp.path().join("lease.jsonl")).unwrap();
        let cancelled = Arc::new(AtomicBool::new(true));
        let control = crate::file_lease::GrantControl::with_budget(
            Duration::from_secs(60),
            signal(&cancelled),
        );
        let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
        assert_eq!(outcome.work.objects_scanned, 0);
        assert_eq!(outcome.work.acl_rewrites, 0);
        assert_eq!(outcome.work.stopped, Some(crate::file_lease::GrantStop::Cancelled));
        assert_eq!(
            outcome.verdict,
            crate::file_lease::GrantVerdict::Stopped(crate::file_lease::GrantStop::Cancelled)
        );
        let error = outcome.finish().expect_err("a stopped pass must not finish as success");
        assert_eq!(format!("{error:#}"), "runner.grant_stopped.cancelled");
        lease.restore().unwrap();
        assert_no_grant_remains(&root_path);
    }

    #[test]
    fn expired_budget_before_the_walk_is_a_stop_never_a_success() {
        let temp = tempfile::tempdir().unwrap();
        let (root_path, root) = tree(&temp);
        let mut lease = file_lease::FileLease::create(&temp.path().join("lease.jsonl")).unwrap();
        let control = crate::file_lease::GrantControl::with_budget(
            Duration::ZERO,
            crate::file_lease::GrantSignal::never(),
        );
        let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
        assert_eq!(outcome.work.objects_scanned, 0);
        assert_eq!(outcome.work.stopped, Some(crate::file_lease::GrantStop::BudgetExhausted));
        let error = outcome.finish().expect_err("an expired pass must not finish as success");
        assert_eq!(format!("{error:#}"), "runner.grant_stopped.budget_exhausted");
        lease.restore().unwrap();
        assert_no_grant_remains(&root_path);
    }

    #[test]
    fn deadline_during_the_walk_stops_with_partial_work() {
        let temp = tempfile::tempdir().unwrap();
        let (root_path, root) = tree(&temp);
        let mut lease = file_lease::FileLease::create(&temp.path().join("lease.jsonl")).unwrap();
        // Ten milliseconds cannot cover five hundred journaled captures, so the
        // budget expires mid-walk on any hardware.
        let control = crate::file_lease::GrantControl::with_budget(
            Duration::from_millis(10),
            crate::file_lease::GrantSignal::never(),
        );
        let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
        assert_eq!(outcome.work.stopped, Some(crate::file_lease::GrantStop::BudgetExhausted));
        assert!(
            outcome.work.objects_scanned < (FILES + 1) as u64,
            "the walk stopped instead of scanning everything: {}",
            outcome.work.objects_scanned
        );
        assert_eq!(outcome.work.acl_rewrites, 0, "a walk-phase stop applies nothing");
        assert!(outcome.finish().is_err());
        // The journaled halt proves no Execute followed this scope, so the
        // lease restores its intents and settles in-process wherever the walk
        // stopped.
        lease.restore().unwrap();
        assert_no_grant_remains(&root_path);
    }

    #[test]
    fn cancel_raised_during_the_walk_stops_at_the_next_object() {
        let temp = tempfile::tempdir().unwrap();
        let (root_path, root) = tree(&temp);
        let journal = temp.path().join("lease.jsonl");
        let mut lease = file_lease::FileLease::create(&journal).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let control = crate::file_lease::GrantControl::with_budget(
            Duration::from_secs(60),
            signal(&cancelled),
        );
        // The same observation the independent probe used: cancel once four
        // intents are durable, then watch the pass stop instead of rewriting
        // the remaining five hundred objects.
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
        assert!(
            outcome.work.objects_scanned < (FILES + 1) as u64,
            "the live cancel was visible between objects: {}",
            outcome.work.objects_scanned
        );
        assert_eq!(outcome.work.acl_rewrites, 0, "a walk-phase stop applies nothing");
        assert_eq!(outcome.work.stopped, Some(crate::file_lease::GrantStop::Cancelled));
        assert!(outcome.finish().is_err());
        // The journaled halt proves no Execute followed this scope, so the
        // lease restores its intents and settles in-process.
        lease.restore().unwrap();
        assert_no_grant_remains(&root_path);
        assert!(
            std::fs::read_to_string(&journal).unwrap().contains("\"entry\":\"released\""),
            "a halted scope has no output and settles"
        );
    }

    #[test]
    fn cancel_raised_during_apply_stops_between_rewrites() {
        let temp = tempfile::tempdir().unwrap();
        let (root_path, root) = tree(&temp);
        let mut lease = file_lease::FileLease::create(&temp.path().join("lease.jsonl")).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let control = crate::file_lease::GrantControl::with_budget(
            Duration::from_secs(60),
            signal(&cancelled),
        );
        // The workspace root is the first object the apply phase rewrites; the
        // moment its ACL carries the grant, cancel, and the batch must stop
        // between objects instead of finishing the tree.
        let stop = cancelled.clone();
        let watched_root = root_path.clone();
        let watcher = std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(20);
            while Instant::now() < until {
                let acl = aura_windows_platform::ExecutionFileSecurity::open(&watched_root, false)
                    .unwrap()
                    .capture()
                    .unwrap();
                if acl.contains(ACCOUNT) {
                    stop.store(true, Ordering::Release);
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            panic!("the apply phase never rewrote the workspace root");
        });
        let outcome = lease.grant_tree(&root, ACCOUNT, Some(WRITE), &control);
        watcher.join().unwrap();
        assert_eq!(
            outcome.work.objects_scanned,
            (FILES + 1) as u64,
            "the walk completed its captures before the apply phase"
        );
        assert!(
            outcome.work.acl_rewrites >= 1 && outcome.work.acl_rewrites < (FILES + 1) as u64,
            "each rewrite before the stop is counted: {}",
            outcome.work.acl_rewrites
        );
        assert_eq!(outcome.work.stopped, Some(crate::file_lease::GrantStop::Cancelled));
        assert!(outcome.finish().is_err());
        lease.restore().unwrap();
        assert_no_grant_remains(&root_path);
    }
}
