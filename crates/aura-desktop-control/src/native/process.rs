//! The processes running now, each with its parent.
use crate::own::Process;
use std::collections::HashMap;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;

/// The Windows session a process runs in, when Windows says.
#[must_use]
pub fn session_of(process_id: u32) -> Option<u32> {
    let mut session = 0_u32;
    // SAFETY: Windows writes the session id into the local.
    unsafe { ProcessIdToSessionId(process_id, &raw mut session) }.ok().map(|()| session)
}

/// One snapshot of the processes, by id; empty when Windows gives none.
#[must_use]
pub fn processes() -> HashMap<u32, Process> {
    let mut found = HashMap::new();
    // SAFETY: the snapshot handle is closed here; each entry is read into the local below, whose
    // size field Windows requires.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return found;
        };
        let mut entry =
            PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut next = Process32FirstW(snapshot, &raw mut entry);
        while next.is_ok() {
            let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(260);
            found.insert(
                entry.th32ProcessID,
                Process {
                    parent: entry.th32ParentProcessID,
                    name: String::from_utf16_lossy(&entry.szExeFile[..length]),
                },
            );
            next = Process32NextW(snapshot, &raw mut entry);
        }
        let _ = CloseHandle(snapshot);
    }
    found
}
