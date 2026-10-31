use std::{
    ffi::{OsString, c_void},
    io,
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Component, Path, PathBuf, Prefix},
    ptr,
    time::{Duration, Instant},
};

use windows_sys::{
    Win32::{
        Foundation::{
            ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED, ERROR_SUCCESS,
            GetLastError, HANDLE, HANDLE_FLAG_INHERIT, LocalFree, SetHandleInformation,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Security::SECURITY_ATTRIBUTES,
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
                ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW,
                EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE,
                REVOKE_ACCESS, SDDL_REVISION_1, SE_FILE_OBJECT, SetEntriesInAclW,
                SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN,
            },
            DACL_SECURITY_INFORMATION, DuplicateTokenEx, FreeSid, GetSecurityDescriptorDacl,
            GetTokenInformation,
            Isolation::{
                CreateAppContainerProfile, DeleteAppContainerProfile,
                DeriveAppContainerSidFromAppContainerName,
            },
            PSECURITY_DESCRIPTOR, PSID, SUB_CONTAINERS_AND_OBJECTS_INHERIT, SecurityImpersonation,
            TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_LINKED_TOKEN,
            TOKEN_QUERY, TokenElevation, TokenIsAppContainer, TokenLinkedToken, TokenPrimary,
        },
        Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE, ReadFile, WriteFile},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
                QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
            },
            Pipes::{CreatePipe, PeekNamedPipe},
            Threading::{
                CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
                CreateProcessAsUserW, CreateProcessW, EXTENDED_STARTUPINFO_PRESENT,
                GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, PROCESS_INFORMATION,
                ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
                WaitForSingleObject,
            },
        },
    },
    core::PWSTR,
};
use zeroize::Zeroizing;

#[cfg(feature = "restricted-user-runtime")]
mod file_access;
mod installation_acl;
#[cfg(feature = "restricted-user-runtime")]
pub(crate) mod restricted_user;
pub use installation_acl::verify_execution_installation_acl;
#[cfg(feature = "restricted-user-runtime")]
pub use installation_acl::verify_execution_maintenance_acl;
#[cfg(all(test, feature = "restricted-user-runtime"))]
mod file_access_tests;
#[cfg(feature = "restricted-user-runtime")]
pub use file_access::ExecutionFileSecurity;
pub(crate) mod launch;
mod process_attributes;
mod terminal;
pub(crate) use process_attributes::ProcessAttributeList;
pub use terminal::{NativeTerminal, NativeTerminalLaunch, TerminalOutputCallback, TerminalSize};
mod user_environment;
pub use user_environment::current_user_environment;
#[cfg(feature = "restricted-user-runtime")]
mod new_execution_file;
mod stdin;
pub use launch::{JobLaunchError, JobLaunchTiming, spawn_compat_process};
#[cfg(any(test, feature = "compat-adoption"))]
pub use launch::{JobLaunchFault, spawn_compat_process_with_fault};
use launch::{LaunchOverrides, spawn_restricted_process_inner};
pub use stdin::NativeProcessStdin;

use crate::{credential::SecretBytes, handle::OwnedHandle, probe_host_compatibility};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRestrictedMode {
    AppContainer,
    StandardUser,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAclAccess {
    Read,
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeEnvironmentEntry {
    pub name: String,
    pub value_utf16: Vec<u16>,
}

/// One post-authorization environment value. It deliberately has no Clone,
/// Serialize, or value-bearing Debug implementation, and both its UTF-8 input
/// and UTF-16 launch representation are zeroized by their owners.
pub struct NativeSecretEnvironmentEntry {
    name: String,
    value_utf16: Zeroizing<Vec<u16>>,
}

impl NativeSecretEnvironmentEntry {
    pub fn from_utf8_secret(name: String, secret: SecretBytes) -> io::Result<Self> {
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sandbox secret environment contains an invalid name",
            ));
        }
        let value = std::str::from_utf8(secret.expose()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "sandbox secret environment is not valid UTF-8",
            )
        })?;
        if value.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sandbox secret environment contains NUL",
            ));
        }
        let value_utf16 = Zeroizing::new(value.encode_utf16().collect());
        drop(secret);
        Ok(Self { name, value_utf16 })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Debug for NativeSecretEnvironmentEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeSecretEnvironmentEntry")
            .field("name", &self.name)
            .field("value", &"REDACTED")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowsSandboxProbe {
    pub appcontainer_api_available: bool,
    pub nested_jobs_available: bool,
    pub standard_user_token_available: bool,
    pub system_drive_compatibility_ready: bool,
    pub nul_compatibility_ready: bool,
    pub elevated_process: bool,
}

pub struct AppContainerProfile {
    name: String,
    sid: PSID,
    created: bool,
}

// SAFETY: `sid` is uniquely owned LocalAlloc memory. No other handle aliases it
// for the profile lifetime, so the profile may move across threads.
unsafe impl Send for AppContainerProfile {}
unsafe impl Sync for AppContainerProfile {}

impl AppContainerProfile {
    pub fn ensure(name: &str) -> io::Result<Self> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AppContainer profile name is invalid",
            ));
        }
        let name_wide = wide_null(name);
        let display = wide_null("Aura Sandbox");
        let description = wide_null("Aura isolated command execution profile");
        let mut sid: PSID = ptr::null_mut();
        // SAFETY: all strings are valid NUL-terminated UTF-16, capabilities are
        // intentionally empty, and `sid` is valid output storage.
        let created_result = unsafe {
            CreateAppContainerProfile(
                name_wide.as_ptr(),
                display.as_ptr(),
                description.as_ptr(),
                ptr::null(),
                0,
                &mut sid,
            )
        };
        let created = created_result >= 0;
        if !created {
            const HRESULT_ALREADY_EXISTS: i32 = 0x8007_00B7_u32 as i32;
            if created_result != HRESULT_ALREADY_EXISTS {
                return Err(hresult_error("create AppContainer profile", created_result));
            }
            // Only ERROR_ALREADY_EXISTS permits deriving the registered
            // profile's SID. Derivation alone does not prove that a profile
            // exists, so no other creation error may take this path.
            // SAFETY: name is valid and `sid` is writable output storage.
            let derived =
                unsafe { DeriveAppContainerSidFromAppContainerName(name_wide.as_ptr(), &mut sid) };
            if derived < 0 || sid.is_null() {
                return Err(hresult_error("create or derive AppContainer profile", created_result));
            }
        }
        if sid.is_null() {
            return Err(io::Error::other("AppContainer profile returned a null SID"));
        }
        Ok(Self { name: name.to_owned(), sid, created })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn was_created(&self) -> bool {
        self.created
    }

    pub fn sid_string(&self) -> io::Result<String> {
        sid_to_string(self.sid)
    }

    pub(crate) const fn sid(&self) -> PSID {
        self.sid
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        if !self.sid.is_null() {
            // SAFETY: the SID was allocated by an AppContainer profile API and
            // has not been freed elsewhere.
            unsafe {
                let _ = FreeSid(self.sid);
            }
        }
    }
}

