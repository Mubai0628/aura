use std::{ffi::c_void, io, mem::size_of, ptr};

use windows_sys::{
    Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_GROUPS,
            TOKEN_QUERY, TokenLogonSid,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    },
    core::PWSTR,
};

use crate::handle::OwnedHandle;

pub(crate) struct ObjectSecurityDescriptor {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

impl ObjectSecurityDescriptor {
    #[cfg(feature = "restricted-user-runtime")]
    pub(crate) fn for_execution_job() -> io::Result<Self> {
        let (owner, _) = crate::current_user_token_identity()?;
        Self::from_sddl(&format!("O:{owner}D:P(A;;GA;;;SY)(A;;GA;;;{owner})"))
    }

    pub(crate) fn for_current_logon() -> io::Result<Self> {
        let logon_sid = current_logon_sid_string()?;
        let sddl = sidecar_pipe_sddl(&logon_sid);
        Self::from_sddl(&sddl)
    }

    #[cfg(feature = "restricted-user-runtime")]
    pub(crate) fn for_execution_account(account_sid: &str) -> io::Result<Self> {
        let valid = account_sid.strip_prefix("S-1-5-21-").is_some_and(|tail| {
            let values = tail.split('-').collect::<Vec<_>>();
            values.len() == 4
                && values
                    .iter()
                    .all(|v| v.parse::<u32>().is_ok_and(|n| n > 0 && n.to_string() == *v))
        });
        if !valid {
            return Err(io::Error::other("dedicated local account SID required"));
        }
        let owner = current_logon_sid_string()?;
        // Read/write data only, never FILE_CREATE_PIPE_INSTANCE, WRITE_DAC,
        // WRITE_OWNER or a shared token that other Core services will accept.
        Self::from_sddl(&format!(
            "D:P(A;;0x{SIDECAR_PIPE_ACCESS_MASK:08X};;;{owner})(A;;0x{SIDECAR_PIPE_ACCESS_MASK:08X};;;SY)(A;;0x{SIDECAR_PIPE_ACCESS_MASK:08X};;;{account_sid})"
        ))
    }

    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let encoded = wide_null(sddl);
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `encoded` is a valid NUL-terminated UTF-16 SDDL string and
        // `descriptor` points to writable storage. The returned allocation is
        // owned by this wrapper and released with LocalFree.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                    .map_err(|_| io::Error::other("SECURITY_ATTRIBUTES size overflow"))?,
                lpSecurityDescriptor: descriptor.cast::<c_void>(),
                bInheritHandle: 0,
            },
        })
    }

    pub(crate) fn attributes_ptr(&mut self) -> *mut c_void {
        (&mut self.attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>()
    }
}

// FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES |
// FILE_WRITE_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE. In particular this
// deliberately excludes FILE_CREATE_PIPE_INSTANCE (0x0004), which is folded
// into GENERIC_WRITE for named pipes and must never be granted to a client.
const SIDECAR_PIPE_ACCESS_MASK: u32 = 0x0012_0183;

fn sidecar_pipe_sddl(logon_sid: &str) -> String {
    format!(
        "D:P(A;;0x{SIDECAR_PIPE_ACCESS_MASK:08X};;;{logon_sid})(A;;0x{SIDECAR_PIPE_ACCESS_MASK:08X};;;SY)"
    )
}

impl Drop for ObjectSecurityDescriptor {
    fn drop(&mut self) {
        if !self.descriptor.is_null() {
            // SAFETY: the descriptor was allocated by
            // ConvertStringSecurityDescriptorToSecurityDescriptorW and has not
            // previously been freed.
            unsafe {
                let _ = LocalFree(self.descriptor.cast());
            }
        }
    }
}

