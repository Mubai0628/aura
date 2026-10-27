//! A model's terminal shows a credential's value as stars of the same width, so the line the
//! console laid out keeps its place.
use super::support::*;
use aura_backend_windows_user::compat::TerminalEndReason;
use aura_windows_user_contract::ManagedShell;

const VALUE: &str = "term-fixture-4417";

#[test]
fn a_model_terminal_shows_a_declared_credential_as_stars_of_its_width() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend_with(&[("ZZ_TERM_FIXTURE_TOKEN", VALUE)]);
    let root = tempfile::tempdir().expect("isolated terminal");
    let mut options = options(ManagedShell::PowerShell, root.path());
    options.secret_environment = vec!["ZZ_TERM_FIXTURE_TOKEN".to_owned()];
    let terminal =
        backend.launch_terminal(&options, Some("http://127.0.0.1:9")).expect("real ConPTY");
    marker(&terminal, ManagedShell::PowerShell, "READY");
    line(&terminal, "[Console]::WriteLine('v=' + $env:ZZ_TERM_FIXTURE_TOKEN + '|' + 'done')");
    let text = until(&terminal, "|done");
    assert!(text.contains(&format!("v={}|done", "*".repeat(VALUE.len()))), "{text}");
    let raw = terminal.read(0, 16 * 1024 * 1024).expect("raw VT").bytes;
    assert!(!String::from_utf8_lossy(&raw).contains(VALUE), "the value never reaches the screen");
    terminal.close(TerminalEndReason::AgentClosed).expect("close");
}
