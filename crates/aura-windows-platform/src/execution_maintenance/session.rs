use crate::{handle::OwnedHandle, sandbox::restricted_user::token_user_sid};
use std::{io, mem::size_of, ptr};
use windows_sys::Win32::{
    Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_STATISTICS, TokenStatistics},
    System::{
        RemoteDesktop::*,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

pub struct MaintenanceSession {
    pub(super) session: u32,
    pub(super) owner: String,
    pub(super) logon: String,
    _token: OwnedHandle,
}
impl MaintenanceSession {
    pub fn id(&self) -> u32 {
        self.session
    }
    pub fn logon(&self) -> &str {
        &self.logon
    }
    pub(super) fn revalidate(&self) -> io::Result<()> {
        let fresh = authenticated_session(self.session)?;
        if self.owner != fresh.owner || self.logon != fresh.logon {
            return Err(io::Error::other("maintenance.owner_session_changed"));
        }
        Ok(())
    }
}
fn logon(token: &OwnedHandle) -> io::Result<String> {
    let mut info = TOKEN_STATISTICS::default();
    let mut size = 0;
    // SAFETY: retained token and correctly sized native output.
    if unsafe {
        GetTokenInformation(
            token.as_raw(),
            TokenStatistics,
            (&raw mut info).cast(),
            size_of::<TOKEN_STATISTICS>() as u32,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(format!(
        "{:08x}{:08x}",
        info.AuthenticationId.HighPart as u32, info.AuthenticationId.LowPart
    ))
}
pub fn current_logon_identity() -> io::Result<String> {
    let mut raw = ptr::null_mut();
    // SAFETY: query only on the current process, handle closed by RAII.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    logon(&OwnedHandle::from_raw(raw)?)
}
fn authenticated_session(session: u32) -> io::Result<MaintenanceSession> {
    super::require_system()?;
    if session == 0 {
        return Err(io::Error::other("maintenance.interactive_session_required"));
    }
    let mut raw = ptr::null_mut();
    // SAFETY: WTS supplies the authoritative logged-on primary token. It is
    // never duplicated, inherited, leaked or used to launch a process.
    if unsafe { WTSQueryUserToken(session, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle::from_raw(raw)?;
    Ok(MaintenanceSession {
        session,
        owner: token_user_sid(&token)?,
        logon: logon(&token)?,
        _token: token,
    })
}
pub fn owner_sessions(owner: &str) -> io::Result<Vec<MaintenanceSession>> {
    super::require_system()?;
    let mut raw = ptr::null_mut();
    let mut count = 0;
    // SAFETY: local WTS enumeration with version 1; allocation is freed below.
    if unsafe { WTSEnumerateSessionsW(ptr::null_mut(), 0, 1, &mut raw, &mut count) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        if count > 1024 || count > 0 && raw.is_null() {
            return Err(io::Error::other("maintenance.session_inventory"));
        }
        let values = if count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(raw, count as usize) }
        };
        let mut output = Vec::new();
        for value in values {
            if value.SessionId == 0
                || ![WTSActive, WTSConnected, WTSDisconnected].contains(&value.State)
            {
                continue;
            }
            // Sessions disappearing between enumeration and token lookup are
            // skipped; no unverified session is eligible for a mutation.
            if let Ok(session) = authenticated_session(value.SessionId)
                && session.owner == owner
            {
                output.push(session);
            }
        }
        Ok(output)
    })();
    unsafe {
        WTSFreeMemory(raw.cast());
    }
    result
}
/// Stable boot GUID from the existing NT system-information interface. Failure
/// is unavailable, never inferred from wall time or caller-provided metadata.
pub fn boot_identity() -> io::Result<String> {
    use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
    #[repr(C)]
    struct BootEnvironment {
        identifier: windows_sys::core::GUID,
        firmware_type: u32,
        boot_flags: u64,
    }
    let mut value = BootEnvironment {
        identifier: windows_sys::core::GUID::from_u128(0),
        firmware_type: 0,
        boot_flags: 0,
    };
    let mut length = 0;
    // SAFETY: SYSTEM_BOOT_ENVIRONMENT_INFORMATION (90), exact aligned structure.
    let status = unsafe {
        NtQuerySystemInformation(
            90,
            (&raw mut value).cast(),
            size_of::<BootEnvironment>() as u32,
            &mut length,
        )
    };
    if status < 0 || length as usize > size_of::<BootEnvironment>() {
        return Err(io::Error::other(format!("maintenance.boot_identity:{status:#x}")));
    }
    let id = value.identifier;
    if id.data1 == 0 && id.data2 == 0 && id.data3 == 0 && id.data4 == [0; 8] {
        return Err(io::Error::other("maintenance.empty_boot_identity"));
    }
    Ok(format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        id.data1,
        id.data2,
        id.data3,
        id.data4[0],
        id.data4[1],
        id.data4[2],
        id.data4[3],
        id.data4[4],
        id.data4[5],
        id.data4[6],
        id.data4[7]
    ))
}
