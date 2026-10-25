use super::support::*;
use aura_windows_user_contract::ManagedShell;

#[test]
fn ps7_and_cmd_report_the_actual_nonzero_exit_and_output() {
    for (shell, script) in [
        (ManagedShell::PowerShell, "[Console]::WriteLine('compat-result'); exit 7"),
        (ManagedShell::Cmd, "echo compat-result\nexit /b 7"),
    ] {
        let result = run(shell, script);
        assert_eq!(result.exit_code, Some(7));
        assert!(String::from_utf8_lossy(&result.stdout).contains("compat-result"));
        assert_eq!(result.active_processes, 0);
    }
}

#[test]
fn combined_output_is_bounded_and_its_job_is_stopped() {
    let result = run(
        ManagedShell::PowerShell,
        "while ($true) { [Console]::WriteLine('xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'); [Console]::Error.WriteLine('xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx') }",
    );
    assert!(result.output_truncated);
    assert!(result.stdout.len() + result.stderr.len() <= 4096);
    assert_eq!(result.active_processes, 0);
}
