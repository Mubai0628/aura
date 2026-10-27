//! The name Windows heads an installation's notifications with
//! (HKCU\Software\Classes\AppUserModelId\<identity>). Aura writes it when it starts; uninstalling
//! removes it. Written and removed without administrator rights.
use std::io;

use windows_sys::Win32::{
    Foundation::ERROR_FILE_NOT_FOUND,
    System::Registry::{HKEY, HKEY_CURRENT_USER, RegDeleteTreeW},
};

use super::uninstall_entry::check;
use super::wide_null;

const IDENTITIES: &str = r"Software\Classes\AppUserModelId";

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
}
