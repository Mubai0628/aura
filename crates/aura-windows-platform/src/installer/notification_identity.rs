//! The name Windows heads an installation's notifications with
//! (HKCU\Software\Classes\AppUserModelId\<identity>). Aura writes it when it starts; uninstalling
//! removes it. Written and removed without administrator rights.
use std::{
    io,
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::Win32::{
    Foundation::ERROR_FILE_NOT_FOUND,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, RRF_RT_REG_SZ, RegDeleteTreeW, RegGetValueW,
        RegOpenKeyExW,
    },
};

use super::uninstall_entry::{Key, check};
use super::wide_null;

const IDENTITIES: &str = r"Software\Classes\AppUserModelId";
/// The value the desktop app writes with the installation's icon.
const ICON_VALUE: &str = "IconUri";

/// The identity an installation's notifications are sent under; the desktop app sets the same.
pub fn notification_identity(namespace: &str) -> String {
    format!("Aura.{namespace}")
}

/// Removes the installation's notification name; `false` when there was none.
pub fn remove_notification_identity(namespace: &str) -> io::Result<bool> {
    remove_identity(HKEY_CURRENT_USER, namespace)
}

fn subkey(namespace: &str) -> io::Result<Vec<u16>> {
    // A namespace is lowercase letters, digits, dots and dashes; nothing else reaches the path.
    if namespace.is_empty()
        || !namespace.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "namespace is not valid"));
    }
    Ok(wide_null(&format!(r"{IDENTITIES}\{}", notification_identity(namespace))))
}

/// Points the installation's notification icon at `to` when it is inside `from`, after the
/// installation moved there; `false` when there is no identity or its icon is elsewhere.
pub fn relocate_notification_icon(namespace: &str, from: &Path, to: &Path) -> io::Result<bool> {
    relocate_icon(HKEY_CURRENT_USER, namespace, from, to)
}

pub(crate) fn relocate_icon(
    root: HKEY,
    namespace: &str,
    from: &Path,
    to: &Path,
) -> io::Result<bool> {
    let path = subkey(namespace)?;
    let name = wide_null(ICON_VALUE);
    let mut buffer = vec![0_u16; 2_048];
    let mut bytes = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
    // SAFETY: the output buffer holds `bytes` bytes and both names are NUL-terminated.
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
        return Ok(false);
    }
    check(status)?;
    let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    let icon = String::from_utf16_lossy(&buffer[..length]);
    let Some(moved) = moved_path(&icon, from, to) else { return Ok(false) };
    let mut handle: HKEY = ptr::null_mut();
    // SAFETY: the root is a predefined or open key and every pointer is live for the call.
    check(unsafe { RegOpenKeyExW(root, path.as_ptr(), 0, KEY_WRITE, &mut handle) })?;
    Key(handle).set_string(ICON_VALUE, &moved.to_string_lossy())?;
    Ok(true)
}

/// `path` with `from` replaced by `to` when it is inside `from`; Windows paths ignore case.
#[must_use]
pub fn moved_path(path: &str, from: &Path, to: &Path) -> Option<PathBuf> {
    let from = from.to_string_lossy();
    let from = from.trim_end_matches('\\');
    let head = path.get(..from.len())?;
    let rest = path.get(from.len()..)?;
    (head.eq_ignore_ascii_case(from) && rest.starts_with('\\'))
        .then(|| PathBuf::from(format!("{}{rest}", to.to_string_lossy().trim_end_matches('\\'))))
}

pub(crate) fn remove_identity(root: HKEY, namespace: &str) -> io::Result<bool> {
    let path = subkey(namespace)?;
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
    use std::ptr;
    use windows_sys::Win32::System::Registry::{
        KEY_WRITE, REG_OPTION_NON_VOLATILE, RegCloseKey, RegCreateKeyExW,
    };

    #[test]
    fn an_identity_is_removed_once_and_a_bad_namespace_is_refused() {
        let folder = tempfile::tempdir().expect("hive folder");
        let hive = AppHive::open(&folder.path().join("test.hive")).expect("app hive");
        let namespace = "aura.desktop.test.notify";
        assert!(!remove_identity(hive.key(), namespace).expect("nothing yet"));
        let path = subkey(namespace).expect("path");
        let mut handle: HKEY = ptr::null_mut();
        // SAFETY: the hive key is open and every pointer is live for the call.
        check(unsafe {
            RegCreateKeyExW(
                hive.key(),
                path.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                ptr::null(),
                &mut handle,
                ptr::null_mut(),
            )
        })
        .expect("create");
        // SAFETY: the handle was opened above and is closed once.
        unsafe { RegCloseKey(handle) };
        assert!(remove_identity(hive.key(), namespace).expect("remove"));
        assert!(!remove_identity(hive.key(), namespace).expect("remove again"));
        assert!(remove_identity(hive.key(), r"aura\..\x").is_err());
    }

    #[test]
    fn a_moved_installation_s_icon_follows_it_and_one_elsewhere_stays() {
        let folder = tempfile::tempdir().expect("hive folder");
        let hive = AppHive::open(&folder.path().join("test.hive")).expect("app hive");
        let namespace = "aura.desktop.test.notify";
        let (from, to) = (Path::new(r"C:\Old\Aura"), Path::new(r"D:\新的\Aura"));
        assert!(!relocate_icon(hive.key(), namespace, from, to).expect("no identity"));
        let path = subkey(namespace).expect("path");
        let mut handle: HKEY = ptr::null_mut();
        // SAFETY: the hive key is open and every pointer is live for the call.
        check(unsafe {
            RegCreateKeyExW(
                hive.key(),
                path.as_ptr(),
                0,
                ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                ptr::null(),
                &mut handle,
                ptr::null_mut(),
            )
        })
        .expect("create");
        Key(handle).set_string(ICON_VALUE, r"c:\old\aura\Aura.ico").expect("icon");
        assert!(relocate_icon(hive.key(), namespace, from, to).expect("moved"));
        assert!(!relocate_icon(hive.key(), namespace, from, to).expect("already moved"));
        assert_eq!(
            moved_path(r"C:\Old\Aura\Aura.ico", from, to),
            Some(PathBuf::from(r"D:\新的\Aura\Aura.ico"))
        );
        assert_eq!(moved_path(r"C:\Old\AuraX\Aura.ico", from, to), None);
        assert_eq!(moved_path(r"C:\Other\Aura.ico", from, to), None);
    }
}