#[cfg(feature = "restricted-user-runtime")]
pub(crate) fn verify_execution_job_owner(handle: HANDLE) -> io::Result<()> {
    use windows_sys::Win32::Security::{
        Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
        OWNER_SECURITY_INFORMATION,
    };
    let (expected, _) = crate::current_user_token_identity()?;
    let mut owner = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    // SAFETY: retained kernel object handle and initialized output pointers.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let result = (|| {
        if owner.is_null() {
            return Err(io::Error::other("execution Job owner missing"));
        }
        let mut text = ptr::null_mut();
        // SAFETY: owner belongs to the retained security descriptor.
        if unsafe { ConvertSidToStringSidW(owner, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let actual = wide_ptr_to_string(text);
        // SAFETY: Windows allocated the string with LocalAlloc.
        unsafe {
            let _ = LocalFree(text.cast());
        }
        if actual? != expected {
            return Err(io::Error::other("execution Job owner changed"));
        }
        Ok(())
    })();
    // SAFETY: GetSecurityInfo returned this LocalAlloc allocation.
    unsafe {
        let _ = LocalFree(descriptor.cast());
    }
    result
}

fn current_logon_sid_string() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo handle is valid in this process and `token` is a
    // writable output pointer. The resulting real token handle is owned below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle::from_raw(token)?;

    let mut required = 0_u32;
    // SAFETY: a null buffer with length zero is the documented sizing call.
    let sized = unsafe {
        GetTokenInformation(token.as_raw(), TokenLogonSid, ptr::null_mut(), 0, &mut required)
    };
    // SAFETY: GetLastError has no preconditions and is read immediately after
    // the failed sizing call.
    let sizing_error = unsafe { GetLastError() };
    if sized != 0 || sizing_error != ERROR_INSUFFICIENT_BUFFER || required == 0 {
        return Err(io::Error::from_raw_os_error(sizing_error as i32));
    }

    let word = size_of::<usize>();
    let required_usize = usize::try_from(required)
        .map_err(|_| io::Error::other("token information size overflow"))?;
    let mut aligned = vec![0_usize; required_usize.div_ceil(word)];
    // SAFETY: `aligned` is suitably aligned and has at least `required` bytes.
    if unsafe {
        GetTokenInformation(
            token.as_raw(),
            TokenLogonSid,
            aligned.as_mut_ptr().cast::<c_void>(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: TokenLogonSid returns TOKEN_GROUPS with one valid SID entry for
    // the lifetime of `aligned`.
    let groups = unsafe { &*aligned.as_ptr().cast::<TOKEN_GROUPS>() };
    if groups.GroupCount != 1 || groups.Groups[0].Sid.is_null() {
        return Err(io::Error::other("process token has no unique logon SID"));
    }

    let mut sid_text: PWSTR = ptr::null_mut();
    // SAFETY: the SID comes from a successful token query and `sid_text` is a
    // writable output pointer. Windows allocates the result with LocalAlloc.
    if unsafe { ConvertSidToStringSidW(groups.Groups[0].Sid, &mut sid_text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = wide_ptr_to_string(sid_text);
    // SAFETY: `sid_text` was allocated by ConvertSidToStringSidW.
    unsafe {
        let _ = LocalFree(sid_text.cast());
    }
    result
}

fn wide_ptr_to_string(value: PWSTR) -> io::Result<String> {
    if value.is_null() {
        return Err(io::Error::other("Windows returned a null string"));
    }
    let mut length = 0_usize;
    // SAFETY: Windows returned a valid NUL-terminated UTF-16 string.
    unsafe {
        while *value.add(length) != 0 {
            length += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(value, length))
            .map_err(|_| io::Error::other("Windows returned invalid UTF-16"))
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{SIDECAR_PIPE_ACCESS_MASK, sidecar_pipe_sddl};

    #[test]
    fn sidecar_pipe_acl_excludes_instance_creation_and_generic_rights() {
        let sddl = sidecar_pipe_sddl("S-1-5-5-1-2");
        assert_eq!(SIDECAR_PIPE_ACCESS_MASK & 0x0004, 0);
        assert!(!sddl.contains("GA"));
        assert!(!sddl.contains("GW"));
        assert!(sddl.contains("S-1-5-5-1-2"));
        assert!(sddl.contains(";;;SY"));
    }
}
