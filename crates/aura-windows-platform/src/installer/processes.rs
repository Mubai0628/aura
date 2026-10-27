//! Programs running from inside a folder, so an installer can ask before it replaces or removes
//! their files, and stop one of its own that nothing needs any more.
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
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        },
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunningProgram {
    pub process_id: u32,
    pub path: PathBuf,
}

/// The executable of the process `handle` was opened for with query access.
fn image_of(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<PathBuf> {
    let mut buffer = vec![0_u16; 32_768];
    let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: the buffer holds `length` units and the handle is open for querying.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut length)
    };
    (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
}

fn image_path(process_id: u32) -> Option<PathBuf> {
    // SAFETY: OpenProcess has no pointer arguments; a null handle means no access.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let image = image_of(handle);
    // SAFETY: the handle was opened above and is closed once.
    unsafe { CloseHandle(handle) };
    image
}

/// Ends `program`, but only while its process id still runs that same executable (an id is
/// reused once its process is gone), and waits up to five seconds for it to be gone, so the files
/// it held can be moved or removed afterwards.
pub fn stop_program(program: &RunningProgram) -> io::Result<()> {
    // SAFETY: OpenProcess has no pointer arguments; a null handle means no access.
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            program.process_id,
        )
    };
    if handle.is_null() {
        // Gone already, or never this caller's to stop.
        return if image_path(program.process_id).is_none() {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        };
    }
    let same = image_of(handle).is_some_and(|image| lowered(&image) == lowered(&program.path));
    let outcome = if same {
        // SAFETY: the handle is open with terminate and synchronize access.
        if unsafe { TerminateProcess(handle, 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            // SAFETY: as above; the wait is bounded.
            unsafe { WaitForSingleObject(handle, 5_000) };
            Ok(())
        }
    } else {
        Ok(())
    };
    // SAFETY: the handle was opened above and is closed once.
    unsafe { CloseHandle(handle) };
    outcome
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

    #[test]
    fn a_stopped_program_is_gone_and_its_folder_can_be_moved() {
        let folder = tempfile::tempdir().expect("folder");
        let inner = folder.path().join("0.1.6");
        std::fs::create_dir(&inner).expect("version folder");
        let program = inner.join("aura-wait.exe");
        std::fs::copy(r"C:\Windows\System32\PING.EXE", &program).expect("copy");
        // Started in its own folder, as Chrome starts a native-messaging host: the working
        // directory is what keeps the folder from being moved.
        let mut child = Command::new(&program)
            .args(["-n", "60", "127.0.0.1"])
            .current_dir(&inner)
            .spawn()
            .expect("start");
        let running = programs_running_in(&inner).expect("running");
        assert_eq!(running.len(), 1, "{running:?}");
        assert!(
            std::fs::rename(&inner, folder.path().join("aside")).is_err(),
            "a program working in the folder keeps it in place"
        );
        stop_program(&running[0]).expect("stopped");
        assert!(child.try_wait().expect("status").is_some(), "the program has ended");
        std::fs::rename(&inner, folder.path().join("aside")).expect("the folder moves now");
        // Its id may be reused by now: a second stop never ends another program.
        stop_program(&running[0]).expect("nothing left to stop");
    }
}
