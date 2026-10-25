use super::MaintenanceSession;
use crate::{ExecutionDevice, handle::OwnedHandle};
use std::{ffi::c_void, io, mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        },
        DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, SetKernelObjectSecurity,
    },
};

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: *mut c_void,
    name: *const UnicodeString,
    attributes: u32,
    descriptor: *mut c_void,
    qos: *mut c_void,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenDirectoryObject(
        handle: *mut *mut c_void,
        access: u32,
        attributes: *const ObjectAttributes,
    ) -> i32;
}

pub struct MaintenanceNamespace<'a> {
    session: &'a MaintenanceSession,
    directory: OwnedHandle,
    parent: OwnedHandle,
    parent_dacl: String,
}
impl MaintenanceSession {
    pub fn namespace(&self) -> io::Result<MaintenanceNamespace<'_>> {
        self.revalidate()?;
        let parent = open(self.id(), true, 0x20000)?;
        let parent_dacl = capture(&parent)?;
        let directory = open(self.id(), false, 0x60000)?;
        Ok(MaintenanceNamespace { session: self, directory, parent, parent_dacl })
    }
}
impl MaintenanceNamespace<'_> {
    pub fn capture(&self) -> io::Result<String> {
        capture(&self.directory)
    }
    pub fn parent_dacl(&self) -> &str {
        &self.parent_dacl
    }
    pub fn apply(
        &self,
        id: &str,
        accounts: &[String],
        expected: &str,
        target: &str,
        install: bool,
    ) -> io::Result<()> {
        self.session.revalidate()?;
        if capture(&self.parent)? != self.parent_dacl {
            return Err(io::Error::other("maintenance.parent_changed"));
        }
        if namespace_plan(id, accounts, if install { expected } else { target })?
            != if install { target } else { expected }
        {
            return Err(io::Error::other("maintenance.namespace_transform"));
        }
        let observed = self.capture()?;
        if observed == target {
            return Ok(());
        }
        if observed != expected {
            return Err(io::Error::other("maintenance.namespace_drift"));
        }
        let wide = target.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut raw = ptr::null_mut();
        // SAFETY: exact planned DACL, no owner/SACL/label writes.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut raw,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ok = unsafe {
            SetKernelObjectSecurity(self.directory.as_raw(), DACL_SECURITY_INFORMATION, raw)
        };
        let error = io::Error::last_os_error();
        unsafe {
            LocalFree(raw.cast());
        }
        if ok == 0 {
            return Err(error);
        }
        if self.capture()? != target {
            return Err(io::Error::other("maintenance.namespace_postcondition"));
        }
        Ok(())
    }
}
fn open(session: u32, parent: bool, access: u32) -> io::Result<OwnedHandle> {
    let path = if parent {
        format!(r"\Sessions\{session}\BaseNamedObjects")
    } else {
        format!(r"\Sessions\{session}\BaseNamedObjects\Restricted")
    };
    let name = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let unicode = UnicodeString {
        length: ((name.len() - 1) * 2) as u16,
        maximum_length: (name.len() * 2) as u16,
        buffer: name.as_ptr(),
    };
    let attrs = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: ptr::null_mut(),
        name: &unicode,
        attributes: 0x40,
        descriptor: ptr::null_mut(),
        qos: ptr::null_mut(),
    };
    let mut raw = ptr::null_mut();
    // SAFETY: only a WTS-verified owner session can construct this path. Handles
    // pin both objects across comparison/write; no supplied object name exists.
    let status = unsafe { NtOpenDirectoryObject(&mut raw, access, &attrs) };
    if status < 0 {
        // SAFETY: deterministic NTSTATUS-to-Win32 translation, no mutation.
        return Err(io::Error::from_raw_os_error(unsafe {
            windows_sys::Win32::Foundation::RtlNtStatusToDosError(status)
        } as i32));
    }
    OwnedHandle::from_raw(raw)
}
fn capture(handle: &OwnedHandle) -> io::Result<String> {
    let mut size = 0;
    unsafe {
        GetKernelObjectSecurity(
            handle.as_raw(),
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            0,
            &mut size,
        );
    }
    if size == 0 || size > 1024 * 1024 {
        return Err(io::Error::other("maintenance.namespace_descriptor_size"));
    }
    let mut bytes = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetKernelObjectSecurity(
            handle.as_raw(),
            DACL_SECURITY_INFORMATION,
            bytes.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut raw = ptr::null_mut();
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            bytes.as_mut_ptr().cast(),
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut raw,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0;
    while unsafe { *raw.add(len) } != 0 {
        len += 1;
    }
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(raw, len) })
        .map_err(io::Error::other);
    unsafe {
        LocalFree(raw.cast());
    }
    text
}

