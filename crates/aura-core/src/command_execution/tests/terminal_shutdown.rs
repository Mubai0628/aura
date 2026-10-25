//! Actual Core owner teardown closes terminal trees even when a consumer retains their records.
use super::*;
use crate::StartupMode;
use aura_backend_windows_user::compat::{TerminalEndReason, TerminalKind, TerminalOptions};
use aura_windows_platform::TerminalSize;

#[tokio::test]
async fn l02_b1_core_owner_teardown_reaps_all_terminal_jobs() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let backend = Arc::clone(&services.commands_required().compat);
    let session = SessionId::new();
    let mut terminals = Vec::new();
    for shell in [ManagedShell::PowerShell, ManagedShell::Cmd] {
        let entry = backend
            .open_terminal(
                session,
                TerminalOptions {
                    shell,
                    kind: TerminalKind::Agent { requires_network: false },
                    working_directory: host.root.path().to_path_buf(),
                    size: TerminalSize { columns: 80, rows: 24 },
                    output_limit: 1024 * 1024,
                },
            )
            .await
            .expect("actual terminal");
        let terminal = backend.terminals.get(session, &entry.id).expect("Core-owned record");
        let root = aura_windows_platform::retain_process(terminal.identity().process_id)
            .expect("retained root identity");
        terminals.push((terminal, root));
    }
    // Production RuntimeServices::drop calls CommandExecution::shutdown. No synthetic stop hook.
    drop(runtime);
    for (terminal, root) in terminals {
        let end = terminal
            .wait_ended(std::time::Duration::from_secs(5))
            .expect("Core teardown terminal result");
        assert_eq!(end.reason, TerminalEndReason::CoreStopped);
        assert!(end.stop_confirmed);
        assert!(!root.is_running().expect("actual process state"));
        assert!(terminal.process_identities().expect("Job state").is_empty());
    }
    assert!(backend.terminals.open(session, || panic!("old Core must not launch")).is_err());
}
