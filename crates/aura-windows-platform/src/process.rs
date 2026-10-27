use std::{io, os::windows::io::AsRawHandle, path::PathBuf};

use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};
use windows_sys::Win32::{
    Foundation::{FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW, WaitForSingleObject,
        },
    },
};

use crate::handle::OwnedHandle;

const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessIdentity {
    pub process_id: u32,
    pub creation_time_100ns: u64,
    pub executable_path: PathBuf,
    pub windows_session_id: u32,
}

pub struct RetainedProcess {
    identity: ProcessIdentity,
    handle: OwnedHandle,
}

impl RetainedProcess {
    #[must_use]
    pub const fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }

    pub fn is_running(&self) -> io::Result<bool> {
        // SAFETY: the retained process handle remains valid for this object's
        // lifetime and is opened with SYNCHRONIZE.
        match unsafe { WaitForSingleObject(self.handle.as_raw(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

pub fn snapshot_process(process_id: u32) -> io::Result<ProcessIdentity> {
    Ok(retain_process(process_id)?.identity)
}

#[cfg(feature = "restricted-user-runtime")]
pub fn process_account_sid(expected: &ProcessIdentity) -> io::Result<String> {
    crate::sandbox::restricted_user::token_user_sid(&execution_process_token(expected)?)
}

#[cfg(feature = "restricted-user-runtime")]
pub(crate) fn execution_process_token(expected: &ProcessIdentity) -> io::Result<OwnedHandle> {
    let process = retain_process(expected.process_id)?;
    if process.identity() != expected {
        return Err(io::Error::other("process identity changed"));
    }
    let mut token = std::ptr::null_mut();
    // SAFETY: retained process handle and initialized token output; query only.
    if unsafe {
        windows_sys::Win32::System::Threading::OpenProcessToken(
            process.handle.as_raw(),
            windows_sys::Win32::Security::TOKEN_QUERY,
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    OwnedHandle::from_raw(token)
}

pub fn retain_process(process_id: u32) -> io::Result<RetainedProcess> {
    // SAFETY: OpenProcess is called with a concrete PID and a non-inheritable
    // handle request. The returned handle is validated and owned immediately.
    let raw = unsafe {
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE_ACCESS, 0, process_id)
    };
    let process = OwnedHandle::from_raw(raw)?;
    let identity = snapshot_process_handle(process.as_raw(), process_id)?;
    Ok(RetainedProcess { identity, handle: process })
}

pub fn named_pipe_client_identity(pipe: &NamedPipeServer) -> io::Result<ProcessIdentity> {
    let mut process_id = 0_u32;
    // SAFETY: Tokio owns a connected server handle for this call and the
    // process_id output points to valid writable storage.
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut process_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    snapshot_process(process_id)
}

pub(crate) fn snapshot_process_handle(
    process: HANDLE,
    process_id: u32,
) -> io::Result<ProcessIdentity> {
    let creation_time_100ns = process_creation_time(process)?;
    let executable_path = process_image_path(process)?;
    let mut windows_session_id = 0_u32;
    // SAFETY: `windows_session_id` is valid writable storage.
    if unsafe { ProcessIdToSessionId(process_id, &mut windows_session_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ProcessIdentity { process_id, creation_time_100ns, executable_path, windows_session_id })
}

pub fn validate_named_pipe_client(
    pipe: &NamedPipeServer,
    expected: &ProcessIdentity,
) -> io::Result<ProcessIdentity> {
    let actual = named_pipe_client_identity(pipe)?;
    let same_path = actual
        .executable_path
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.executable_path.to_string_lossy());
    if actual.process_id != expected.process_id
        || actual.creation_time_100ns != expected.creation_time_100ns
        || actual.windows_session_id != expected.windows_session_id
        || !same_path
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "named pipe client identity did not match the Electron Main snapshot",
        ));
    }
    Ok(actual)
}

/// The process at the server end of a connected pipe, for a client that cannot know its process
/// in advance and checks its image and session instead.
pub fn named_pipe_server_identity(pipe: &NamedPipeClient) -> io::Result<ProcessIdentity> {
    let mut process_id = 0_u32;
    // SAFETY: Tokio owns a connected client handle for this call and `process_id` is writable
    // storage.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut process_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    snapshot_process(process_id)
}

pub fn validate_named_pipe_server(
    pipe: &NamedPipeClient,
    expected: &ProcessIdentity,
) -> io::Result<ProcessIdentity> {
    let mut process_id = 0_u32;
    // SAFETY: Tokio owns a connected client handle for this call and
    // `process_id` is writable storage.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut process_id) } == 0 {
        return Err(io::Error::last_os_error());
    }
    validate_process_identity(process_id, expected, "named pipe server")
}

fn validate_process_identity(
    process_id: u32,
    expected: &ProcessIdentity,
    label: &str,
) -> io::Result<ProcessIdentity> {
    let actual = snapshot_process(process_id)?;
    let same_path = actual
        .executable_path
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.executable_path.to_string_lossy());
    if actual.process_id != expected.process_id
        || actual.creation_time_100ns != expected.creation_time_100ns
        || actual.windows_session_id != expected.windows_session_id
        || !same_path
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{label} identity did not match the frozen process snapshot"),
        ));
    }
    Ok(actual)
}

