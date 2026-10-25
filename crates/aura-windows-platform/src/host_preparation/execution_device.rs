//! Install-scoped, fixed NUL/KsecDD capabilities. Never replaces owner, group or mandatory label.
use super::*;
use sha2::{Digest, Sha256};
use windows_sys::{
    Wdk::{
        Foundation::{NtQueryObject, ObjectBasicInformation},
        System::SystemInformation::NtQuerySystemInformation,
    },
    Win32::Storage::FileSystem::{FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionDevice {
    Null,
    Cng,
    Ksec,
}
impl ExecutionDevice {
    pub const EXECUTION: [Self; 2] = [Self::Null, Self::Ksec];

    pub fn access_mask(self) -> u32 {
        self.capability().access_mask()
    }

    /// Fixed open-only probe; never issues I/O or changes security.
    pub fn access_probe(self) -> serde_json::Value {
        let check = |mask| match self.open(mask) {
            Ok(_) => serde_json::json!({"opened":true}),
            Err(error) => serde_json::json!({"opened":false,"os_error":error.raw_os_error()}),
        };
        serde_json::json!({"mask":self.access_mask(),"required":check(self.access_mask()),
            "write_dac":check(WRITE_DAC),"write_owner":check(WRITE_OWNER),
            "delete":check(0x10000)})
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Cng => "cng",
            Self::Ksec => "ksec",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::Null => "Null",
            Self::Cng => "Cng",
            Self::Ksec => "Ksec",
        }
    }
    fn path(self) -> &'static str {
        match self {
            Self::Null => r"\\?\GLOBALROOT\Device\Null",
            Self::Cng => r"\\?\GLOBALROOT\Device\CNG",
            Self::Ksec => r"\\?\GLOBALROOT\Device\KsecDD",
        }
    }
    /// The shared capability identity for this device.
    #[must_use]
    fn capability(self) -> crate::capability::CapabilityDevice {
        match self {
            Self::Null => crate::capability::CapabilityDevice::Null,
            Self::Cng => crate::capability::CapabilityDevice::Cng,
            Self::Ksec => crate::capability::CapabilityDevice::Ksec,
        }
    }

    pub fn sid(self, id: &str) -> io::Result<String> {
        if id.len() != 36
            || !id.bytes().enumerate().all(|(i, c)| {
                if [8, 13, 18, 23].contains(&i) {
                    c == b'-'
                } else {
                    c.is_ascii_hexdigit() && !c.is_ascii_uppercase()
                }
            })
        {
            return Err(io::Error::other("canonical installation UUID required"));
        }
        let hash = Sha256::digest(format!("Aura.R06.{}Capability.v1/{id}", self.prefix()));
        let parts = hash[..16]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().expect("four bytes")).to_string())
            .collect::<Vec<_>>();
        Ok(format!("S-1-5-21-{}", parts.join("-")))
    }

    /// Identity of this kernel resource for the current mapping. Restart loads
    /// the owning driver at a new image base; an Aura service restart and a
    /// Session change keep the same mapping. This is not the NT BootIdentifier,
    /// not LastBootUpTime, and never a caller-supplied GUID.
    pub fn capture_generation(self) -> io::Result<String> {
        let handle = self.open(READ_CONTROL)?;
        if let Some(generation) = file_id_generation(handle.as_raw()) {
            return Ok(generation);
        }
        if let Some(generation) = object_creation_generation(handle.as_raw())? {
            return Ok(generation);
        }
        loaded_driver_generation(self.driver_image())
    }

    fn driver_image(self) -> &'static str {
        match self {
            Self::Null => "null.sys",
            Self::Cng => "cng.sys",
            Self::Ksec => "ksecdd.sys",
        }
    }

    pub fn capture_dacl(self) -> io::Result<String> {
        let handle = self.open(READ_CONTROL)?;
        let mut needed = 0;
        // SAFETY: documented sizing query for the fixed device's DACL.
        unsafe {
            GetKernelObjectSecurity(
                handle.as_raw(),
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        // SAFETY: aligned buffer spans the reported size and handle is retained.
        if unsafe {
            GetKernelObjectSecurity(
                handle.as_raw(),
                DACL_SECURITY_INFORMATION,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        security_descriptor_to_sddl(buffer.as_mut_ptr().cast(), DACL_SECURITY_INFORMATION)
    }

    fn open(self, access: u32) -> io::Result<OwnedHandle> {
        // Bypass per-logon DOS device mappings: the resource is the fixed approved
        // device, never a user-defined alias named NUL or a supplied file path.
        let path = wide_null(self.path());
        // SAFETY: fixed NUL-terminated path, no create/truncate or optional pointers.
        OwnedHandle::from_raw(unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        })
    }

    pub fn canonicalize_dacl(sddl: &str) -> io::Result<String> {
        Self::canonical_dacl(sddl)
    }

    pub(crate) fn canonical_dacl(sddl: &str) -> io::Result<String> {
        let encoded = wide_null(sddl);
        let mut descriptor = ptr::null_mut();
        // SAFETY: valid NUL terminated input and initialized output.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _guard = LocalAllocation(descriptor.cast());
        security_descriptor_to_sddl(descriptor, DACL_SECURITY_INFORMATION)
    }

    /// Only one exact ACE for the derived capability can change. Unknown or
    /// duplicate entries involving this SID fail closed, including deny entries.
    pub fn plan(self, id: &str, before: &str, install: bool) -> io::Result<String> {
        let sid = self.sid(id)?;
        let before = Self::canonical_dacl(before)?;
        if !before.starts_with("D:") || before.contains("NO_ACCESS_CONTROL") {
            return Err(io::Error::other("Device must have an explicit non-NULL DACL"));
        }
        let ace = format!("(A;;0x{:x};;;{sid})", self.access_mask());
        let count = before.matches(&ace).count();
        let without = before.replace(&ace, "");
        if count > 1 || without.contains(&sid) {
            return Err(io::Error::other("Device capability SID has unknown or duplicate ACEs"));
        }
        if install {
            if count == 1 { Ok(before) } else { Self::canonical_dacl(&format!("{before}{ace}")) }
        } else {
            Self::canonical_dacl(&without)
        }
    }

    pub fn inspect(self, id: &str) -> io::Result<bool> {
        let before = self.capture_dacl()?;
        Ok(self.plan(id, &before, true)? == before)
    }

    /// Caller must persist the reviewed transition before calling. This is a
    /// compare-before-write on the fixed device, not an OS-wide atomic ACL CAS.
    /// Concurrent external ACL changes after comparison remain a host limitation.
    pub fn apply(self, id: &str, expected: &str, target: &str, install: bool) -> io::Result<()> {
        if install && self == Self::Cng {
            return Err(io::Error::other(
                "CNG experiment retired; only inspection/removal allowed",
            ));
        }
        if !crate::current_process_is_elevated()? {
            return Err(io::Error::other("Device capability changes require administrator Setup"));
        }
        let expected = Self::canonical_dacl(expected)?;
        let target = Self::canonical_dacl(target)?;
        if self.plan(id, &expected, install)? != target {
            return Err(io::Error::other("Device transition exceeds owned capability ACE"));
        }
        let current = self.capture_dacl()?;
        if current == target {
            return Ok(());
        }
        if current != expected {
            return Err(io::Error::other("Device DACL changed since review; preserve journal"));
        }
        let handle = self.open(READ_CONTROL | WRITE_DAC)?;
        let encoded = wide_null(&target);
        let mut descriptor = ptr::null_mut();
        // SAFETY: input/output buffers satisfy the conversion API contract.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _guard = LocalAllocation(descriptor.cast());
        // SAFETY: only the DACL of the fixed device handle is updated. No owner,
        // group, label, SACL or file permissions are requested or changed.
        if unsafe {
            SetKernelObjectSecurity(handle.as_raw(), DACL_SECURITY_INFORMATION, descriptor)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if self.capture_dacl()? != target {
            return Err(io::Error::other(
                "Device postcondition changed; preserve journal, no blind rollback",
            ));
        }
        Ok(())
    }
}

#[repr(C)]
struct ObjectBasicInformationBuffer {
    attributes: u32,
    granted_access: u32,
    handle_count: u32,
    pointer_count: u32,
    paged_pool_charge: u32,
    non_paged_pool_charge: u32,
    reserved: [u32; 3],
    name_info_size: u32,
    type_info_size: u32,
    security_descriptor_size: u32,
    creation_time: i64,
}

fn file_id_generation(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<String> {
    let mut identity = FILE_ID_INFO { VolumeSerialNumber: 0, FileId: Default::default() };
    // SAFETY: the handle is the open device and the output buffer matches FileIdInfo.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&raw mut identity).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).ok()?,
        )
    };
    if ok == 0 || identity.FileId.Identifier.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(format!(
        "file:{:016x}:{}",
        identity.VolumeSerialNumber,
        identity.FileId.Identifier.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    ))
}

fn object_creation_generation(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<Option<String>> {
    let mut info = ObjectBasicInformationBuffer {
        attributes: 0,
        granted_access: 0,
        handle_count: 0,
        pointer_count: 0,
        paged_pool_charge: 0,
        non_paged_pool_charge: 0,
        reserved: [0; 3],
        name_info_size: 0,
        type_info_size: 0,
        security_descriptor_size: 0,
        creation_time: 0,
    };
    let mut returned = 0u32;
    // SAFETY: ObjectBasicInformation is class 0; the local layout matches the
    // documented ntddk OBJECT_BASIC_INFORMATION including CreationTime.
    let status = unsafe {
        NtQueryObject(
            handle,
            ObjectBasicInformation,
            (&raw mut info).cast(),
            u32::try_from(size_of::<ObjectBasicInformationBuffer>())
                .map_err(|_| io::Error::other("maintenance.object_generation_size"))?,
            &mut returned,
        )
    };
    if status != 0 {
        return Err(io::Error::other("maintenance.object_generation"));
    }
    if info.creation_time == 0 {
        return Ok(None);
    }
    Ok(Some(format!("object:{:016x}", info.creation_time as u64)))
}

const SYSTEM_MODULE_INFORMATION: i32 = 11;

#[repr(C)]
struct SystemModule {
    section: *mut c_void,
    mapped_base: *mut c_void,
    image_base: *mut c_void,
    image_size: u32,
    flags: u32,
    load_order_index: u16,
    init_order_index: u16,
    load_count: u16,
    offset_to_file_name: u16,
    full_path_name: [u8; 256],
}

const _: () = assert!(size_of::<SystemModule>() == 296);

fn loaded_driver_generation(basename: &str) -> io::Result<String> {
    parse_loaded_driver_generation(&system_module_information()?, basename)
}

fn system_module_information() -> io::Result<Vec<u8>> {
    let mut length = 0u32;
    // SAFETY: class 11 is SystemModuleInformation; the first call is the
    // sizing query and only writes the required length.
    let status = unsafe {
        NtQuerySystemInformation(SYSTEM_MODULE_INFORMATION, ptr::null_mut(), 0, &mut length)
    };
    if status >= 0 || length < 8 {
        return Err(io::Error::other("maintenance.object_generation"));
    }
    let mut buffer = vec![0u8; length as usize];
    let mut returned = 0u32;
    // SAFETY: buffer is sized from the previous query; class and length match.
    let status = unsafe {
        NtQuerySystemInformation(
            SYSTEM_MODULE_INFORMATION,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut returned,
        )
    };
    if status < 0 || (returned as usize) < 8 || (returned as usize) > buffer.len() {
        return Err(io::Error::other("maintenance.object_generation"));
    }
    buffer.truncate(returned as usize);
    Ok(buffer)
}

fn parse_loaded_driver_generation(buffer: &[u8], basename: &str) -> io::Result<String> {
    let mut found = None;
    for module in system_modules(buffer)? {
        if !module_basename(&module).is_some_and(|name| name.eq_ignore_ascii_case(basename)) {
            continue;
        }
        let base = module.image_base as usize;
        if base == 0 {
            return Err(io::Error::other("maintenance.object_generation"));
        }
        if found.is_some() {
            return Err(io::Error::other("maintenance.object_generation_ambiguous"));
        }
        found = Some(base);
    }
    let Some(base) = found else {
        return Err(io::Error::other("maintenance.object_generation"));
    };
    Ok(format!("driver:{basename}:{base:016x}"))
}

fn system_modules(buffer: &[u8]) -> io::Result<Vec<SystemModule>> {
    let count = u32::from_le_bytes(
        buffer
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| io::Error::other("maintenance.object_generation"))?,
    ) as usize;
    let modules =
        buffer.get(8..).ok_or_else(|| io::Error::other("maintenance.object_generation"))?;
    if modules.len() < count.saturating_mul(size_of::<SystemModule>()) {
        return Err(io::Error::other("maintenance.object_generation"));
    }
    let mut records = Vec::with_capacity(count);
    for index in 0..count {
        let offset = index * size_of::<SystemModule>();
        let slice = modules
            .get(offset..offset + size_of::<SystemModule>())
            .ok_or_else(|| io::Error::other("maintenance.object_generation"))?;
        // SAFETY: slice is exactly one SystemModule record.
        records.push(unsafe { slice.as_ptr().cast::<SystemModule>().read_unaligned() });
    }
    Ok(records)
}

fn module_basename(module: &SystemModule) -> Option<&str> {
    let end = module.full_path_name.iter().position(|&byte| byte == 0).unwrap_or(256);
    let path = module.full_path_name.get(..end)?;
    let offset = usize::from(module.offset_to_file_name);
    let name = path.get(offset..).filter(|bytes| !bytes.is_empty()).unwrap_or(path);
    std::str::from_utf8(name).ok().filter(|name| !name.is_empty())
}

#[cfg(test)]
fn loaded_driver_present(basename: &str) -> io::Result<bool> {
    Ok(system_modules(&system_module_information()?)?.iter().any(|module| {
        module_basename(module).is_some_and(|name| name.eq_ignore_ascii_case(basename))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "073773f9-1798-4a46-a6d1-b8d5c9e3d19a";
    #[test]
    fn ksec_uses_exact_minimum_mask_and_keeps_retired_cng_separate() {
        let device = ExecutionDevice::Ksec;
        let sid = device.sid(ID).expect("sid");
        assert_eq!(sid, "S-1-5-21-163667669-2389923007-225062444-4129039185");
        assert_eq!(device.access_mask(), 0x100003);
        assert_eq!(ExecutionDevice::EXECUTION, [ExecutionDevice::Null, device]);
        let before = "D:P(A;;FA;;;SY)(A;;0x1201bf;;;WD)";
        let after = device.plan(ID, before, true).expect("add");
        assert!(after.ends_with(&format!("(A;;0x100003;;;{sid})")));
        assert_eq!(device.plan(ID, &after, false).expect("remove"), before);
        for ace in [
            format!("(A;;0x12019f;;;{sid})"),
            format!("(D;;0x100003;;;{sid})"),
            format!("(A;;0x100003;;;{sid})(A;;0x100003;;;{sid})"),
        ] {
            assert!(device.plan(ID, &format!("D:{ace}"), true).is_err());
            assert!(device.plan(ID, &format!("D:{ace}"), false).is_err());
        }
        let cng = ExecutionDevice::Cng.plan(ID, &after, true).expect("historical CNG");
        assert_eq!(ExecutionDevice::Cng.plan(ID, &cng, false).expect("retire"), after);
        assert!(ExecutionDevice::Cng.apply(ID, before, before, true).is_err());
    }
    #[test]
    fn cng_and_null_are_distinct_and_never_remove_each_other() {
        let before = "D:P(A;;FA;;;SY)(A;;0x1201bf;;;WD)";
        let null = ExecutionDevice::Null.plan(ID, before, true).expect("NUL");
        let both = ExecutionDevice::Cng.plan(ID, &null, true).expect("CNG");
        assert_eq!(ExecutionDevice::Cng.plan(ID, &both, false).expect("remove CNG"), null);
        let cng = ExecutionDevice::Cng.plan(ID, before, true).expect("CNG only");
        assert_eq!(ExecutionDevice::Null.plan(ID, &both, false).expect("remove NUL"), cng);
        assert_eq!(
            ExecutionDevice::Cng.sid(ID).expect("CNG SID"),
            "S-1-5-21-160093851-375851460-2835756871-2934485828"
        );
        assert_ne!(ExecutionDevice::Cng.path(), ExecutionDevice::Null.path());
        let bad = format!("D:(A;;FA;;;{})", ExecutionDevice::Cng.sid(ID).expect("sid"));
        assert!(ExecutionDevice::Cng.plan(ID, &bad, false).is_err());
    }
    #[test]
    fn exact_add_remove_preserve_other_aces_and_protection() {
        let original = "D:P(A;;FA;;;SY)(A;;0x1201bf;;;WD)";
        let after = ExecutionDevice::Null.plan(ID, original, true).expect("plan");
        assert_eq!(ExecutionDevice::Null.plan(ID, &after, true).expect("idempotent"), after);
        assert_eq!(ExecutionDevice::Null.plan(ID, &after, false).expect("remove"), original);
        let changed = format!("{after}(A;;FR;;;BA)");
        assert_eq!(
            ExecutionDevice::Null.plan(ID, &changed, false).expect("preserve other"),
            format!("{original}(A;;FR;;;BA)")
        );
        assert_eq!(ExecutionDevice::Null.plan(ID, original, false).expect("absent"), original);
    }
    #[test]
    fn rejects_sid_collisions_duplicates_and_null_acl() {
        let sid = ExecutionDevice::Null.sid(ID).expect("sid");
        for ace in [
            format!("(A;;FA;;;{sid})"),
            format!("(D;;0x12019f;;;{sid})"),
            format!("(A;;0x12019f;;;{sid})(A;;0x12019f;;;{sid})"),
        ] {
            assert!(ExecutionDevice::Null.plan(ID, &format!("D:{ace}"), true).is_err());
        }
        assert!(ExecutionDevice::Null.plan(ID, "D:NO_ACCESS_CONTROL", true).is_err());
        assert!(ExecutionDevice::Null.sid("S-1-1-0").is_err());
        assert_eq!(sid, "S-1-5-21-1945148321-1189346774-2431522260-765611959");
        assert_ne!(
            sid,
            ExecutionDevice::Null
                .sid("073773f9-1798-4a46-a6d1-b8d5c9e3d19b")
                .expect("other installation")
        );
    }
    #[test]
    fn loaded_driver_generation_reads_distinct_image_bases() {
        let mut buffer = vec![0u8; 8 + size_of::<SystemModule>() * 2];
        buffer[0..4].copy_from_slice(&2u32.to_le_bytes());
        write_module(
            &mut buffer[8..],
            0xffff_f800_1234_0000,
            b"\\SystemRoot\\System32\\drivers\\Null.SYS",
        );
        write_module(
            &mut buffer[8 + size_of::<SystemModule>()..],
            0xffff_f800_5678_0000,
            b"\\SystemRoot\\System32\\drivers\\ksecdd.sys",
        );
        assert_eq!(
            parse_loaded_driver_generation(&buffer, "null.sys").expect("null"),
            "driver:null.sys:fffff80012340000"
        );
        assert_eq!(
            parse_loaded_driver_generation(&buffer, "ksecdd.sys").expect("ksec"),
            "driver:ksecdd.sys:fffff80056780000"
        );
        write_module(&mut buffer[8..], 0, b"\\SystemRoot\\System32\\drivers\\Null.SYS");
        assert!(parse_loaded_driver_generation(&buffer, "null.sys").is_err());
    }

    fn write_module(slot: &mut [u8], image_base: u64, path: &[u8]) {
        slot.fill(0);
        slot[16..24].copy_from_slice(&image_base.to_le_bytes());
        let slash = path.iter().rposition(|&byte| byte == b'\\').map_or(0, |index| index + 1);
        slot[38..40].copy_from_slice(&(slash as u16).to_le_bytes());
        slot[40..40 + path.len()].copy_from_slice(path);
    }

    #[test]
    fn device_generation_is_stable_for_the_live_object() {
        let mut captured = Vec::new();
        for device in ExecutionDevice::EXECUTION {
            assert!(loaded_driver_present(device.driver_image()).expect("loaded"));
            match device.capture_generation() {
                Ok(first) => {
                    let second = device.capture_generation().expect("stable");
                    assert_eq!(first, second);
                    assert!(
                        first.starts_with("file:")
                            || first.starts_with("object:")
                            || first.starts_with(&format!("driver:{}:", device.driver_image()))
                    );
                    assert!(!first.ends_with(":0000000000000000"));
                    captured.push(first);
                }
                Err(error) => assert_eq!(error.to_string(), "maintenance.object_generation"),
            }
        }
        if captured.len() == 2 {
            assert_ne!(captured[0], captured[1]);
        }
    }
}
