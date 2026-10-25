//! Receipts refuse same-object rewrites and additions; controls really roll back.
use super::*;
use aura_fs_tools::staging_fault::{self, Stage};
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixture() -> Harness {
    Harness::new(|root| {
        std::fs::write(root.join("source.txt"), b"source bytes").expect("source");
        std::fs::write(root.join("old.txt"), b"original bytes").expect("overwrite source");
        std::fs::create_dir_all(root.join("tree")).expect("tree");
        std::fs::write(root.join("tree/member.txt"), b"tree bytes").expect("member");
        std::fs::create_dir_all(root.join("broken")).expect("broken copy input");
        std::fs::write(root.join("broken/a.txt"), b"copied before failure").expect("first member");
        std::fs::create_dir_all(root.join("elsewhere")).expect("junction destination");
        junction::create(root.join("elsewhere"), root.join("broken/z-link"))
            .expect("late copy failure");
    })
}

fn failing_copy(h: &Harness) -> FsChangesetOperation {
    FsChangesetOperation::Copy {
        source: h.target("broken"),
        destination: h.target("broken-copy"),
        source_expected: FsExpectedState::Directory,
        destination_expected: FsExpectedState::Absent,
    }
}

fn run(h: &Harness, first: FsChangesetOperation) -> FsToolError {
    h.fs.apply_changeset(
        FsApplyChangesetRequest {
            workspace_id: h.workspace_id,
            session_id: h.session_a,
            operations: vec![first, failing_copy(h)],
        },
        CorrelationId::new(),
    )
    .expect_err("the second operation really fails")
}

fn rewrite_same_object(path: &Path, bytes: &[u8]) {
    let before = aura_windows_platform::inspect_existing_path(path, false)
        .expect("identity before")
        .identity;
    std::fs::write(path, bytes).expect("external in-place rewrite");
    let after =
        aura_windows_platform::inspect_existing_path(path, false).expect("identity after").identity;
    assert_eq!(before, after, "the counterexample cannot be rejected by identity alone");
}

