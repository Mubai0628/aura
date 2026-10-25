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

#[test]
fn exit_codes_are_the_ones_the_script_had_inline() {
    assert_eq!(run(ManagedShell::PowerShell, "exit 5").exit_code, Some(5));
    // As before: a script that ends without `exit` reports success after a failed native command.
    let native = run(ManagedShell::PowerShell, "cmd.exe /c exit 3\n[Console]::WriteLine('after')");
    assert_eq!(native.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&native.stdout).contains("after"));
}
