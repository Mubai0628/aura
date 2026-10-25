use super::support::*;
use aura_backend_windows_user::compat::{LaunchFault, TerminalEndReason};
use aura_windows_platform::TerminalSize;
use aura_windows_user_contract::ManagedShell;

#[test]
fn launch_failure_and_invalid_dimensions_never_execute_input() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let root = tempfile::tempdir().expect("isolated failed launch");
    for fault in [LaunchFault::Assignment, LaunchFault::Resume] {
        let error = match backend.launch_terminal_with_fault(
            &options(ManagedShell::Cmd, root.path()),
            Some("http://127.0.0.1:9"),
            fault,
        ) {
            Ok(terminal) => {
                terminal.close(TerminalEndReason::UserClosed).expect("reap unexpected launch");
                panic!("fault must refuse launch")
            }
            Err(error) => error,
        };
        assert!(error.to_string().contains("not_started"), "{error}");
    }
    let mut bad = options(ManagedShell::Cmd, root.path());
    bad.size = TerminalSize { columns: 0, rows: 24 };
    assert!(backend.launch_terminal(&bad, Some("http://127.0.0.1:9")).is_err());
    bad.size = TerminalSize { columns: 80, rows: 32_768 };
    assert!(backend.launch_terminal(&bad, Some("http://127.0.0.1:9")).is_err());
}

#[test]
fn deleting_one_session_keeps_the_other_terminal_running() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let registry = aura_backend_windows_user::compat::TerminalRegistry::default();
    let root = tempfile::tempdir().expect("isolated session lifetime");
    let first = aura_domain_common::SessionId::new();
    let second = aura_domain_common::SessionId::new();
    let a = registry
        .open(first, || {
            backend.launch_terminal(
                &options(ManagedShell::Cmd, root.path()),
                Some("http://127.0.0.1:9"),
            )
        })
        .expect("first");
    let b = registry
        .open(second, || {
            backend.launch_terminal(
                &options(ManagedShell::Cmd, root.path()),
                Some("http://127.0.0.1:9"),
            )
        })
        .expect("second");
    let ended = registry.get(first, &a.id).expect("retained record");
    registry.close_session(first).expect("only first session deleted");
    assert!(registry.open(first, || panic!("deleted session must never launch")).is_err());
    assert!(registry.get(first, &a.id).is_err());
    assert_eq!(
        ended.wait_ended(std::time::Duration::ZERO).expect("deleted").reason,
        TerminalEndReason::SessionDeleted
    );
    marker(
        &registry.get(second, &b.id).expect("other stays alive"),
        ManagedShell::Cmd,
        "OTHER-ALIVE",
    );
    registry.shutdown().expect("cleanup");
}
