use crate::handle::OwnedHandle;
use std::{io, mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::ConvertStringSidToSidW, DuplicateTokenEx, GetLengthSid, GetTokenInformation,
        SID_AND_ATTRIBUTES, SecurityImpersonation, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
        TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL, TOKEN_MANDATORY_POLICY,
        TOKEN_MANDATORY_POLICY_NO_WRITE_UP, TOKEN_QUERY, TokenIntegrityLevel, TokenMandatoryPolicy,
        TokenPrimary,
    },
    System::{
        SystemServices::SE_GROUP_INTEGRITY,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

pub(super) fn low_primary_token() -> io::Result<OwnedHandle> {
    let access = TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT;
    let mut raw = ptr::null_mut();
    // SAFETY: query/duplicate the calling Core token, never a user logon or external process.
    if unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let original = OwnedHandle::from_raw(raw)?;
    let mut raw = ptr::null_mut();
    // SAFETY: the resulting primary token is private, owned, and no privileges are enabled.
    if unsafe {
        DuplicateTokenEx(
            original.as_raw(),
            access,
            ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut raw,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle::from_raw(raw)?;
    let encoded = "S-1-16-4096\0".encode_utf16().collect::<Vec<_>>();
    let mut sid = ptr::null_mut();
    // SAFETY: NUL-terminated SID input, writable allocation output.
    if unsafe { ConvertStringSidToSidW(encoded.as_ptr(), &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES { Sid: sid, Attributes: SE_GROUP_INTEGRITY as u32 },
    };
    // SAFETY: the live label and its valid SID remain allocated throughout the call.
    let changed = unsafe {
        SetTokenInformation(
            token.as_raw(),
            TokenIntegrityLevel,
            (&raw const label).cast(),
            size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid),
        )
    };
    let error = io::Error::last_os_error();
    // SAFETY: ConvertStringSidToSidW returned a LocalAlloc allocation.
    unsafe {
        LocalFree(sid);
    }
    if changed == 0 {
        return Err(error);
    }
    let mut policy = TOKEN_MANDATORY_POLICY::default();
    let mut length = 0;
    // SAFETY: exact writable token information record and owned token handle.
    if unsafe {
        GetTokenInformation(
            token.as_raw(),
            TokenMandatoryPolicy,
            (&raw mut policy).cast(),
            size_of::<TOKEN_MANDATORY_POLICY>() as u32,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if policy.Policy & TOKEN_MANDATORY_POLICY_NO_WRITE_UP == 0 {
        return Err(io::Error::other(
            "git.read_isolation_unavailable: Core令牌未启用禁止向上写入策略",
        ));
    }
    Ok(token)
}
