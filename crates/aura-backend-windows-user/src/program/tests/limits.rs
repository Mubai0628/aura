//! A plugin's limits hold for the whole process tree; the user's own programs have none.
use super::{super::*, support::*};
use aura_windows_platform::JobLimits;
use std::{path::Path, sync::atomic::AtomicBool, time::Duration};

/// CMD starts one child (`ping`) and says so only if the child ran.
fn run_with(root: &Path, limits: Option<JobLimits>) -> ProgramOutput {
    let image = command_image();
    let environment = environment(root, false);
    let arguments =
        vec!["/d".into(), "/c".into(), "ping -n 1 127.0.0.1 >nul && echo child-ran".into()];
    run(
        ProgramSpec {
            image: &image,
            arguments: &arguments,
            working_directory: root,
            environment: environment.entries(),
            input: ProgramInput::Closed,
            cancelled: &AtomicBool::new(false),
            timeout: Duration::from_secs(30),
            max_output_bytes: 64 * 1024,
            limits,
        },
        &mut |_: &[u8]| Ok(vec![]),
        &mut |_| {},
    )
    .expect("a creation-time Job")
}

#[test]
fn a_limited_tree_cannot_start_more_processes_than_it_is_allowed() {
    let root = tempfile::tempdir().expect("temporary directory");
    let free = run_with(root.path(), None);
    assert!(String::from_utf8_lossy(&free.stdout).contains("child-ran"), "{free:?}");
    let limited = run_with(
        root.path(),
        Some(JobLimits { memory_bytes: 256 * 1024 * 1024, active_processes: 1 }),
    );
    assert!(limited.started && limited.stop_confirmed, "{limited:?}");
    assert!(
        !String::from_utf8_lossy(&limited.stdout).contains("child-ran"),
        "the child could not start: {}",
        String::from_utf8_lossy(&limited.stderr)
    );
    assert!(!limited.memory_limit_reached);
}
