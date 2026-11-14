//! A PowerShell script's errors name its own
//! lines - not a line of Aura's command file - in UTF-8 plain text, and its exit code is unchanged.
use super::support::*;
use aura_windows_user_contract::ManagedShell;

fn stderr(result: &aura_backend_windows_user::compat::DirectResult) -> String {
    String::from_utf8(result.stderr.clone()).expect("stderr is UTF-8")
}

#[test]
fn a_parse_error_names_the_scripts_own_line_and_nothing_runs() {
    let result = run(ManagedShell::PowerShell, "[Console]::WriteLine('ran')\n$a = 1\n$b = 'x`'y'");
    assert_eq!(result.exit_code, Some(1));
    assert!(result.stdout.is_empty(), "nothing ran before the parse error");
    let text = stderr(&result);
    assert!(text.contains("ParserError: line 3, column"), "{text}");
    assert!(text.contains("$b = 'x`'y'"), "the line is shown: {text}");
    assert!(!text.contains("command.ps1") && !text.contains('\u{1b}'), "{text}");
}

#[test]
fn a_runtime_error_names_its_line_in_utf8_plain_text() {
    let result = run(
        ManagedShell::PowerShell,
        "[Console]::WriteLine('one')\nGet-Item -LiteralPath 'C:\\nope\\未找到.txt'",
    );
    assert_eq!(result.exit_code, Some(1));
    let text = stderr(&result);
    assert!(text.contains("   2 |"), "the script's own line: {text}");
    assert!(text.contains("未找到.txt"), "{text}");
    assert!(!text.contains('\u{1b}'), "no colour codes: {text}");
}

/// The exit code is the last command's, as a terminal shows it: a script that ends with a failing
/// program (cargo test, npm, git) reports that program's code, not success.
#[test]
fn the_exit_code_is_the_last_commands() {
    let code = |script: &str| run(ManagedShell::PowerShell, script).exit_code;
    assert_eq!(code("exit 5"), Some(5));
    assert_eq!(code("cmd.exe /c exit 3"), Some(3), "a failing program last");
    assert_eq!(code("if ($true) { cmd.exe /c exit 4 }"), Some(4), "inside a block");
    assert_eq!(code("foreach ($i in 1..2) { cmd.exe /c exit 6 }"), Some(6), "inside a loop");
    assert_eq!(code("cmd.exe /c exit 0"), Some(0));
    assert_eq!(code("[Console]::WriteLine('ok')"), Some(0));
    // A failure the script went on from, or handled, is not the last command's.
    let native = run(ManagedShell::PowerShell, "cmd.exe /c exit 3\n[Console]::WriteLine('after')");
    assert_eq!(native.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&native.stdout).contains("after"));
    assert_eq!(
        code("cmd.exe /c exit 3\nif ($LASTEXITCODE -ne 0) { [Console]::WriteLine('handled') }"),
        Some(0)
    );
    assert_eq!(code("throw 'boom'"), Some(1));
}
