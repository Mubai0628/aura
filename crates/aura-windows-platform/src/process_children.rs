//! Read-only child identities for observing owned process lifetimes, including ConPTY hosts.
use crate::{ProcessIdentity, handle::OwnedHandle, snapshot_process};
use std::{io, mem::size_of};
use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    },
};

pub fn child_process_identities(parent: u32) -> io::Result<Vec<ProcessIdentity>> {
    // SAFETY: owned snapshot, correctly sized writable record; bounded Win32 iteration.
    let snapshot =
        OwnedHandle::from_raw(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })?;
    let mut entry =
        PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut next = unsafe { Process32FirstW(snapshot.as_raw(), &mut entry) };
    let mut identities = Vec::new();
    while next != 0 {
        if entry.th32ParentProcessID == parent {
            match snapshot_process(entry.th32ProcessID) {
                Ok(identity) => identities.push(identity),
                Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {}
                Err(error) => return Err(error),
            }
        }
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        next = unsafe { Process32NextW(snapshot.as_raw(), &mut entry) };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(error);
    }
    Ok(identities)
}
