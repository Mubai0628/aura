use super::support::*;
use aura_windows_user_contract::ManagedShell;
use std::sync::atomic::AtomicBool;

#[test]
fn repeated_short_cmd_commands_do_not_register_exiting_processes_as_background_work() {
    let directory = tempfile::tempdir().expect("owned short-command directory");
    let backend = backend();
    for index in 0..20 {
        let mut execution = backend
            .launch(&command(ManagedShell::Cmd, directory.path(), "echo short"), None)
            .expect("launch short CMD command");
        let result = execution
            .run_direct(&AtomicBool::new(false))
            .expect("exiting child accounting does not fail the direct result");
        assert_eq!(result.exit_code, Some(0), "sample {index}");
        assert_eq!(result.active_processes, 0, "sample {index}: {result:?}");
        assert!(result.stop_confirmed, "sample {index}");
    }
}
