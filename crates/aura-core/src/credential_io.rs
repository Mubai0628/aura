//! Platform read adapter shared by the two independent credential authorities.
use aura_domain_common::CredentialRef;
use aura_windows_platform::{CredentialStoreError, StoredCredential, WindowsCredentialStore};

pub(crate) fn read(reference: &CredentialRef) -> Result<StoredCredential, CredentialStoreError> {
    #[cfg(test)]
    if let Some(code) = faults::error_for(reference) {
        return Err(CredentialStoreError::Windows(std::io::Error::from_raw_os_error(code)));
    }
    WindowsCredentialStore.read_record(reference)
}

// Inject failure only at the actual platform boundary, for one named temporary credential.
// These tests do not claim to change the host ACL or to observe a native Windows denial.
#[cfg(test)]
pub(crate) mod faults {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};
    thread_local! { static ERRORS: RefCell<BTreeMap<String, i32>> = const { RefCell::new(BTreeMap::new()) }; }
    pub(crate) struct Guard(String);
    pub(crate) fn inject(reference: &CredentialRef, code: i32) -> Guard {
        let key = reference.to_string();
        ERRORS.with_borrow_mut(|map| {
            assert!(map.insert(key.clone(), code).is_none());
        });
        Guard(key)
    }
    pub(super) fn error_for(reference: &CredentialRef) -> Option<i32> {
        ERRORS.with_borrow(|map| map.get(&reference.to_string()).copied())
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            ERRORS.with_borrow_mut(|map| {
                map.remove(&self.0);
            });
        }
    }
}