#[test]
fn copied_file_receipt_preserves_a_same_identity_rewrite() {
    let _serial = staging_interference_lock();
    let h = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let _hook = staging_fault::install(
        Stage::FilePublished,
        Some(std::fs::canonicalize(h.root.path()).expect("canonical workspace").join("copy.txt")),
        Arc::new(move |path| {
            rewrite_same_object(path, b"competitor copy");
            seen.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let error = run(
        &h,
        FsChangesetOperation::Copy {
            source: h.target("source.txt"),
            destination: h.target("copy.txt"),
            source_expected: FsExpectedState::FileHash {
                hash: ContentHash::blake3(b"source bytes"),
            },
            destination_expected: FsExpectedState::Absent,
        },
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1, "real seam missing; operation: {error}");
    assert!(matches!(error, FsToolError::ReconciliationRequired(_)), "{error}");
    assert_eq!(
        std::fs::read(h.root.path().join("copy.txt")).expect("kept competitor bytes"),
        b"competitor copy"
    );
}

#[test]
fn moved_file_receipt_preserves_a_same_identity_rewrite() {
    let _serial = staging_interference_lock();
    let h = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let _hook = staging_fault::install(
        Stage::ObjectMoved,
        Some(std::fs::canonicalize(h.root.path()).expect("canonical workspace").join("moved.txt")),
        Arc::new(move |path| {
            rewrite_same_object(path, b"competitor move");
            seen.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let error = run(
        &h,
        FsChangesetOperation::Move {
            source: h.target("source.txt"),
            destination: h.target("moved.txt"),
            source_expected: FsExpectedState::FileHash {
                hash: ContentHash::blake3(b"source bytes"),
            },
            destination_expected: FsExpectedState::Absent,
        },
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1, "real seam missing; operation: {error}");
    assert!(matches!(error, FsToolError::ReconciliationRequired(_)), "{error}");
    assert!(!h.root.path().join("source.txt").exists(), "rewritten bytes must not move back");
    assert_eq!(
        std::fs::read(h.root.path().join("moved.txt")).expect("kept move bytes"),
        b"competitor move"
    );
}

#[test]
fn moved_directory_receipt_refuses_to_relocate_a_late_member() {
    let _serial = staging_interference_lock();
    let h = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let _hook = staging_fault::install(
        Stage::ObjectMoved,
        Some(std::fs::canonicalize(h.root.path()).expect("canonical workspace").join("moved-tree")),
        Arc::new(move |path| {
            std::fs::write(path.join("theirs.txt"), b"late member").expect("late real member");
            seen.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let error = run(
        &h,
        FsChangesetOperation::Move {
            source: h.target("tree"),
            destination: h.target("moved-tree"),
            source_expected: FsExpectedState::Directory,
            destination_expected: FsExpectedState::Absent,
        },
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1, "real seam missing; operation: {error}");
    assert!(matches!(error, FsToolError::ReconciliationRequired(_)), "{error}");
    assert!(!h.root.path().join("tree").exists());
    assert_eq!(
        std::fs::read(h.root.path().join("moved-tree/theirs.txt")).expect("member kept in place"),
        b"late member"
    );
}

#[test]
fn partial_directory_copy_does_not_claim_a_rewritten_member() {
    let _serial = staging_interference_lock();
    let h = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let _hook = staging_fault::install(
        Stage::FilePublished,
        Some(
            std::fs::canonicalize(h.root.path())
                .expect("canonical workspace")
                .join("broken-copy/a.txt"),
        ),
        Arc::new(move |path| {
            rewrite_same_object(path, b"competitor partial copy");
            seen.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let error =
        h.fs.apply_changeset(
            FsApplyChangesetRequest {
                workspace_id: h.workspace_id,
                session_id: h.session_a,
                operations: vec![failing_copy(&h)],
            },
            CorrelationId::new(),
        )
        .expect_err("reparse point fails the real copy");
    assert_eq!(calls.load(Ordering::Relaxed), 1, "real seam missing; operation: {error}");
    assert!(error.to_string().contains("partial material"), "{error}");
    assert_eq!(
        std::fs::read(h.root.path().join("broken-copy/a.txt")).expect("partial copy kept"),
        b"competitor partial copy"
    );
}

#[test]
fn staged_backup_receipt_refuses_same_identity_rewritten_material() {
    let _serial = staging_interference_lock();
    let h = fixture();
    let root = std::fs::canonicalize(h.root.path()).expect("canonical workspace");
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let _hook = staging_fault::install(
        Stage::FilePublished,
        Some(root.join("old.txt")),
        Arc::new(move |_| {
            let staging = std::fs::read_dir(&root)
                .expect("staging parent")
                .map(|item| item.expect("entry").path())
                .find(|path| {
                    path.file_name()
                        .expect("name")
                        .to_string_lossy()
                        .starts_with(".aura-transaction-")
                })
                .expect("this execution's registered backup");
            rewrite_same_object(&staging.join("00000000"), b"competitor backup");
            seen.fetch_add(1, Ordering::Relaxed);
        }),
    );
    let error = run(
        &h,
        FsChangesetOperation::WriteFile {
            target: h.target("old.txt"),
            content: "our new bytes".into(),
            encoding: None,
            line_ending: None,
            expected: FsExpectedState::FileHash { hash: ContentHash::blake3(b"original bytes") },
        },
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1, "real seam missing; operation: {error}");
    assert!(matches!(error, FsToolError::ReconciliationRequired(_)), "{error}");
    assert!(!h.root.path().join("old.txt").exists(), "untrusted backup is not restored");
    let backups = std::fs::read_dir(h.root.path())
        .expect("staging paths")
        .map(|item| item.expect("entry").path().join("00000000"))
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read(&backups[0]).expect("backup kept"), b"competitor backup");
}

#[test]
fn untouched_copy_move_and_backup_receipts_really_compensate() {
    let _serial = staging_interference_lock();
    for kind in ["copy", "move", "directory_move", "overwrite"] {
        let h = fixture();
        let first = match kind {
            "copy" => FsChangesetOperation::Copy {
                source: h.target("source.txt"),
                destination: h.target("result.txt"),
                source_expected: FsExpectedState::FileHash {
                    hash: ContentHash::blake3(b"source bytes"),
                },
                destination_expected: FsExpectedState::Absent,
            },
            "move" => FsChangesetOperation::Move {
                source: h.target("source.txt"),
                destination: h.target("result.txt"),
                source_expected: FsExpectedState::FileHash {
                    hash: ContentHash::blake3(b"source bytes"),
                },
                destination_expected: FsExpectedState::Absent,
            },
            "directory_move" => FsChangesetOperation::Move {
                source: h.target("tree"),
                destination: h.target("result-tree"),
                source_expected: FsExpectedState::Directory,
                destination_expected: FsExpectedState::Absent,
            },
            _ => FsChangesetOperation::WriteFile {
                target: h.target("old.txt"),
                content: "our new bytes".into(),
                encoding: None,
                line_ending: None,
                expected: FsExpectedState::FileHash {
                    hash: ContentHash::blake3(b"original bytes"),
                },
            },
        };
        let error = run(&h, first);
        assert!(
            matches!(error, FsToolError::ChangesetFailed { compensated_steps: 1, .. }),
            "{kind}: {error}"
        );
        assert!(!h.root.path().join("result.txt").exists());
        assert!(!h.root.path().join("result-tree").exists());
        assert_eq!(
            std::fs::read(h.root.path().join("source.txt")).expect("original source"),
            b"source bytes"
        );
        assert_eq!(
            std::fs::read(h.root.path().join("old.txt")).expect("original overwrite"),
            b"original bytes"
        );
        assert_eq!(
            std::fs::read(h.root.path().join("tree/member.txt")).expect("original tree"),
            b"tree bytes"
        );
    }
}