/// Installer-only rollback of an authenticated, admin-protected receipt. This
/// cannot add permissions or select a different object class. Exact parent and
/// target DACLs reject a recycled session owned by another user.
pub fn remove_recorded_namespace(
    id: &str,
    accounts: &[String],
    session: u32,
    parent: &str,
    before: &str,
    after: &str,
) -> io::Result<bool> {
    if !crate::current_process_is_elevated()?
        || session == 0
        || namespace_plan(id, accounts, before)? != after
    {
        return Err(io::Error::other("maintenance.invalid_removal_receipt"));
    }
    let parent_handle = match open(session, true, 0x20000) {
        Ok(v) => v,
        Err(e) if matches!(e.raw_os_error(), Some(2 | 3)) => return Ok(false),
        Err(e) => return Err(e),
    };
    if capture(&parent_handle)? != parent {
        return Err(io::Error::other("maintenance.removal_parent_changed"));
    }
    let handle = match open(session, false, 0x60000) {
        Ok(v) => v,
        Err(e) if matches!(e.raw_os_error(), Some(2 | 3)) => return Ok(false),
        Err(e) => return Err(e),
    };
    let current = capture(&handle)?;
    if current == before {
        return Ok(true);
    }
    if current != after {
        return Err(io::Error::other("maintenance.removal_namespace_changed"));
    }
    let wide = before.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut raw = ptr::null_mut();
    // SAFETY: the supplied before state is validated against the exact owned
    // fixed-ACE transformation and the retained current object's after state.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut raw,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let ok = unsafe { SetKernelObjectSecurity(handle.as_raw(), DACL_SECURITY_INFORMATION, raw) };
    let error = io::Error::last_os_error();
    unsafe {
        LocalFree(raw.cast());
    }
    if ok == 0 {
        return Err(error);
    }
    if capture(&handle)? != before {
        return Err(io::Error::other("maintenance.removal_postcondition"));
    }
    Ok(true)
}
/// Each configured account and the installation-derived namespace SID receives
/// exactly mask 0xF, no inheritance. Any pre-existing scoped ACE is refused.
pub fn namespace_plan(id: &str, accounts: &[String], before: &str) -> io::Result<String> {
    let cap = crate::execution_namespace_sid(id)?;
    if accounts.is_empty() || accounts.len() > 2 {
        return Err(io::Error::other("maintenance.account_count"));
    }
    let mut sids = accounts.to_vec();
    sids.push(cap);
    let mut seen = std::collections::BTreeSet::new();
    let mut value = ExecutionDevice::canonical_dacl(before)?;
    if !value.starts_with("D:") || value.contains("NO_ACCESS_CONTROL") {
        return Err(io::Error::other("maintenance.null_namespace_acl"));
    }
    for sid in sids {
        if !sid.starts_with("S-1-5-21-") || !seen.insert(sid.clone()) || value.contains(&sid) {
            return Err(io::Error::other("maintenance.scoped_namespace_ace_exists"));
        }
        value.push_str(&format!("(A;;0xf;;;{sid})"));
    }
    ExecutionDevice::canonical_dacl(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_only_adds_fixed_noninherited_distinct_aces() {
        let id = "073773f9-1798-4a46-a6d1-b8d5c9e3d19a";
        let accounts = vec!["S-1-5-21-1-2-3-1002".into(), "S-1-5-21-1-2-3-1003".into()];
        let before = "D:(A;;CCDC;;;WD)(A;;CCDCRC;;;RC)(A;;GA;;;SY)";
        let after = namespace_plan(id, &accounts, before).expect("plan");
        assert!(after.starts_with(before));
        assert_eq!(after.matches("(A;").count(), 6);
        assert!(namespace_plan(id, &accounts, &after).is_err());
        assert!(namespace_plan(id, &vec![accounts[0].clone(); 2], before).is_err());
        assert!(namespace_plan(id, &["S-1-1-0".into()], before).is_err());
        assert!(namespace_plan(id, &accounts, "D:NO_ACCESS_CONTROL").is_err());
    }
}