fn process_creation_time(process: HANDLE) -> io::Result<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: all FILETIME output pointers refer to valid writable values.
    if unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

fn process_image_path(process: HANDLE) -> io::Result<PathBuf> {
    let mut buffer = vec![0_u16; 32_768];
    let mut length = u32::try_from(buffer.len())
        .map_err(|_| io::Error::other("process path buffer length overflow"))?;
    // SAFETY: `buffer` is writable for `length` UTF-16 units.
    if unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(
        usize::try_from(length).map_err(|_| io::Error::other("process path length overflow"))?,
    );
    Ok(PathBuf::from(
        String::from_utf16(&buffer)
            .map_err(|_| io::Error::other("process image path contains invalid UTF-16"))?,
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessWorkingSet {
    pub working_set_bytes: usize,
    pub peak_working_set_bytes: usize,
    /// Committed private bytes (`PagefileUsage`), which excludes shared file
    /// cache a scan may have mapped.
    pub private_bytes: usize,
}

pub fn current_process_working_set() -> io::Result<ProcessWorkingSet> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::{
        System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        System::Threading::GetCurrentProcess,
    };
    // SAFETY: `counters` is a valid PROCESS_MEMORY_COUNTERS and the current
    // process handle is valid for the duration of this call.
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS = zeroed();
        counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(
            GetCurrentProcess(),
            &raw mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(ProcessWorkingSet {
            working_set_bytes: counters.WorkingSetSize,
            peak_working_set_bytes: counters.PeakWorkingSetSize,
            private_bytes: counters.PagefileUsage,
        })
    }
}

/// How many threads this process currently owns, as the OS counts them.
///
/// A thread leak is invisible to every in-process counter and can hide behind
/// stable memory readings, so a long-running path that starts threads has to be
/// measured from outside its own bookkeeping. This is a read-only snapshot.
pub fn current_process_thread_count() -> io::Result<u32> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
    };
    // SAFETY: the snapshot handle is owned and closed by `OwnedHandle`, and the
    // entry walk is bounded by the API's own iteration.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = OwnedHandle::from_raw(snapshot)?;
        let mut entry: THREADENTRY32 = zeroed();
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        let own_process = std::process::id();
        let mut count = 0_u32;
        if Thread32First(snapshot.as_raw(), &raw mut entry) == 0 {
            return Err(io::Error::last_os_error());
        }
        loop {
            if entry.th32OwnerProcessID == own_process {
                count = count.saturating_add(1);
            }
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            if Thread32Next(snapshot.as_raw(), &raw mut entry) == 0 {
                break;
            }
        }
        Ok(count)
    }
}
