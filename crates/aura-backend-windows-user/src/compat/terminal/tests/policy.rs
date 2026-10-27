use super::super::{
    TerminalKind,
    launch::{terminal_arguments, terminal_environment},
};
use aura_windows_user_contract::ManagedShell;

#[test]
fn interactive_shell_policy_keeps_user_profiles_and_filters_only_agent_secrets() {
    let trusted = vec![
        ("AURA_PRIVATE".into(), "temporary-internal-value".into()),
        ("CODEX_PRIVATE".into(), "temporary-internal-value".into()),
        ("TEST_TOKEN".into(), "temporary-test-value".into()),
        ("DEEPSEEK_TEST".into(), "temporary-test-value".into()),
        ("PATH".into(), "C:\\Windows\\System32".into()),
        ("HTTP_PROXY".into(), "http://127.0.0.1:8".into()),
    ];
    let user = terminal_environment(trusted.clone(), TerminalKind::User, None, &[])
        .expect("user environment");
    let agent = terminal_environment(
        trusted.clone(),
        TerminalKind::Agent { requires_network: false },
        Some("http://127.0.0.1:9"),
        &[],
    )
    .expect("agent environment");
    assert!(user.iter().any(|entry| entry.name == "TEST_TOKEN"));
    assert!(user.iter().any(|entry| entry.name == "DEEPSEEK_TEST"));
    assert!(
        !user
            .iter()
            .any(|entry| entry.name.starts_with("AURA_") || entry.name.starts_with("CODEX_"))
    );
    assert!(!agent.iter().any(|entry| entry.name == "TEST_TOKEN" || entry.name == "DEEPSEEK_TEST"));
    assert_eq!(
        String::from_utf16(
            &agent
                .iter()
                .find(|entry| entry.name == "HTTP_PROXY")
                .expect("soft offline")
                .value_utf16
        )
        .expect("UTF16"),
        "http://127.0.0.1:9"
    );
    assert!(
        terminal_environment(trusted, TerminalKind::Agent { requires_network: false }, None, &[])
            .is_err()
    );
    let user = terminal_arguments(ManagedShell::PowerShell, TerminalKind::User);
    let agent = terminal_arguments(
        ManagedShell::PowerShell,
        TerminalKind::Agent { requires_network: false },
    );
    assert!(!user.iter().any(|value| value == "-NoProfile"));
    assert!(!user.iter().any(|value| value == "-MTA"), "user pwsh keeps its normal apartment");
    assert!(agent.iter().any(|value| value == "-NoProfile"));
    assert!(!user.iter().chain(&agent).any(|value| value == "-NonInteractive"));
    assert!(
        terminal_arguments(ManagedShell::Cmd, TerminalKind::User).iter().any(|value| value == "/d")
    );
}
