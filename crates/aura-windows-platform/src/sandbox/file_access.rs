//! Per-object ACL changes through retained no-follow handles. Lease handles
//! request WRITE_DAC rather than MAXIMUM_ALLOWED so commands can still open
//! the same files with `FileShare.Read`. Existing objects are leased
//! individually; new files can inherit the temporary directory grant. Tests
//! exercise both properties.
use super::*;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Security::{
    GetKernelObjectSecurity, GetSecurityDescriptorControl, InitializeSecurityDescriptor,
    PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_AUTO_INHERIT_REQ, SE_DACL_AUTO_INHERITED,
    SE_DACL_PROTECTED, SECURITY_DESCRIPTOR, SetKernelObjectSecurity, SetSecurityDescriptorControl,
    SetSecurityDescriptorDacl, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES, READ_CONTROL, WRITE_DAC,
};

pub struct ExecutionFileSecurity {
    file: crate::VerifiedFile,
}
impl ExecutionFileSecurity {
    /// `retain_root` prevents moving the root while commands run. Individual
    /// file handles permit rename/delete; cleanup still addresses the same
    /// object even when its original pathname is gone.
    ///
    /// Access is WRITE_DAC/READ_CONTROL rather than MAXIMUM_ALLOWED so a
    /// command can still open the file with `FileShare.Read` (PowerShell
    /// `[IO.File]::ReadAllText`). MAXIMUM_ALLOWED includes write data and
    /// conflicts with that share mode.
    pub fn open(path: &Path, retain_root: bool) -> io::Result<Self> {
        let file = crate::path::open_native_with_desired_access(
            path,
            READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE | if retain_root { 0 } else { FILE_SHARE_DELETE },
        )?;
        if file.info().is_reparse_point
            || (!file.info().is_directory && file.info().number_of_links != 1)
        {
            return Err(io::Error::other("execution ACL target is an alias"));
        }
        Ok(Self { file })
    }
    /// Adopt an object that was opened by identity instead of by path — recovery
    /// asking the volume where a recorded object lives now. The identity was
    /// verified by that lookup; the alias rules every lease target must satisfy
    /// are enforced again here so no caller can adopt an aliased object.
    pub fn from_verified(file: crate::VerifiedFile) -> io::Result<Self> {
        if file.info().is_reparse_point
            || (!file.info().is_directory && file.info().number_of_links != 1)
        {
            return Err(io::Error::other("execution ACL target is an alias"));
        }
        Ok(Self { file })
    }
    pub fn info(&self) -> &crate::NativePathInfo {
        self.file.info()
    }
    pub fn deleted(&self) -> io::Result<bool> {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_STANDARD_INFO, FileStandardInfo, GetFileInformationByHandleEx,
        };
        let mut value = FILE_STANDARD_INFO::default();
        if unsafe {
            GetFileInformationByHandleEx(
                self.handle(),
                FileStandardInfo,
                (&raw mut value).cast(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(value.DeletePending && value.NumberOfLinks == 0)
    }
    fn handle(&self) -> HANDLE {
        self.file.file().as_raw_handle().cast()
    }

    fn descriptor(&self) -> io::Result<Vec<usize>> {
        descriptor_for_handle(self.handle())
    }
    pub fn capture(&self) -> io::Result<String> {
        sddl(self.descriptor()?.as_mut_ptr().cast())
    }
    /// Compute a candidate; the caller journals before calling apply_exact.
    pub fn plan(&self, grants: &[(&str, u32)]) -> io::Result<String> {
        self.plan_with_inheritance(grants, false)
    }
    pub fn plan_with_inheritance(
        &self,
        grants: &[(&str, u32)],
        inherit: bool,
    ) -> io::Result<String> {
        if grants.iter().any(|(_, mask)| *mask & !0x0013_01ff != 0) {
            return Err(io::Error::other("invalid execution file rights"));
        }
        self.plan_entries(grants, inherit, false)
    }
    /// Controller access is derived from this process, not a request field. It
    /// permits ownership/ACL cleanup for files newly created by the account.
    pub fn plan_for_attempt(&self, account: &str, write_sid: Option<&str>) -> io::Result<String> {
        let (controller, _) = crate::current_user_token_identity()?;
        let mut grants =
            vec![(account, if write_sid.is_some() { 0x0013_01bf } else { 0x0012_00a9 })];
        if let Some(sid) = write_sid {
            grants.push((sid, 0x0013_01bf));
            grants.push((controller.as_str(), 0x001f_01ff));
        }
        self.plan_entries(&grants, true, true)
    }
    fn plan_entries(
        &self,
        grants: &[(&str, u32)],
        inherit: bool,
        protect: bool,
    ) -> io::Result<String> {
        let mut descriptor = self.descriptor()?;
        plan_descriptor(
            descriptor.as_mut_ptr().cast(),
            grants,
            inherit && self.info().is_directory,
            protect,
        )
    }
    /// Recompute the exact fixed non-inheriting read transform from a journal,
    /// without temporarily applying its before-state to the real object.
    pub fn read_only_plan(before: &str, sid: &str) -> io::Result<String> {
        let mut descriptor = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide_null(before).as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _owner = LocalAllocation(descriptor.cast());
        plan_descriptor(descriptor, &[(sid, 0x0012_00a9)], false, false)
    }
    /// SetSecurityInfo may emit a different ACE order than SetEntriesInAclW.
    /// Allow ACLs with the same flags and allow-ACE bag are the same grant.
    #[must_use]
    pub fn sddl_equivalent(left: &str, right: &str) -> bool {
        left == right || dacl_allow_aces_match(left, right)
    }

    /// New unprotected output follows the restored parent's inheritance mode.
    /// Legacy directories create explicit ACEs; keep that representation
    /// without changing masks, trustees, ordering or ownership. Protected
    /// output has its own intentional inheritance boundary and is left alone.
    pub(super) fn match_parent_inheritance(&self) -> io::Result<()> {
        let mut descriptor = self.descriptor()?;
        let pointer = descriptor.as_mut_ptr().cast();
        let control = descriptor_control(pointer)?;
        if control & SE_DACL_PROTECTED != 0 || control & SE_DACL_AUTO_INHERITED == 0 {
            return Ok(());
        }
        let parent = self
            .info()
            .final_path
            .parent()
            .ok_or_else(|| io::Error::other("new execution file parent missing"))?;
        let parent = crate::path::open_native_with_desired_access(
            parent,
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )?;
        if !parent.info().is_directory || parent.info().is_reparse_point {
            return Err(io::Error::other("new execution file parent is an alias"));
        }
        let mut parent_descriptor = descriptor_for_handle(parent.file().as_raw_handle().cast())?;
        if descriptor_control(parent_descriptor.as_mut_ptr().cast())? & SE_DACL_AUTO_INHERITED != 0
        {
            return Ok(());
        }
        let before = sddl(pointer)?;
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        if unsafe { GetSecurityDescriptorDacl(pointer, &mut present, &mut dacl, &mut defaulted) }
            == 0
            || present == 0
            || dacl.is_null()
        {
            return Err(io::Error::other("new execution file explicit DACL required"));
        }
        for index in 0..unsafe { (*dacl).AceCount } as u32 {
            let mut ace = ptr::null_mut();
            if unsafe { windows_sys::Win32::Security::GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            unsafe {
                (*ace.cast::<windows_sys::Win32::Security::ACE_HEADER>()).AceFlags &= !0x10;
            }
        }
        if unsafe {
            SetSecurityDescriptorControl(
                pointer,
                SE_DACL_AUTO_INHERIT_REQ | SE_DACL_AUTO_INHERITED,
                0,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        self.apply_exact(&before, &sddl(pointer)?)
    }
}

fn descriptor_for_handle(handle: HANDLE) -> io::Result<Vec<usize>> {
    let mut bytes = 0;
    unsafe {
        GetKernelObjectSecurity(handle, DACL_SECURITY_INFORMATION, ptr::null_mut(), 0, &mut bytes);
    }
    if bytes == 0 || bytes > 1024 * 1024 {
        return Err(io::Error::other("invalid execution ACL size"));
    }
    let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetKernelObjectSecurity(
            handle,
            DACL_SECURITY_INFORMATION,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

fn descriptor_control(descriptor: PSECURITY_DESCRIPTOR) -> io::Result<u16> {
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(control)
}

fn plan_descriptor(
    descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR,
    grants: &[(&str, u32)],
    inherit: bool,
    protect: bool,
) -> io::Result<String> {
    let mut control = 0;
    let mut revision = 0;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
        || unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
            == 0
        || present == 0
        || dacl.is_null()
    {
        return Err(io::Error::other("explicit execution DACL required"));
    }
    if protect {
        // Convert inherited entries before merging grants. Otherwise an
        // existing owner ACE and a new owner grant become duplicate explicit
        // ACEs only after the merge, and Windows canonicalizes a different ACL.
        for index in 0..unsafe { (*dacl).AceCount } as u32 {
            let mut raw = ptr::null_mut();
            if unsafe { windows_sys::Win32::Security::GetAce(dacl, index, &mut raw) } == 0 {
                return Err(io::Error::last_os_error());
            }
            unsafe {
                (*raw.cast::<windows_sys::Win32::Security::ACE_HEADER>()).AceFlags &= !0x10;
            }
        }
    }
    let mut acl_owner = None;
    for (sid, mask) in grants {
        let inheritance = if inherit { SUB_CONTAINERS_AND_OBJECTS_INHERIT } else { 0 };
        let (entry, _sid) = explicit_access_for_sid(sid, *mask, GRANT_ACCESS, inheritance)?;
        let mut updated = ptr::null_mut();
        let status = unsafe { SetEntriesInAclW(1, &entry, dacl, &mut updated) };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        acl_owner = Some(LocalAllocation(updated.cast()));
        dacl = updated;
    }
    let mut absolute = SECURITY_DESCRIPTOR::default();
    if protect {
        // Temporary inheritable grants must be recognisable as inherited by
        // newly created output, even when the saved original uses legacy ACLs.
        control |= SE_DACL_PROTECTED | SE_DACL_AUTO_INHERITED;
    }
    if unsafe { InitializeSecurityDescriptor((&raw mut absolute).cast(), 1) } == 0
        || unsafe { SetSecurityDescriptorDacl((&raw mut absolute).cast(), 1, dacl, 0) } == 0
        || unsafe {
            SetSecurityDescriptorControl(
                (&raw mut absolute).cast(),
                SE_DACL_AUTO_INHERITED | SE_DACL_PROTECTED,
                control & (SE_DACL_AUTO_INHERITED | SE_DACL_PROTECTED),
            )
        } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let result = sddl((&raw mut absolute).cast());
    drop(acl_owner);
    result.map(|value| dedupe_repeated_allow_aces(&value))
}
impl ExecutionFileSecurity {
    pub fn apply_exact(&self, expected: &str, next: &str) -> io::Result<()> {
        if !Self::sddl_equivalent(&self.capture()?, expected) {
            return Err(io::Error::other("execution ACL changed; preserve journal"));
        }
        let mut descriptor = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide_null(next).as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalAllocation(descriptor.cast());
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
            == 0
            || present == 0
            || dacl.is_null()
        {
            return Err(io::Error::other("explicit execution DACL required"));
        }
        let mut control = 0;
        let mut revision = 0;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let protection = if control & SE_DACL_PROTECTED != 0 {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        // The retained-object setter needs the request bit as well as the
        // inherited bit. Without it Windows clears the inheritance model and
        // temporary Controller grants on new output later become explicit.
        // Restore a legacy descriptor as legacy; only modern targets request
        // modern inheritance. Existing children remain individually journalled.
        if control & SE_DACL_AUTO_INHERITED != 0
            && unsafe {
                SetSecurityDescriptorControl(
                    descriptor,
                    SE_DACL_AUTO_INHERIT_REQ,
                    SE_DACL_AUTO_INHERIT_REQ,
                )
            } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // Set this object's DACL only. SetSecurityInfo would propagate
        // inheritable ACEs to existing children; the lease visits each object.
        if unsafe {
            SetKernelObjectSecurity(
                self.handle(),
                DACL_SECURITY_INFORMATION | protection,
                descriptor,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let observed = self.capture()?;
        if !Self::sddl_equivalent(&observed, next) {
            return Err(io::Error::other("execution ACL postcondition mismatch"));
        }
        Ok(())
    }
}
fn dacl_control_and_aces(sddl: &str) -> Option<(&str, Vec<&str>)> {
    let body = sddl.strip_prefix("D:")?;
    let start = body.find('(')?;
    let control = &body[..start];
    let mut aces = Vec::new();
    let mut index = start;
    let bytes = body.as_bytes();
    while index < body.len() {
        if bytes[index] != b'(' {
            return None;
        }
        let relative = body.get(index + 1..)?.find(')')?;
        let end = index + 1 + relative + 1;
        aces.push(body.get(index..end)?);
        index = end;
    }
    Some((control, aces))
}

fn dacl_control_key(control: &str) -> String {
    // Windows may set SE_DACL_AUTO_INHERITED when rewriting a parent DACL.
    // Protected/defaulted bits still have to match.
    control.replace("AI", "")
}

/// Windows retains only one copy of a byte-identical allow ACE when a DACL is
/// written, so a plan that still lists both copies can never match the
/// read-back and the apply fails *after* the object was changed. Collapse exact
/// duplicates here — same SDDL text means same SID, type, mask and flags — so
/// the planned ACL is the ACL the write keeps. Deny, object and callback ACEs
/// are never touched, control bits and relative order are preserved, and a
/// DACL that merely reorders distinct grants is left alone.
fn dedupe_repeated_allow_aces(sddl: &str) -> String {
    let Some((control, aces)) = dacl_control_and_aces(sddl) else {
        return sddl.to_string();
    };
    let mut seen = std::collections::HashSet::new();
    let mut deduped = String::with_capacity(sddl.len());
    deduped.push_str("D:");
    deduped.push_str(control);
    for ace in aces {
        if ace.starts_with("(A;") && !seen.insert(ace) {
            continue;
        }
        deduped.push_str(ace);
    }
    deduped
}

fn dacl_allow_aces_match(left: &str, right: &str) -> bool {
    let Some((left_control, mut left_aces)) = dacl_control_and_aces(left) else {
        return false;
    };
    let Some((right_control, mut right_aces)) = dacl_control_and_aces(right) else {
        return false;
    };
    if dacl_control_key(left_control) != dacl_control_key(right_control)
        || left_aces.len() != right_aces.len()
    {
        return false;
    }
    if left_aces
        .iter()
        .chain(right_aces.iter())
        .any(|ace| ace.starts_with("(D;") || ace.starts_with("(OD;"))
    {
        return false;
    }
    left_aces.sort_unstable();
    right_aces.sort_unstable();
    left_aces == right_aces
}

fn sddl(descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR) -> io::Result<String> {
    let mut text = ptr::null_mut();
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let _text = LocalAllocation(text.cast());
    wide_ptr_to_string(text)
}
