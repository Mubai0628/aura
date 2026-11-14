use super::support::*;
use aura_backend_windows_user::compat::{TerminalEndReason, TerminalRegistry};
use aura_domain_common::SessionId;
use aura_windows_user_contract::ManagedShell;

#[test]
fn closing_reaps_children_grandchildren_and_the_pseudoconsole_host() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let root = tempfile::tempdir().expect("isolated tree");
    let before =
        aura_windows_platform::child_process_identities(std::process::id()).expect("before");
    let terminal = backend
        .launch_terminal(
            &options(ManagedShell::PowerShell, root.path()),
            Some("http://127.0.0.1:9"),
        )
        .expect("ConPTY");
    marker(&terminal, ManagedShell::PowerShell, "TREE-READY");
    let fixture = env!("CARGO_BIN_EXE_aura-compat-fixture").replace('\'', "''");
    line(
        &terminal,
        &format!("& '{fixture}' tree '{}'", root.path().display().to_string().replace('\'', "''")),
    );
    await_file(&root.path().join("tree.ready"));
    let mut identities = terminal.process_identities().expect("Job members");
    assert!(identities.len() >= 3, "shell + child + grandchild");
    let hosts = aura_windows_platform::child_process_identities(std::process::id())
        .expect("hosts")
        .into_iter()
        .filter(|identity| {
            !before.contains(identity)
                && identity
                    .executable_path
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("conhost.exe"))
        })
        .collect::<Vec<_>>();
    assert!(!hosts.is_empty(), "observe the actual headless conhost");
    identities.extend(hosts);
    let retained = identities
        .iter()
        .map(|identity| {
            aura_windows_platform::retain_process(identity.process_id)
                .expect("retained exact process")
        })
        .collect::<Vec<_>>();
    let end = terminal.close(TerminalEndReason::UserClosed).expect("tree stop");
    assert!(end.stop_confirmed);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while retained.iter().any(|process| process.is_running().expect("process observation")) {
        assert!(std::time::Instant::now() < deadline, "no descendant or conhost may remain");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(terminal.close(TerminalEndReason::CoreStopped).expect("idempotent close"), end);
}

#[test]
fn ten_megabytes_are_drained_and_retention_reports_eviction() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let root = tempfile::tempdir().expect("isolated large output");
    let terminal = backend
        .launch_terminal(
            &options(ManagedShell::PowerShell, root.path()),
            Some("http://127.0.0.1:9"),
        )
        .expect("ConPTY");
    marker(&terminal, ManagedShell::PowerShell, "LARGE-READY");
    line(
        &terminal,
        "[Console]::WriteLine('L02-' + 'DATA-BEGIN'); [Console]::Write(('X' * 10485760)); [Console]::WriteLine('L02-' + 'LARGE-END')",
    );
    until(&terminal, "L02-LARGE-END");
    let output = super::vt_screen::render(
        &terminal.read(0, 16 * 1024 * 1024).expect("all VT bytes").bytes,
        120,
        30,
    );
    let body = output
        .split_once("L02-DATA-BEGIN")
        .expect("output start")
        .1
        .split_once("L02-LARGE-END")
        .expect("output end")
        .0;
    assert_eq!(body.bytes().filter(|byte| *byte == b'X').count(), 10485760);
    let end = terminal.close(TerminalEndReason::UserClosed).expect("drained close");
    assert!(end.stop_confirmed);
    let mut request = options(ManagedShell::Cmd, root.path());
    request.output_limit = 1024;
    let small =
        backend.launch_terminal(&request, Some("http://127.0.0.1:9")).expect("bounded buffer");
    for _ in 0..200 {
        line(&small, "echo 012345678901234567890123456789");
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    small.close(TerminalEndReason::UserClosed).expect("close large pending output");
    let page = small.read(0, 1024).expect("retained bytes");
    assert!(page.truncated);
    assert!(page.start_offset > 0);
    assert!(page.bytes.len() <= 1024);
}

#[test]
fn registry_is_session_bound_and_shutdown_refuses_new_launches() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    let registry = TerminalRegistry::default();
    let root = tempfile::tempdir().expect("isolated registry");
    let session = SessionId::new();
    let other = SessionId::new();
    let entry = registry
        .open(session, || {
            backend.launch_terminal(
                &options(ManagedShell::Cmd, root.path()),
                Some("http://127.0.0.1:9"),
            )
        })
        .expect("registered");
    assert!(registry.get(other, &entry.id).is_err());
    marker(&registry.get(session, &entry.id).expect("owned"), ManagedShell::Cmd, "REGISTRY");
    registry.shutdown().expect("all terminals reaped");
    assert_eq!(
        registry
            .get(session, &entry.id)
            .expect("ended record")
            .wait_ended(std::time::Duration::ZERO)
            .expect("ended")
            .reason,
        TerminalEndReason::CoreStopped
    );
    assert!(registry.open(session, || panic!("closed registry must never spawn")).is_err());
}
