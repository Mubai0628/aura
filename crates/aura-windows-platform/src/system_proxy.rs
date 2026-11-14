//! Read-only lookup of the current user's Internet Settings proxy values. Never writes the
//! registry and never changes the system's proxy.
use std::io;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW},
};

const INTERNET_SETTINGS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings";

/// The values Windows keeps for the manual proxy and the automatic configuration script.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InternetSettingsValues {
    /// `ProxyEnable`: whether the manual proxy is switched on.
    pub proxy_enable: Option<u32>,
    /// `ProxyServer`: `host:port`, or `scheme=host:port` entries separated by `;`.
    pub proxy_server: Option<String>,
    /// `AutoConfigURL`: the automatic configuration (PAC) script.
    pub auto_config_url: Option<String>,
}

pub fn current_user_internet_settings() -> io::Result<InternetSettingsValues> {
    Ok(InternetSettingsValues {
        proxy_enable: read_dword("ProxyEnable")?,
        proxy_server: read_string("ProxyServer")?,
        auto_config_url: read_string("AutoConfigURL")?,
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn read_dword(name: &str) -> io::Result<Option<u32>> {
    let key = wide(INTERNET_SETTINGS);
    let value = wide(name);
    let mut data = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    // SAFETY: fixed read-only HKCU lookup into a four-byte buffer whose size is passed along.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut data).cast(),
            &mut bytes,
        )
    };
    match result {
        ERROR_SUCCESS => Ok(Some(data)),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
        other => Err(io::Error::from_raw_os_error(other as i32)),
    }
}

fn read_string(name: &str) -> io::Result<Option<String>> {
    let key = wide(INTERNET_SETTINGS);
    let value = wide(name);
    let mut data = vec![0u16; 4096];
    let mut bytes = (data.len() * 2) as u32;
    // SAFETY: fixed read-only HKCU lookup, bounded writable buffer and byte count.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    match result {
        ERROR_SUCCESS => {}
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => return Ok(None),
        other => return Err(io::Error::from_raw_os_error(other as i32)),
    }
    data.truncate((bytes as usize / 2).min(data.len()));
    while data.last() == Some(&0) {
        data.pop();
    }
    let text = String::from_utf16(&data).map_err(io::Error::other)?;
    Ok((!text.trim().is_empty()).then_some(text))
}
