//! Native headless launch with explicit process ownership and pipe inheritance.
use super::*;

pub(super) fn spawn_job_process(
    executable: PathBuf,
    arguments: &[String],
    working_directory: PathBuf,
    workspace: RenderWorkspace,
    appcontainer: Option<AppContainerProfile>,
    edge_slot: Option<EdgeRenderSlot>,
) -> Result<HeadlessRenderSession, HeadlessRenderError> {
    let job = create_kill_on_close_job().map_err(map_io)?;
    let application = wide_null(&executable.to_string_lossy());
    let command = std::iter::once(executable.to_string_lossy().into_owned())
        .chain(arguments.iter().cloned())
        .map(|argument| quote_windows_argument(&argument))
        .collect::<Vec<_>>()
        .join(" ");
    let mut command = wide_null(&command);
    let directory = wide_null(&working_directory.to_string_lossy());
    let (stdin_read, stdin_write) = create_inheritable_pipe().map_err(map_io)?;
    set_non_inheritable(&stdin_write).map_err(map_io)?;
    let (stdout_read, stdout_write) = create_inheritable_pipe().map_err(map_io)?;
    set_non_inheritable(&stdout_read).map_err(map_io)?;
    let (stderr_read, stderr_write) = create_inheritable_pipe().map_err(map_io)?;
    set_non_inheritable(&stderr_read).map_err(map_io)?;
    let inherited = vec![stdin_read.as_raw(), stdout_write.as_raw(), stderr_write.as_raw()];
    let sid = appcontainer.as_ref().map(AppContainerProfile::sid);
    let mut attributes =
        ProcessAttributeList::with_job(sid, inherited, job.as_raw()).map_err(map_io)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb =
        u32::try_from(size_of_startup()).map_err(|_| HeadlessRenderError::Unavailable)?;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin_read.as_raw();
    startup.StartupInfo.hStdOutput = stdout_write.as_raw();
    startup.StartupInfo.hStdError = stderr_write.as_raw();
    startup.lpAttributeList = attributes.as_raw();
    let mut information = PROCESS_INFORMATION::default();
    // The child gets an explicit environment built from the parent's own, minus
    // the application-compatibility layer variable. A null pointer here means
    // "inherit the parent's live block", which is how a layer injected into the
    // user's shell — with no Aura code involved — made this browser exit 0 with
    // no DOM. The render must be the browser's own behaviour, not a shim's, and
    // the parent process, the user's environment and every other shell boundary
    // are untouched: this block is built here, for this child only.
    let environment =
        crate::child_environment::inherited_block_without(&[COMPATIBILITY_LAYER_VARIABLE])
            .map_err(map_io)?;
    let flags = CREATE_NO_WINDOW
        | CREATE_SUSPENDED
        | EXTENDED_STARTUPINFO_PRESENT
        | CREATE_UNICODE_ENVIRONMENT;
    // SAFETY: strings, the attribute list and the environment block stay alive for
    // the call. Only the three standard-stream handles are inherited.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            flags,
            environment.as_ptr().cast(),
            directory.as_ptr(),
            &raw const startup.StartupInfo,
            &mut information,
        )
    };
    drop(stdin_read);
    drop(stdout_write);
    drop(stderr_write);
    drop(stdin_write);
    if created == 0 {
        return Err(HeadlessRenderError::Unavailable);
    }
    let process = OwnedHandle::from_raw(information.hProcess).map_err(map_io)?;
    let thread = OwnedHandle::from_raw(information.hThread).map_err(map_io)?;
    // SAFETY: the process is still suspended; both handles are owned.
    #[cfg(test)]
    crate::job::creation_tests::inspect_created(job.as_raw(), process.as_raw()).map_err(map_io)?;
    // SAFETY: this is the suspended primary thread from CreateProcessW.
    if unsafe { ResumeThread(thread.as_raw()) } == u32::MAX {
        unsafe {
            let _ = TerminateProcess(process.as_raw(), 1);
        }
        return Err(HeadlessRenderError::Unavailable);
    }
    drop(thread);
    let stdout_too_large = Arc::new(AtomicBool::new(false));
    let stdout = spawn_reader(
        stdout_read,
        job.try_clone().map_err(map_io)?,
        Some(stdout_too_large.clone()),
    )?;
    let stderr = spawn_reader(stderr_read, job.try_clone().map_err(map_io)?, None)?;
    Ok(HeadlessRenderSession {
        job,
        process,
        stdout: Some(stdout),
        stderr: Some(stderr),
        stdout_too_large,
        _workspace: workspace,
        _appcontainer: appcontainer,
        _edge_slot: edge_slot,
    })
}
