use super::support::*;
use aura_backend_windows_user::compat::{TerminalEndReason, TerminalRegistry};
use aura_windows_user_contract::ManagedShell;

#[test]
fn user_label_close_releases_registration_and_output_owner() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let directory = tempfile::tempdir().expect("isolated terminal");
    let registry = TerminalRegistry::default();
    let session = aura_domain_common::SessionId::new();
    let entry = registry
        .open(session, || {
            backend.launch_terminal(
                &options(ManagedShell::Cmd, directory.path()),
                Some("http://127.0.0.1:9"),
            )
        })
        .expect("terminal");
    let terminal = registry.get(session, &entry.id).expect("record");
    let weak = std::sync::Arc::downgrade(&terminal);
    marker(&terminal, ManagedShell::Cmd, "CLOSE-RELEASE");
    let process =
        aura_windows_platform::retain_process(terminal.identity().process_id).expect("root");
    drop(terminal);
    assert!(
        registry
            .close_and_remove(session, &entry.id, TerminalEndReason::UserClosed)
            .expect("close")
            .stop_confirmed
    );
    assert!(registry.list(session).expect("list").is_empty());
    assert!(registry.get(session, &entry.id).is_err());
    assert!(weak.upgrade().is_none(), "buffer and monitor owner released");
    assert!(!process.is_running().expect("root exited"));
}
