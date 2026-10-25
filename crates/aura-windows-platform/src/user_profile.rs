//! Production profile discovery is a Windows known-folder lookup. Environment
//! variables (including USERPROFILE) cannot redirect Aura's global Memory Vault.
use std::{ffi::OsString, io, os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::Win32::{
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_Profile, SHGetKnownFolderPath},
};

pub fn user_profile_directory() -> io::Result<PathBuf> {
    let mut buffer = std::ptr::null_mut();
    // SAFETY: Windows allocates the null-terminated result; it is copied before
    // CoTaskMemFree and the allocation is never exposed to safe callers.
    let status =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Profile, 0, std::ptr::null_mut(), &mut buffer) };
    if status < 0 || buffer.is_null() {
        return Err(io::Error::other("Windows profile known folder unavailable"));
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
        return Err(io::Error::other("Windows profile known folder is not absolute"));
    }
    Ok(path)
}
