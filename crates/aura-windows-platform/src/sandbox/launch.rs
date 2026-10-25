//! Shared suspended creation, Job membership confirmation and resume; native boundary only.
use super::*;
mod adoption;
mod creation;
pub(super) use adoption::adopt_created_process;
pub(super) use creation::{NativeStartup, create_native_process};
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "compat-adoption"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobLaunchFault {
    Assignment,
    Resume,
}

#[derive(Default, Debug)]
pub struct JobLaunchTiming {
    pub root_created: bool,
    /// Membership observed immediately after creation, before adoption can assign the root.
    pub created_in_job: bool,
    pub before_create: Duration,
    pub created: Duration,
    pub assigned: Duration,
    pub resumed: Duration,
    pub root_identity: Option<crate::ProcessIdentity>,
    pub failure_stopped: bool,
}

#[derive(Debug)]
pub struct JobLaunchError {
    pub source: io::Error,
    pub timing: Box<JobLaunchTiming>,
}
impl std::fmt::Display for JobLaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = if !self.timing.root_created || self.timing.failure_stopped {
            "not_started"
        } else {
            "outcome_unknown"
        };
        write!(f, "compat.{state}: {}", self.source)
    }
}
impl std::error::Error for JobLaunchError {}

/// The compat caller supplies a non-breakaway Job assigned atomically during root creation.
pub fn spawn_compat_process(
    job: &crate::RootJob,
    executable: &Path,
    arguments: &[Vec<u16>],
    directory: &Path,
    environment: &[NativeEnvironmentEntry],
    cmd_script: Option<&Path>,
) -> Result<(NativeSandboxProcess, JobLaunchTiming), JobLaunchError> {
    spawn_owned_job(
        job,
        executable,
        arguments,
        directory,
        environment,
        LaunchOverrides { cmd_script, ..LaunchOverrides::default() },
    )
}

#[cfg(any(test, feature = "compat-adoption"))]
pub fn spawn_compat_process_with_fault(
    job: &crate::RootJob,
    executable: &Path,
    arguments: &[Vec<u16>],
    directory: &Path,
    environment: &[NativeEnvironmentEntry],
    cmd_script: Option<&Path>,
    fault: JobLaunchFault,
) -> Result<(NativeSandboxProcess, JobLaunchTiming), JobLaunchError> {
    spawn_owned_job(
        job,
        executable,
        arguments,
        directory,
        environment,
        LaunchOverrides { cmd_script, fault: Some(fault), ..LaunchOverrides::default() },
    )
}

pub(crate) fn spawn_owned_job(
    job: &crate::RootJob,
    executable: &Path,
    arguments: &[Vec<u16>],
    directory: &Path,
    environment: &[NativeEnvironmentEntry],
    overrides: LaunchOverrides<'_>,
) -> Result<(NativeSandboxProcess, JobLaunchTiming), JobLaunchError> {
    let mut timing = JobLaunchTiming::default();
    let started = Instant::now();
    let result = job.duplicate_for_launch().and_then(|job| {
        spawn_restricted_process_inner(
            NativeRestrictedMode::StandardUser,
            None,
            executable,
            arguments,
            directory,
            environment,
            None,
            LaunchOverrides {
                token: overrides.token,
                desktop: overrides.desktop,
                cmd_script: overrides.cmd_script,
                job: Some(job),
                job_at_creation: true,
                trace: Some((&mut timing, started)),
                #[cfg(any(test, feature = "compat-adoption"))]
                fault: overrides.fault,
            },
        )
    });
    match result {
        Ok(process) => Ok((process, timing)),
        Err(source) => Err(JobLaunchError { source, timing: Box::new(timing) }),
    }
}

