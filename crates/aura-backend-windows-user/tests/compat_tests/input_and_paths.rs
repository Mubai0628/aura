use super::support::*;
use aura_windows_user_contract::ManagedShell;
use std::{sync::atomic::AtomicBool, time::Duration};

#[test]
fn blocked_large_stdin_cannot_block_timeout_or_claim_delivery() {
    let root = tempfile::tempdir().expect("owned blocked-input workspace");
    let backend = backend();
    let mut request = command(ManagedShell::PowerShell, root.path(), "Start-Sleep -Seconds 30");
    request.stdin = "x".repeat(4 * 1024 * 1024);
    request.timeout_ms = 800;
    let mut execution = backend.launch(&request, None).expect("launch blocked stdin fixture");
    let result = execution
        .run_direct(&AtomicBool::new(false))
        .expect("bounded timeout despite blocked writer");
    assert_eq!(result.stop_reason, "timed_out");
    assert!(result.elapsed < Duration::from_secs(5));
    assert!(result.stop_confirmed);
    assert!(result.stdin_completed);
    assert!(result.stdin_error.is_some(), "partial delivery must not be called successful");
}

#[test]
fn unicode_and_long_working_directory_uses_the_verified_directory_for_both_shells() {
    let root = tempfile::tempdir().expect("owned unicode workspace");
    let work = root.path().join("空 格长目录".repeat(25));
    std::fs::create_dir(&work).expect("owned long unicode directory");
    let backend = backend();
    for (shell, script) in [
        (
            ManagedShell::PowerShell,
            "[IO.File]::WriteAllText('path-proof.txt',[Environment]::CurrentDirectory)",
        ),
        (ManagedShell::Cmd, "echo %CD%>path-proof.txt"),
    ] {
        let mut execution = backend
            .launch(&command(shell, &work, script), None)
            .expect("launch verified long directory");
        let result = execution.run_direct(&AtomicBool::new(false)).expect("actual path result");
        assert_eq!(result.exit_code, Some(0), "{:?}", result.stderr);
        let actual = std::fs::read_to_string(work.join("path-proof.txt"))
            .expect("artifact in selected directory");
        assert!(actual.trim().eq_ignore_ascii_case(&work.display().to_string()));
        assert!(execution.stop(Duration::from_secs(1)).confirmed);
    }
}
