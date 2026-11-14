//! Production profile discovery is a Windows known-folder lookup. Environment
//! variables (including USERPROFILE) cannot redirect Aura's global Memory Vault.
use std::{ffi::OsString, io, os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::{
    Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_LocalAppData, FOLDERID_Profile, SHGetKnownFolderPath},
    },
    core::GUID,
};

pub fn user_profile_directory() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_Profile, "Windows profile known folder")
}

/// The user's local application data folder as Windows knows it. `LOCALAPPDATA` does not change
/// it, so a process started with a `LOCALAPPDATA` of its own still finds the installed product's
/// records there.
pub fn local_app_data_directory() -> io::Result<PathBuf> {
    known_folder(&FOLDERID_LocalAppData, "Windows local application data known folder")
}

fn known_folder(id: &GUID, name: &str) -> io::Result<PathBuf> {
    let mut buffer = std::ptr::null_mut();
    // SAFETY: Windows allocates the null-terminated result; it is copied before
    // CoTaskMemFree and the allocation is never exposed to safe callers.
    let status = unsafe { SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut buffer) };
    if status < 0 || buffer.is_null() {
        // SAFETY: the shell may return an allocation even on failure; null is allowed.
        unsafe { CoTaskMemFree(buffer.cast()) };
        return Err(io::Error::other(format!("{name} unavailable")));
    }
    let path = unsafe {
        let mut len = 0;
        while *buffer.add(len) != 0 {
            len += 1;
        }
        let path = PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(buffer, len)));
        CoTaskMemFree(buffer.cast());
        path
    };
    if !path.is_absolute() {
        return Err(io::Error::other(format!("{name} is not absolute")));
    }
    Ok(path)
}
