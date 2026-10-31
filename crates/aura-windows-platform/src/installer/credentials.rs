//! The Credential Manager entries one Aura installation owns: those whose target starts with
//! `Aura/<namespace hash>/`. Entries outside that prefix (tool credentials such as `Aura/mcp/...`)
//! are shared by every Aura on the machine and are never touched here. Like every Credential
//! Manager call in this crate, these wait for the user's Credential Manager lock.
use std::{io, ptr, slice};

use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, GetLastError},
    Security::Credentials::{CREDENTIALW, CredDeleteW, CredEnumerateW, CredFree},
};

use super::wide_null;

/// `Aura/<32 lowercase hex>/`, the only prefix these functions accept.
fn checked(prefix: &str) -> io::Result<&str> {
    let hash = prefix.strip_prefix("Aura/").and_then(|rest| rest.strip_suffix('/'));
    match hash {
        Some(hash)
            if hash.len() == 32 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
        {
            Ok(prefix)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "credential prefix must be Aura/<namespace hash>/",
        )),
    }
}

struct Enumerated {
    list: *mut *mut CREDENTIALW,
    count: u32,
}

impl Drop for Enumerated {
    fn drop(&mut self) {
        // SAFETY: the list was allocated by CredEnumerateW and is freed once.
        unsafe { CredFree(self.list.cast()) };
    }
}

fn enumerate(prefix: &str) -> io::Result<Vec<(String, u32)>> {
    let filter = wide_null(&format!("{}*", checked(prefix)?));
    let _lock = crate::credential_lock::hold()?;
    let mut found = Enumerated { list: ptr::null_mut(), count: 0 };
    // SAFETY: the filter is NUL-terminated and the output pointers are live.
    if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut found.count, &mut found.list) } == 0 {
        // SAFETY: read immediately after the failed call.
        let error = unsafe { GetLastError() };
        std::mem::forget(found);
        return if error == ERROR_NOT_FOUND {
            Ok(Vec::new())
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        };
    }
    // SAFETY: CredEnumerateW returned `count` credential pointers.
    let entries = unsafe { slice::from_raw_parts(found.list, found.count as usize) };
    let mut targets = Vec::with_capacity(entries.len());
    for entry in entries {
        // SAFETY: every returned pointer refers to a credential that lives until CredFree.
        let credential = unsafe { &**entry };
        if credential.TargetName.is_null() {
            continue;
        }
        // SAFETY: TargetName is a NUL-terminated string owned by the credential.
        let length =
            (0..).take_while(|&index| unsafe { *credential.TargetName.add(index) } != 0).count();
        // SAFETY: `length` units precede the terminator.
        let name = String::from_utf16_lossy(unsafe {
            slice::from_raw_parts(credential.TargetName, length)
        });
        if name.starts_with(prefix) {
            targets.push((name, credential.Type));
        }
    }
    Ok(targets)
}

/// The targets of the installation's credentials.
pub fn installation_credentials(prefix: &str) -> io::Result<Vec<String>> {
    Ok(enumerate(prefix)?.into_iter().map(|(name, _)| name).collect())
}

/// Deletes the installation's credentials and returns how many went.
pub fn delete_installation_credentials(prefix: &str) -> io::Result<usize> {
    // One hold for the whole pass, so no other Aura call lands between the listing and the deletes.
    let _lock = crate::credential_lock::hold()?;
    let mut deleted = 0;
    for (name, kind) in enumerate(prefix)? {
        let target = wide_null(&name);
        // SAFETY: the target is NUL-terminated.
        if unsafe { CredDeleteW(target.as_ptr(), kind, 0) } != 0 {
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CredentialStoreError, SecretBytes, WindowsCredentialStore};
    use aura_domain_common::CredentialRef;

    #[test]
    fn only_a_namespace_prefix_is_accepted() {
        assert!(checked("Aura/").is_err());
        assert!(checked("Aura/mcp/").is_err());
        assert!(checked("Aura/0123456789abcdef0123456789abcdef").is_err());
        assert!(checked("Aura/0123456789ABCDEF0123456789ABCDEF/").is_err());
        assert!(checked("Aura/0123456789abcdef0123456789abcdef/").is_ok());
    }

    #[test]
    fn the_installations_own_credentials_are_listed_and_deleted_and_no_others()
    -> Result<(), CredentialStoreError> {
        let mine: String = format!("{:032x}", rand_suffix());
        let other: String = format!("{:032x}", rand_suffix() ^ 1);
        let reference = |hash: &str, name: &str| -> CredentialRef {
            format!("wincred://aura/{hash}/credential/{name}").parse().expect("reference")
        };
        let store = WindowsCredentialStore;
        let written = [reference(&mine, "one"), reference(&mine, "two"), reference(&other, "one")];
        for credential in &written {
            store.write(credential, &SecretBytes::new(b"test secret".to_vec()))?;
        }
        let prefix = format!("Aura/{mine}/");
        let mut listed = installation_credentials(&prefix).expect("list");
        listed.sort();
        assert_eq!(listed, [format!("{prefix}credential/one"), format!("{prefix}credential/two")]);
        assert_eq!(delete_installation_credentials(&prefix).expect("delete"), 2);
        assert!(installation_credentials(&prefix).expect("list again").is_empty());
        assert!(store.delete(&written[2])?, "another installation's credential was deleted");
        Ok(())
    }

    fn rand_suffix() -> u128 {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).expect("random");
        u128::from_le_bytes(bytes)
    }
}
