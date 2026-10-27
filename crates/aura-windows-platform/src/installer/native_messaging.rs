//! Chrome's per-user registration of a native-messaging host
//! (HKCU\Software\Google\Chrome\NativeMessagingHosts\<name>): its default value is the path of the
//! host's manifest file. Written and removed without administrator rights.
use std::{
    io,
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::ERROR_FILE_NOT_FOUND,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, RRF_RT_REG_SZ,
        RegCreateKeyExW, RegDeleteTreeW, RegGetValueW,
    },
};

use super::uninstall_entry::{Key, check};
use super::wide_null;

const HOSTS: &str = r"Software\Google\Chrome\NativeMessagingHosts";

/// Registers `name` for Chrome with the manifest at `manifest`.
pub fn write_chrome_native_host(name: &str, manifest: &Path) -> io::Result<()> {
    write_host(HKEY_CURRENT_USER, name, manifest)
}

/// The manifest Chrome finds for `name`, when it is registered.
pub fn read_chrome_native_host(name: &str) -> io::Result<Option<PathBuf>> {
    read_host(HKEY_CURRENT_USER, name)
}

/// Removes the registration; `false` when there was none.
pub fn remove_chrome_native_host(name: &str) -> io::Result<bool> {
    remove_host(HKEY_CURRENT_USER, name)
}

/// Chrome's rule for host names: lowercase letters, digits, `_` and `.`, not starting or ending
/// with `.` and without `..`.
fn subkey(name: &str) -> io::Result<Vec<u16>> {
    let valid = !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'.'
        })
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.contains("..");
    if !valid {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "native host name is not valid"));
    }
    Ok(wide_null(&format!(r"{HOSTS}\{name}")))
}

pub(crate) fn write_host(root: HKEY, name: &str, manifest: &Path) -> io::Result<()> {
    let path = subkey(name)?;
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
    Key(handle).set_string("", &manifest.to_string_lossy())
}

pub(crate) fn read_host(root: HKEY, name: &str) -> io::Result<Option<PathBuf>> {
    let path = subkey(name)?;
    let mut buffer = vec![0_u16; 2_048];
    let mut bytes = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
    // SAFETY: the output buffer holds `bytes` bytes; a null value name reads the default value.
    let status = unsafe {
        RegGetValueW(
            root,
            path.as_ptr(),
            ptr::null(),
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
    let manifest = String::from_utf16(&buffer[..length])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "manifest path is not UTF-16"))?;
    Ok((!manifest.is_empty()).then(|| PathBuf::from(manifest)))
}

pub(crate) fn remove_host(root: HKEY, name: &str) -> io::Result<bool> {
    let path = subkey(name)?;
    // SAFETY: the root is a predefined or open key and the path is NUL-terminated.
    let status = unsafe { RegDeleteTreeW(root, path.as_ptr()) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    check(status).map(|()| true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::uninstall_entry::AppHive;

    #[test]
    fn a_host_is_registered_read_back_and_removed_and_a_bad_name_is_refused() {
        let folder = tempfile::tempdir().expect("hive folder");
        let hive = AppHive::open(&folder.path().join("test.hive")).expect("app hive");
        let manifest = folder.path().join("中文 路径").join("com.aura.chrome_control.json");
        let name = "com.aura.chrome_control.t0123456789ab";
        assert_eq!(read_host(hive.key(), name).expect("nothing yet"), None);
        write_host(hive.key(), name, &manifest).expect("write");
        assert_eq!(read_host(hive.key(), name).expect("read"), Some(manifest.clone()));
        assert!(remove_host(hive.key(), name).expect("remove"));
        assert!(!remove_host(hive.key(), name).expect("remove again"));
        for bad in ["", "Com.Aura", r"a\b", ".a", "a.", "a..b", "a b"] {
            assert!(write_host(hive.key(), bad, &manifest).is_err(), "{bad}");
        }
    }
}
