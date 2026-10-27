//! A console program a script starts with `Start-Process` shares the run's hidden console - what it
//! prints is the command's own output - instead of opening a window of its own; a call that names
//! its own window is left as written.
use super::support::*;
use aura_windows_user_contract::ManagedShell;

fn stdout(result: &aura_backend_windows_user::compat::DirectResult) -> String {
    String::from_utf8_lossy(&result.stdout).into_owned()
}

#[test]
fn a_console_program_started_with_start_process_opens_no_window() {
    let result = run(
        ManagedShell::PowerShell,
        "$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', 'echo shared-console' -PassThru -Wait\nexit $p.ExitCode",
    );
    assert_eq!(result.exit_code, Some(0));
    assert!(stdout(&result).contains("shared-console"), "{}", stdout(&result));
    let alias = run(
        ManagedShell::PowerShell,
        "$p = start cmd.exe -ArgumentList '/c', 'echo by-alias' -PassThru -Wait\nexit $p.ExitCode",
    );
    assert!(stdout(&alias).contains("by-alias"), "{}", stdout(&alias));
}

/// A server started this way keeps running after the script returns, as before; sharing the
/// console must not make the command wait for it.
#[test]
fn a_long_running_program_it_starts_does_not_hold_the_command_open() {
    let root = tempfile::tempdir().expect("owned compat workspace");
    let backend = backend();
    let mut execution = backend
        .launch(
            &command(
                ManagedShell::PowerShell,
                root.path(),
                "$null = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', 'echo server-started & ping -n 30 127.0.0.1 >nul' -PassThru\n'script-returned'",
            ),
            None,
        )
        .expect("launch in Job");
    let started = std::time::Instant::now();
    let direct = execution
        .run_direct(&std::sync::atomic::AtomicBool::new(false))
        .expect("direct result with a running background program");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "the command waited {:?} for the background program",
        started.elapsed()
    );
    assert_eq!(direct.exit_code, Some(0));
    assert!(stdout(&direct).contains("script-returned"), "{}", stdout(&direct));
    assert!(direct.active_processes > 0, "the background program keeps running");
    assert!(execution.stop(std::time::Duration::from_secs(3)).confirmed);
}

#[test]
fn a_call_that_names_its_own_window_runs_as_written() {
    let code = |script: &str| run(ManagedShell::PowerShell, script).exit_code;
    assert_eq!(
        code(
            "$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c exit 4' -WindowStyle Hidden -PassThru -Wait\nexit $p.ExitCode"
        ),
        Some(4)
    );
    assert_eq!(
        code(
            "$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c exit 5' -NoNewWindow -PassThru -Wait\nexit $p.ExitCode"
        ),
        Some(5)
    );
}