pub fn delete_appcontainer_profile(name: &str) -> io::Result<()> {
    let encoded = wide_null(name);
    // SAFETY: `encoded` is a valid NUL-terminated profile name.
    let result = unsafe { DeleteAppContainerProfile(encoded.as_ptr()) };
    if result < 0 {
        return Err(hresult_error("delete AppContainer profile", result));
    }
    Ok(())
}

pub struct NativeSandboxProcess {
    process: OwnedHandle,
    _job: OwnedHandle,
    stdin_write: Option<OwnedHandle>,
    stdout_read: OwnedHandle,
    stderr_read: OwnedHandle,
    process_id: u32,
    executable: PathBuf,
}

impl NativeSandboxProcess {
    #[must_use]
    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn wait_timeout(&self, timeout: Duration) -> io::Result<Option<u32>> {
        let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
        // SAFETY: the process handle is owned and valid for the wait.
        match unsafe { WaitForSingleObject(self.process.as_raw(), timeout_ms) } {
            WAIT_OBJECT_0 => {
                let mut exit_code = 0_u32;
                // SAFETY: `exit_code` is writable and process is signaled.
                if unsafe { GetExitCodeProcess(self.process.as_raw(), &mut exit_code) } == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(Some(exit_code))
                }
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    pub fn terminate(&self, exit_code: u32) -> io::Result<()> {
        // Terminate the Execution Job rather than only its root process. This
        // makes timeout and cancellation terminal for the complete process
        // tree and does not depend on child cooperation.
        // SAFETY: the Job handle is owned and the result is checked.
        if unsafe { TerminateJobObject(self._job.as_raw(), exit_code) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Returns the number of processes that Windows still reports as active in
    /// this execution Job. A signaled root-process handle alone is not proof
    /// that descendants have stopped.
    pub fn active_processes_in_job(&self) -> io::Result<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let mut returned = 0_u32;
        // SAFETY: the Job handle is owned, `accounting` is a writable value of
        // exactly the requested information class, and the byte length is
        // checked before conversion.
        if unsafe {
            QueryInformationJobObject(
                self._job.as_raw(),
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

    /// Waits until the complete execution Job is empty. This is the terminal
    /// evidence used by the short-lived Tool runtime; it does not infer tree
    /// termination from the root process exit code.
    pub fn wait_for_process_tree_exit(&self, timeout: Duration) -> io::Result<bool> {
        let started = Instant::now();
        loop {
            if self.active_processes_in_job()? == 0 {
                return Ok(true);
            }
            let elapsed = started.elapsed();
            if elapsed >= timeout {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(10).min(timeout.saturating_sub(elapsed)));
        }
    }

    pub fn token_is_appcontainer(&self) -> io::Result<bool> {
        let token = process_token(self.process.as_raw())?;
        token_information_flag(&token, TokenIsAppContainer)
    }

    pub fn token_is_elevated(&self) -> io::Result<bool> {
        let token = process_token(self.process.as_raw())?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0_u32;
        // SAFETY: token is queryable and output exactly matches TokenElevation.
        if unsafe {
            GetTokenInformation(
                token.as_raw(),
                TokenElevation,
                (&raw mut elevation).cast(),
                u32::try_from(size_of::<TOKEN_ELEVATION>())
                    .map_err(|_| io::Error::other("token elevation size overflow"))?,
                &mut returned,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(elevation.TokenIsElevated != 0)
    }

    pub fn drain_stdout(&self, max_bytes: usize) -> io::Result<Vec<u8>> {
        read_available_pipe(&self.stdout_read, max_bytes)
    }

    pub fn drain_stderr(&self, max_bytes: usize) -> io::Result<Vec<u8>> {
        read_available_pipe(&self.stderr_read, max_bytes)
    }

    pub fn write_stdin(&self, bytes: &[u8]) -> io::Result<()> {
        let handle = self.stdin_write.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "sandbox stdin is already closed")
        })?;
        let mut written_total = 0_usize;
        while written_total < bytes.len() {
            let remaining = &bytes[written_total..];
            let chunk_length = u32::try_from(remaining.len()).unwrap_or(u32::MAX);
            let mut written = 0_u32;
            // SAFETY: the retained stdin pipe handle is valid, `remaining`
            // points to `chunk_length` readable bytes, and `written` is a
            // valid out-parameter for the synchronous operation.
            if unsafe {
                WriteFile(
                    handle.as_raw(),
                    remaining.as_ptr().cast(),
                    chunk_length,
                    &mut written,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "sandbox stdin accepted zero bytes",
                ));
            }
            written_total = written_total
                .checked_add(usize::try_from(written).map_err(|_| {
                    io::Error::other("sandbox stdin byte count cannot be represented")
                })?)
                .ok_or_else(|| io::Error::other("sandbox stdin byte count overflow"))?;
        }
        Ok(())
    }

    pub fn close_stdin(&mut self) {
        self.stdin_write.take();
    }
    pub fn take_stdin(&mut self) -> Option<NativeProcessStdin> {
        self.stdin_write.take().map(stdin::NativeProcessStdin)
    }
}

pub fn spawn_restricted_process(
    mode: NativeRestrictedMode,
    profile: Option<&AppContainerProfile>,
    executable: &Path,
    arguments: &[Vec<u16>],
    working_directory: &Path,
    environment: &[NativeEnvironmentEntry],
) -> io::Result<NativeSandboxProcess> {
    spawn_restricted_process_inner(
        mode,
        profile,
        executable,
        arguments,
        working_directory,
        environment,
        None,
        LaunchOverrides::default(),
    )
}

/// Current-user launch for an explicit NativeApproved backend. This still uses
/// a kill-on-close Job and Aura-owned environment; it never logs on a dedicated
/// account or inherits the caller's environment block.
pub fn spawn_standard_user_process(
    executable: &Path,
    arguments: &[Vec<u16>],
    working_directory: &Path,
    environment: &[NativeEnvironmentEntry],
    cmd_script: Option<&Path>,
) -> io::Result<NativeSandboxProcess> {
    spawn_restricted_process_inner(
        NativeRestrictedMode::StandardUser,
        None,
        executable,
        arguments,
        working_directory,
        environment,
        None,
        LaunchOverrides { cmd_script, ..LaunchOverrides::default() },
    )
}

pub fn spawn_restricted_process_with_secret_environment(
    mode: NativeRestrictedMode,
    profile: Option<&AppContainerProfile>,
    executable: &Path,
    arguments: &[Vec<u16>],
    working_directory: &Path,
    environment: &[NativeEnvironmentEntry],
    secret_environment: NativeSecretEnvironmentEntry,
) -> io::Result<NativeSandboxProcess> {
    spawn_restricted_process_inner(
        mode,
        profile,
        executable,
        arguments,
        working_directory,
        environment,
        Some(&secret_environment),
        LaunchOverrides::default(),
    )
}

pub(crate) fn create_inheritable_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
            .map_err(|_| io::Error::other("SECURITY_ATTRIBUTES size overflow"))?,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut read = ptr::null_mut();
    let mut write = ptr::null_mut();
    // SAFETY: output storage and security attributes are valid; ownership of
    // both returned handles is transferred into OwnedHandle immediately.
    if unsafe { CreatePipe(&mut read, &mut write, &raw const attributes, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let read = OwnedHandle::from_raw(read)?;
    let write = OwnedHandle::from_raw(write)?;
    Ok((read, write))
}

pub(crate) fn set_non_inheritable(handle: &OwnedHandle) -> io::Result<()> {
    // SAFETY: handle is valid and the documented inheritance flag is cleared.
    if unsafe { SetHandleInformation(handle.as_raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn read_available_pipe(handle: &OwnedHandle, max_bytes: usize) -> io::Result<Vec<u8>> {
    if max_bytes == 0 {
        return Ok(Vec::new());
    }
    let mut available = 0_u32;
    // SAFETY: handle is a live anonymous-pipe read endpoint and all optional
    // output pointers except available bytes are null as documented.
    if unsafe {
        PeekNamedPipe(
            handle.as_raw(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        )
    } == 0
    {
        // SAFETY: the error is read immediately after PeekNamedPipe failed.
        let error = unsafe { GetLastError() };
        if matches!(error, ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED) {
            return Ok(Vec::new());
        }
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    if available == 0 {
        return Ok(Vec::new());
    }
    let requested =
        usize::try_from(available).unwrap_or(usize::MAX).min(max_bytes).min(u32::MAX as usize);
    let mut bytes = vec![0_u8; requested];
    let mut read = 0_u32;
    // SAFETY: buffer is writable for requested bytes, handle has read access,
    // and available was observed immediately before this synchronous read.
    if unsafe {
        ReadFile(
            handle.as_raw(),
            bytes.as_mut_ptr(),
            u32::try_from(requested).unwrap_or(u32::MAX),
            &mut read,
            ptr::null_mut(),
        )
    } == 0
    {
        // SAFETY: the error is read immediately after ReadFile failed.
        let error = unsafe { GetLastError() };
        if matches!(error, ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED) {
            return Ok(Vec::new());
        }
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    bytes.truncate(usize::try_from(read).unwrap_or(0));
    Ok(bytes)
}

pub fn probe_windows_sandbox() -> WindowsSandboxProbe {
    // Unknown elevation state must never be treated as elevated; doing so
    // would silently skip the UAC boundary.
    let elevated_process = current_process_is_elevated().unwrap_or(false);
    let host_compatibility = probe_host_compatibility().ok();
    WindowsSandboxProbe {
        appcontainer_api_available: true,
        nested_jobs_available: create_kill_on_close_job().is_ok(),
        standard_user_token_available: standard_user_primary_token().is_ok(),
        system_drive_compatibility_ready: host_compatibility
            .as_ref()
            .is_some_and(|probe| probe.system_drive_ready),
        // The same readiness rule the launch gates and the Host Preparation plan
        // use, so a projection cannot report ready while a gate refuses (or the
        // other way round). An unverifiable installation answers "not ready".
        nul_compatibility_ready: crate::installation_identity::probe_null_device_readiness()
            .is_ready(),
        elevated_process,
    }
}

fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
    // SAFETY: null attributes/name create a private unnamed job.
    let job = OwnedHandle::from_raw(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
    // SAFETY: handle is valid and the inheritance mask is documented.
    if unsafe { SetHandleInformation(job.as_raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: information class, pointer, and size match.
    if unsafe {
        SetInformationJobObject(
            job.as_raw(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .map_err(|_| io::Error::other("job information size overflow"))?,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

pub fn current_process_is_elevated() -> io::Result<bool> {
    let token = current_process_token(TOKEN_QUERY)?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0_u32;
    // SAFETY: token is queryable and output buffer exactly matches the class.
    if unsafe {
        GetTokenInformation(
            token.as_raw(),
            TokenElevation,
            (&raw mut elevation).cast(),
            u32::try_from(size_of::<TOKEN_ELEVATION>())
                .map_err(|_| io::Error::other("token elevation size overflow"))?,
            &mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(elevation.TokenIsElevated != 0)
}

pub fn current_process_is_appcontainer() -> io::Result<bool> {
    let token = current_process_token(TOKEN_QUERY)?;
    token_information_flag(&token, TokenIsAppContainer)
}

fn standard_user_primary_token() -> io::Result<OwnedHandle> {
    let current = current_process_token(TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY)?;
    if current_process_is_elevated()? {
        let mut linked = TOKEN_LINKED_TOKEN::default();
        let mut returned = 0_u32;
        // SAFETY: token is queryable; linked token output has the documented
        // layout and ownership transfers to the caller.
        if unsafe {
            GetTokenInformation(
                current.as_raw(),
                TokenLinkedToken,
                (&raw mut linked).cast(),
                u32::try_from(size_of::<TOKEN_LINKED_TOKEN>())
                    .map_err(|_| io::Error::other("linked token size overflow"))?,
                &mut returned,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        return OwnedHandle::from_raw(linked.LinkedToken);
    }

    let mut duplicated = ptr::null_mut();
    // SAFETY: current token is valid, desired type is primary, and output
    // storage receives a new owned handle.
    if unsafe {
        DuplicateTokenEx(
            current.as_raw(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut duplicated,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    OwnedHandle::from_raw(duplicated)
}

use user_environment::current_process_token;

fn process_token(process: HANDLE) -> io::Result<OwnedHandle> {
    let mut token = ptr::null_mut();
    // SAFETY: process is a retained process handle and output receives one
    // query-only token handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    OwnedHandle::from_raw(token)
}

fn token_information_flag(token: &OwnedHandle, class: i32) -> io::Result<bool> {
    let mut value = 0_u32;
    let mut returned = 0_u32;
    // SAFETY: TokenIsAppContainer returns one DWORD and token is queryable.
    if unsafe {
        GetTokenInformation(
            token.as_raw(),
            class,
            (&raw mut value).cast(),
            u32::try_from(size_of::<u32>())
                .map_err(|_| io::Error::other("token flag size overflow"))?,
            &mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(value != 0)
}

pub fn capture_file_dacl(path: &Path) -> io::Result<String> {
    let path_wide = wide_null(&path.to_string_lossy());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: path is NUL-terminated and descriptor is valid output storage.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let descriptor_guard = LocalAllocation(descriptor.cast());
    let mut encoded: PWSTR = ptr::null_mut();
    let mut length = 0_u32;
    // SAFETY: descriptor is returned by GetNamedSecurityInfoW and output
    // pointers are writable. The resulting string is LocalAlloc-owned.
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut encoded,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let encoded_guard = LocalAllocation(encoded.cast());
    let result = wide_ptr_to_string(encoded)?;
    drop(encoded_guard);
    drop(descriptor_guard);
    Ok(result)
}

pub fn grant_file_dacl(
    path: &Path,
    sid: &str,
    access: NativeAclAccess,
    recursive: bool,
) -> io::Result<()> {
    let path_wide = wide_null(&path.to_string_lossy());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut current_dacl = ptr::null_mut();
    // SAFETY: path and output pointers satisfy GetNamedSecurityInfoW.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut current_dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor_guard = LocalAllocation(descriptor.cast());
    let permissions = FILE_GENERIC_READ
        | if access == NativeAclAccess::ReadWrite { FILE_GENERIC_WRITE } else { 0 };
    let (entry, _sid_guard) = explicit_access_for_sid(
        sid,
        permissions,
        GRANT_ACCESS,
        if recursive { SUB_CONTAINERS_AND_OBJECTS_INHERIT } else { 0 },
    )?;
    let mut new_dacl = ptr::null_mut();
    // SAFETY: entry and current DACL are valid for this call; new DACL is
    // LocalAlloc-owned and released below.
    let status = unsafe { SetEntriesInAclW(1, &entry, current_dacl, &mut new_dacl) };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _dacl_guard = LocalAllocation(new_dacl.cast());
    // SAFETY: path is valid and new DACL remains allocated for the call.
    let status = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            new_dacl,
            ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

pub fn revoke_file_dacl(path: &Path, sid: &str, recursive: bool) -> io::Result<()> {
    let path_wide = wide_null(&path.to_string_lossy());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut current_dacl = ptr::null_mut();
    // SAFETY: path and output pointers satisfy GetNamedSecurityInfoW.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut current_dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor_guard = LocalAllocation(descriptor.cast());
    // Each sandbox execution owns a unique AppContainer SID. REVOKE_ACCESS can
    // therefore remove only Aura's ACEs for that execution without rewriting
    // unrelated DACL entries that changed while the process was running.
    let (entry, _sid_guard) = explicit_access_for_sid(
        sid,
        0,
        REVOKE_ACCESS,
        if recursive { SUB_CONTAINERS_AND_OBJECTS_INHERIT } else { 0 },
    )?;
    let mut new_dacl = ptr::null_mut();
    // SAFETY: all ACL pointers are valid and new ACL is LocalAlloc-owned.
    let status = unsafe { SetEntriesInAclW(1, &entry, current_dacl, &mut new_dacl) };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _dacl_guard = LocalAllocation(new_dacl.cast());
    // SAFETY: path and new DACL remain valid for the call.
    let status = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            new_dacl,
            ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

pub fn restore_file_dacl(path: &Path, original_sddl: &str) -> io::Result<()> {
    let path_wide = wide_null(&path.to_string_lossy());
    let sddl_wide = wide_null(original_sddl);
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: SDDL is NUL-terminated and descriptor is writable output.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl_wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let _descriptor_guard = LocalAllocation(descriptor.cast());
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    // SAFETY: descriptor was parsed successfully and output storage is valid.
    if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    if present == 0 {
        return Err(io::Error::other("original security descriptor has no DACL"));
    }
    // SAFETY: path is valid and DACL points into the live descriptor.
    let status = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: pointer was allocated by a Win32 LocalAlloc-returning API.
            unsafe {
                let _ = LocalFree(self.0);
            }
        }
    }
}

fn explicit_access_for_sid(
    sid: &str,
    permissions: u32,
    access_mode: i32,
    inheritance: u32,
) -> io::Result<(EXPLICIT_ACCESS_W, LocalAllocation)> {
    let sid_wide = wide_null(sid);
    let mut sid_ptr: PSID = ptr::null_mut();
    // SAFETY: SID text is NUL-terminated and sid_ptr is writable. The returned
    // SID is LocalAlloc-owned and retained until SetEntriesInAclW completes.
    if unsafe { ConvertStringSidToSidW(sid_wide.as_ptr(), &mut sid_ptr) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid_guard = LocalAllocation(sid_ptr.cast());
    let mut entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: permissions,
        grfAccessMode: access_mode,
        grfInheritance: inheritance,
        ..EXPLICIT_ACCESS_W::default()
    };
    entry.Trustee.pMultipleTrustee = ptr::null_mut();
    entry.Trustee.MultipleTrusteeOperation = NO_MULTIPLE_TRUSTEE;
    entry.Trustee.TrusteeForm = TRUSTEE_IS_SID;
    entry.Trustee.TrusteeType = TRUSTEE_IS_UNKNOWN;
    entry.Trustee.ptstrName = sid_ptr.cast();
    Ok((entry, sid_guard))
}

fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut encoded: PWSTR = ptr::null_mut();
    // SAFETY: SID comes from an AppContainer API and output is writable.
    if unsafe { ConvertSidToStringSidW(sid, &mut encoded) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _guard = LocalAllocation(encoded.cast());
    wide_ptr_to_string(encoded)
}

fn wide_ptr_to_string(value: PWSTR) -> io::Result<String> {
    if value.is_null() {
        return Err(io::Error::other("Windows returned a null UTF-16 string"));
    }
    let mut length = 0_usize;
    // SAFETY: Windows returned a NUL-terminated UTF-16 string.
    unsafe {
        while *value.add(length) != 0 {
            length += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(value, length))
            .map_err(|_| io::Error::other("Windows returned invalid UTF-16"))
    }
}

fn quote_windows_argument_wide(argument: &[u16]) -> Vec<u16> {
    let quote = u16::from(b'"');
    let slash = u16::from(b'\\');
    if !argument.is_empty()
        && !argument
            .iter()
            .any(|unit| *unit == quote || *unit == u16::from(b' ') || *unit == u16::from(b'\t'))
    {
        return argument.to_vec();
    }
    let mut quoted = vec![quote];
    let mut backslashes = 0_usize;
    for unit in argument {
        if *unit == slash {
            backslashes += 1;
        } else {
            if *unit == quote {
                quoted.extend(std::iter::repeat_n(slash, backslashes * 2 + 1));
            } else {
                quoted.extend(std::iter::repeat_n(slash, backslashes));
            }
            backslashes = 0;
            quoted.push(*unit);
        }
    }
    quoted.extend(std::iter::repeat_n(slash, backslashes * 2));
    quoted.push(quote);
    quoted
}

pub(crate) fn environment_block(
    entries: &[NativeEnvironmentEntry],
    secret_entry: Option<&NativeSecretEnvironmentEntry>,
    working_directory: &Path,
) -> io::Result<Zeroizing<Vec<u16>>> {
    let mut entries = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.value_utf16.as_slice()))
        .collect::<Vec<_>>();
    if let Some(entry) = secret_entry {
        entries.push((entry.name.as_str(), entry.value_utf16.as_slice()));
    }
    entries.sort_by_key(|(name, _)| name.to_uppercase());
    let mut block = Zeroizing::new(Vec::new());
    if let Some(drive) = working_directory_drive(working_directory) {
        // A custom environment block does not inherit Windows' hidden
        // per-drive current-directory entries. CreateProcess documents that
        // callers must add them explicitly; normal plan entries remain unable
        // to inject names containing '='.
        block.extend([u16::from(b'='), u16::from(drive), u16::from(b':'), u16::from(b'=')]);
        block.extend(working_directory.as_os_str().encode_wide());
        block.push(0);
    }
    let mut previous = None::<String>;
    for (name, value_utf16) in entries {
        if name.is_empty() || name.contains(['=', '\0']) || value_utf16.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sandbox environment contains an invalid name or NUL",
            ));
        }
        let folded = name.to_uppercase();
        if previous.as_ref() == Some(&folded) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sandbox environment contains duplicate names",
            ));
        }
        previous = Some(folded);
        block.extend(name.encode_utf16());
        block.push(u16::from(b'='));
        block.extend(value_utf16);
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(block)
}

fn working_directory_drive(path: &Path) -> Option<u8> {
    let Component::Prefix(prefix) = path.components().next()? else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => Some(drive.to_ascii_uppercase()),
        _ => None,
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_os_null(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn hresult_error(action: &str, result: i32) -> io::Error {
    io::Error::other(format!("{action} failed with HRESULT 0x{:08x}", result as u32))
}

/// Bound CMD may be `\\?\C:\Windows\System32\cmd.exe` or a WinSxS hard link.
/// Path string equality against `GetWindowsDirectory()\System32\cmd.exe` is not
/// an identity check and would reject the published installation image.
fn is_windows_cmd_image(executable: &Path) -> io::Result<bool> {
    let system_cmd = crate::windows_directory()?.join("System32/cmd.exe");
    let system = crate::inspect_existing_path(&system_cmd, false)?;
    let candidate = crate::inspect_existing_path(executable, false)?;
    Ok(!candidate.is_directory
        && !candidate.is_reparse_point
        && !system.is_directory
        && !system.is_reparse_point
        && candidate.identity == system.identity)
}

/// `\\?\C:\...` VOLUME_NAME_DOS paths are rejected as UNC by CMD and mangle
/// PowerShell `$PWD` / FileSystem paths. Launch cwd, argv and environment
/// values must be ordinary drive paths of the same objects.
fn utf16_eq_ignore_ascii_case(left: &[u16], ascii: &str) -> bool {
    let right = ascii.encode_utf16().collect::<Vec<_>>();
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right).all(|(left_unit, right_unit)| {
        let Ok(left_byte) = u8::try_from(*left_unit) else {
            return false;
        };
        let Ok(right_byte) = u8::try_from(right_unit) else {
            return false;
        };
        left_byte.eq_ignore_ascii_case(&right_byte)
    })
}

/// PowerShell `-File \\?\C:\...ps1` is treated as UNC and leaves `$PWD` null.
fn rewrite_powershell_file_arguments(arguments: &[Vec<u16>]) -> io::Result<Vec<Vec<u16>>> {
    let mut rewritten = Vec::with_capacity(arguments.len());
    let mut file_operand = false;
    for argument in arguments {
        if file_operand {
            file_operand = false;
            let os = OsString::from_wide(argument);
            let converted = dos_drive_path(Path::new(&os))?;
            rewritten.push(converted.as_os_str().encode_wide().collect());
            continue;
        }
        if utf16_eq_ignore_ascii_case(argument, "-File") {
            file_operand = true;
        }
        rewritten.push(argument.clone());
    }
    if file_operand {
        return Err(io::Error::other("runner.missing_powershell_file_argument"));
    }
    Ok(rewritten)
}

pub fn dos_drive_path(path: &Path) -> io::Result<PathBuf> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(io::Error::other("runner.unsupported_drive_path"));
    };
    let (Prefix::Disk(drive) | Prefix::VerbatimDisk(drive)) = prefix.kind() else {
        return Err(io::Error::other("runner.unsupported_drive_path"));
    };
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(io::Error::other("runner.unsupported_drive_path"));
    }
    let mut converted = PathBuf::from(format!("{}:\\", drive.to_ascii_uppercase() as char));
    converted.extend(components);
    let original = crate::inspect_existing_path(path, false)?;
    let usable = crate::inspect_existing_path(&converted, false)?;
    if original.identity != usable.identity
        || original.is_reparse_point
        || usable.is_reparse_point
        || original.is_directory != usable.is_directory
    {
        return Err(io::Error::other("runner.drive_path_identity"));
    }
    Ok(converted)
}

#[derive(Debug)]
struct OsContextError {
    action: String,
    source: io::Error,
}

impl std::fmt::Display for OsContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.action, self.source)
    }
}

impl std::error::Error for OsContextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Names the platform requirement behind an `ERROR_ENVVAR_NOT_FOUND` from an
/// AppContainer launch, when the block is missing the variable that carries it.
///
/// Measured on this machine: creating an AppContainer child resolves
/// the container's local storage through `LOCALAPPDATA` **in the block it is handed**,
/// and a custom block replaces the parent's environment entirely. With a block that
/// lacked it, the launch failed with code 203 before the child image was considered —
/// with either `CreateProcess` or `CreateProcessAsUserW`, with and without an explicit
/// application name, and with `SystemRoot`, `windir`, `ComSpec`, `PATH`, `PATHEXT`,
/// `TEMP`, `TMP`, `USERPROFILE` and `APPDATA` all present. Adding `LOCALAPPDATA`
/// alone made the same launch succeed. The variable is a plan responsibility — the
/// production baseline in `aura-backend-windows-user` sets it to Aura's own work
/// directory — so this reports the requirement rather than inventing a value.
fn environment_requirement_hint(
    mode: NativeRestrictedMode,
    environment: &[NativeEnvironmentEntry],
    source: &io::Error,
) -> String {
    let missing_local_app_data =
        !environment.iter().any(|entry| entry.name.eq_ignore_ascii_case("LOCALAPPDATA"));
    if mode == NativeRestrictedMode::AppContainer
        && missing_local_app_data
        && source.raw_os_error() == Some(203)
    {
        " appcontainer_local_app_data_missing: ERROR_ENVVAR_NOT_FOUND (203) and this \
         plan's environment has no LOCALAPPDATA, which an AppContainer child is created \
         with; supply an Aura-owned value as the execution baseline does"
            .to_owned()
    } else {
        String::new()
    }
}

fn last_os_error_with_context(action: &str) -> io::Error {
    io::Error::other(OsContextError {
        action: action.to_owned(),
        source: io::Error::last_os_error(),
    })
}

/// Everything about one launch that its failure path reports.
struct LaunchFacts<'a> {
    mode: NativeRestrictedMode,
    /// Whether the calling process is elevated (which selects the token branch).
    elevated: bool,
    has_explicit_token: bool,
    has_standard_user_token: bool,
    application: &'a Path,
    directory: &'a Path,
    environment: &'a [NativeEnvironmentEntry],
    secret_environment: Option<&'a NativeSecretEnvironmentEntry>,
    appcontainer_profile: bool,
}

/// The launch described for its error path: API shape, token state, image,
/// working directory and the environment's **names**.
///
/// Values are deliberately excluded: the environment block can carry credential
/// material, and the question this answers — environment, image, token or startup
/// attribute? — is answered by the shape and the names alone.
fn launch_context(facts: LaunchFacts<'_>) -> String {
    let mut names = facts.environment.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();
    if let Some(entry) = facts.secret_environment {
        names.push(entry.name.as_str());
    }
    names.sort_unstable();
    format!(
        "mode={:?} caller_elevated={} explicit_token={} standard_user_token={} \
         appcontainer_profile={} image={} cwd={} env_names=[{}]",
        facts.mode,
        facts.elevated,
        facts.has_explicit_token,
        facts.has_standard_user_token,
        facts.appcontainer_profile,
        facts.application.display(),
        facts.directory.display(),
        names.join(","),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStrExt;

    #[test]
    fn job_tree_child_helper() {
        if std::env::var_os("AURA_JOB_TREE_HELPER").is_none() {
            return;
        }
        let pid_file =
            PathBuf::from(std::env::var_os("AURA_JOB_TREE_PID_FILE").expect("job tree PID file"));
        let mut ping = std::process::Command::new(r"C:\Windows\System32\ping.exe");
        let mut child = crate::test_spawn::spawn(ping.args(["-n", "30", "127.0.0.1"]))
            .expect("spawn job tree child");
        std::fs::write(pid_file, child.id().to_string()).expect("publish child PID");
        let _ = child.wait();
    }

    #[test]
    fn process_probe_never_claims_standard_user_when_token_cannot_be_created() {
        let probe = probe_windows_sandbox();
        assert!(probe.appcontainer_api_available);
        assert!(probe.nested_jobs_available);
        let host = probe_host_compatibility().expect("host compatibility probe");
        assert_eq!(probe.system_drive_compatibility_ready, host.system_drive_ready);
        // The projection must report exactly the rule the launch gates apply.
        assert_eq!(
            probe.nul_compatibility_ready,
            crate::installation_identity::probe_null_device_readiness().is_ready()
        );
    }

    #[test]
    fn captures_a_real_directory_dacl() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let sddl = capture_file_dacl(directory.path()).expect("capture DACL");
        assert!(sddl.starts_with("D:"));
    }

    #[test]
    fn managed_cmd_script_accepts_the_bound_final_path() {
        let system = crate::windows_directory().expect("Windows").join("System32/cmd.exe");
        let bound = crate::inspect_existing_path(&system, false).expect("inspect CMD");
        assert_ne!(
            bound.final_path.as_os_str(),
            system.as_os_str(),
            "VOLUME_NAME_DOS final paths must not be compared as GetWindowsDirectory strings"
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let script = directory.path().join("command.cmd");
        std::fs::write(&script, "@echo off\r\necho AURA-CMD-STABLE\r\n").expect("write CMD script");
        let bound_directory = crate::inspect_existing_path(directory.path(), false)
            .expect("inspect working directory");
        let bound_script =
            crate::inspect_existing_path(&script, false).expect("inspect CMD script");
        assert!(
            bound_directory.final_path.to_string_lossy().starts_with(r"\\?\"),
            "working directory must keep the VOLUME_NAME_DOS prefix before CMD launch"
        );
        let process = spawn_standard_user_process(
            &bound.final_path,
            &[],
            &bound_directory.final_path,
            &[],
            Some(bound_script.final_path.as_path()),
        )
        .expect("bound CMD final path must launch");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exited = false;
        for _ in 0..200 {
            stdout.extend(process.drain_stdout(64 * 1024).expect("drain stdout"));
            stderr.extend(process.drain_stderr(64 * 1024).expect("drain stderr"));
            if process.wait_timeout(Duration::from_millis(10)).expect("wait for command").is_some()
            {
                exited = true;
                break;
            }
        }
        stdout.extend(process.drain_stdout(64 * 1024).expect("final stdout"));
        stderr.extend(process.drain_stderr(64 * 1024).expect("final stderr"));
        assert!(exited, "managed CMD did not exit");
        assert!(String::from_utf8_lossy(&stdout).contains("AURA-CMD-STABLE"), "{stdout:?}");
        assert!(stderr.is_empty(), "{stderr:?}");
        let mut remaining = None;
        for _ in 0..50 {
            let active = process.active_processes_in_job().expect("query job");
            if active == 0 {
                remaining = Some(0);
                break;
            }
            remaining = Some(active);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(remaining, Some(0), "managed CMD must not leave a console host in the job");
    }

    #[test]
    fn managed_cmd_script_rejects_a_non_cmd_image() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let script = directory.path().join("command.cmd");
        std::fs::write(&script, "@echo off\r\n").expect("write CMD script");
        let error = match spawn_standard_user_process(
            Path::new(r"C:\Windows\System32\ping.exe"),
            &[],
            directory.path(),
            &[],
            Some(script.as_path()),
        ) {
            Ok(_) => panic!("non-CMD image must not take the managed script command line"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), "runner.invalid_cmd_image");
    }

    #[test]
    fn dos_drive_path_strips_the_volume_name_dos_prefix() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let bound =
            crate::inspect_existing_path(directory.path(), false).expect("inspect directory");
        let converted = dos_drive_path(&bound.final_path).expect("strip DOS prefix");
        assert!(!converted.to_string_lossy().starts_with(r"\\?\"), "{converted:?}");
        assert_eq!(
            crate::inspect_existing_path(&converted, false).expect("inspect converted").identity,
            bound.identity
        );
    }

    #[test]
    fn powershell_file_argument_strips_the_volume_name_dos_prefix() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let script = directory.path().join("command.ps1");
        std::fs::write(&script, "exit 0\n").expect("write script");
        let bound = crate::inspect_existing_path(&script, false).expect("inspect script");
        assert!(
            bound.final_path.to_string_lossy().starts_with(r"\\?\"),
            "VOLUME_NAME_DOS script path must keep the prefix before rewrite"
        );
        let arguments = [
            "-File".encode_utf16().collect::<Vec<_>>(),
            bound.final_path.as_os_str().encode_wide().collect(),
        ];
        let rewritten =
            rewrite_powershell_file_arguments(&arguments).expect("rewrite PowerShell -File");
        let path = String::from_utf16(&rewritten[1]).expect("utf16 path");
        assert!(!path.starts_with(r"\\?\"), "{path}");
        assert_eq!(
            crate::inspect_existing_path(Path::new(&path), false)
                .expect("inspect converted script")
                .identity,
            bound.identity
        );
    }

    #[test]
    fn managed_powershell_file_keeps_pwd_on_a_bound_final_path() {
        let pwsh = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/managed-runtimes/powershell-7.6.5-win-x64/payload/pwsh.exe");
        if !pwsh.is_file() {
            return;
        }
        let directory = tempfile::tempdir().expect("temporary directory");
        let script = directory.path().join("command.ps1");
        std::fs::write(
            &script,
            "if ($null -eq $PWD) { [Console]::Error.Write('PWD-NULL'); exit 2 }; [Console]::Out.Write(('PWD:' + $PWD.Path))\n",
        )
        .expect("write PowerShell script");
        let bound_directory = crate::inspect_existing_path(directory.path(), false)
            .expect("inspect working directory");
        let bound_script =
            crate::inspect_existing_path(&script, false).expect("inspect PowerShell script");
        let bound_executable =
            crate::inspect_existing_path(&pwsh, false).expect("inspect PowerShell executable");
        assert!(
            bound_script.final_path.to_string_lossy().starts_with(r"\\?\"),
            "script must keep the VOLUME_NAME_DOS prefix before launch"
        );
        let arguments = [
            "-MTA",
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ]
        .into_iter()
        .map(|value| value.encode_utf16().collect::<Vec<_>>())
        .chain(std::iter::once(bound_script.final_path.as_os_str().encode_wide().collect()))
        .collect::<Vec<_>>();
        let process = spawn_standard_user_process(
            &bound_executable.final_path,
            &arguments,
            &bound_directory.final_path,
            &[],
            None,
        )
        .expect("bound PowerShell -File must launch");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exited = false;
        for _ in 0..200 {
            stdout.extend(process.drain_stdout(64 * 1024).expect("drain stdout"));
            stderr.extend(process.drain_stderr(64 * 1024).expect("drain stderr"));
            if process.wait_timeout(Duration::from_millis(10)).expect("wait for command").is_some()
            {
                exited = true;
                break;
            }
        }
        stdout.extend(process.drain_stdout(64 * 1024).expect("final stdout"));
        stderr.extend(process.drain_stderr(64 * 1024).expect("final stderr"));
        assert!(exited, "managed PowerShell did not exit");
        let text = String::from_utf8_lossy(&stdout);
        assert!(text.contains("PWD:"), "{text:?} stderr={stderr:?}");
        assert!(!text.contains("PWD-NULL"), "{text:?}");
        assert!(!String::from_utf8_lossy(&stderr).contains("PWD-NULL"), "{stderr:?}");
    }

    #[test]
    fn restricted_process_captures_stdout_and_stderr_without_handle_leakage() {
        let executable = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        let arguments = ["/d", "/s", "/c", "echo aura-stdout & echo aura-stderr 1>&2"]
            .map(|value| value.encode_utf16().collect::<Vec<_>>());
        let process = spawn_restricted_process(
            NativeRestrictedMode::StandardUser,
            None,
            &executable,
            &arguments,
            Path::new(r"C:\Windows"),
            &[],
        )
        .expect("spawn restricted command");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exited = false;
        for _ in 0..200 {
            stdout.extend(process.drain_stdout(64 * 1024).expect("drain stdout"));
            stderr.extend(process.drain_stderr(64 * 1024).expect("drain stderr"));
            if process.wait_timeout(Duration::from_millis(10)).expect("wait for command").is_some()
            {
                exited = true;
                break;
            }
        }
        stdout.extend(process.drain_stdout(64 * 1024).expect("final stdout"));
        stderr.extend(process.drain_stderr(64 * 1024).expect("final stderr"));
        assert!(exited, "restricted command did not exit");
        assert!(String::from_utf8_lossy(&stdout).contains("aura-stdout"));
        assert!(String::from_utf8_lossy(&stderr).contains("aura-stderr"));
    }

    #[test]
    fn restricted_process_accepts_bound_stdin_and_observes_explicit_eof() {
        let executable = PathBuf::from(r"C:\Windows\System32\more.com");
        let mut process = spawn_restricted_process(
            NativeRestrictedMode::StandardUser,
            None,
            &executable,
            &[],
            Path::new(r"C:\Windows"),
            &[],
        )
        .expect("spawn stdin fixture");

        process.write_stdin(b"aura-stdin-fixture\r\n").expect("write stdin fixture");
        process.close_stdin();

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = None;
        for _ in 0..200 {
            stdout.extend(process.drain_stdout(64 * 1024).expect("drain stdout"));
            stderr.extend(process.drain_stderr(64 * 1024).expect("drain stderr"));
            if let Some(code) =
                process.wait_timeout(Duration::from_millis(10)).expect("wait for stdin fixture")
            {
                exit_code = Some(code);
                break;
            }
        }
        stdout.extend(process.drain_stdout(64 * 1024).expect("final stdout"));
        stderr.extend(process.drain_stderr(64 * 1024).expect("final stderr"));

        assert_eq!(
            exit_code,
            Some(0),
            "stdin fixture failed: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert!(String::from_utf8_lossy(&stdout).contains("aura-stdin-fixture"));
        assert!(
            matches!(process.write_stdin(b"late"), Err(error) if error.kind() == io::ErrorKind::BrokenPipe)
        );
    }

    #[test]
    fn terminating_execution_job_terminates_descendant_processes() {
        const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

        let fixture = tempfile::tempdir().expect("job tree fixture");
        let pid_file = fixture.path().join("child.pid");
        let executable = std::env::current_exe().expect("current test executable");
        let arguments = ["--exact", "sandbox::tests::job_tree_child_helper", "--nocapture"]
            .map(|value| value.encode_utf16().collect::<Vec<_>>());
        let environment = [
            NativeEnvironmentEntry {
                name: "AURA_JOB_TREE_HELPER".to_owned(),
                value_utf16: "1".encode_utf16().collect(),
            },
            NativeEnvironmentEntry {
                name: "AURA_JOB_TREE_PID_FILE".to_owned(),
                value_utf16: pid_file.as_os_str().encode_wide().collect(),
            },
            NativeEnvironmentEntry {
                name: "SystemRoot".to_owned(),
                value_utf16: r"C:\Windows".encode_utf16().collect(),
            },
        ];
        let process = spawn_restricted_process(
            NativeRestrictedMode::StandardUser,
            None,
            &executable,
            &arguments,
            Path::new(r"C:\Windows"),
            &environment,
        )
        .expect("spawn job tree root");

        let mut child_pid = None;
        for _ in 0..300 {
            child_pid = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok());
            if child_pid.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let child_pid = child_pid.unwrap_or_else(|| {
            let _ = process.terminate(1);
            panic!("job tree helper did not publish its child PID")
        });
        assert!(
            process.active_processes_in_job().expect("query active Job members") >= 2,
            "the execution Job did not account for both root and child processes"
        );
        // SAFETY: OpenProcess receives a concrete child PID and requests only
        // the wait right; the returned handle is immediately owned.
        let child = OwnedHandle::from_raw(unsafe {
            windows_sys::Win32::System::Threading::OpenProcess(SYNCHRONIZE_ACCESS, 0, child_pid)
        })
        .expect("open child process for synchronization");

        process.terminate(0xC000_013A).expect("terminate execution job");
        assert!(
            process.wait_timeout(Duration::from_secs(2)).expect("wait root").is_some(),
            "root process survived Execution Job termination"
        );
        assert!(
            process.wait_for_process_tree_exit(Duration::from_secs(2)).expect("wait job tree"),
            "Execution Job retained an active process after termination"
        );
        // SAFETY: child is a retained synchronization handle.
        let child_wait = unsafe { WaitForSingleObject(child.as_raw(), 2_000) };
        assert_eq!(child_wait, WAIT_OBJECT_0, "descendant survived Execution Job termination");
    }
}
