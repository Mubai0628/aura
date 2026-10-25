//! The installation's entry in Settings > Apps (HKCU\Software\Microsoft\Windows\CurrentVersion\
//! Uninstall\<key>): per user, written without administrator rights.
use std::{
    io,
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
    },
};

use super::wide_null;

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

pub struct UninstallEntry<'a> {
    pub key: &'a str,
    pub display_name: &'a str,
    pub display_version: &'a str,
    pub publisher: &'a str,
    pub install_location: &'a Path,
    pub display_icon: &'a Path,
    pub uninstall_command: &'a str,
    pub estimated_size_kb: u32,
}

pub fn write_uninstall_entry(entry: &UninstallEntry<'_>) -> io::Result<()> {
    write_entry(HKEY_CURRENT_USER, entry)
}

/// Removes the entry; `false` when there was none.
pub fn remove_uninstall_entry(key: &str) -> io::Result<bool> {
    remove_entry(HKEY_CURRENT_USER, key)
}

/// Where the entry says the installation is, when there is an entry.
pub fn read_uninstall_location(key: &str) -> io::Result<Option<PathBuf>> {
    read_location(HKEY_CURRENT_USER, key)
}

fn subkey(key: &str) -> io::Result<Vec<u16>> {
    if key.is_empty() || key.contains(['\\', '/']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "uninstall key must be one segment",
        ));
    }
    Ok(wide_null(&format!(r"{UNINSTALL}\{key}")))
}

fn check(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS { Ok(()) } else { Err(io::Error::from_raw_os_error(status as i32)) }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by RegCreateKeyExW and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    fn set_string(&self, name: &str, value: &str) -> io::Result<()> {
        let name = wide_null(name);
        let value = wide_null(value);
        let bytes = u32::try_from(value.len() * 2)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "registry value too long"))?;
        // SAFETY: the key is open for writing; name and data are live NUL-terminated buffers.
        check(unsafe {
            RegSetValueExW(self.0, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), bytes)
        })
    }

    fn set_dword(&self, name: &str, value: u32) -> io::Result<()> {
        let name = wide_null(name);
        let bytes = value.to_le_bytes();
        // SAFETY: the key is open for writing; the data buffer holds four bytes.
        check(unsafe { RegSetValueExW(self.0, name.as_ptr(), 0, REG_DWORD, bytes.as_ptr(), 4) })
    }
}

pub(crate) fn write_entry(root: HKEY, entry: &UninstallEntry<'_>) -> io::Result<()> {
    let path = subkey(entry.key)?;
    let mut handle: HKEY = ptr::null_mut();
    // SAFETY: the root is a predefined or open key and every pointer is live for the call.
    check(unsafe {
        RegCreateKeyExW(
            root,
            path.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            ptr::null(),
            &mut handle,
            ptr::null_mut(),
        )
    })?;
    let key = Key(handle);
    key.set_string("DisplayName", entry.display_name)?;
    key.set_string("DisplayVersion", entry.display_version)?;
    key.set_string("Publisher", entry.publisher)?;
    key.set_string("InstallLocation", &entry.install_location.to_string_lossy())?;
    key.set_string("DisplayIcon", &entry.display_icon.to_string_lossy())?;
    key.set_string("UninstallString", entry.uninstall_command)?;
    key.set_dword("EstimatedSize", entry.estimated_size_kb)?;
    key.set_dword("NoModify", 1)?;
    key.set_dword("NoRepair", 1)
}

pub(crate) fn remove_entry(root: HKEY, key: &str) -> io::Result<bool> {
    let path = subkey(key)?;
    // SAFETY: the root is a predefined or open key and the path is NUL-terminated.
    let status = unsafe { RegDeleteTreeW(root, path.as_ptr()) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    check(status).map(|()| true)
}

pub(crate) fn read_location(root: HKEY, key: &str) -> io::Result<Option<PathBuf>> {
    let path = subkey(key)?;
    let name = wide_null("InstallLocation");
    let mut buffer = vec![0_u16; 2_048];
    let mut bytes = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
    // SAFETY: the output buffer holds `bytes` bytes and every other pointer is live.
    let status = unsafe {
        RegGetValueW(
            root,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check(status)?;
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    let location = String::from_utf16(&buffer[..length])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "InstallLocation is not UTF-16"))?;
    Ok((!location.is_empty()).then(|| PathBuf::from(location)))
}

/// A registry hive in a file, opened for this process only: the tests write entries there and never
/// into the user's own registry.
#[cfg(test)]
pub(crate) struct AppHive(HKEY);

#[cfg(test)]
impl AppHive {
    pub(crate) fn open(file: &Path) -> io::Result<Self> {
        use windows_sys::Win32::System::Registry::{
            KEY_ALL_ACCESS, REG_PROCESS_APPKEY, RegLoadAppKeyW,
        };
        let file = wide_null(&file.to_string_lossy());
        let mut handle: HKEY = ptr::null_mut();
        // SAFETY: the path is NUL-terminated and the output pointer is live.
        check(unsafe {
            RegLoadAppKeyW(file.as_ptr(), &mut handle, KEY_ALL_ACCESS, REG_PROCESS_APPKEY, 0)
        })?;
        Ok(Self(handle))
    }

    pub(crate) fn key(&self) -> HKEY {
        self.0
    }
}

#[cfg(test)]
impl Drop for AppHive {
    fn drop(&mut self) {
        // SAFETY: the hive key was opened by RegLoadAppKeyW and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_is_written_read_back_and_removed() {
        let folder = tempfile::tempdir().expect("hive folder");
        let hive = AppHive::open(&folder.path().join("test.hive")).expect("app hive");
        let location = folder.path().join("中文 路径").join("Aura");
        let entry = UninstallEntry {
            key: "AuraTest",
            display_name: "Aura",
            display_version: "0.1.0",
            publisher: "Aura Project",
            install_location: &location,
            display_icon: &location.join("Aura.ico"),
            uninstall_command: "\"Uninstall Aura.exe\" --uninstall",
            estimated_size_kb: 1_024,
        };
        assert_eq!(read_location(hive.key(), "AuraTest").expect("read nothing"), None);
        write_entry(hive.key(), &entry).expect("write");
        assert_eq!(read_location(hive.key(), "AuraTest").expect("read"), Some(location.clone()));
        assert!(remove_entry(hive.key(), "AuraTest").expect("remove"));
        assert!(!remove_entry(hive.key(), "AuraTest").expect("remove again"));
        assert!(write_entry(hive.key(), &UninstallEntry { key: r"A\B", ..entry }).is_err());
    }
}
