//! Query the already authenticated Runner token. No impersonation, extra
//! logon, elevation, credential handling or file access is performed as it.
use crate::{ProcessIdentity, VerifiedFile, handle::OwnedHandle};
use std::{io, mem::size_of, os::windows::io::AsRawHandle, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, LUID},
    Security::{
        Authorization::*, DACL_SECURITY_INFORMATION, GetKernelObjectSecurity,
        OWNER_SECURITY_INFORMATION,
    },
    Storage::FileSystem::{FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL},
};
#[cfg(test)]
#[path = "execution_metadata_tests.rs"]
mod tests;

pub struct ExecutionMetadataAccess {
    context: AUTHZ_CLIENT_CONTEXT_HANDLE,
    manager: AUTHZ_RESOURCE_MANAGER_HANDLE,
    sid: String,
}

impl ExecutionMetadataAccess {
    pub fn for_runner(expected: &ProcessIdentity, account: &str) -> io::Result<Self> {
        Self::from_token(&crate::process::execution_process_token(expected)?, account)
    }

    fn from_token(token: &OwnedHandle, account: &str) -> io::Result<Self> {
        let sid = crate::sandbox::restricted_user::token_user_sid(token)?;
        if sid != account {
            return Err(io::Error::other("metadata Runner account mismatch"));
        }
        let mut value = Self { context: ptr::null_mut(), manager: ptr::null_mut(), sid };
        // Local token context, no auditing callbacks or remote/group lookup.
        if unsafe {
            AuthzInitializeResourceManager(
                AUTHZ_RM_FLAG_NO_AUDIT,
                None,
                None,
                None,
                ptr::null(),
                &mut value.manager,
            )
        } == 0
            || unsafe {
                AuthzInitializeContextFromToken(
                    0,
                    token.as_raw(),
                    value.manager,
                    ptr::null(),
                    LUID::default(),
                    ptr::null(),
                    &mut value.context,
                )
            } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(value)
    }

    pub fn account(&self) -> &str {
        &self.sid
    }

    /// Pin the ancestor identity without requesting write permissions. A
    /// directory already readable by this token needs no ACL mutation.
    pub fn pin_directory(&self, path: &Path) -> io::Result<(VerifiedFile, bool)> {
        let file = crate::path::open_native_with_desired_access(
            path,
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )?;
        if !file.info().is_directory || file.info().is_reparse_point {
            return Err(io::Error::other("metadata ancestor is not an unaliased directory"));
        }
        let mut bytes = 0;
        let flags = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        unsafe {
            GetKernelObjectSecurity(
                file.file().as_raw_handle().cast(),
                flags,
                ptr::null_mut(),
                0,
                &mut bytes,
            );
        }
        if bytes == 0 || bytes > 1024 * 1024 {
            return Err(io::Error::other("invalid ancestor descriptor size"));
        }
        let mut descriptor = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        if unsafe {
            GetKernelObjectSecurity(
                file.file().as_raw_handle().cast(),
                flags,
                descriptor.as_mut_ptr().cast(),
                bytes,
                &mut bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let allowed = self.allows_descriptor(descriptor.as_mut_ptr().cast())?;
        Ok((file, allowed))
    }

    pub fn allows_plan(&self, file: &VerifiedFile, dacl: &str) -> io::Result<bool> {
        let mut descriptor = ptr::null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.file().as_raw_handle().cast(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let mut owner = ptr::null_mut();
        let converted = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
            )
        };
        let error = io::Error::last_os_error();
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(descriptor);
        }
        if converted == 0 {
            return Err(error);
        }
        let mut length = 0;
        unsafe {
            while *owner.add(length) != 0 {
                length += 1;
            }
        }
        let prefix = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(owner, length) });
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(owner.cast());
        }
        let text: Vec<u16> = format!("{prefix}{dacl}").encode_utf16().chain(Some(0)).collect();
        let mut raw = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut raw,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let result = self.allows_descriptor(raw);
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(raw);
        }
        result
    }

    fn allows_descriptor(
        &self,
        raw: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
    ) -> io::Result<bool> {
        let request =
            AUTHZ_ACCESS_REQUEST { DesiredAccess: FILE_READ_ATTRIBUTES, ..Default::default() };
        let mut granted = 0;
        let mut error = 0;
        let mut sacl = 0;
        let mut reply = AUTHZ_ACCESS_REPLY {
            ResultListLength: 1,
            GrantedAccessMask: &mut granted,
            SaclEvaluationResults: &mut sacl,
            Error: &mut error,
        };
        if unsafe {
            AuthzAccessCheck(
                0,
                self.context,
                &request,
                ptr::null_mut(),
                raw,
                ptr::null(),
                0,
                &mut reply,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        match error {
            ERROR_SUCCESS => Ok(granted & FILE_READ_ATTRIBUTES != 0),
            ERROR_ACCESS_DENIED => Ok(false),
            _ => Err(io::Error::from_raw_os_error(error as i32)),
        }
    }
}
impl Drop for ExecutionMetadataAccess {
    fn drop(&mut self) {
        unsafe {
            if !self.context.is_null() {
                AuthzFreeContext(self.context);
            }
            if !self.manager.is_null() {
                AuthzFreeResourceManager(self.manager);
            }
        }
    }
}
