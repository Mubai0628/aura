use std::{
    ffi::c_void,
    io,
    mem::size_of,
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{
        GetLastError, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Storage::FileSystem::WriteFile,
    System::{
        IO::{CreateIoCompletionPort, GetQueuedCompletionStatus},
        JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectAssociateCompletionPortInformation,
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Threading::{
            CREATE_SUSPENDED, CreateProcessW, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
            INFINITE, PROCESS_INFORMATION, ResumeThread, STARTUPINFOEXW, TerminateProcess,
            WaitForSingleObject,
        },
    },
};

use crate::{
    handle::OwnedHandle,
    process::{ProcessIdentity, snapshot_process_handle},
    sandbox::{ProcessAttributeList, create_inheritable_pipe, set_non_inheritable},
};

#[cfg(feature = "restricted-user-runtime")]
use windows_sys::Win32::System::{JobObjects::AssignProcessToJobObject, Threading::STARTUPINFOW};

const COMPONENT_JOB_COMPLETION_KEY: usize = 0x4155_5241;

#[cfg(feature = "restricted-user-runtime")]
mod account_runner;
#[cfg(test)]
pub(crate) mod creation_tests;
mod limits;
pub use limits::JobLimits;
mod membership;
#[cfg(feature = "restricted-user-runtime")]
mod recovery;
#[cfg(feature = "restricted-user-runtime")]
pub use account_runner::AccountRunnerLaunch;

pub struct RootJob {
    handle: OwnedHandle,
}

impl RootJob {
    pub fn create() -> io::Result<Self> {
        // SAFETY: null security/name pointers request a private, non-inheritable
        // unnamed job. The returned handle is owned immediately.
        let handle = OwnedHandle::from_raw(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
        Self::configure(handle)
    }
    fn configure(handle: OwnedHandle) -> io::Result<Self> {
        // Explicitly clear handle inheritance even though the default security
        // attributes already create a non-inheritable handle.
        // SAFETY: `handle` is valid and the flags arguments are documented.
        if unsafe { SetHandleInformation(handle.as_raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: the information pointer and byte size match the requested job
        // information class for the lifetime of the call.
        if unsafe {
            SetInformationJobObject(
                handle.as_raw(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .map_err(|_| io::Error::other("job information size overflow"))?,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    /// Creates the suspended desktop atomically in the Root Job, then resumes it.
    /// The Launcher itself is never assigned.
    pub fn spawn_assigned(
        &self,
        executable: &Path,
        arguments: &[String],
        working_directory: &Path,
    ) -> io::Result<ManagedProcess> {
        let application = wide_null(&executable.to_string_lossy());
        let command = std::iter::once(executable.to_string_lossy().into_owned())
            .chain(arguments.iter().cloned())
            .map(|argument| quote_windows_argument(&argument))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = wide_null(&command);
        let directory = wide_null(&working_directory.to_string_lossy());
        let mut attributes = ProcessAttributeList::job_only(self.handle.as_raw())?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
            .map_err(|_| io::Error::other("STARTUPINFOEXW size overflow"))?;
        startup.lpAttributeList = attributes.as_raw();
        let mut information = PROCESS_INFORMATION::default();
        // SAFETY: all strings are NUL-terminated and alive for the call. Handle
        // inheritance is disabled and output storage is valid.
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                directory.as_ptr(),
                &raw const startup.StartupInfo,
                &mut information,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }

        let process = OwnedHandle::from_raw(information.hProcess)?;
        let thread = OwnedHandle::from_raw(information.hThread)?;
        // SAFETY: both handles are valid, and the process has not executed user
        // code because its primary thread remains suspended.
        #[cfg(test)]
        creation_tests::inspect_created(self.handle.as_raw(), process.as_raw())?;
        // SAFETY: this is the suspended primary thread returned by CreateProcessW.
        if unsafe { ResumeThread(thread.as_raw()) } == u32::MAX {
            unsafe {
                let _ = TerminateProcess(process.as_raw(), 1);
            }
            return Err(io::Error::last_os_error());
        }
        drop(thread);
        Ok(ManagedProcess {
            process,
            process_id: information.dwProcessId,
            executable: executable.to_path_buf(),
        })
    }
}

/// Core-owned lifecycle job for its long-lived security components. It sets
/// only KILL_ON_JOB_CLOSE: no UI restriction or breakaway flag is applied, so
/// Sandbox Host may create a stricter nested Execution Job while every child
/// remains inside the parent lifecycle boundary.
pub struct ComponentJob {
    handle: OwnedHandle,
    completion_port: OwnedHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobCompletionNotification {
    pub message: u32,
    pub process_id: u32,
}

impl ComponentJob {
    pub fn create() -> io::Result<Self> {
        let handle = create_kill_on_close_job()?;
        // SAFETY: INVALID_HANDLE_VALUE with no existing port is the documented
        // way to create an unassociated IOCP. The returned handle is owned.
        let completion_port = OwnedHandle::from_raw(unsafe {
            CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1)
        })?;
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: COMPONENT_JOB_COMPLETION_KEY as *mut c_void,
            CompletionPort: completion_port.as_raw(),
        };
        // The completion port is associated before any process can enter the
        // job, so initial process notifications cannot race the association.
        // SAFETY: information class, pointer, and byte count match exactly.
        if unsafe {
            SetInformationJobObject(
                handle.as_raw(),
                JobObjectAssociateCompletionPortInformation,
                (&raw const association).cast(),
                u32::try_from(size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>())
                    .map_err(|_| io::Error::other("completion-port association size overflow"))?,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle, completion_port })
    }

    /// Creates the child suspended, inherits only a one-shot anonymous-pipe
    /// read handle, atomically enters the Component Job, writes the 256-bit secret,
    /// closes the parent endpoint, and only then resumes child code.
    pub fn spawn_sidecar(
        &self,
        executable: &Path,
        arguments: &[String],
        working_directory: &Path,
        bootstrap_secret: &[u8; 32],
    ) -> io::Result<ManagedProcess> {
        let (bootstrap_read, bootstrap_write) = create_inheritable_pipe()?;
        set_non_inheritable(&bootstrap_write)?;
        let inherited_handle = bootstrap_read.as_raw();
        let mut attributes =
            ProcessAttributeList::with_job(None, vec![inherited_handle], self.handle.as_raw())?;

        let application = wide_null(&executable.to_string_lossy());
        let bootstrap_argument = format!("--bootstrap-handle={}", inherited_handle as usize);
        let command = std::iter::once(executable.to_string_lossy().into_owned())
            .chain(arguments.iter().cloned())
            .chain(std::iter::once(bootstrap_argument))
            .map(|argument| quote_windows_argument(&argument))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = wide_null(&command);
        let directory = wide_null(&working_directory.to_string_lossy());
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
            .map_err(|_| io::Error::other("STARTUPINFOEXW size overflow"))?;
        startup.lpAttributeList = attributes.as_raw();
        let mut information = PROCESS_INFORMATION::default();
        // SAFETY: strings and the initialized handle-list attribute remain
        // alive for the call; only the bootstrap read handle is inheritable.
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                1,
                CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                directory.as_ptr(),
                &raw const startup.StartupInfo,
                &mut information,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        drop(bootstrap_read);
        let process = OwnedHandle::from_raw(information.hProcess)?;
        let thread = OwnedHandle::from_raw(information.hThread)?;

        // A failure at any point after process creation is fail-closed; child
        // user code never runs outside the Component Job or without its secret.
        #[cfg(test)]
        creation_tests::inspect_created(self.handle.as_raw(), process.as_raw())?;
        let mut written = 0_u32;
        // SAFETY: the write endpoint is valid and the 32-byte secret remains
        // alive for the synchronous call.
        if unsafe {
            WriteFile(
                bootstrap_write.as_raw(),
                bootstrap_secret.as_ptr(),
                u32::try_from(bootstrap_secret.len())
                    .map_err(|_| io::Error::other("bootstrap secret length overflow"))?,
                &mut written,
                ptr::null_mut(),
            )
        } == 0
            || written != u32::try_from(bootstrap_secret.len()).unwrap_or(u32::MAX)
        {
            unsafe {
                let _ = TerminateProcess(process.as_raw(), 1);
            }
            return Err(io::Error::last_os_error());
        }
        drop(bootstrap_write);
        if unsafe { ResumeThread(thread.as_raw()) } == u32::MAX {
            unsafe {
                let _ = TerminateProcess(process.as_raw(), 1);
            }
            return Err(io::Error::last_os_error());
        }
        drop(thread);
        Ok(ManagedProcess {
            process,
            process_id: information.dwProcessId,
            executable: executable.to_path_buf(),
        })
    }

    /// Receives the next best-effort Job notification. Callers must still use
    /// the retained process handle as the authoritative terminal-state check.
    pub fn poll_notification(
        &self,
        timeout: std::time::Duration,
    ) -> io::Result<Option<JobCompletionNotification>> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        let mut message = 0_u32;
        let mut completion_key = 0_usize;
        let mut process_value = ptr::null_mut();
        // SAFETY: all output pointers are valid and the completion port remains
        // owned for the duration of the wait.
        let succeeded = unsafe {
            GetQueuedCompletionStatus(
                self.completion_port.as_raw(),
                &mut message,
                &mut completion_key,
                &mut process_value,
                timeout_ms,
            )
        };
        if succeeded == 0 {
            // SAFETY: the error is read immediately after the failed wait.
            let error = unsafe { GetLastError() };
            if error == WAIT_TIMEOUT {
                return Ok(None);
            }
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        if completion_key != COMPONENT_JOB_COMPLETION_KEY {
            return Err(io::Error::other("component job completion key mismatch"));
        }
        Ok(Some(JobCompletionNotification {
            message,
            process_id: u32::try_from(process_value as usize)
                .map_err(|_| io::Error::other("job process id overflow"))?,
        }))
    }
}

pub(crate) fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
    // SAFETY: null security/name pointers create a private, non-inheritable
    // unnamed job; the returned handle is owned immediately.
    let handle = OwnedHandle::from_raw(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
    if unsafe { SetHandleInformation(handle.as_raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // Deliberately omit BREAKAWAY and BasicUIRestrictions. A nested Execution
    // Job can only tighten these lifecycle limits.
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            handle.as_raw(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .map_err(|_| io::Error::other("job information size overflow"))?,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

pub(crate) fn terminate_job(handle: &OwnedHandle, exit_code: u32) -> io::Result<()> {
    // SAFETY: the Job handle is owned for the lifetime of this call.
    if unsafe { TerminateJobObject(handle.as_raw(), exit_code) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn job_active_processes(handle: &OwnedHandle) -> io::Result<u32> {
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let mut returned = 0_u32;
    // SAFETY: `accounting` matches JobObjectBasicAccountingInformation.
    if unsafe {
        QueryInformationJobObject(
            handle.as_raw(),
            JobObjectBasicAccountingInformation,
            (&raw mut accounting).cast(),
            u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
                .map_err(|_| io::Error::other("job accounting size overflow"))?,
            &mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if returned != 0
        && returned
            != u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
                .map_err(|_| io::Error::other("job accounting size overflow"))?
    {
        return Err(io::Error::other(
            "Windows returned an unexpected Job accounting record length",
        ));
    }
    Ok(accounting.ActiveProcesses)
}

pub(crate) fn wait_for_job_empty(
    handle: &OwnedHandle,
    timeout: std::time::Duration,
) -> io::Result<bool> {
    let started = std::time::Instant::now();
    loop {
        if job_active_processes(handle)? == 0 {
            return Ok(true);
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Ok(false);
        }
        std::thread::sleep(
            std::time::Duration::from_millis(10).min(timeout.saturating_sub(elapsed)),
        );
    }
}

pub struct ManagedProcess {
    process: OwnedHandle,
    process_id: u32,
    executable: PathBuf,
}

impl ManagedProcess {
    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn identity(&self) -> io::Result<ProcessIdentity> {
        snapshot_process_handle(self.process.as_raw(), self.process_id)
    }

    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // SAFETY: the retained process handle is valid and uniquely owned.
        if unsafe { TerminateProcess(self.process.as_raw(), exit_code) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Checks the retained process handle without relying on a reusable PID or
    /// on best-effort Job completion notifications.
    pub fn try_exit_code(&self) -> io::Result<Option<u32>> {
        // SAFETY: the retained process handle is valid and timeout zero makes
        // this a non-blocking authoritative state check.
        let wait = unsafe { WaitForSingleObject(self.process.as_raw(), 0) };
        if wait == WAIT_TIMEOUT {
            return Ok(None);
        }
        if wait != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut exit_code = 0_u32;
        // SAFETY: the process is signaled and `exit_code` is writable.
        if unsafe { GetExitCodeProcess(self.process.as_raw(), &mut exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(exit_code))
    }

    pub fn wait(self) -> io::Result<u32> {
        // SAFETY: the process handle remains valid for the duration of the wait.
        let wait = unsafe { WaitForSingleObject(self.process.as_raw(), INFINITE) };
        if wait != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut exit_code = 0_u32;
        // SAFETY: the process is signaled and `exit_code` is writable storage.
        if unsafe { GetExitCodeProcess(self.process.as_raw(), &mut exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(exit_code)
    }
}

pub(crate) fn quote_windows_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument.chars().any(|character| character.is_whitespace() || character == '"')
    {
        return argument.to_owned();
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0_usize;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
        } else {
            if character == '"' {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
            } else {
                quoted.push_str(&"\\".repeat(backslashes));
            }
            backslashes = 0;
            quoted.push(character);
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

pub(crate) fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::quote_windows_argument;

    #[test]
    fn quotes_windows_paths_and_embedded_quotes() {
        assert_eq!(quote_windows_argument("plain"), "plain");
        assert_eq!(quote_windows_argument(r"C:\Aura Test\"), r#""C:\Aura Test\\""#);
        assert_eq!(quote_windows_argument("a\"b"), r#""a\"b""#);
        assert_eq!(
            quote_windows_argument(r"D:\用户\Aura 应用\Aura.exe"),
            r#""D:\用户\Aura 应用\Aura.exe""#
        );
    }
}
