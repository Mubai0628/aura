//! Programs running from inside a folder, so an installer can ask before it replaces or removes
//! their files.
use std::{
    io,
    mem::size_of,
    path::{Path, PathBuf},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunningProgram {
    pub process_id: u32,
    pub path: PathBuf,
}

fn image_path(process_id: u32) -> Option<PathBuf> {
    // SAFETY: OpenProcess has no pointer arguments; a null handle means no access.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0_u16; 32_768];
    let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: the buffer holds `length` units and the handle is open for querying.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut length)
    };
    // SAFETY: the handle was opened above and is closed once.
    unsafe { CloseHandle(handle) };
    (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
}

fn lowered(path: &Path) -> String {
    let text = dunce::simplified(path).to_string_lossy().to_lowercase();
    text.trim_end_matches(['\\', '/']).to_owned()
}

/// The programs, other than this one, whose executable is inside `folder`.
pub fn programs_running_in(folder: &Path) -> io::Result<Vec<RunningProgram>> {
    let folder = lowered(&dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf()));
    let prefix = format!("{folder}\\");
    // SAFETY: CreateToolhelp32Snapshot takes no pointers.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetCurrentProcessId has no preconditions.
    let own = unsafe { GetCurrentProcessId() };
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut found = Vec::new();
    // SAFETY: the snapshot is valid and `entry.dwSize` is set.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32ProcessID != own
            && let Some(path) = image_path(entry.th32ProcessID)
            && lowered(&path).starts_with(&prefix)
        {
            found.push(RunningProgram { process_id: entry.th32ProcessID, path });
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: the snapshot handle is closed once.
    unsafe { CloseHandle(snapshot) };
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn a_program_started_from_the_folder_is_found_and_others_are_not() {
        let folder = tempfile::tempdir().expect("folder");
        let program = folder.path().join("aura-wait.exe");
        std::fs::copy(r"C:\Windows\System32\PING.EXE", &program).expect("copy");
        assert!(programs_running_in(folder.path()).expect("before").is_empty());
        let mut child =
            Command::new(&program).args(["-n", "30", "127.0.0.1"]).spawn().expect("start");
        let running = programs_running_in(folder.path()).expect("during");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(running.len(), 1, "{running:?}");
        assert_eq!(running[0].process_id, child.id());
        let elsewhere = tempfile::tempdir().expect("other folder");
        assert!(programs_running_in(elsewhere.path()).expect("elsewhere").is_empty());
    }
}
