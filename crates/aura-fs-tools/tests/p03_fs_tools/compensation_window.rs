//! Interference after the empty-directory check and immediately before actual deletion.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

fn failed_changeset(
    harness: &Harness,
) -> Result<aura_workspace_domain::FsApplyChangesetResult, FsToolError> {
    harness.fs.apply_changeset(
        FsApplyChangesetRequest {
            workspace_id: harness.workspace_id,
            session_id: harness.session_a,
            operations: vec![
                FsChangesetOperation::CreateDirectory { target: harness.target("mine") },
                FsChangesetOperation::Copy {
                    source: harness.target("source-dir"),
                    destination: harness.target("copied-dir"),
                    source_expected: FsExpectedState::Directory,
                    destination_expected: FsExpectedState::Absent,
                },
            ],
        },
        CorrelationId::new(),
    )
}

fn harness() -> Harness {
    Harness::new(|root| {
        std::fs::create_dir_all(root.join("source-dir")).expect("the copy source directory");
        std::fs::write(root.join("source-dir/normal.txt"), "normal\n").expect("the source file");
        std::fs::create_dir_all(root.join("real-dir")).expect("the junction destination");
        junction::create(root.join("real-dir"), root.join("source-dir/link"))
            .expect("the reparse point that fails the copy");
    })
}

#[test]
fn compensation_directory_handle_blocks_replacement_at_the_last_delete_window() {
    let _serial = staging_interference_lock();
    let harness = harness();
    let reached = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    let reached_in = Arc::clone(&reached);
    let error_in = Arc::clone(&error);
    let _interference = aura_fs_tools::staging_fault::install(
        aura_fs_tools::staging_fault::Stage::BeforeCompensationDelete,
        None,
        Arc::new(move |path| {
            if path.file_name().is_some_and(|name| name == "mine") {
                reached_in.store(true, Ordering::Release);
                *error_in.lock().expect("record the actual Windows refusal") = Some(
                    std::fs::rename(path, path.with_file_name("moved"))
                        .expect_err("the verified directory is still held against replacement")
                        .raw_os_error(),
                );
            }
        }),
    );
    let result = failed_changeset(&harness);
    assert!(result.is_err(), "the original copy failure is still reported");
    assert!(reached.load(Ordering::Acquire), "real compensation reached its last deletion window");
    assert_eq!(
        *error.lock().expect("read the actual refusal"),
        Some(Some(32)),
        "Windows refused rename of the held directory"
    );
    assert!(
        !harness.root.path().join("mine").exists(),
        "this execution's own empty directory was removed"
    );
    assert!(!harness.root.path().join("moved").exists());
    assert_eq!(
        std::fs::read_to_string(harness.root.path().join("source-dir/normal.txt"))
            .expect("the unchanged source"),
        "normal\n"
    );
}

#[test]
fn compensation_preserves_a_member_added_after_its_empty_check() {
    let _serial = staging_interference_lock();
    let harness = harness();
    let reached = Arc::new(AtomicBool::new(false));
    let reached_in = Arc::clone(&reached);
    let _interference = aura_fs_tools::staging_fault::install(
        aura_fs_tools::staging_fault::Stage::BeforeCompensationDelete,
        None,
        Arc::new(move |path| {
            if path.file_name().is_some_and(|name| name == "mine") {
                std::fs::write(path.join("theirs.txt"), b"third party")
                    .expect("a third party really adds a member in the last window");
                reached_in.store(true, Ordering::Release);
            }
        }),
    );
    let result = failed_changeset(&harness);
    assert!(reached.load(Ordering::Acquire));
    assert!(
        matches!(result, Err(FsToolError::ReconciliationRequired(_))),
        "the new member requires reconciliation: {result:?}"
    );
    assert_eq!(
        std::fs::read(harness.root.path().join("mine/theirs.txt"))
            .expect("the competitor's content survived"),
        b"third party"
    );
}
