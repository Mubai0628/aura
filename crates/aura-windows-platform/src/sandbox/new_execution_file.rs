//! Retire attempt/account grants on newly created files without replacing any
//! unrelated ACE. Only confirmed attempt output, still owned by the attempt
//! account, passes to the trusted Controller; any other file keeps its owner
//! and every ACE that does not carry attempt authority.
use super::*;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, AddAce,
    Authorization::{GetSecurityInfo, SetSecurityInfo},
    GetAce, InitializeAcl, OWNER_SECURITY_INFORMATION,
};

impl ExecutionFileSecurity {
    pub fn retire_created_file(&self, account: &str, write_sid: &str) -> io::Result<()> {
        let path = &self.info().final_path;
        // Open only the already verified object with all rights this caller
        // actually has. A foreign/aliased object is never taken over.
        let file = crate::path::open_native_with_desired_access(
            path,
            0x0200_0000,
            false,
            windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE,
        )?;
        if file.info().identity != self.info().identity
            || file.info().is_reparse_point
            || (!file.info().is_directory && file.info().number_of_links != 1)
        {
            return Err(io::Error::other("new execution file identity changed"));
        }
        let handle = file.file().as_raw_handle().cast();
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let _descriptor = LocalAllocation(descriptor.cast());
        if dacl.is_null() || owner.is_null() {
            return Err(io::Error::other("new execution file null security"));
        }
        let (controller, _) = crate::current_user_token_identity()?;
        let actual_owner = sid_string(owner)?;
        // Only a file still owned by the attempt account is confirmed new
        // output and follows the recycle policy of passing ownership to the
        // Controller. Anything else keeps its owner and every unrelated ACE
        // exactly as they are; only attempt authority may be removed.
        let attempt_output = actual_owner == account;
        if attempt_output {
            let (entry, _sid) = explicit_access_for_sid(&controller, 0, GRANT_ACCESS, 0)?;
            let status = unsafe {
                SetSecurityInfo(
                    handle,
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION,
                    entry.Trustee.ptstrName.cast(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
        } else if actual_owner != controller {
            return Err(io::Error::other("new execution file owner changed"));
        }
        let old_acl = unsafe { &*dacl };
        let mut storage = vec![0usize; (old_acl.AclSize as usize).div_ceil(size_of::<usize>())];
        let next = storage.as_mut_ptr().cast();
        if unsafe { InitializeAcl(next, old_acl.AclSize as u32, old_acl.AclRevision as u32) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut stripped = false;
        for index in 0..old_acl.AceCount as u32 {
            let mut raw = ptr::null_mut();
            if unsafe { GetAce(dacl, index, &mut raw) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = unsafe { &*raw.cast::<ACE_HEADER>() };
            if header.AceType == 0 && header.AceSize as usize >= size_of::<ACCESS_ALLOWED_ACE>() {
                let allowed = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
                let sid = sid_string((&raw const allowed.SidStart).cast_mut().cast())?;
                if sid == account || sid == write_sid {
                    stripped = true;
                    continue;
                }
            }
            if unsafe {
                AddAce(next, old_acl.AclRevision as u32, u32::MAX, raw, header.AceSize as u32)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        if !attempt_output && !stripped {
            // Nothing on the file carries attempt authority, so its descriptor
            // is left alone instead of being observably rewritten.
            return Ok(());
        }
        // Persist a direct Controller owner grant on confirmed new output, so
        // a later parent inheritance refresh cannot leave it unreadable.
        let (final_acl, _final_guard) = if attempt_output {
            let (entry, _sid) = explicit_access_for_sid(&controller, 0x001f_01ff, GRANT_ACCESS, 0)?;
            let mut merged = ptr::null_mut();
            let status = unsafe { SetEntriesInAclW(1, &entry, next, &mut merged) };
            if status != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            (merged, Some(LocalAllocation(merged.cast())))
        } else {
            (next, None)
        };
        let status = unsafe {
            SetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                final_acl,
                ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        self.match_parent_inheritance()?;
        let captured = self.capture()?;
        if [account, write_sid].iter().any(|sid| captured.contains(&format!(";;;{sid})"))) {
            return Err(io::Error::other("new execution file grant retirement failed"));
        }
        Ok(())
    }
}
fn sid_string(sid: PSID) -> io::Result<String> {
    let mut text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _text = LocalAllocation(text.cast());
    wide_ptr_to_string(text)
}
