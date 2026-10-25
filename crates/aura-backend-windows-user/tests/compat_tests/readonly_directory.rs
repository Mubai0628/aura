use super::support::*;
use aura_windows_user_contract::ManagedShell;
use std::{path::PathBuf, sync::atomic::AtomicBool};

struct RestoreOwnedDacl(PathBuf, String);
impl Drop for RestoreOwnedDacl {
    fn drop(&mut self) {
        aura_windows_platform::restore_file_dacl(&self.0, &self.1)
            .expect("restore only the owned temporary directory DACL");
    }
}

#[test]
fn a_readonly_working_directory_does_not_need_to_hold_the_generated_shell_script() {
    let directory = tempfile::tempdir().expect("owned readonly fixture directory");
    let original = aura_windows_platform::capture_file_dacl(directory.path())
        .expect("capture owned fixture DACL");
    let _restore = RestoreOwnedDacl(directory.path().into(), original);
    // Deny file/subdirectory creation to everyone; retain owner's read and
    // WRITE_DAC so the temporary fixture can always restore its own descriptor.
    aura_windows_platform::restore_file_dacl(
        directory.path(),
        "D:P(D;;0x00000006;;;WD)(A;;FA;;;OW)",
    )
    .expect("make only the owned fixture directory readonly");
    assert!(std::fs::write(directory.path().join("forbidden.txt"), b"fixture").is_err());
    let backend = backend();
    for (shell, script) in [
        (ManagedShell::PowerShell, "[Console]::WriteLine('readonly fixture')"),
        (ManagedShell::Cmd, "echo readonly fixture"),
    ] {
        let mut execution = backend
            .launch(&command(shell, directory.path(), script), None)
            .expect("launch from actual readonly working directory");
        assert!(!execution.scratch_directory().starts_with(directory.path()));
        let result = execution.run_direct(&AtomicBool::new(false)).expect("readonly direct result");
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.active_processes, 0);
        assert!(String::from_utf8_lossy(&result.stdout).contains("readonly fixture"));
    }
}
