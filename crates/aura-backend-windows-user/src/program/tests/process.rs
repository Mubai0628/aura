use super::{super::*, support::*};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[test]
fn native_program_has_only_job_and_output_guarantees_and_preserves_separate_streams() {
    use aura_security_contract::EnforcementGuarantee as G;
    assert_eq!(guarantees(), [G::ChildProcessControl, G::BoundedOutput].into());
    let root = tempfile::tempdir().expect("temporary directory");
    let result = command(
        root.path(),
        "echo package-ok & echo package-error 1>&2",
        &AtomicBool::new(false),
        Duration::from_secs(5),
        65536,
        ProgramInput::Closed,
        &mut |_: &[u8]| Ok(vec![]),
    );
    assert!(result.started && result.stop_confirmed, "{result:?}");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.reason, ProgramStop::Exited);
    assert!(String::from_utf8_lossy(&result.stdout).contains("package-ok"));
    assert!(String::from_utf8_lossy(&result.stderr).contains("package-error"));
}

#[test]
fn cancel_and_timeout_stop_a_real_child_tree_and_output_limit_is_bounded() {
    let root = tempfile::tempdir().expect("temporary directory");
    std::fs::write(
        root.path().join("child.cmd"),
        b"@echo child-ready\r\n@for /l %%i in (1,0,2) do @rem\r\n",
    )
    .expect("owned child fixture");
    for cancellation in [true, false] {
        let cancelled = AtomicBool::new(false);
        let mut saw_child = false;
        let began = Instant::now();
        let result = command(
            root.path(),
            "cmd /d /c child.cmd",
            &cancelled,
            Duration::from_secs(1),
            65536,
            ProgramInput::Closed,
            &mut |bytes: &[u8]| {
                if String::from_utf8_lossy(bytes).contains("child-ready") {
                    saw_child = true;
                    if cancellation {
                        cancelled.store(true, Ordering::Release);
                    }
                }
                Ok(vec![])
            },
        );
        assert!(saw_child, "the inner CMD must actually run: {result:?}");
        assert_eq!(
            result.reason,
            if cancellation { ProgramStop::Cancelled } else { ProgramStop::TimedOut }
        );
        assert!(result.stop_confirmed, "{result:?}");
        assert!(began.elapsed() < Duration::from_secs(5));
    }
    let result = command(
        root.path(),
        "for /l %i in (1,0,2) do @echo bounded-package-output",
        &AtomicBool::new(false),
        Duration::from_secs(5),
        1024,
        ProgramInput::Closed,
        &mut |_: &[u8]| Ok(vec![]),
    );
    assert_eq!(result.reason, ProgramStop::OutputLimit);
    assert!(result.stop_confirmed);
    assert_eq!(result.stdout.len() + result.stderr.len(), 1024);
}

#[test]
fn prestart_cancel_and_changed_image_never_publish_start() {
    let root = tempfile::tempdir().expect("temporary directory");
    let image = command_image();
    let environment = environment(root.path(), false);
    let args = vec!["/d".into(), "/c".into(), "echo never-started".into()];
    let mut started = false;
    let output = run(
        ProgramSpec {
            image: &image,
            arguments: &args,
            working_directory: root.path(),
            environment: environment.entries(),
            input: ProgramInput::Closed,
            cancelled: &AtomicBool::new(true),
            timeout: Duration::from_secs(5),
            max_output_bytes: 1024,
            limits: None,
        },
        &mut |_: &[u8]| Ok(vec![]),
        &mut |_| started = true,
    )
    .expect("cancelled before creation");
    assert!(!started && !output.started && output.stop_confirmed);
    let copy = root.path().join("fixture.exe");
    std::fs::copy(&image.path, &copy).expect("owned executable copy");
    let (binding, retained) = crate::file_binding::capture(&copy, true).expect("binding");
    drop(retained);
    std::fs::write(&copy, b"changed temporary executable").expect("owned mutation");
    assert!(
        run(
            ProgramSpec {
                image: &binding,
                arguments: &[],
                working_directory: root.path(),
                environment: environment.entries(),
                input: ProgramInput::Closed,
                cancelled: &AtomicBool::new(false),
                timeout: Duration::from_secs(5),
                max_output_bytes: 1024,
                limits: None
            },
            &mut |_: &[u8]| Ok(vec![]),
            &mut |_| started = true
        )
        .is_err()
    );
    assert!(!started);
}

#[test]
fn blocked_runtime_stdin_and_protocol_failure_cannot_hold_up_cancellation() {
    let root = tempfile::tempdir().expect("temporary directory");
    let began = Instant::now();
    let result = command(
        root.path(),
        "echo hello & for /l %i in (1,0,2) do @rem",
        &AtomicBool::new(false),
        Duration::from_millis(400),
        65536,
        ProgramInput::Frames,
        &mut |_: &[u8]| Ok(vec![vec![b'x'; 1024 * 1024]]),
    );
    assert_eq!(result.reason, ProgramStop::TimedOut);
    assert!(result.stop_confirmed, "{result:?}");
    assert!(began.elapsed() < Duration::from_secs(5));
    let result = command(
        root.path(),
        "echo malformed & for /l %i in (1,0,2) do @rem",
        &AtomicBool::new(false),
        Duration::from_secs(5),
        65536,
        ProgramInput::Frames,
        &mut |_: &[u8]| anyhow::bail!("synthetic protocol refusal"),
    );
    assert_eq!(result.reason, ProgramStop::ProtocolError);
    assert!(result.stop_confirmed, "{result:?}");
}

#[test]
fn failed_input_delivery_cannot_be_reported_as_a_successful_exit() {
    let root = tempfile::tempdir().expect("temporary directory");
    // This process never reads stdin and exits immediately. One MiB cannot fit
    // in the pipe buffer, so the writer must observe a real broken peer.
    let result = command(
        root.path(),
        "echo hello",
        &AtomicBool::new(false),
        Duration::from_secs(5),
        65536,
        ProgramInput::Frames,
        &mut |_: &[u8]| Ok(vec![vec![b'x'; 1024 * 1024]]),
    );
    assert_eq!(result.reason, ProgramStop::ProtocolError, "{result:?}");
    assert!(result.stop_confirmed, "a failed write is not an unknown process stop");
}