#[derive(Default)]
pub(crate) struct LaunchOverrides<'a> {
    pub(crate) token: Option<&'a OwnedHandle>,
    pub(crate) desktop: Option<&'a [u16]>,
    pub(crate) cmd_script: Option<&'a Path>,
    pub(crate) job: Option<OwnedHandle>,
    pub(crate) job_at_creation: bool,
    pub(crate) trace: Option<(&'a mut JobLaunchTiming, Instant)>,
    #[cfg(any(test, feature = "compat-adoption"))]
    pub(crate) fault: Option<JobLaunchFault>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_restricted_process_inner(
    mode: NativeRestrictedMode,
    profile: Option<&AppContainerProfile>,
    executable: &Path,
    arguments: &[Vec<u16>],
    working_directory: &Path,
    environment: &[NativeEnvironmentEntry],
    secret_environment: Option<&NativeSecretEnvironmentEntry>,
    mut overrides: LaunchOverrides<'_>,
) -> io::Result<NativeSandboxProcess> {
    if mode == NativeRestrictedMode::AppContainer && profile.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "AppContainer launch requires a profile",
        ));
    }
    let elevated = current_process_is_elevated()?;
    let standard_user_token = elevated.then(standard_user_primary_token).transpose()?;
    let job = match overrides.job.take() {
        Some(job) => job,
        None => create_kill_on_close_job()?,
    };
    let arguments = rewrite_powershell_file_arguments(arguments)?;
    let application = wide_os_null(executable.as_os_str());
    let mut command_line =
        quote_windows_argument_wide(&executable.as_os_str().encode_wide().collect::<Vec<_>>());
    for argument in &arguments {
        command_line.push(u16::from(b' '));
        command_line.extend(quote_windows_argument_wide(argument));
    }
    command_line.push(0);
    if let Some(script) = overrides.cmd_script {
        if !is_windows_cmd_image(executable)?
            || !script.is_absolute()
            || script.to_string_lossy().contains(['"', '\r', '\n', '%', '!'])
        {
            return Err(io::Error::other("runner.invalid_cmd_image"));
        }
        // CMD /s /c requires its own outer quote pair; CRT argument escaping
        // must not rewrite this command tail. The only inserted value is the
        // Core-generated, validated absolute script path, never model text.
        // `/u` would force UTF-16LE on redirected internal commands and break
        // the UTF-8 wrapper (`chcp 65001`) plus mixed child-process output.
        let command_exe = dos_drive_path(executable)?;
        let command_script = dos_drive_path(script)?;
        if command_script.to_string_lossy().contains(['"', '\r', '\n', '%', '!']) {
            return Err(io::Error::other("runner.invalid_cmd_image"));
        }
        command_line = format!(
            "\"{}\" /d /q /e:on /v:off /s /c \"\"{}\"\"",
            command_exe.display(),
            command_script.display()
        )
        .encode_utf16()
        .chain([0])
        .collect();
    }
    let directory_path = dos_drive_path(working_directory)?;
    let directory = wide_os_null(directory_path.as_os_str());
    // The plan's entries stay reachable for the failure report, which lists the
    // environment's names; the packed block is a separate value.
    let environment_wide = environment_block(environment, secret_environment, &directory_path)?;
    let (stdin_read, stdin_write) = create_inheritable_pipe()?;
    set_non_inheritable(&stdin_write)?;
    let (stdout_read, stdout_write) = create_inheritable_pipe()?;
    set_non_inheritable(&stdout_read)?;
    let (stderr_read, stderr_write) = create_inheritable_pipe()?;
    set_non_inheritable(&stderr_read)?;

    let inherited_handles = vec![stdin_read.as_raw(), stdout_write.as_raw(), stderr_write.as_raw()];
    let mut attributes = if overrides.job_at_creation {
        ProcessAttributeList::with_job(
            profile.map(|profile| profile.sid),
            inherited_handles,
            job.as_raw(),
        )?
    } else {
        ProcessAttributeList::new(profile.map(|profile| profile.sid), inherited_handles)?
    };
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
        .map_err(|_| io::Error::other("STARTUPINFOEX size overflow"))?;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.lpDesktop =
        overrides.desktop.map_or(ptr::null_mut(), |v| v.as_ptr().cast_mut());
    startup.StartupInfo.hStdInput = stdin_read.as_raw();
    startup.StartupInfo.hStdOutput = stdout_write.as_raw();
    startup.StartupInfo.hStdError = stderr_write.as_raw();
    startup.lpAttributeList = attributes.as_raw();
    // CREATE_NEW_CONSOLE would attach a conhost that can outlive cmd.exe and
    // look like leftover background work. Redirected standard handles already
    // give CMD a console-less pipe session.
    let flags = CREATE_NO_WINDOW
        | CREATE_SUSPENDED
        | CREATE_UNICODE_ENVIRONMENT
        | EXTENDED_STARTUPINFO_PRESENT;
    // SAFETY: all pointers remain valid for this call, the command buffer is
    // mutable as required, and the attribute list restricts inheritance to the
    // three inheritable standard-stream handles. When Aura itself is elevated,
    // CreateProcessAsUserW uses the linked standard-user token; otherwise
    // CreateProcessW preserves the already-standard caller token.
    if let Some((trace, origin)) = overrides.trace.as_mut() {
        trace.before_create = origin.elapsed();
    }
    let created = create_native_process(NativeStartup {
        application: &application,
        command_line: &mut command_line,
        directory: &directory,
        environment: &environment_wide,
        startup: &startup,
        token: overrides.token.or(standard_user_token.as_ref()),
        as_user: mode == NativeRestrictedMode::AppContainer,
        inherit_handles: true,
        flags,
    });
    let information = created.map_err(|source| {
        // The failing API, the caller's token state, the image, the working
        // directory and the environment's *names* — never its values, which can
        // carry credentials. Enough to tell an environment problem from a token,
        // image or attribute problem without a debugger.
        io::Error::other(OsContextError {
            action: format!(
                "create restricted process [{}{}]",
                launch_context(LaunchFacts {
                    mode,
                    elevated,
                    has_explicit_token: overrides.token.is_some(),
                    has_standard_user_token: standard_user_token.is_some(),
                    application: executable,
                    directory: &directory_path,
                    environment,
                    secret_environment,
                    appcontainer_profile: profile.is_some(),
                }),
                environment_requirement_hint(mode, environment, &source)
            ),
            source,
        })
    })?;
    drop(stdin_read);
    drop(stdout_write);
    drop(stderr_write);
    let process = adopt_created_process(&job, information, &mut overrides)?;
    Ok(NativeSandboxProcess {
        process,
        _job: job,
        stdin_write: Some(stdin_write),
        stdout_read,
        stderr_read,
        process_id: information.dwProcessId,
        executable: executable.to_path_buf(),
    })
}
