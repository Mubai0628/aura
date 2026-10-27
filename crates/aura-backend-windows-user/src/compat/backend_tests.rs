//! Aura's own PowerShell is read in full the first
//! time, then only compared; a launch that failed checks it in full again and says so.
use super::*;
use std::path::PathBuf;

fn backend() -> CompatBackend {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64");
    CompatBackend::new(&root.join("payload"), &root.join("manifest.json"))
        .expect("verified local managed PS7 and system CMD candidate")
}

fn command(directory: &Path) -> ManagedShellCommand {
    ManagedShellCommand {
        shell: ManagedShell::PowerShell,
        script: "exit 0".into(),
        working_directory: directory.display().to_string(),
        stdin: String::new(),
        timeout_ms: 20_000,
        max_output_bytes: 4096,
        secret_environment: Vec::new(),
    }
}

#[test]
fn aura_powershell_is_read_in_full_once_and_then_only_compared() {
    let _serial = checked_program::TEST_SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let root = tempfile::tempdir().expect("owned compat workspace");
    let before = checked_program::full_checks(&backend.powershell);
    for _ in 0..2 {
        let mut execution = backend.launch(&command(root.path()), None).expect("launch in Job");
        execution.run_direct(&std::sync::atomic::AtomicBool::new(false)).expect("ran");
    }
    assert!(
        checked_program::is_unchanged(&backend.powershell),
        "held as checked, not read in full on every launch"
    );
    assert_eq!(checked_program::full_checks(&backend.powershell), before, "launches read nothing");
}

#[test]
fn a_powershell_launch_that_failed_checks_it_again_and_says_so() {
    let _serial = checked_program::TEST_SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let root = tempfile::tempdir().expect("owned compat workspace");
    assert!(backend.launch_with_fault(&command(root.path()), LaunchFault::Resume).is_err());
    assert!(
        checked_program::logged_for_tests().iter().any(|(event, fields)| {
            event == "program.checked_after_failure"
                && fields["program"] == backend.powershell.path
                && fields["failure"] == "launch_failed"
        }),
        "a record of the check after the failed launch"
    );
}
