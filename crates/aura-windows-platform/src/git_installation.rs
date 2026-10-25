//! Read-only native Git discovery. Never prepares the host or writes registry values.
use std::{io, path::PathBuf};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW},
};

pub fn machine_git_installation() -> io::Result<Option<PathBuf>> {
    let key = "SOFTWARE\\GitForWindows\0".encode_utf16().collect::<Vec<_>>();
    let value = "InstallPath\0".encode_utf16().collect::<Vec<_>>();
    let mut data = vec![0u16; 32768];
    let mut bytes = (data.len() * 2) as u32;
    // SAFETY: fixed read-only HKLM lookup, bounded writable buffer and byte count.
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if matches!(result, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) {
        return Ok(None);
    }
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    data.truncate((bytes as usize / 2).min(data.len()));
    while data.last() == Some(&0) {
        data.pop();
    }
    let path = PathBuf::from(String::from_utf16(&data).map_err(io::Error::other)?);
    if !path.is_absolute() {
        return Err(io::Error::other("git.installation_path_not_absolute"));
    }
    Ok(Some(path))
}
