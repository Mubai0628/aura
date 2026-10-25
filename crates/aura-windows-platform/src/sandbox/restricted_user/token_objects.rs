//! Default ACL for objects created by this attempt's restricted token.
use super::*;
use windows_sys::Win32::Security::{SetTokenInformation, TOKEN_DEFAULT_DACL, TokenDefaultDacl};

pub(super) fn allow_attempt_objects(token: &OwnedHandle, sid: &str) -> io::Result<()> {
    let mut needed = 0;
    // SAFETY: sizing query on the newly created restricted token.
    unsafe {
        GetTokenInformation(token.as_raw(), TokenDefaultDacl, ptr::null_mut(), 0, &mut needed);
    }
    if needed < size_of::<TOKEN_DEFAULT_DACL>() as u32 {
        return Err(io::Error::other("invalid token default ACL size"));
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned buffer spans the requested structure and ACL bytes.
    if unsafe {
        GetTokenInformation(
            token.as_raw(),
            TokenDefaultDacl,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(last_os_error_with_context("R06 read token default ACL"));
    }
    let original = unsafe { &*buffer.as_ptr().cast::<TOKEN_DEFAULT_DACL>() };
    if original.DefaultDacl.is_null() {
        return Err(io::Error::other("refuse an unrestricted NULL default ACL"));
    }
    // Preserve creator/System entries. Add only this attempt's synthetic SID;
    // existing files and the parent token's default ACL are never modified.
    let (entry, _sid) = explicit_access_for_sid(sid, 0x1000_0000, GRANT_ACCESS, 0)?;
    let mut acl = ptr::null_mut();
    let status = unsafe { SetEntriesInAclW(1, &entry, original.DefaultDacl, &mut acl) };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _acl = LocalAllocation(acl.cast());
    let value = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    // SAFETY: SetTokenInformation copies this ACL into the child token.
    if unsafe {
        SetTokenInformation(
            token.as_raw(),
            TokenDefaultDacl,
            (&raw const value).cast(),
            size_of::<TOKEN_DEFAULT_DACL>() as u32,
        )
    } == 0
    {
        return Err(last_os_error_with_context("R06 set token default ACL"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::{GetLengthSid, TOKEN_GROUPS, TokenRestrictedSids};

    #[test]
    fn shared_capabilities_are_only_restricting_sids_not_default_object_grants() {
        let write = "S-1-5-21-1234-5678-9012-3456";
        let null_sid = crate::ExecutionDevice::Null
            .sid("073773f9-1798-4a46-a6d1-b8d5c9e3d19a")
            .expect("derive");
        let parent = current_process_token(TOKEN_QUERY).expect("parent");
        let before = default_acl(&parent);
        // Construct a token only: does not install or require a device ACE.
        let ksec_sid = crate::ExecutionDevice::Ksec
            .sid("073773f9-1798-4a46-a6d1-b8d5c9e3d19a")
            .expect("KsecDD SID");
        let namespace_sid =
            execution_namespace_sid("073773f9-1798-4a46-a6d1-b8d5c9e3d19a").expect("namespace SID");
        let token =
            create_write_token(write, &[null_sid.clone(), ksec_sid.clone(), namespace_sid.clone()])
                .expect("token");
        let mut needed = 0;
        unsafe {
            GetTokenInformation(
                token.as_raw(),
                TokenRestrictedSids,
                ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw(),
                    TokenRestrictedSids,
                    storage.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            },
            0
        );
        // SAFETY: successful query provides TOKEN_GROUPS and its inline array.
        let groups = unsafe { &*storage.as_ptr().cast::<TOKEN_GROUPS>() };
        let entries = unsafe {
            std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        };
        let mut sids =
            entries.iter().map(|v| sid_to_string(v.Sid).expect("SID")).collect::<Vec<_>>();
        sids.sort();
        let mut expected =
            vec![write.to_owned(), null_sid.clone(), ksec_sid.clone(), namespace_sid.clone()];
        expected.sort();
        assert_eq!(sids, expected, "no Everyone/account SID in restricting list");
        for device_sid in [&null_sid, &ksec_sid, &namespace_sid] {
            let (entry, _guard) =
                explicit_access_for_sid(device_sid, 0, GRANT_ACCESS, 0).expect("SID bytes");
            let sid: windows_sys::Win32::Security::PSID = entry.Trustee.ptstrName.cast();
            // SAFETY: converted SID remains owned by _guard.
            let bytes =
                unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), GetLengthSid(sid) as usize) };
            assert!(!default_acl(&token).windows(bytes.len()).any(|v| v == bytes));
        }
        assert_eq!(default_acl(&parent), before);
        assert!(create_write_token(&null_sid, std::slice::from_ref(&null_sid)).is_err());
        assert!(create_write_token(write, &[ksec_sid.clone(), ksec_sid]).is_err());
    }

    fn default_acl(token: &OwnedHandle) -> Vec<u8> {
        let mut needed = 0;
        // SAFETY: valid token, sizing query followed by aligned output storage.
        unsafe {
            GetTokenInformation(token.as_raw(), TokenDefaultDacl, ptr::null_mut(), 0, &mut needed);
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        assert_ne!(
            unsafe {
                GetTokenInformation(
                    token.as_raw(),
                    TokenDefaultDacl,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            },
            0
        );
        // SAFETY: successful query supplies the default ACL inside retained storage.
        let acl = unsafe { (*buffer.as_ptr().cast::<TOKEN_DEFAULT_DACL>()).DefaultDacl };
        assert!(!acl.is_null());
        unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize as usize) }.to_vec()
    }

    #[test]
    fn attempt_object_acl_changes_only_the_new_restricted_token() {
        let parent = current_process_token(TOKEN_QUERY).expect("parent token");
        let before = default_acl(&parent);
        let child =
            create_attempt_token("S-1-5-21-1234-5678-9012-3456", None).expect("attempt token");
        assert_eq!(default_acl(&parent), before, "parent token ACL must not change");
        assert_ne!(default_acl(&child), before, "attempt capability must enter new object ACLs");
        // SAFETY: retained child token supports query.
        assert_ne!(unsafe { IsTokenRestricted(child.as_raw()) }, 0);
    }
}
