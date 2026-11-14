//! An Agent terminal and the user's PowerShell history: its lines are not saved to the user's
//! history file, and the lines in that file are not offered in it.
use super::support::*;
use aura_windows_user_contract::ManagedShell;

#[test]
fn an_agent_terminal_neither_saves_to_nor_reads_the_users_powershell_history() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    // The history the user's own terminals saved, where PSReadLine keeps it under APPDATA.
    let saved =
        backend.home().join("Microsoft/Windows/PowerShell/PSReadLine/ConsoleHost_history.txt");
    std::fs::create_dir_all(saved.parent().expect("its folder")).expect("history folder");
    let before: String = (0..40).map(|n| format!("Write-Output user-line-{n}\n")).collect();
    std::fs::write(&saved, &before).expect("the user's history");
    let root = tempfile::tempdir().expect("isolated terminal");
    let terminal = backend
        .launch_terminal(
            &options(ManagedShell::PowerShell, root.path()),
            Some("http://127.0.0.1:9"),
        )
        .expect("real ConPTY");
    marker(&terminal, ManagedShell::PowerShell, "HISTORY-READY");
    // The printed fields are joined in the output only, so the echoed line never matches.
    line(
        &terminal,
        "$o = Get-PSReadLineOption; $n = [Microsoft.PowerShell.PSConsoleReadLine]::GetHistoryItems().Count; \
         [Console]::WriteLine('L02' + '-FIELDS=' + $o.HistorySaveStyle + '|' + $n + '|' + 'L02' + '-END')",
    );
    let text = until(&terminal, "L02-END");
    let start = text.rfind("L02-FIELDS=").expect("the fields") + "L02-FIELDS=".len();
    let fields: Vec<&str> = text[start..].split('|').collect();
    assert!(fields.len() >= 3, "{text}");
    assert_eq!(fields[0].trim(), "SaveNothing", "{text}");
    // Only the lines typed here, the marker's and this one, none of the user's forty.
    let lines: usize = fields[1].trim().parse().expect("a count");
    assert!(lines <= 3, "the user's history was loaded: {lines} lines");
    terminal
        .close(aura_backend_windows_user::compat::TerminalEndReason::UserClosed)
        .expect("close");
    assert_eq!(
        std::fs::read_to_string(&saved).expect("the user's history"),
        before,
        "this terminal's lines reached the user's history file"
    );
}
