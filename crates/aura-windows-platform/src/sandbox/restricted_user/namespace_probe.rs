//! Fixed shared namespace capability. No native ACL mutation or BNO fallback.
use crate::handle::OwnedHandle;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{ffi::c_void, io, mem::size_of, ptr};
use windows_sys::Win32::System::{
    RemoteDesktop::ProcessIdToSessionId, Threading::GetCurrentProcessId,
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
pub fn current_windows_session_id() -> io::Result<u32> {
    let mut session = 0;
    // SAFETY: initialized output and this process's real ID.
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(session)
}
pub fn execution_namespace_sid(id: &str) -> io::Result<String> {
    // Reuse canonical UUID validation, not a device capability SID.
    crate::ExecutionDevice::Null.sid(id)?;
    let hash = Sha256::digest(format!("Aura.R06.NamespaceCapability.v1/{id}"));
    Ok(format!(
        "S-1-5-21-{}",
        hash[..16]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().expect("four bytes")).to_string())
            .collect::<Vec<_>>()
            .join("-")
    ))
}
pub fn execution_namespace_access() -> Value {
    let session = match current_windows_session_id() {
        Ok(v) => v,
        Err(e) => return json!({"error":e.to_string()}),
    };
    let path = format!(r"\Sessions\{session}\BaseNamedObjects\Restricted");
    let name = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let unicode = UnicodeString {
        length: ((name.len() - 1) * 2) as u16,
        maximum_length: (name.len() * 2) as u16,
        buffer: name.as_ptr(),
    };
    let attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: ptr::null_mut(),
        name: &unicode,
        attributes: 0x40,
        descriptor: ptr::null_mut(),
        qos: ptr::null_mut(),
    };
    let check = |mask| {
        let mut raw = ptr::null_mut();
        // SAFETY: fixed current-session directory, initialized output, retained UTF-16.
        let status = unsafe { NtOpenDirectoryObject(&mut raw, mask, &attributes) };
        if status < 0 {
            return json!({"opened":false,"ntstatus":format!("0x{:08x}",status as u32)});
        }
        match OwnedHandle::from_raw(raw) {
            Ok(_handle) => json!({"opened":true}),
            Err(e) => json!({"opened":false,"error":e.to_string()}),
        }
    };
    json!({"session":session,"path":path,"mask":15,"required":check(15),
        "write_dac":check(0x40000),"write_owner":check(0x80000),"delete":check(0x10000)})
}

/// Read the already installed, fixed current-session namespace DACL. This
/// does not grant access or experiment with any RPC/DNS endpoint.
pub fn execution_namespace_dacl() -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, GetKernelObjectSecurity,
        },
    };
    let session = current_windows_session_id()?;
    let path = format!(r"\Sessions\{session}\BaseNamedObjects\Restricted");
    let name = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let unicode = UnicodeString {
        length: ((name.len() - 1) * 2) as u16,
        maximum_length: (name.len() * 2) as u16,
        buffer: name.as_ptr(),
    };
    let attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: ptr::null_mut(),
        name: &unicode,
        attributes: 0x40,
        descriptor: ptr::null_mut(),
        qos: ptr::null_mut(),
    };
    let mut raw = ptr::null_mut();
    let status = unsafe { NtOpenDirectoryObject(&mut raw, 0x20000, &attributes) };
    if status < 0 {
        return Err(io::Error::other(format!("namespace read failed 0x{:08x}", status as u32)));
    }
    let handle = OwnedHandle::from_raw(raw)?;
    let mut bytes = 0;
    unsafe {
        GetKernelObjectSecurity(
            handle.as_raw(),
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            0,
            &mut bytes,
        );
    }
    if bytes == 0 || bytes > 1024 * 1024 {
        return Err(io::Error::other("namespace descriptor size"));
    }
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetKernelObjectSecurity(
            handle.as_raw(),
            DACL_SECURITY_INFORMATION,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut value = ptr::null_mut();
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            buffer.as_mut_ptr().cast(),
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut value,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let result = super::super::wide_ptr_to_string(value);
    unsafe {
        LocalFree(value.cast());
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_sid_matches_approved_scope_and_differs_from_devices() {
        let id = "073773f9-1798-4a46-a6d1-b8d5c9e3d19a";
        let sid = execution_namespace_sid(id).expect("namespace SID");
        assert_eq!(sid, "S-1-5-21-298066063-1538511868-2029362712-2212329363");
        for device in crate::ExecutionDevice::EXECUTION {
            assert_ne!(sid, device.sid(id).expect("device SID"));
        }
        assert!(execution_namespace_sid("invalid").is_err());
    }
}
