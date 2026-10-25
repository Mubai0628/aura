use std::{ffi::OsString, io, os::windows::ffi::OsStringExt, path::PathBuf};

use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;

/// Reads the Windows directory from the kernel instead of trusting inherited
/// process environment variables. Runtime authorization may use the returned
/// path as the root for system-supplied executables and baseline variables.
pub fn windows_directory() -> io::Result<PathBuf> {
    let mut buffer = vec![0_u16; 260];
    loop {
        let capacity = u32::try_from(buffer.len())
            .map_err(|_| io::Error::other("Windows directory path is too long"))?;
        // SAFETY: `buffer` is writable for `capacity` UTF-16 code units and
        // remains alive for the duration of the call.
        let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), capacity) };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(length)
            .map_err(|_| io::Error::other("Windows directory length is invalid"))?;
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        buffer.resize(
            length
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Windows directory length overflow"))?,
            0,
        );
    }
}

/// Canonical system location of Windows PowerShell 5.1. Existence and file
/// identity are verified separately by the Sandbox executable snapshot.
pub fn windows_powershell_executable() -> io::Result<PathBuf> {
    Ok(windows_directory()?.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
}

/// System-owned all-users application-data directory; no inherited environment
/// variable or model-supplied installation path participates in discovery.
pub fn common_application_data_directory() -> io::Result<PathBuf> {
    use windows_sys::Win32::UI::Shell::{
        CSIDL_COMMON_APPDATA, SHGFP_TYPE_CURRENT, SHGetFolderPathW,
    };
    let mut path = [0_u16; 260];
    let status = unsafe {
        SHGetFolderPathW(
            std::ptr::null_mut(),
            CSIDL_COMMON_APPDATA as i32,
            std::ptr::null_mut(),
            SHGFP_TYPE_CURRENT as u32,
            path.as_mut_ptr(),
        )
    };
    if status < 0 {
        return Err(io::Error::other("common application data unavailable"));
    }
    let len = path
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| io::Error::other("common application data path length"))?;
    Ok(PathBuf::from(OsString::from_wide(&path[..len])))
}
