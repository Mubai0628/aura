//! A PowerShell terminal that failed to start
//! checks Aura's PowerShell in full again and says so, as the system CMD's does.
use super::super::{TerminalKind, TerminalOptions};
use crate::{checked_program, compat::CompatBackend};
use aura_windows_platform::TerminalSize;
use aura_windows_user_contract::ManagedShell;
use std::path::PathBuf;

#[test]
fn a_powershell_terminal_that_failed_to_start_checks_it_again_and_says_so() {
    let _serial = checked_program::TEST_SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64");
    let backend = CompatBackend::new(&root.join("payload"), &root.join("manifest.json"))
        .expect("verified local managed PS7 and system CMD candidate");
    let directory = tempfile::tempdir().expect("isolated failed launch");
    let options = TerminalOptions {
        shell: ManagedShell::PowerShell,
        kind: TerminalKind::User,
        working_directory: directory.path().to_path_buf(),
        size: TerminalSize { columns: 80, rows: 24 },
        output_limit: 65_536,
    };
    let refused =
        backend.launch_terminal_with_fault(&options, None, crate::compat::LaunchFault::Resume);
    assert!(refused.is_err(), "the fault refuses the launch");
    assert!(
        checked_program::logged_for_tests().iter().any(|(event, fields)| {
            event == "program.checked_after_failure"
                && fields["program"] == backend.powershell.path
                && fields["failure"] == "launch_failed"
        }),
        "a record of the check after the failed launch"
    );
}
