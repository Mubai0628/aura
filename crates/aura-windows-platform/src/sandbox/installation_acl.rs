//! Read-only verification for the installation's protected file boundaries.
use super::*;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL_SIZE_INFORMATION, AclSizeInformation,
    Authorization::GetSecurityInfo, GetAce, GetAclInformation, GetSecurityDescriptorControl,
    OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
};
// Winnt.h ACE discriminants; no object/callback ACE is accepted here.
const ACCESS_ALLOWED_ACE_TYPE: u32 = 0;
const ACCESS_DENIED_ACE_TYPE: u32 = 1;

/// Only owner/System/Administrators may mutate installation files. `private`
/// additionally forbids other principals' read grants. Unknown ACE forms fail
/// closed. Read-only access is sufficient for the dedicated Runner to verify.
pub fn verify_execution_installation_acl(
    path: &Path,
    owner: &str,
    private: bool,
) -> io::Result<()> {
    verify_acl(path, Some(owner), private, true, false)
}
/// SYSTEM code/config directories and their ancestors cannot be controlled by
/// an unelevated owner. System roots may be TrustedInstaller-owned; ancestors
/// may allow creation of unrelated children but never replacing this boundary.
///
/// Gated like its only caller (the execution-maintenance path): the installation
/// check above is the one the always-available builds need.
#[cfg(feature = "restricted-user-runtime")]
pub fn verify_execution_maintenance_acl(path: &Path, boundary: bool) -> io::Result<()> {
    verify_acl(path, None, false, boundary, !boundary)
}
fn trusted_system_owner(sid: &str) -> bool {
    matches!(
        sid,
        "S-1-5-18"
            | "S-1-5-32-544"
            | "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
    )
}
fn verify_acl(
    path: &Path,
    owner: Option<&str>,
    private: bool,
    protected: bool,
    ancestor: bool,
) -> io::Result<()> {
    let file = crate::path::open_native_with_desired_access(
        path,
        0x0002_0000 | 0x80,
        false,
        windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
            | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
    )?;
    if file.info().is_reparse_point {
        return Err(io::Error::other("installation reparse boundary"));
    }
    let mut owner_sid = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    let status = unsafe {
        GetSecurityInfo(
            file.file().as_raw_handle().cast(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner_sid,
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
    if owner_sid.is_null() || dacl.is_null() {
        return Err(io::Error::other("installation explicit owner/DACL required"));
    }
    let actual_owner = sid_text(owner_sid)?;
    // UAC-created files use the token's default owner (Administrators).
    // The installation directory must still belong to the exact user. Files
    // may additionally belong to the two already trusted installer principals;
    // all their ACEs are checked below and the containing root stays pinned.
    if owner.map_or_else(
        || !trusted_system_owner(&actual_owner),
        |value| {
            actual_owner != value
                && (file.info().is_directory
                    || !matches!(actual_owner.as_str(), "S-1-5-18" | "S-1-5-32-544"))
        },
    ) {
        return Err(io::Error::other("installation owner changed"));
    }
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // Directories form the protected inheritance boundary. Files may inherit
    // only its already constrained entries, checked individually below.
    if protected && file.info().is_directory && control & SE_DACL_PROTECTED == 0 {
        return Err(io::Error::other("installation directory not protected"));
    }
    let mut size = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut size).cast(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    for index in 0..size.AceCount {
        let mut raw = ptr::null_mut();
        if unsafe { GetAce(dacl, index, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let header = unsafe { &*raw.cast::<ACE_HEADER>() };
        if header.AceType as u32 == ACCESS_DENIED_ACE_TYPE {
            continue;
        }
        if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE
            || (header.AceSize as usize) < size_of::<ACCESS_ALLOWED_ACE>()
        {
            return Err(io::Error::other("installation unsupported ACE"));
        }
        let ace = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = sid_text((&raw const ace.SidStart).cast_mut().cast())?;
        if owner.is_some_and(|value| sid == value)
            || matches!(sid.as_str(), "S-1-5-18" | "S-1-5-32-544")
            || owner.is_none() && trusted_system_owner(&sid)
        {
            continue;
        }
        // Data/EA/attribute/child writes, deletion, owner/DACL changes and
        // GENERIC_WRITE/ALL. Private metadata also has no untrusted read ACE.
        let unsafe_mask = if ancestor { 0x500d_0040 } else { 0x500d_0156 };
        // Inherit-only ACEs on a system ancestor (e.g. CREATOR OWNER) confer no
        // access to that ancestor. The concrete child is checked separately.
        if ancestor && header.AceFlags & 0x08 != 0 {
            continue;
        }
        if private && ace.Mask != 0 || ace.Mask & unsafe_mask != 0 {
            return Err(io::Error::other("installation untrusted file access"));
        }
    }
    Ok(())
}
fn sid_text(sid: PSID) -> io::Result<String> {
    let mut value = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut value) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _value = LocalAllocation(value.cast());
    wide_ptr_to_string(value)
}
