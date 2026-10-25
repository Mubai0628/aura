use super::support::*;
use aura_backend_windows_user::compat::LaunchFault;
use aura_windows_user_contract::ManagedShell;

#[test]
fn assignment_or_resume_failure_never_runs_the_script() {
    let root = tempfile::tempdir().expect("owned failed-launch workspace");
    let backend = backend();
    for fault in [LaunchFault::Assignment, LaunchFault::Resume] {
        let error = match backend.launch_with_fault(
            &command(ManagedShell::Cmd, root.path(), "echo forbidden > script-ran"),
            fault,
        ) {
            Ok(mut child) => {
                child.stop(std::time::Duration::from_secs(3));
                panic!("fault must prevent start")
            }
            Err(error) => error,
        };
        assert!(error.to_string().contains("not_started"), "{error}");
        assert!(!root.path().join("script-ran").exists());
    }
}

#[test]
fn invalid_or_relative_working_directory_is_refused_before_start() {
    let root = tempfile::tempdir().expect("owned preflight workspace");
    let backend = backend();
    let mut request = command(ManagedShell::Cmd, root.path(), "echo forbidden > script-ran");
    request.working_directory = "relative-workspace".into();
    assert!(backend.launch(&request, None).is_err());
    assert!(!root.path().join("script-ran").exists());
}
