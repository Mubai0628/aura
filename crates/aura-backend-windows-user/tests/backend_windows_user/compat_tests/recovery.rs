use super::support::*;
use aura_windows_platform::RootJob;
use aura_windows_user_contract::ManagedShell;
use std::{sync::atomic::AtomicBool, time::Duration};

#[test]
fn only_the_recorded_owner_job_is_reopened_and_stopped_without_replaying_the_command() {
    let root = tempfile::tempdir().expect("owned recovery workspace");
    let backend = backend();
    let mut execution = backend
        .launch(
            &command(
                ManagedShell::Cmd,
                root.path(),
                &fixture_script(ManagedShell::Cmd, "tree", root.path()),
            ),
            None,
        )
        .expect("owned job");
    wait_file(&root.path().join("tree.ready"));
    std::fs::write(root.path().join("release"), b"release").expect("release direct root");
    let result = execution.run_direct(&AtomicBool::new(false)).expect("direct terminal");
    assert!(result.active_processes > 0);
    let receipt = execution.receipt();
    let recovery = RootJob::open_for_recovery(&receipt.installation, &receipt.nonce)
        .expect("owner-verified reopen")
        .expect("same live Job");
    assert!(RootJob::open_for_recovery("not-an-installation", &receipt.nonce).is_err());
    recovery.stop_for_recovery().expect("confirm job empty");
    assert!(execution.stop(Duration::from_secs(1)).confirmed);
    assert_eq!(
        std::fs::read_to_string(root.path().join("starts")).expect("count actual launches"),
        "1"
    );
}
