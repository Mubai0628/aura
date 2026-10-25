use super::*;

#[test]
fn compat_root_is_already_in_job_at_both_pre_resume_faults() {
    let root = tempfile::tempdir().expect("owned suspended Shell workspace");
    let executable = Path::new(r"C:\Windows\System32\cmd.exe");
    let arguments = ["/d", "/c", "echo forbidden > script-ran"]
        .map(|argument| argument.encode_utf16().collect::<Vec<_>>());
    for fault in [JobLaunchFault::Assignment, JobLaunchFault::Resume] {
        let job = crate::RootJob::create().expect("owned non-breakaway Job");
        let error = match spawn_compat_process_with_fault(
            &job,
            executable,
            &arguments,
            root.path(),
            &[],
            None,
            fault,
        ) {
            Ok((process, _)) => {
                process.terminate(1).expect("stop unexpected launch");
                panic!("pre-resume fault must refuse launch")
            }
            Err(error) => error,
        };
        assert!(error.timing.root_created, "fault must observe a real created root");
        assert!(error.timing.created_in_job, "root escaped creation Job before {fault:?}");
        assert!(error.timing.failure_stopped, "fault cleanup must confirm the root exit");
        assert!(job.process_identities().expect("query retained Job").is_empty());
        assert!(!root.path().join("script-ran").exists(), "no first instruction may run");
    }
}

#[test]
fn terminal_root_is_already_in_job_at_both_pre_resume_faults() {
    let root = tempfile::tempdir().expect("owned suspended terminal workspace");
    let arguments = ["/d", "/k"].map(|argument| argument.encode_utf16().collect::<Vec<_>>());
    for fault in [JobLaunchFault::Assignment, JobLaunchFault::Resume] {
        let error = match NativeTerminal::spawn_with_fault(
            NativeTerminalLaunch {
                executable: Path::new(r"C:\Windows\System32\cmd.exe"),
                arguments: &arguments,
                directory: root.path(),
                environment: &[],
                size: TerminalSize { columns: 80, rows: 24 },
            },
            std::sync::Arc::new(|_| Ok(())),
            fault,
        ) {
            Ok(terminal) => {
                terminal.close().expect("stop unexpected terminal launch");
                panic!("pre-resume fault must refuse launch")
            }
            Err(error) => error,
        };
        assert!(error.timing.root_created, "fault must observe a real created root");
        assert!(error.timing.created_in_job, "ConPTY root escaped creation Job before {fault:?}");
        assert!(error.timing.failure_stopped, "fault cleanup must confirm the terminal root exit");
    }
}

#[test]
fn closing_the_creation_job_stops_a_root_that_has_never_resumed() {
    let root = tempfile::tempdir().expect("owned suspended-root fixture");
    let job = crate::RootJob::create().expect("private kill-on-close Job");
    let launch_job = job.duplicate_for_launch().expect("creation Job handle");
    let (pipe_read, pipe_write) = create_inheritable_pipe().expect("owned inheritance fixture");
    let mut attributes =
        ProcessAttributeList::with_job(None, vec![pipe_write.as_raw()], launch_job.as_raw())
            .expect("creation Job attributes");
    let application = wide_os_null(Path::new(r"C:\Windows\System32\cmd.exe").as_os_str());
    let mut command_line = r#""C:\Windows\System32\cmd.exe" /d /c "echo forbidden > script-ran""#
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let directory = wide_os_null(root.path().as_os_str());
    let environment = environment_block(&[], None, root.path()).expect("isolated environment");
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.lpAttributeList = attributes.as_raw();
    let information = create_native_process(NativeStartup {
        application: &application,
        command_line: &mut command_line,
        directory: &directory,
        environment: &environment,
        startup: &startup,
        token: None,
        as_user: false,
        inherit_handles: true,
        flags: CREATE_NO_WINDOW
            | CREATE_SUSPENDED
            | CREATE_UNICODE_ENVIRONMENT
            | EXTENDED_STARTUPINFO_PRESENT,
    })
    .expect("create a suspended root already in the Job");
    let process = OwnedHandle::from_raw(information.hProcess).expect("owned root handle");
    let thread = OwnedHandle::from_raw(information.hThread).expect("owned suspended thread");
    let identity =
        crate::process::snapshot_process_handle(process.as_raw(), information.dwProcessId)
            .expect("exact root creation identity");
    assert!(job.contains_identity(&identity).expect("IsProcessInJob before any resume"));
    assert_eq!(unsafe { WaitForSingleObject(process.as_raw(), 0) }, WAIT_TIMEOUT);
    // No ResumeThread, AssignProcessToJobObject or explicit TerminateProcess: the last
    // Job handle closing alone must stop this exact suspended process.
    drop(thread);
    drop(attributes);
    drop(pipe_read);
    drop(pipe_write);
    drop(launch_job);
    drop(job);
    // SAFETY: retain the exact created root handle for a bounded termination observation.
    assert_eq!(unsafe { WaitForSingleObject(process.as_raw(), 5_000) }, WAIT_OBJECT_0);
    assert!(!root.path().join("script-ran").exists(), "the root never ran its first instruction");
}

#[test]
fn a_created_root_without_an_identity_or_confirmed_cleanup_is_unknown() {
    let error = JobLaunchError {
        source: io::Error::other("identity query failed"),
        timing: Box::new(JobLaunchTiming {
            root_created: true,
            created: Duration::from_millis(1),
            ..JobLaunchTiming::default()
        }),
    };
    assert!(error.to_string().contains("outcome_unknown"), "{error}");
}

#[test]
fn not_started_requires_no_root_or_confirmed_cleanup() {
    for timing in [
        JobLaunchTiming::default(),
        JobLaunchTiming { root_created: true, failure_stopped: true, ..JobLaunchTiming::default() },
    ] {
        let error =
            JobLaunchError { source: io::Error::other("launch refused"), timing: Box::new(timing) };
        assert!(error.to_string().contains("not_started"), "{error}");
    }
}
