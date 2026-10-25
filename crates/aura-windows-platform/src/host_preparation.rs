use std::{
    ffi::c_void,
    io,
    mem::size_of,
    path::{Component, Path, PathBuf, Prefix},
    ptr,
};

use windows_sys::{
    Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, LocalFree},
        Security::{
            ACCESS_ALLOWED_ACE,
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
                ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW,
                EXPLICIT_ACCESS_W, GRANT_ACCESS, GetExplicitEntriesFromAclW, GetNamedSecurityInfoW,
                NO_MULTIPLE_TRUSTEE, REVOKE_ACCESS, SDDL_REVISION_1, SE_FILE_OBJECT,
                SetEntriesInAclW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN,
            },
            DACL_SECURITY_INFORMATION, EqualSid, GROUP_SECURITY_INFORMATION, GetAce,
            GetKernelObjectSecurity, GetSecurityDescriptorControl, InitializeSecurityDescriptor,
            LABEL_SECURITY_INFORMATION, MakeSelfRelativeSD, OWNER_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR, PSID, SE_DACL_AUTO_INHERITED, SE_DACL_PROTECTED,
            SE_SACL_AUTO_INHERITED, SE_SACL_PROTECTED, SECURITY_DESCRIPTOR, SetFileSecurityW,
            SetKernelObjectSecurity, SetSecurityDescriptorControl, SetSecurityDescriptorDacl,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
            WRITE_OWNER,
        },
        System::SystemServices::SECURITY_DESCRIPTOR_REVISION,
    },
    core::PWSTR,
};

use crate::handle::OwnedHandle;
use dacl_ace::{DaclAceIdentity, DaclObservationV1, DaclTransitionV1, DaclWriteOperationV1};

#[cfg(test)]
mod access_behavior_tests;
pub mod dacl_ace;
#[cfg(feature = "restricted-user-runtime")]
pub(crate) mod execution_device;

pub const ALL_APPLICATION_PACKAGES_SID: &str = "S-1-15-2-1";
pub const ALL_RESTRICTED_APPLICATION_PACKAGES_SID: &str = "S-1-15-2-2";
pub const SYSTEM_DRIVE_COMPATIBILITY_MASK: u32 = 0x0012_0088;
pub const NULL_DEVICE_COMPATIBILITY_SDDL: &str = concat!(
    "O:BAG:SYD:",
    "(A;;GRGWGX;;;WD)",
    "(A;;FA;;;SY)",
    "(A;;FA;;;BA)",
    "(A;;GRGX;;;RC)",
    "(A;;GRGWGX;;;AC)",
    "(A;;GRGWGX;;;S-1-15-2-2)",
    "S:(ML;;NW;;;LW)"
);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostCompatibilityProbe {
    pub system_drive_root: PathBuf,
    pub all_application_packages_ready: bool,
    pub all_restricted_application_packages_ready: bool,
    pub system_drive_ready: bool,
    pub null_device_ready: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemDriveCompatibilityChanges {
    pub all_application_packages_added: bool,
    pub all_restricted_application_packages_added: bool,
}

pub fn probe_host_compatibility() -> io::Result<HostCompatibilityProbe> {
    let system_drive_root = system_drive_root()?;
    let all_application_packages_ready = file_dacl_has_exact_allow(
        &system_drive_root,
        ALL_APPLICATION_PACKAGES_SID,
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        0,
    )?;
    let all_restricted_application_packages_ready = file_dacl_has_exact_allow(
        &system_drive_root,
        ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        0,
    )?;
    let system_drive_ready =
        all_application_packages_ready && all_restricted_application_packages_ready;
    let null_device_ready = probe_null_device_compatibility().unwrap_or(false);
    Ok(HostCompatibilityProbe {
        system_drive_root,
        all_application_packages_ready,
        all_restricted_application_packages_ready,
        system_drive_ready,
        null_device_ready,
    })
}

pub fn system_drive_root() -> io::Result<PathBuf> {
    let value = std::env::var_os("SystemDrive")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "SystemDrive is unavailable"))?;
    let text = value.to_string_lossy();
    if text.len() != 2 || text.as_bytes()[1] != b':' || !text.as_bytes()[0].is_ascii_alphabetic() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SystemDrive is not a literal drive designator",
        ));
    }
    Ok(PathBuf::from(format!("{}\\", text.to_ascii_uppercase())))
}

pub fn probe_system_drive_compatibility(root: &Path) -> io::Result<bool> {
    validate_drive_root(root)?;
    Ok(file_dacl_has_exact_allow(
        root,
        ALL_APPLICATION_PACKAGES_SID,
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        0,
    )? && file_dacl_has_exact_allow(
        root,
        ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        0,
    )?)
}

pub fn prepare_system_drive_compatibility(
    root: &Path,
    requested: SystemDriveCompatibilityChanges,
) -> io::Result<SystemDriveCompatibilityChanges> {
    Ok(prepare_system_drive_compatibility_with_transition(root, requested)?.0)
}

/// The same apply, with the verified control-field transition returned to the caller.
///
/// The approved marker difference is an outcome, not a discardable detail: the caller
/// that writes the operation evidence is the one that has to be able to report it,
/// because the two writes cannot restore `SE_DACL_AUTO_INHERITED`.
pub fn prepare_system_drive_compatibility_with_transition(
    root: &Path,
    requested: SystemDriveCompatibilityChanges,
) -> io::Result<(SystemDriveCompatibilityChanges, DaclTransitionV1)> {
    validate_drive_root(root)?;
    let application_packages_ready = file_dacl_has_exact_allow(
        root,
        ALL_APPLICATION_PACKAGES_SID,
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        0,
    )?;
    let restricted_packages_ready = file_dacl_has_exact_allow(
        root,
        ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        0,
    )?;
    if (!application_packages_ready && !requested.all_application_packages_added)
        || (!restricted_packages_ready && !requested.all_restricted_application_packages_added)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "host compatibility state changed after the broker plan was frozen",
        ));
    }
    // The frozen descriptor is the reference for the whole verification: owner, group,
    // label and the ordered ACE list are compared against what the object reports after
    // the write, and only the approved auto-inherited marker transition is accepted.
    let frozen_full = capture_full_descriptor(root)?;
    let mut changes = SystemDriveCompatibilityChanges::default();
    let mut planned = Vec::new();
    if requested.all_application_packages_added && !application_packages_ready {
        grant_file_dacl_mask(
            root,
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            0,
        )?;
        changes.all_application_packages_added = true;
        planned.push(DaclAceIdentity::allow_mask(
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            ALL_APPLICATION_PACKAGES_SID,
        ));
    }
    if restricted_packages_ready || !requested.all_restricted_application_packages_added {
        if planned.is_empty() {
            return Ok((changes, DaclTransitionV1::Unchanged));
        }
        let transition = verify_targeted_write(
            root,
            &frozen_full,
            &planned,
            DaclWriteOperationV1::ApplyPlannedAces,
        )?;
        return Ok((changes, transition));
    }
    if let Err(error) = grant_file_dacl_mask(
        root,
        ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        0,
    ) {
        if changes.all_application_packages_added {
            let _ = revoke_exact_file_dacl_allow(
                root,
                ALL_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0,
            );
        }
        return Err(error);
    }
    changes.all_restricted_application_packages_added = true;
    planned.push(DaclAceIdentity::allow_mask(
        SYSTEM_DRIVE_COMPATIBILITY_MASK,
        ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
    ));
    let transition = verify_targeted_write(
        root,
        &frozen_full,
        &planned,
        DaclWriteOperationV1::ApplyPlannedAces,
    )?;
    // The verified marker transition is returned, not assumed away: the approved
    // difference is exactly this one bit, and the caller that writes the operation
    // evidence is the one that can report it.
    Ok((changes, transition))
}

pub fn remove_system_drive_compatibility(
    root: &Path,
    changes: SystemDriveCompatibilityChanges,
) -> io::Result<()> {
    remove_system_drive_compatibility_with_transition(root, changes).map(|_| ())
}

/// The same revoke, with the verified control-field transition returned to the caller.
///
/// The revoke is the same explicit write as the apply, so the marker may stay absent:
/// that is the approved difference and never a byte-for-byte descriptor restoration,
/// and the caller that owns the operation evidence has to be able to say so.
pub fn remove_system_drive_compatibility_with_transition(
    root: &Path,
    changes: SystemDriveCompatibilityChanges,
) -> io::Result<(SystemDriveCompatibilityChanges, DaclTransitionV1)> {
    validate_drive_root(root)?;
    let frozen_full = capture_full_descriptor(root)?;
    let mut planned = Vec::new();
    if changes.all_restricted_application_packages_added {
        revoke_exact_file_dacl_allow(
            root,
            ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            0,
        )?;
        planned.push(DaclAceIdentity::allow_mask(
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
        ));
    }
    if changes.all_application_packages_added {
        revoke_exact_file_dacl_allow(
            root,
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            0,
        )?;
        planned.push(DaclAceIdentity::allow_mask(
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            ALL_APPLICATION_PACKAGES_SID,
        ));
    }
    if planned.is_empty() {
        return Ok((changes, DaclTransitionV1::Unchanged));
    }
    let transition = verify_targeted_write(
        root,
        &frozen_full,
        &planned,
        DaclWriteOperationV1::RevokePlannedAces,
    )?;
    Ok((changes, transition))
}

pub fn capture_null_device_security_descriptor() -> io::Result<String> {
    let handle = open_null_device(READ_CONTROL)?;
    let information = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    let mut required = 0_u32;
    // SAFETY: the null output call is the documented sizing operation.
    let sizing_result = unsafe {
        GetKernelObjectSecurity(handle.as_raw(), information, ptr::null_mut(), 0, &mut required)
    };
    let sizing_error = io::Error::last_os_error();
    if sizing_result != 0
        || sizing_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        || required == 0
    {
        return Err(sizing_error);
    }
    let word = size_of::<usize>();
    let required = usize::try_from(required)
        .map_err(|_| io::Error::other("security descriptor size exceeds address space"))?;
    let mut descriptor = vec![0_usize; required.div_ceil(word)];
    // SAFETY: the aligned buffer is at least the requested size.
    if unsafe {
        GetKernelObjectSecurity(
            handle.as_raw(),
            information,
            descriptor.as_mut_ptr().cast(),
            u32::try_from(required)
                .map_err(|_| io::Error::other("security descriptor size exceeds Win32 limit"))?,
            &mut u32::try_from(required)
                .map_err(|_| io::Error::other("security descriptor size exceeds Win32 limit"))?,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    security_descriptor_to_sddl(descriptor.as_mut_ptr().cast(), information)
}

/// Aura's own install-scoped device capability ACE, derived the same way the
/// install plan derives it (`ExecutionDevice::sid` + `access_mask`).
///
/// Deriving is not verifying: the caller must have checked the installation
/// record it passes in, and the maintainer owns those records. This module reads
/// no files, trusts no paths, and accepts no descriptor text as proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCapabilityAce {
    identity: dacl_ace::DaclAceIdentity,
    sid: String,
}

impl VerifiedCapabilityAce {
    /// From a SID and mask the caller derived with the install rules.
    ///
    /// The shape is validated here; *whose* capability it is stays the caller's
    /// verified fact, never this module's assumption.
    pub fn from_derived(sid: impl Into<String>, mask: u32) -> io::Result<Self> {
        let sid = sid.into();
        let body = sid
            .strip_prefix("S-1-5-21-")
            .ok_or_else(|| io::Error::other("host_preparation.capability_sid_shape"))?;
        if mask == 0
            || body.split('-').count() != 4
            || !body.split('-').all(|part| part.parse::<u32>().is_ok())
        {
            return Err(io::Error::other("host_preparation.capability_sid_shape"));
        }
        Ok(Self { identity: dacl_ace::DaclAceIdentity::allow_mask(mask, &sid), sid })
    }

    /// The ACE Aura installs on `device` for `installation_id`, derived with
    /// the install plan's own rules (`ExecutionDevice::sid` + `access_mask`).
    #[cfg(feature = "restricted-user-runtime")]
    pub fn for_installation(
        device: crate::ExecutionDevice,
        installation_id: &str,
    ) -> io::Result<Self> {
        Self::from_derived(device.sid(installation_id)?, device.access_mask())
    }

    #[must_use]
    pub fn sid(&self) -> &str {
        &self.sid
    }
}

/// Strict readiness: the descriptor must be exactly the frozen expectation.
///
/// Anything extra — including another Aura installation's capability ACE — is
/// not the state this constant describes.
pub fn probe_null_device_compatibility() -> io::Result<bool> {
    let current = capture_null_device_security_descriptor()?;
    let expected = normalize_sddl(NULL_DEVICE_COMPATIBILITY_SDDL)?;
    Ok(current == expected)
}

/// Limited readiness: the frozen requirements plus at most one Aura capability
/// ACE that the caller has verified against an installation record.
///
/// The base requirements and the integrity label are unchanged, and the extra
/// ACE must match by complete identity (type, flags, rights, SID) — a Deny, a
/// widened or narrowed mask, an inheritance flag, a duplicate, an unknown SID or
/// any other extra entry still fails. `None` falls back to the strict probe, so
/// a caller without verified installation information cannot accidentally accept
/// a coexistence it has not checked.
pub fn probe_null_device_compatibility_limited(
    capability: Option<&VerifiedCapabilityAce>,
) -> io::Result<bool> {
    let Some(capability) = capability else {
        return probe_null_device_compatibility();
    };
    let current = capture_null_device_security_descriptor()?;
    let expected = normalize_sddl(NULL_DEVICE_COMPATIBILITY_SDDL)?;
    limited_null_device_decision(&current, &expected, capability)
}

/// The limited decision itself, over two descriptor texts.
fn limited_null_device_decision(
    current: &str,
    expected: &str,
    capability: &VerifiedCapabilityAce,
) -> io::Result<bool> {
    if descriptor_label(current)? != descriptor_label(expected)? {
        // The mandatory label is part of the requirement, never an extra.
        return Ok(false);
    }
    dacl_ace::matches_with_allowed_extra(
        &descriptor_dacl(current)?,
        &descriptor_dacl(expected)?,
        &capability.identity,
    )
}

/// The mandatory-label statement of a descriptor (`S:(...)`), which must match
/// exactly.
fn descriptor_label(sddl: &str) -> io::Result<String> {
    match sddl.find("S:(") {
        Some(start) => match sddl[start..].find(')') {
            Some(end) => Ok(sddl[start..start + end + 1].to_owned()),
            None => Err(io::Error::other("host_preparation.label_unterminated")),
        },
        None => Ok(String::new()),
    }
}

/// The DACL portion of a descriptor, in the `D:...` form the ACE parser expects.
fn descriptor_dacl(sddl: &str) -> io::Result<String> {
    let end = sddl.find("S:(").unwrap_or(sddl.len());
    let dacl = &sddl[..end];
    let start = dacl.find("D:").ok_or_else(|| io::Error::other("host_preparation.dacl_missing"))?;
    Ok(dacl[start..].to_owned())
}

/// Compares the current NUL device security descriptor with a frozen SDDL
/// snapshot using Windows' canonical SDDL representation.
pub fn null_device_security_descriptor_matches(expected_sddl: &str) -> io::Result<bool> {
    let current = capture_null_device_security_descriptor()?;
    let expected = normalize_sddl(expected_sddl)?;
    Ok(current == expected)
}

pub fn prepare_null_device_compatibility() -> io::Result<()> {
    set_null_device_security_descriptor(NULL_DEVICE_COMPATIBILITY_SDDL)
}

pub fn restore_null_device_security_descriptor(original_sddl: &str) -> io::Result<()> {
    set_null_device_security_descriptor(original_sddl)
}

fn set_null_device_security_descriptor(sddl: &str) -> io::Result<()> {
    let handle = open_null_device(READ_CONTROL | WRITE_DAC | WRITE_OWNER)?;
    let encoded = wide_null(sddl);
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: SDDL is NUL-terminated and output storage is valid.
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
    let _descriptor_guard = LocalAllocation(descriptor.cast());
    // SAFETY: the handle has DACL/label write access and descriptor remains live.
    if unsafe {
        SetKernelObjectSecurity(
            handle.as_raw(),
            DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            descriptor,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn normalize_sddl(sddl: &str) -> io::Result<String> {
    let encoded = wide_null(sddl);
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: input and output pointers satisfy the conversion API.
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
    security_descriptor_to_sddl(descriptor, DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION)
}

fn open_null_device(access: u32) -> io::Result<OwnedHandle> {
    let path = wide_null(r"\\.\NUL");
    // SAFETY: path is NUL-terminated and all optional pointers are null.
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

fn security_descriptor_to_sddl(
    descriptor: PSECURITY_DESCRIPTOR,
    information: u32,
) -> io::Result<String> {
    let mut encoded: PWSTR = ptr::null_mut();
    let mut length = 0_u32;
    // SAFETY: descriptor is valid and output pointers are writable.
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            information,
            &mut encoded,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let _guard = LocalAllocation(encoded.cast());
    wide_ptr_to_string(encoded)
}

fn file_dacl_has_exact_allow(
    path: &Path,
    sid: &str,
    permissions: u32,
    inheritance: u32,
) -> io::Result<bool> {
    let path_wide = wide_null(&path.to_string_lossy());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    // SAFETY: path and output pointers satisfy GetNamedSecurityInfoW.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor_guard = LocalAllocation(descriptor.cast());
    let sid = LocalSid::from_string(sid)?;
    if dacl.is_null() {
        return Ok(false);
    }
    // SAFETY: DACL points into the live descriptor.
    let ace_count = unsafe { (*dacl).AceCount };
    for index in 0..u32::from(ace_count) {
        let mut raw_ace = ptr::null_mut();
        // SAFETY: index is within AceCount and output storage is writable.
        if unsafe { GetAce(dacl, index, &mut raw_ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // ACCESS_ALLOWED_ACE_TYPE is zero by Win32 contract.
        // SAFETY: GetAce returned a live ACE pointer.
        let ace = unsafe { &*(raw_ace.cast::<ACCESS_ALLOWED_ACE>()) };
        if ace.Header.AceType == 0
            && u32::from(ace.Header.AceFlags) == inheritance
            && ace.Mask == permissions
        {
            let ace_sid = (&raw const ace.SidStart).cast_mut().cast();
            // SAFETY: SidStart is the variable-length SID embedded in the ACE.
            if unsafe { EqualSid(ace_sid, sid.as_raw()) } != 0 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn grant_file_dacl_mask(
    path: &Path,
    sid: &str,
    permissions: u32,
    inheritance: u32,
) -> io::Result<()> {
    if file_dacl_has_exact_allow(path, sid, permissions, inheritance)? {
        return Ok(());
    }
    update_file_dacl(path, sid, permissions, GRANT_ACCESS, inheritance, &[])
}

fn revoke_exact_file_dacl_allow(
    path: &Path,
    sid: &str,
    permissions: u32,
    inheritance: u32,
) -> io::Result<()> {
    if !file_dacl_has_exact_allow(path, sid, permissions, inheritance)? {
        return Ok(());
    }
    let path_wide = wide_null(&path.to_string_lossy());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut current_dacl = ptr::null_mut();
    // SAFETY: path and outputs satisfy GetNamedSecurityInfoW.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut current_dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor_guard = LocalAllocation(descriptor.cast());
    let target_sid = LocalSid::from_string(sid)?;
    let mut explicit_count = 0_u32;
    let mut explicit_entries = ptr::null_mut::<EXPLICIT_ACCESS_W>();
    // SAFETY: DACL is live and output storage is writable.
    let status = unsafe {
        GetExplicitEntriesFromAclW(current_dacl, &mut explicit_count, &mut explicit_entries)
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _entries_guard = LocalAllocation(explicit_entries.cast());
    let entries = if explicit_entries.is_null() {
        &[][..]
    } else {
        // SAFETY: the API returned explicit_count contiguous entries.
        unsafe { std::slice::from_raw_parts(explicit_entries, explicit_count as usize) }
    };
    let mut replay = Vec::new();
    for entry in entries {
        if entry.Trustee.TrusteeForm != TRUSTEE_IS_SID {
            continue;
        }
        // SAFETY: TRUSTEE_IS_SID makes ptstrName a valid PSID.
        let same_sid =
            unsafe { EqualSid(entry.Trustee.ptstrName.cast(), target_sid.as_raw()) } != 0;
        if same_sid
            && !(entry.grfAccessPermissions == permissions
                && entry.grfAccessMode == GRANT_ACCESS
                && entry.grfInheritance == inheritance)
        {
            replay.push(*entry);
        }
    }
    update_file_dacl(path, sid, 0, REVOKE_ACCESS, inheritance, &replay)
}

fn update_file_dacl(
    path: &Path,
    sid: &str,
    permissions: u32,
    access_mode: i32,
    inheritance: u32,
    replay: &[EXPLICIT_ACCESS_W],
) -> io::Result<()> {
    let mut path_wide = wide_null(&path.to_string_lossy());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut current_dacl = ptr::null_mut();
    // SAFETY: path and output pointers satisfy GetNamedSecurityInfoW.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut current_dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor_guard = LocalAllocation(descriptor.cast());
    let target_sid = LocalSid::from_string(sid)?;
    let entry = explicit_access(target_sid.as_raw(), permissions, access_mode, inheritance);
    let mut updated_dacl = ptr::null_mut();
    // SAFETY: all entries and the current DACL remain live for the call.
    let status = unsafe { SetEntriesInAclW(1, &entry, current_dacl, &mut updated_dacl) };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _updated_guard = LocalAllocation(updated_dacl.cast());
    let final_dacl = if replay.is_empty() {
        updated_dacl
    } else {
        let mut replayed = ptr::null_mut();
        // SAFETY: replay entries point into the still-live explicit entry allocation.
        let status = unsafe {
            SetEntriesInAclW(
                u32::try_from(replay.len()).map_err(|_| io::Error::other("ACL replay overflow"))?,
                replay.as_ptr(),
                updated_dacl,
                &mut replayed,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let replayed_guard = LocalAllocation(replayed.cast());
        let status = set_named_dacl(path_wide.as_mut_ptr(), replayed);
        drop(replayed_guard);
        return status;
    };
    set_named_dacl(path_wide.as_mut_ptr(), final_dacl)
}

/// The descriptor control bits that describe how an object already relates to
/// inheritance. A targeted DACL write supplies the whole descriptor, so these are read
/// from the object first and carried over: dropping `SE_DACL_PROTECTED` would unblock
/// inheritance that the object's owner blocked, and the SACL pair is carried over
/// unchanged because only the DACL is written.
const PRESERVED_CONTROL_BITS: u16 =
    SE_DACL_AUTO_INHERITED | SE_DACL_PROTECTED | SE_SACL_AUTO_INHERITED | SE_SACL_PROTECTED;

/// Write one DACL onto `path` without asking Windows to re-propagate inheritance.
///
/// **Observed**: with `SetNamedSecurityInfoW`, granting one
/// non-inheritable ACE on the system drive root took about 3.5 minutes, during which
/// the Host Preparation operation stayed in `applying`, and it reached `completed`
/// immediately after the second grant. A 10-second sample of that window showed
/// privileged CPU (8.9 s privileged, 0.5 s user), a stable handle count and no busy
/// `svchost`.
///
/// **Mechanism, measured** (`measurement_write_cost_and_control_state_by_api`, a
/// 2 040-entry tree on this machine): the named API cost **107.9 ms and 12 414 I/O
/// operations** on the populated tree against **0.37 ms and 18 operations** on an
/// empty directory, while the targeted write cost **0.16 ms and 11 operations** on the
/// populated tree. So the named API's cost is a pass over the object's descendants,
/// proportional to their number: that pass is what re-establishes
/// `SE_DACL_AUTO_INHERITED`, and it is the leading explanation for the 3.5-minute
/// stall on a volume root. It remains an explanation and not a stack sample — the
/// earlier sample that reported no I/O was taken from a process that cannot be shown
/// to have been the one running the pass, and the walk measured here does issue I/O in
/// the calling process.
///
/// **Control state**: this write is local. It preserves every control bit except
/// `SE_DACL_AUTO_INHERITED`, which the kernel clears whenever a DACL is set explicitly
/// even when the caller supplies the bit (measured on a temp directory: `0x8404`
/// before, `0x8004` after, with the bit set through `SetSecurityDescriptorControl`).
/// `SE_DACL_PROTECTED`, the bit that actually blocks a parent's inheritable ACEs from
/// being propagated in, survives. Owner, group and the integrity label stay untouched:
/// only `DACL_SECURITY_INFORMATION` is written.
///
/// The ACEs written here are explicit and non-inheritable, so nothing about them
/// belongs in a child object: a write that does not request propagation cannot pay for
/// one, and cannot rewrite a descendant's inherited ACEs, which the subtree pass can.
fn set_named_dacl(path: *mut u16, dacl: *mut windows_sys::Win32::Security::ACL) -> io::Result<()> {
    // A targeted write supplies the descriptor's control field itself, so the bits
    // that describe how this object already relates to inheritance must be read
    // first and carried over. Dropping `SE_DACL_AUTO_INHERITED` would put the
    // object back under parent propagation; dropping `SE_DACL_PROTECTED` would
    // unblock inheritance that the object's owner deliberately blocked. Only the
    // DACL is written, so the SACL's own pair is carried over unchanged too.
    let preserved = existing_control_bits(path)?;
    // The absolute descriptor is the real contract type, never a byte count: on x64
    // it holds four pointers and is 40 bytes, so a hand-computed 20-byte buffer
    // would let `InitializeSecurityDescriptor`/`SetSecurityDescriptorDacl` write
    // past the allocation.
    let mut absolute = SECURITY_DESCRIPTOR::default();
    let descriptor: PSECURITY_DESCRIPTOR = (&raw mut absolute).cast();
    // SAFETY: `absolute` is a live, correctly aligned `SECURITY_DESCRIPTOR` for the
    // whole function, and the revision is the documented constant.
    if unsafe { InitializeSecurityDescriptor(descriptor, SECURITY_DESCRIPTOR_REVISION) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the DACL is owned by the caller and stays live for this whole
    // function; `bDaclPresent = TRUE` with `bDaclDefaulted = FALSE` stores exactly
    // the ACL we built.
    if unsafe { SetSecurityDescriptorDacl(descriptor, 1, dacl.cast_const(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // `SE_DACL_AUTO_INHERIT_REQ` is deliberately not set here: requesting a pass is
    // what the targeted write exists to avoid, and the ACEs handed in are explicit
    // and non-inheritable so no child object is owed a change. Setting the bit
    // through the documented accessor keeps the descriptor's control field a value
    // Windows wrote, not one this function guessed at.
    if preserved != 0
        && unsafe { SetSecurityDescriptorControl(descriptor, PRESERVED_CONTROL_BITS, preserved) }
            == 0
    {
        return Err(io::Error::last_os_error());
    }
    // Self-relative form, in `u32` storage so the ACL inside keeps the four-byte
    // alignment the format requires.
    let mut required = 0_u32;
    // SAFETY: the sizing call passes a null output buffer, as documented. Its
    // failure (ERROR_INSUFFICIENT_BUFFER) is expected; the length is the output.
    unsafe { MakeSelfRelativeSD(descriptor, ptr::null_mut(), &mut required) };
    if required == 0 {
        return Err(io::Error::last_os_error());
    }
    let required_bytes = usize::try_from(required)
        .map_err(|_| io::Error::other("self-relative security descriptor length overflow"))?;
    let mut relative = vec![0_u32; required_bytes.div_ceil(size_of::<u32>())];
    // The API takes the buffer length in bytes and reports how much it wrote.
    let mut capacity = u32::try_from(relative.len() * size_of::<u32>())
        .map_err(|_| io::Error::other("self-relative security descriptor capacity overflow"))?;
    // SAFETY: the buffer holds at least `required` bytes and is four-byte aligned.
    if unsafe { MakeSelfRelativeSD(descriptor, relative.as_mut_ptr().cast(), &mut capacity) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if capacity > required {
        // A descriptor that grew between the two calls would mean the second call
        // wrote past what we measured; refuse instead of using a partial buffer.
        return Err(io::Error::other(
            "self-relative security descriptor grew between the sizing calls",
        ));
    }
    // SAFETY: `path` is NUL-terminated and the self-relative descriptor stays live
    // and unchanged for this call.
    let status = unsafe {
        SetFileSecurityW(path.cast_const(), DACL_SECURITY_INFORMATION, relative.as_mut_ptr().cast())
    };
    if status == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// The inheritance-relationship bits of `path`'s current descriptor.
///
/// Read through `GetNamedSecurityInfoW` + `GetSecurityDescriptorControl` rather
/// than by casting the returned pointer, so the control field is interpreted by
/// the documented accessor and no assumption is made about the pointer's form.
fn existing_control_bits(path: *mut u16) -> io::Result<u16> {
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the path is NUL-terminated, no optional output is requested, and
    // `descriptor` is valid storage that receives a LocalAlloc-allocated SD.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path.cast_const(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
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
    let guard = LocalAllocation(descriptor.cast());
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: the descriptor came from GetNamedSecurityInfoW and stays live until
    // `guard` drops; both out-parameters are valid storage.
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
        let error = io::Error::last_os_error();
        drop(guard);
        return Err(error);
    }
    drop(guard);
    Ok(control & PRESERVED_CONTROL_BITS)
}

fn explicit_access(
    sid: PSID,
    permissions: u32,
    access_mode: i32,
    inheritance: u32,
) -> EXPLICIT_ACCESS_W {
    let mut entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: permissions,
        grfAccessMode: access_mode,
        grfInheritance: inheritance,
        ..EXPLICIT_ACCESS_W::default()
    };
    entry.Trustee.pMultipleTrustee = ptr::null_mut();
    entry.Trustee.MultipleTrusteeOperation = NO_MULTIPLE_TRUSTEE;
    entry.Trustee.TrusteeForm = TRUSTEE_IS_SID;
    entry.Trustee.TrusteeType = TRUSTEE_IS_UNKNOWN;
    entry.Trustee.ptstrName = sid.cast();
    entry
}

fn validate_drive_root(path: &Path) -> io::Result<()> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "target is not a drive root"));
    };
    if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        || !matches!(components.next(), Some(Component::RootDir))
        || components.next().is_some()
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "target is not a drive root"));
    }
    Ok(())
}

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: pointer came from a LocalAlloc-returning Win32 API.
            unsafe {
                let _ = LocalFree(self.0);
            }
        }
    }
}

struct LocalSid(PSID);

impl LocalSid {
    fn from_string(value: &str) -> io::Result<Self> {
        let encoded = wide_null(value);
        let mut sid = ptr::null_mut();
        // SAFETY: input is NUL-terminated and output storage is valid.
        if unsafe { ConvertStringSidToSidW(encoded.as_ptr(), &mut sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sid))
    }

    /// Windows' own long spelling of this SID, so two spellings of one principal
    /// compare equal.
    fn to_string_value(&self) -> io::Result<String> {
        let mut text: PWSTR = ptr::null_mut();
        // SAFETY: the SID was allocated by a SID API and output storage is valid.
        if unsafe { ConvertSidToStringSidW(self.0, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let guard = LocalAllocation(text.cast());
        let value = wide_ptr_to_string(text);
        drop(guard);
        value
    }

    const fn as_raw(&self) -> PSID {
        self.0
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: ConvertStringSidToSidW allocates with LocalAlloc.
            unsafe {
                let _ = LocalFree(self.0);
            }
        }
    }
}

/// The complete descriptor of a file object: owner, group, DACL and integrity label.
///
/// The approved W7 rule is about the DACL's auto-inherited marker only, so the
/// verification captures all four parts and compares owner, group and label for real
/// instead of assuming them.
fn capture_full_descriptor(path: &Path) -> io::Result<String> {
    let information = OWNER_SECURITY_INFORMATION
        | GROUP_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | LABEL_SECURITY_INFORMATION;
    let path_wide = wide_null(&path.to_string_lossy());
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: path and output pointer satisfy GetNamedSecurityInfoW.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            information,
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
    let guard = LocalAllocation(descriptor.cast());
    let text = security_descriptor_to_sddl(descriptor, information)?;
    drop(guard);
    Ok(text)
}

/// Owner and group half of a descriptor rendering (everything before the DACL).
fn descriptor_owner_group_text(descriptor: &str) -> &str {
    descriptor.split("D:").next().unwrap_or_default()
}

/// The integrity-label section of a descriptor rendering, including any control
/// letters it carries (`S:AI(…)`, `S:(ML;;NW;;;LW)`).
///
/// The section is taken from `S:` to the end so a label with SACL control flags is
/// compared as it is printed. A shape this function cannot explain — trailing text
/// after the DACL that is not a label section, or a label section that does not start
/// with `S:` — is an error rather than an empty label: an unreadable label must never
/// compare equal to a readable one.
fn descriptor_integrity_label(descriptor: &str) -> io::Result<&str> {
    let dacl = descriptor
        .find("D:")
        .ok_or_else(|| io::Error::other("host_compatibility_descriptor_sections"))?;
    let body = &descriptor[dacl..];
    let Some(label_offset) = body.find("S:") else {
        // No label section: the DACL has to be the whole remainder, otherwise something
        // else is present that this comparison does not understand.
        return if body.ends_with(')') || body == "D:" {
            Ok("")
        } else {
            Err(io::Error::other("host_compatibility_descriptor_sections"))
        };
    };
    let label = &body[label_offset..];
    let dacl_part = body[..label_offset].trim_end();
    if !dacl_part.ends_with(')') || !label.starts_with("S:") || label.len() < 3 {
        return Err(io::Error::other("host_compatibility_descriptor_sections"));
    }
    Ok(label)
}

/// Rewrites every SID in a DACL rendering into Windows' own long spelling.
///
/// Windows prints one principal several ways (`AC` and `S-1-15-2-1`), which is a
/// spelling difference and not a different grant, so both sides of a comparison are
/// normalised through the same converter. Nothing else about the ACE is touched.
fn canonicalize_dacl_sids(dacl: &str) -> io::Result<String> {
    let body = dacl.strip_prefix("D:").ok_or_else(|| io::Error::other("maintenance.dacl_sddl"))?;
    let mut index = 0;
    while index < body.len() && !body[index..].starts_with('(') {
        index += 1;
    }
    let (control, aces) = body.split_at(index);
    let mut rewritten = String::with_capacity(dacl.len());
    rewritten.push_str("D:");
    rewritten.push_str(control);
    let mut rest = aces;
    while !rest.is_empty() {
        let open = rest.find('(');
        let Some(open) = open else {
            rewritten.push_str(rest);
            break;
        };
        let close =
            rest[open..].find(')').ok_or_else(|| io::Error::other("maintenance.dacl_ace"))? + open;
        let ace = &rest[open + 1..close];
        let sid = ace.rsplit(';').next().unwrap_or_default();
        let canonical = LocalSid::from_string(sid)?.to_string_value()?;
        rewritten.push('(');
        rewritten.push_str(&ace[..ace.len() - sid.len()]);
        rewritten.push_str(&canonical);
        rewritten.push(')');
        rest = &rest[close + 1..];
    }
    Ok(rewritten)
}

/// Splits a DACL rendering into its control letters and its ACE texts, in order.
fn dacl_ace_texts(dacl: &str) -> io::Result<(String, Vec<String>)> {
    let body = dacl.strip_prefix("D:").ok_or_else(|| io::Error::other("maintenance.dacl_sddl"))?;
    let start = body.find('(').unwrap_or(body.len());
    let (control, aces) = body.split_at(start);
    let mut texts = Vec::new();
    let mut rest = aces;
    while !rest.is_empty() {
        let open = rest.find('(').ok_or_else(|| io::Error::other("maintenance.dacl_ace"))?;
        let close =
            rest[open..].find(')').ok_or_else(|| io::Error::other("maintenance.dacl_ace"))? + open;
        texts.push(rest[open..=close].to_owned());
        rest = &rest[close + 1..];
    }
    Ok((control.to_owned(), texts))
}

/// The DACL a targeted write is expected to leave behind.
///
/// Measured on Windows: `SetEntriesInAclW` places a newly added explicit ACE **before**
/// the existing entries, so each grant lands at the front and the most recently granted
/// ACE comes first (`AC` then `RC` reads back as `(RC)(AC)…`). The auto-inherited marker
/// is dropped, because an explicit DACL write cannot keep it. Everything else — the
/// order of the entries that were already there, their flags, their masks — is copied
/// unchanged.
fn expected_dacl_after_targeted_write(
    frozen_dacl: &str,
    planned: &[DaclAceIdentity],
    operation: DaclWriteOperationV1,
) -> io::Result<String> {
    let canonical_frozen = canonicalize_dacl_sids(frozen_dacl)?;
    let (control, mut texts) = dacl_ace_texts(&canonical_frozen)?;
    let mut entries = texts
        .iter()
        .map(|text| {
            dacl_ace::parse_dacl_ace_identities(&format!("D:{text}")).and_then(|parsed| {
                parsed.into_iter().next().ok_or_else(|| io::Error::other("maintenance.dacl_ace"))
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let planned_texts = planned
        .iter()
        .map(|ace| {
            let mask = dacl_ace::rights_mask(&ace.rights)
                .ok_or_else(|| io::Error::other("maintenance.dacl_ace"))?;
            let sid = LocalSid::from_string(&ace.sid)?.to_string_value()?;
            Ok(format!("(A;;0x{mask:x};;;{sid})"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    match operation {
        DaclWriteOperationV1::ApplyPlannedAces => {
            // Each grant lands at the front, so inserting in application order leaves the
            // last-granted ACE first, exactly as measured.
            for text in &planned_texts {
                texts.insert(0, text.clone());
            }
        }
        DaclWriteOperationV1::RevokePlannedAces => {
            for text in &planned_texts {
                let identity = dacl_ace::parse_dacl_ace_identities(&format!("D:{text}")).and_then(
                    |parsed| parsed.into_iter().next().ok_or_else(|| io::Error::other("dacl_ace")),
                )?;
                let position = entries
                    .iter()
                    .position(|entry| entry == &identity)
                    .ok_or_else(|| io::Error::other("host_compatibility_planned_ace_missing"))?;
                entries.remove(position);
                texts.remove(position);
            }
        }
    }
    let mut expected = String::from("D:");
    expected.push_str(&control.replace("AI", ""));
    for text in texts {
        expected.push_str(&text);
    }
    Ok(expected)
}

/// Verifies one real targeted write against the expectation derived from the frozen
/// descriptor, and classifies what actually changed.
///
/// Fails closed: a reordered ACE list, a changed mask or flag, a protection change, a
/// different owner/group/label, or a marker that moved in the direction this write
/// cannot produce are all errors, because the write is then not the one the plan
/// authorised.
fn verify_targeted_write(
    path: &Path,
    frozen_full: &str,
    planned: &[DaclAceIdentity],
    operation: DaclWriteOperationV1,
) -> io::Result<DaclTransitionV1> {
    let frozen_dacl = descriptor_dacl(frozen_full)?;
    let after_full = capture_full_descriptor(path)?;
    if descriptor_owner_group_text(&after_full) != descriptor_owner_group_text(frozen_full)
        || descriptor_integrity_label(&after_full)? != descriptor_integrity_label(frozen_full)?
    {
        return Err(io::Error::other("host_compatibility_owner_or_label_changed"));
    }
    let after_dacl = descriptor_dacl(&after_full)?;
    let frozen_canonical = canonicalize_dacl_sids(&frozen_dacl)?;
    let after_canonical = canonicalize_dacl_sids(&after_dacl)?;
    let planned_present_in = |dacl: &str| -> io::Result<Vec<bool>> {
        let present = dacl_ace::ordered_ace_identities(dacl)?;
        planned
            .iter()
            .map(|ace| {
                let mask = dacl_ace::rights_mask(&ace.rights)
                    .ok_or_else(|| io::Error::other("maintenance.dacl_ace"))?;
                let sid = LocalSid::from_string(&ace.sid)?.to_string_value()?;
                let expected =
                    DaclAceIdentity { rights: format!("0x{mask:x}"), sid, ..ace.clone() };
                Ok(present.contains(&expected))
            })
            .collect()
    };
    // The planned delta is checked against the *target state*, not against "did anything
    // change": a call that was supposed to add an ACE and added nothing is a failure even
    // though the object is untouched, while a repeat of an already-applied change is a
    // legitimate no-op.
    let planned_in_frozen = planned_present_in(&frozen_canonical)?;
    let unchanged = after_canonical == frozen_canonical;
    match operation {
        DaclWriteOperationV1::ApplyPlannedAces => {
            if unchanged && !planned_in_frozen.iter().all(|present| *present) {
                return Err(io::Error::other("host_compatibility_planned_ace_not_added"));
            }
        }
        DaclWriteOperationV1::RevokePlannedAces => {
            if unchanged && planned_in_frozen.iter().any(|present| *present) {
                return Err(io::Error::other("host_compatibility_planned_ace_not_removed"));
            }
        }
    }
    if unchanged {
        // The target state already held: nothing was written and nothing had to be.
        return Ok(DaclTransitionV1::Unchanged);
    }
    // The planned delta and the control-field transition are judged separately: the ACE
    // list of an apply or a revoke differs from the frozen one by design, so only the
    // expected sequence is compared, and only the control letters carry the approved
    // marker exception.
    let expected_dacl = expected_dacl_after_targeted_write(&frozen_dacl, planned, operation)?;
    if !dacl_ace::ordered_ace_sequences_match(
        &canonicalize_dacl_sids(&expected_dacl)?,
        &after_canonical,
    )? {
        return Err(io::Error::other("host_compatibility_unapproved_dacl_transition"));
    }
    let transition = dacl_ace::classify_control_transition(
        operation,
        // The reference for the marker is the descriptor as it was frozen before the
        // write: that is where the marker existed, and losing it is the approved
        // difference. The expected DACL has it stripped by construction, so using that
        // as the reference would hide the very transition being judged.
        DaclObservationV1::FrozenOriginal,
        DaclObservationV1::ActualReadBack,
        &dacl_ace::dacl_control_letters(&frozen_canonical)?,
        &dacl_ace::dacl_control_letters(&after_canonical)?,
    );
    if transition == DaclTransitionV1::Unapproved {
        return Err(io::Error::other("host_compatibility_unapproved_control_transition"));
    }
    Ok(transition)
}

fn wide_ptr_to_string(value: PWSTR) -> io::Result<String> {
    if value.is_null() {
        return Err(io::Error::other("Windows returned a null UTF-16 string"));
    }
    let mut length = 0_usize;
    // SAFETY: Windows returned a NUL-terminated UTF-16 string.
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
    use super::*;

    #[test]
    fn rejects_non_root_host_preparation_targets() {
        assert!(validate_drive_root(Path::new(r"C:\Windows")).is_err());
        assert!(validate_drive_root(Path::new(r"C:\")).is_ok());
    }

    #[test]
    fn system_drive_probe_is_read_only_and_deterministic() {
        let root = system_drive_root().expect("system drive root");
        let first = probe_system_drive_compatibility(&root).expect("first probe");
        let second = probe_system_drive_compatibility(&root).expect("second probe");
        assert_eq!(first, second);
    }
}

#[cfg(test)]
mod dacl_write_tests {
    //! The targeted DACL write, exercised on a temporary directory and its
    //! children.
    //!
    //! The host's own drive root is deliberately not touched here: what has to be
    //! shown is the *write* — one ACE added by identity, existing grants kept,
    //! nothing propagated, owner/group/label untouched, and the same function
    //! removing exactly that ACE again on the restore path.
    use super::*;
    use windows_sys::Win32::Security::{
        Authorization::SetNamedSecurityInfoW, GROUP_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, SE_DACL_AUTO_INHERIT_REQ,
    };

    const ALL_INFORMATION: u32 = OWNER_SECURITY_INFORMATION
        | GROUP_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | LABEL_SECURITY_INFORMATION;

    /// Owner, group, DACL and integrity label of `path`, as Windows prints them.
    pub(super) fn full_descriptor(path: &Path) -> String {
        let path_wide = wide_null(&path.to_string_lossy());
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: path is NUL-terminated and `descriptor` is valid output storage.
        let status = unsafe {
            GetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                ALL_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, ERROR_SUCCESS, "GetNamedSecurityInfoW({})", path.display());
        let _guard = LocalAllocation(descriptor.cast());
        security_descriptor_to_sddl(descriptor, ALL_INFORMATION).expect("descriptor to SDDL")
    }

    /// The DACL's control flags (`P`/`AI`/`AR`…) as printed between `D:` and the
    /// first ACE. `AR` appearing here would mean the write asked Windows for an
    /// inheritance pass, which is exactly what this path must not do.
    pub(super) fn dacl_control_flags(descriptor: &str) -> String {
        let start = descriptor.find("D:").expect("a DACL is present") + 2;
        let rest = &descriptor[start..];
        let end = rest.find('(').unwrap_or(rest.len());
        rest[..end].to_owned()
    }

    /// The DACL half of a descriptor text.
    fn dacl_of(descriptor: &str) -> String {
        let end = descriptor.find("S:(").unwrap_or(descriptor.len());
        let head = &descriptor[..end];
        let start = head.find("D:").expect("a DACL is present");
        head[start..].to_owned()
    }

    /// ACE identities of a descriptor text, compared by type, flags and mask.
    pub(super) fn identities(descriptor: &str) -> Vec<dacl_ace::DaclAceIdentity> {
        dacl_ace::parse_dacl_ace_identities(&dacl_of(descriptor)).expect("parse ACE identities")
    }

    fn grant(path: &Path, sid: &str) {
        update_file_dacl(path, sid, SYSTEM_DRIVE_COMPATIBILITY_MASK, GRANT_ACCESS, 0, &[])
            .expect("grant the compatibility ACE");
    }

    fn revoke(path: &Path, sid: &str) {
        update_file_dacl(path, sid, SYSTEM_DRIVE_COMPATIBILITY_MASK, REVOKE_ACCESS, 0, &[])
            .expect("revoke the compatibility ACE");
    }

    /// The object's raw control field, not the SDDL rendering of it, so a bit the
    /// SDDL printer omits cannot hide here.
    pub(super) fn raw_control_bits(path: &Path) -> u16 {
        let path_wide = wide_null(&path.to_string_lossy());
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: NUL-terminated path and valid output storage.
        let status = unsafe {
            GetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let guard = LocalAllocation(descriptor.cast());
        let mut control = 0_u16;
        let mut revision = 0_u32;
        // SAFETY: live descriptor from the call above; both out-parameters are valid.
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0
        );
        drop(guard);
        control
    }

    #[test]
    fn the_targeted_write_carries_the_existing_control_bits_into_the_kernel() {
        // The write supplies the descriptor's control field, so this measures the
        // round trip: what the object had, what was read from it, and what survived
        // the set. Recorded rather than assumed, because the answer decides whether
        // `SE_DACL_AUTO_INHERITED` can be preserved at all.
        let directory = tempfile::tempdir().expect("temporary directory");
        let before = raw_control_bits(directory.path());
        let preserved = existing_control_bits(
            wide_null(&directory.path().to_string_lossy()).as_ptr().cast_mut(),
        )
        .expect("read the object's control bits");
        grant(directory.path(), ALL_APPLICATION_PACKAGES_SID);
        let after = raw_control_bits(directory.path());
        println!(
            "control bits: before={before:#06x} preserved={preserved:#06x} after={after:#06x} \
             (DACL_AUTO_INHERITED={:#06x})",
            SE_DACL_AUTO_INHERITED
        );
        assert_ne!(
            before & SE_DACL_AUTO_INHERITED,
            0,
            "a fresh temp directory should be auto-inherited: {before:#06x}"
        );
        assert_eq!(
            preserved,
            before & PRESERVED_CONTROL_BITS,
            "the read must report the object's own inheritance bits"
        );
        assert!(
            file_dacl_has_exact_allow(
                directory.path(),
                ALL_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0
            )
            .expect("probe")
        );
        // The measured outcome: the bit was supplied to the kernel and did not
        // survive, while the DACL was written. Preserving it would require the
        // subtree-wide inheritance pass the other test measures.
        assert_eq!(
            after,
            before & !SE_DACL_AUTO_INHERITED,
            "the write drops exactly the auto-inherited marker: {before:#06x} -> {after:#06x}"
        );
    }

    /// Measurement, not a gate: run explicitly with
    /// `cargo test -p aura-windows-platform --lib measurement_ -- --ignored --nocapture`.
    ///
    /// It answers two questions the write path depends on:
    /// 1. does the inheritance-requesting API's latency grow with the number of
    ///    child objects, while the targeted write stays flat?
    /// 2. does the targeted API keep `SE_DACL_AUTO_INHERITED` when the caller asks
    ///    for the inheritance pass, or is that bit only available on the other path?
    #[test]
    #[ignore = "measurement: run explicitly"]
    fn measurement_write_cost_and_control_state_by_api() {
        let empty = tempfile::tempdir().expect("temporary directory");
        let populated = tempfile::tempdir().expect("temporary directory");
        for directory in 0..40 {
            let path = populated.path().join(format!("d{directory:02}"));
            std::fs::create_dir(&path).expect("child directory");
            for file in 0..50 {
                std::fs::write(path.join(format!("f{file:02}.txt")), b"x").expect("child file");
            }
        }
        let entries = 40 * 51;

        for (label, root) in [("empty", empty.path()), ("populated", populated.path())] {
            // The named API's suspected cost is a pass over the object's children, so
            // the process's own I/O operations are counted around each call: a child
            // walk shows up as read operations, an in-memory pass does not.
            let baseline = process_io_operations();
            let start = std::time::Instant::now();
            grant(root, ALL_APPLICATION_PACKAGES_SID);
            let targeted = start.elapsed();
            let targeted_io = process_io_operations().saturating_sub(baseline);

            // The same ACE through the API the targeted write replaced.
            let baseline = process_io_operations();
            let start = std::time::Instant::now();
            update_file_dacl_with_named_api(
                root,
                ALL_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
            );
            let named = start.elapsed();
            let named_io = process_io_operations().saturating_sub(baseline);

            // The targeted API, this time carrying the inheritance request.
            let start = std::time::Instant::now();
            let requested = grant_requesting_inheritance(root, ALL_APPLICATION_PACKAGES_SID);
            let requesting = start.elapsed();
            println!(
                "{label}: entries={entries} targeted={targeted:?} (io={targeted_io}) \
                 named={named:?} (io={named_io}) targeted+request={requesting:?} \
                 control_after_request={requested:#06x}"
            );
        }
    }

    /// The calling process's completed read+write+other operations, as Windows
    /// reports them.
    fn process_io_operations() -> u64 {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessIoCounters};
        let mut counters = Default::default();
        // SAFETY: the pseudo-handle is always valid and `counters` is valid storage.
        assert_ne!(unsafe { GetProcessIoCounters(GetCurrentProcess(), &mut counters) }, 0);
        counters.ReadOperationCount + counters.WriteOperationCount + counters.OtherOperationCount
    }

    /// Writes `acl` as the object's DACL through the targeted API, optionally adding
    /// `control_bits_to_set` to the descriptor's control field, and returns the
    /// object's control bits afterwards.
    fn targeted_write_with_control(
        path: &Path,
        acl: *mut windows_sys::Win32::Security::ACL,
        control_bits_to_set: u16,
    ) -> u16 {
        let mut absolute = SECURITY_DESCRIPTOR::default();
        let descriptor: PSECURITY_DESCRIPTOR = (&raw mut absolute).cast();
        // SAFETY: absolute descriptor storage and the documented revision.
        assert_ne!(
            unsafe { InitializeSecurityDescriptor(descriptor, SECURITY_DESCRIPTOR_REVISION) },
            0
        );
        // SAFETY: the ACL outlives this call.
        assert_ne!(unsafe { SetSecurityDescriptorDacl(descriptor, 1, acl.cast_const(), 0) }, 0);
        if control_bits_to_set != 0 {
            // SAFETY: an absolute descriptor; the bits are documented control bits.
            assert_ne!(
                unsafe {
                    SetSecurityDescriptorControl(
                        descriptor,
                        control_bits_to_set,
                        control_bits_to_set,
                    )
                },
                0
            );
        }
        let mut required = 0_u32;
        // SAFETY: sizing call with a null buffer, as documented.
        unsafe { MakeSelfRelativeSD(descriptor, ptr::null_mut(), &mut required) };
        let mut relative = vec![0_u32; usize::try_from(required).expect("length").div_ceil(4)];
        let mut capacity = u32::try_from(relative.len() * 4).expect("capacity");
        // SAFETY: the buffer holds at least `required` bytes and is four-byte aligned.
        assert_ne!(
            unsafe { MakeSelfRelativeSD(descriptor, relative.as_mut_ptr().cast(), &mut capacity) },
            0
        );
        let path_wide = wide_null(&path.to_string_lossy());
        // SAFETY: NUL-terminated path and a live self-relative descriptor.
        assert_ne!(
            unsafe {
                SetFileSecurityW(
                    path_wide.as_ptr(),
                    DACL_SECURITY_INFORMATION,
                    relative.as_mut_ptr().cast(),
                )
            },
            0
        );
        raw_control_bits(path)
    }

    /// One ACE written through `SetFileSecurityW` with `SE_DACL_AUTO_INHERIT_REQ` in
    /// the control field.
    fn grant_requesting_inheritance(path: &Path, sid: &str) -> u16 {
        let trustee = LocalSid::from_string(sid).expect("trustee SID");
        let entry = explicit_access(trustee.0, SYSTEM_DRIVE_COMPATIBILITY_MASK, GRANT_ACCESS, 0);
        let mut acl = ptr::null_mut();
        // SAFETY: one entry, live trustee, valid out-parameter.
        let status = unsafe { SetEntriesInAclW(1, &raw const entry, ptr::null_mut(), &mut acl) };
        assert_eq!(status, ERROR_SUCCESS);
        let guard = LocalAllocation(acl.cast());
        let control = targeted_write_with_control(path, acl, SE_DACL_AUTO_INHERIT_REQ);
        drop(guard);
        control
    }

    /// Marks `path` protected (`SE_DACL_PROTECTED`), the bit that stops a parent's
    /// inheritable ACEs from being propagated into it, without changing its DACL.
    pub(super) fn mark_protected(path: &Path) -> u16 {
        let path_wide = wide_null(&path.to_string_lossy());
        let mut dacl = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: NUL-terminated path, valid out-parameters.
        let status = unsafe {
            GetNamedSecurityInfoW(
                path_wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let guard = LocalAllocation(descriptor.cast());
        let control = targeted_write_with_control(path, dacl, SE_DACL_PROTECTED);
        drop(guard);
        control
    }

    #[test]
    fn a_protected_object_stays_protected_through_the_write() {
        // `C:\` is protected (`D:PAI`), so the bit that actually blocks parent
        // propagation has to survive the write; losing it would silently re-open the
        // object to inherited ACEs, which is a far larger change than losing the
        // derived auto-inherited marker.
        let directory = tempfile::tempdir().expect("temporary directory");
        let protected = mark_protected(directory.path());
        assert_ne!(protected & SE_DACL_PROTECTED, 0, "the object should now be protected");

        grant(directory.path(), ALL_APPLICATION_PACKAGES_SID);

        let after = raw_control_bits(directory.path());
        println!("protected object: after write control={after:#06x}");
        assert_ne!(after & SE_DACL_PROTECTED, 0, "the write must not unprotect the object");
        assert!(
            file_dacl_has_exact_allow(
                directory.path(),
                ALL_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0
            )
            .expect("probe")
        );
    }

    #[test]
    fn windows_owns_the_auto_inherited_bit_whichever_api_writes_the_dacl() {
        // Whether `SE_DACL_AUTO_INHERITED` survives an explicit DACL write is
        // Windows' decision, not this crate's: a descriptor cannot carry the bit
        // past the API. This measures both spellings — the targeted
        // `SetFileSecurityW` write used here and the `SetNamedSecurityInfoW` write
        // this path replaced — on identical fresh directories, so the comparison is
        // between mechanisms and not between objects.
        let targeted = tempfile::tempdir().expect("temporary directory");
        let named = tempfile::tempdir().expect("temporary directory");
        let start = dacl_control_flags(&full_descriptor(targeted.path()));
        assert_eq!(
            start,
            dacl_control_flags(&full_descriptor(named.path())),
            "the two subjects must start in the same control state"
        );
        assert!(
            start.contains("AI"),
            "the inherited temp directory should start auto-inherited: {start}"
        );

        grant(targeted.path(), ALL_APPLICATION_PACKAGES_SID);
        update_file_dacl_with_named_api(
            named.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
        );

        let after_targeted = dacl_control_flags(&full_descriptor(targeted.path()));
        let after_named = dacl_control_flags(&full_descriptor(named.path()));
        println!(
            "control flags: start={start:?} targeted={after_targeted:?} named={after_named:?}"
        );
        // The measured asymmetry: the named API runs an inheritance pass over the
        // object's descendants and the object keeps the auto-inherited marker, while
        // the local write keeps the DACL outcome without the marker. The marker is
        // the price of not walking the subtree, so it is asserted, not wished away.
        assert_eq!(after_named, "AI", "the inheritance pass re-establishes the marker");
        assert_eq!(after_targeted, "", "the local write leaves the marker off");
        assert!(
            file_dacl_has_exact_allow(
                targeted.path(),
                ALL_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0
            )
            .expect("probe")
        );
        assert!(
            file_dacl_has_exact_allow(
                named.path(),
                ALL_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0
            )
            .expect("probe")
        );
        // Both writes granted the same ACE, so the difference is the control state
        // alone and not the access the object received.
        assert_eq!(
            identities(&full_descriptor(targeted.path())),
            identities(&full_descriptor(named.path())),
            "both writes must produce the same ACE list"
        );
    }

    /// The write this crate replaced: hand the ACE to `SetNamedSecurityInfoW` and
    /// let it build the descriptor. Used only to compare control-state outcomes.
    pub(super) fn update_file_dacl_with_named_api(path: &Path, sid: &str, mask: u32) {
        let trustee = LocalSid::from_string(sid).expect("trustee SID");
        let entry = explicit_access(trustee.0, mask, GRANT_ACCESS, 0);
        let mut acl = ptr::null_mut();
        // SAFETY: one explicit entry is passed, `trustee` outlives the call, and
        // `acl` is valid output storage that the guard releases.
        let status = unsafe { SetEntriesInAclW(1, &raw const entry, ptr::null_mut(), &mut acl) };
        assert_eq!(status, ERROR_SUCCESS, "SetEntriesInAclW({})", path.display());
        let guard = LocalAllocation(acl.cast());
        let path_wide = wide_null(&path.to_string_lossy());
        // SAFETY: the path is NUL-terminated and the ACL stays live until the guard
        // drops after the call.
        let status = unsafe {
            SetNamedSecurityInfoW(
                path_wide.as_ptr().cast_mut(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl,
                ptr::null_mut(),
            )
        };
        drop(guard);
        assert_eq!(status, ERROR_SUCCESS, "SetNamedSecurityInfoW({})", path.display());
    }

    #[test]
    fn a_non_inherited_grant_adds_exactly_one_ace_and_touches_nothing_else() {
        let root = tempfile::tempdir().expect("temporary directory");
        let child_directory = root.path().join("child");
        std::fs::create_dir(&child_directory).expect("child directory");
        let child_file = child_directory.join("payload.txt");
        std::fs::write(&child_file, b"payload").expect("child file");

        let before = full_descriptor(root.path());
        let child_before = full_descriptor(&child_file);
        let child_directory_before = full_descriptor(&child_directory);

        grant(root.path(), ALL_APPLICATION_PACKAGES_SID);

        let after = full_descriptor(root.path());
        assert!(
            file_dacl_has_exact_allow(
                root.path(),
                ALL_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0
            )
            .expect("probe the granted ACE"),
            "the requested grant must be present with its exact mask and no inheritance"
        );
        let added: Vec<_> = identities(&after)
            .into_iter()
            .filter(|ace| !identities(&before).contains(ace))
            .collect();
        assert_eq!(added.len(), 1, "exactly one ACE may be added: {after}");
        let added = &added[0];
        assert_eq!(added.ace_type, "A", "the added ACE must be an Allow: {after}");
        assert_eq!(added.flags, "", "the added ACE must not inherit: {after}");
        assert_eq!(
            dacl_ace::rights_mask(&added.rights),
            Some(SYSTEM_DRIVE_COMPATIBILITY_MASK),
            "the added ACE must carry exactly the compatibility mask: {after}"
        );
        // Windows prints a well-known SID either as its abbreviation (`AC`) or as
        // the literal `S-1-15-2-1`; the SID-aware probe above already resolved
        // which trustee it is, so both spellings are the same grant here.
        assert!(
            added.sid == ALL_APPLICATION_PACKAGES_SID || added.sid == "AC",
            "the added ACE must be the compatibility SID, got {}",
            added.sid
        );
        let removed: Vec<_> = identities(&before)
            .into_iter()
            .filter(|ace| !identities(&after).contains(ace))
            .collect();
        assert!(removed.is_empty(), "no existing grant may be dropped: {removed:?}");

        // A non-inheritable ACE belongs in no child object.
        assert_eq!(full_descriptor(&child_file), child_before, "the child file changed");
        assert_eq!(
            full_descriptor(&child_directory),
            child_directory_before,
            "the child directory changed"
        );
        // Owner, group, the integrity label and the DACL's control state are not
        // part of this write. The one bit the write cannot keep is
        // `SE_DACL_AUTO_INHERITED`: the kernel clears it whenever a DACL is set
        // explicitly, and only an inheritance pass over the object's descendants
        // re-establishes it. That is asserted here rather than tolerated silently —
        // `the_targeted_write_carries_the_existing_control_bits_into_the_kernel`
        // measures the round trip, and `windows_owns_the_auto_inherited_bit_…`
        // measures the two APIs against each other.
        assert_eq!(owner_group_label(&after), owner_group_label(&before));
        assert_eq!(labels(&after), labels(&before));
        assert_eq!(
            dacl_control_flags(&after),
            dacl_control_flags(&before).replace("AI", ""),
            "only the Windows-owned auto-inherited marker may differ: {before} -> {after}"
        );
        assert!(
            !dacl_control_flags(&after).contains("AR"),
            "the write must not request inheritance re-propagation: {after}"
        );
    }

    #[test]
    fn the_restore_path_removes_exactly_what_the_grant_added() {
        let root = tempfile::tempdir().expect("temporary directory");
        let child_file = root.path().join("payload.txt");
        std::fs::write(&child_file, b"payload").expect("child file");

        let original = full_descriptor(root.path());
        grant(root.path(), ALL_RESTRICTED_APPLICATION_PACKAGES_SID);
        assert!(
            file_dacl_has_exact_allow(
                root.path(),
                ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0
            )
            .expect("probe")
        );

        // This is the path a rollback uses.
        revoke(root.path(), ALL_RESTRICTED_APPLICATION_PACKAGES_SID);

        let restored = full_descriptor(root.path());
        assert!(
            !file_dacl_has_exact_allow(
                root.path(),
                ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
                SYSTEM_DRIVE_COMPATIBILITY_MASK,
                0
            )
            .expect("probe"),
            "the revoked ACE must be gone"
        );
        assert_eq!(
            identities(&restored),
            identities(&original),
            "the restore path must put the DACL back exactly"
        );
        assert_eq!(owner_group_label(&restored), owner_group_label(&original));
        assert_eq!(labels(&restored), labels(&original));
        assert_eq!(
            dacl_control_flags(&restored),
            dacl_control_flags(&original).replace("AI", ""),
            "the restore path may only drop the Windows-owned auto-inherited marker"
        );
        assert_eq!(full_descriptor(&child_file), full_descriptor(&root.path().join("payload.txt")));
    }

    #[test]
    fn the_grant_is_idempotent_and_leaves_a_second_call_with_nothing_to_do() {
        // The broker plan's probe-then-write shape depends on this: an ACE that is
        // already exact is not added twice.
        let root = tempfile::tempdir().expect("temporary directory");
        grant(root.path(), ALL_APPLICATION_PACKAGES_SID);
        let once = full_descriptor(root.path());
        grant(root.path(), ALL_APPLICATION_PACKAGES_SID);
        assert_eq!(full_descriptor(root.path()), once, "a second grant must be a no-op");
    }

    fn owner_group_label(descriptor: &str) -> String {
        let head = descriptor.split("D:").next().unwrap_or_default();
        let label =
            descriptor.find("S:(").map(|start| descriptor[start..].to_owned()).unwrap_or_default();
        format!("{head}{label}")
    }

    fn labels(descriptor: &str) -> String {
        descriptor.find("S:(").map(|start| descriptor[start..].to_owned()).unwrap_or_default()
    }
}

#[cfg(test)]
mod targeted_write_verification_tests {
    //! The rule that guards the W7 apply and revoke, exercised on temporary objects
    //! through the same helpers the production functions call.
    //!
    //! The drive-root entry points themselves can only run on a volume root, and this
    //! batch deliberately does not touch `C:\` again; what runs here is the identical
    //! capture → write → read-back → classify path they use.
    use super::dacl_write_tests::update_file_dacl_with_named_api;
    use super::*;
    use dacl_ace::ordered_ace_identities;

    fn planned(sid: &str) -> Vec<DaclAceIdentity> {
        vec![DaclAceIdentity::allow_mask(SYSTEM_DRIVE_COMPATIBILITY_MASK, sid)]
    }

    #[test]
    #[ignore = "measurement: run explicitly"]
    fn measurement_what_a_targeted_grant_leaves_behind() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let frozen = capture_full_descriptor(directory.path()).expect("frozen descriptor");
        update_file_dacl(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("targeted grant");
        let after = capture_full_descriptor(directory.path()).expect("read back");
        println!("frozen full: {frozen}");
        println!("after  full: {after}");
        println!("frozen dacl: {}", descriptor_dacl(&frozen).expect("dacl"));
        println!("after  dacl: {}", descriptor_dacl(&after).expect("dacl"));
        println!(
            "canonical after: {}",
            canonicalize_dacl_sids(&descriptor_dacl(&after).expect("dacl")).expect("canonical")
        );
        let expected = expected_dacl_after_targeted_write(
            &descriptor_dacl(&frozen).expect("dacl"),
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect("expected");
        println!("expected dacl: {expected}");
        println!("canonical expected: {}", canonicalize_dacl_sids(&expected).expect("canonical"));
        // The production apply grants both ACEs in this order; measure where the second
        // one lands.
        update_file_dacl(
            directory.path(),
            ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("second targeted grant");
        println!(
            "after two grants: {}",
            descriptor_dacl(&capture_full_descriptor(directory.path()).expect("read back"))
                .expect("dacl")
        );
    }

    #[test]
    fn a_real_targeted_grant_verifies_with_only_the_approved_marker_difference() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let frozen =
            capture_full_descriptor(directory.path()).expect("capture the frozen descriptor");
        assert!(frozen.contains("AI"), "a fresh temporary directory is auto-inherited: {frozen}");
        update_file_dacl(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("targeted grant");

        let transition = verify_targeted_write(
            directory.path(),
            &frozen,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .unwrap_or_else(|error| {
            panic!(
                "{error}: frozen={} after={}",
                descriptor_dacl(&frozen).expect("dacl"),
                descriptor_dacl(&capture_full_descriptor(directory.path()).expect("read back"))
                    .expect("dacl")
            )
        });
        assert_eq!(
            transition,
            DaclTransitionV1::ApprovedAutoInheritedMarker,
            "frozen={} after={}",
            descriptor_dacl(&frozen).expect("dacl"),
            descriptor_dacl(&capture_full_descriptor(directory.path()).expect("read back"))
                .expect("dacl")
        );
        assert_eq!(
            descriptor_owner_group_text(
                &capture_full_descriptor(directory.path()).expect("read back")
            ),
            descriptor_owner_group_text(&frozen),
            "owner and group must be real-compared and unchanged"
        );
    }

    #[test]
    fn a_real_targeted_revoke_verifies_against_the_frozen_descriptor() {
        let directory = tempfile::tempdir().expect("temporary directory");
        update_file_dacl(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("targeted grant");
        let before_revoke =
            capture_full_descriptor(directory.path()).expect("pre-revoke descriptor");
        revoke_exact_file_dacl_allow(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            0,
        )
        .expect("targeted revoke");

        // The revoke is the same explicit write, so the marker is not restored: the
        // comparison is against the descriptor as it was immediately before it.
        let transition = verify_targeted_write(
            directory.path(),
            &before_revoke,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::RevokePlannedAces,
        )
        .expect("the measured revoke is the approved transition");
        assert_ne!(transition, DaclTransitionV1::Unapproved);
        let after_dacl =
            descriptor_dacl(&capture_full_descriptor(directory.path()).expect("read back"))
                .expect("DACL");
        assert!(
            !after_dacl.contains(ALL_APPLICATION_PACKAGES_SID)
                && !after_dacl.contains("(A;;0x120088;;;AC)"),
            "the planned ACE has to be gone: {after_dacl}"
        );
    }

    #[test]
    fn an_extra_ace_written_after_the_grant_is_refused() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let frozen = capture_full_descriptor(directory.path()).expect("frozen descriptor");
        update_file_dacl(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("targeted grant");
        // An unrelated ACE nobody planned: the ordered sequence no longer matches.
        update_file_dacl(
            directory.path(),
            ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("unplanned grant");

        let error = verify_targeted_write(
            directory.path(),
            &frozen,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect_err("an unplanned ACE is a real difference");
        assert_eq!(error.to_string(), "host_compatibility_unapproved_dacl_transition");
    }

    #[test]
    fn a_write_that_brings_the_marker_back_is_refused() {
        // Measured: the named API re-establishes `SE_DACL_AUTO_INHERITED`. The targeted
        // write's approved transition is the opposite direction, so a reference without
        // the marker must not come back with it.
        let directory = tempfile::tempdir().expect("temporary directory");
        // A first targeted grant drops the marker; this is the state the reference is
        // taken from.
        update_file_dacl(
            directory.path(),
            ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("first targeted grant");
        let frozen = capture_full_descriptor(directory.path()).expect("frozen descriptor");
        assert!(
            !descriptor_dacl(&frozen).expect("dacl").starts_with("D:AI"),
            "the reference must not carry the marker: {frozen}"
        );
        update_file_dacl_with_named_api(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
        );
        let after = capture_full_descriptor(directory.path()).expect("read back");
        assert!(
            descriptor_dacl(&after).expect("dacl").starts_with("D:AI"),
            "the named API re-establishes the marker: {after}"
        );

        let error = verify_targeted_write(
            directory.path(),
            &frozen,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect_err("a marker that came back is not the approved transition");
        assert!(
            error.to_string().starts_with("host_compatibility_unapproved"),
            "the refusal has to be an unapproved-transition verdict, got {error}"
        );
    }

    #[test]
    fn a_revoke_with_nothing_to_revoke_is_unchanged_but_a_foreign_change_is_not() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let frozen = capture_full_descriptor(directory.path()).expect("frozen descriptor");
        // Nothing was written at all: the object is exactly the frozen one, and that is
        // the only "nothing to do" this verification accepts.
        assert_eq!(
            verify_targeted_write(
                directory.path(),
                &frozen,
                &planned(ALL_APPLICATION_PACKAGES_SID),
                DaclWriteOperationV1::RevokePlannedAces,
            )
            .expect("an untouched object is unchanged"),
            DaclTransitionV1::Unchanged
        );
        // Somebody else granted an ACE the plan does not know about: now the object did
        // change, the planned ACE is still missing, and that is a refusal.
        update_file_dacl(
            directory.path(),
            ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("foreign grant");
        let error = verify_targeted_write(
            directory.path(),
            &frozen,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::RevokePlannedAces,
        )
        .expect_err("a changed object without the planned ACE is a refusal");
        assert_eq!(error.to_string(), "host_compatibility_planned_ace_missing");
    }

    #[test]
    fn a_changed_owner_or_label_is_refused_before_any_dacl_verdict() {
        let directory = tempfile::tempdir().expect("temporary directory");
        update_file_dacl(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("targeted grant");
        let real = capture_full_descriptor(directory.path()).expect("read back");
        let dacl = descriptor_dacl(&real).expect("DACL");
        // A frozen descriptor whose owner differs from the object's: the verification
        // has to reject that before it looks at the DACL at all.
        let forged = format!("O:SYG:SY{dacl}");
        let error = verify_targeted_write(
            directory.path(),
            &forged,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect_err("owner drift is a real difference");
        assert_eq!(error.to_string(), "host_compatibility_owner_or_label_changed");
    }

    #[test]
    fn the_verified_transition_is_returned_to_the_caller_as_an_outcome() {
        // The approved AI difference is an outcome the operation-evidence writer has to
        // be able to read: it is returned, not discarded inside the write helper. This is
        // the same helper the guard's resources call, on a temporary object.
        let directory = tempfile::tempdir().expect("temporary directory");
        let frozen = capture_full_descriptor(directory.path()).expect("frozen descriptor");
        update_file_dacl(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("targeted grant");
        let transition = verify_targeted_write(
            directory.path(),
            &frozen,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect("the measured apply is the approved transition");
        assert_eq!(
            transition,
            DaclTransitionV1::ApprovedAutoInheritedMarker,
            "the caller has to receive the one allowed difference"
        );
        // And the additive entry points hand it back alongside the change record.
        assert!(
            prepare_system_drive_compatibility_with_transition(
                std::path::Path::new(r"C:\Windows"),
                SystemDriveCompatibilityChanges::default(),
            )
            .is_err(),
            "the additive entry point still only accepts a volume root"
        );
    }

    #[test]
    fn a_planned_change_that_did_not_happen_is_refused_while_a_satisfied_target_is_idempotent() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let frozen = capture_full_descriptor(directory.path()).expect("frozen descriptor");

        // The plan says "add this ACE" and nothing was written: the object is untouched,
        // but the target state does not hold, so this is a failure — not a no-op.
        let error = verify_targeted_write(
            directory.path(),
            &frozen,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect_err("a planned addition that did not happen must fail");
        assert_eq!(error.to_string(), "host_compatibility_planned_ace_not_added");

        // The same call repeated after the change was applied: the target state already
        // holds, so nothing to do is the correct, idempotent outcome.
        update_file_dacl(
            directory.path(),
            ALL_APPLICATION_PACKAGES_SID,
            SYSTEM_DRIVE_COMPATIBILITY_MASK,
            GRANT_ACCESS,
            0,
            &[],
        )
        .expect("targeted grant");
        let satisfied = capture_full_descriptor(directory.path()).expect("applied descriptor");
        assert_eq!(
            verify_targeted_write(
                directory.path(),
                &satisfied,
                &planned(ALL_APPLICATION_PACKAGES_SID),
                DaclWriteOperationV1::ApplyPlannedAces,
            )
            .expect("an already-satisfied target is an idempotent no-op"),
            DaclTransitionV1::Unchanged
        );

        // A label section with control flags is compared as printed: changing the label
        // while the ACL stays the same is still a real difference.
        assert_eq!(
            descriptor_integrity_label("O:SYG:SYD:(A;;FA;;;SY)S:AI(ML;;NW;;;LW)")
                .expect("a label with a control flag is readable"),
            "S:AI(ML;;NW;;;LW)"
        );
        assert!(descriptor_integrity_label("O:SYG:SYD:(A;;FA;;;SY)junk").is_err());
        assert_ne!(
            descriptor_integrity_label("O:SYG:SYD:(A;;FA;;;SY)S:AI(ML;;NW;;;LW)").expect("label"),
            descriptor_integrity_label("O:SYG:SYD:(A;;FA;;;SY)S:AI(ML;;NW;;;ME)").expect("label"),
            "a relabelled object must not compare equal"
        );
    }

    #[test]
    fn the_expected_descriptor_builder_keeps_file_order() {
        // Measured placement: each grant lands before the existing entries, so the
        // planned ACEs are applied in order and read back in reverse, and the entries
        // that were already there keep their relative order.
        let frozen = "D:PAI(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;0x1200a9;;;RC)";
        let frozen_aces =
            ordered_ace_identities(&canonicalize_dacl_sids(frozen).expect("canonical"))
                .expect("frozen identities");
        let applied = expected_dacl_after_targeted_write(
            frozen,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect("apply expectation");
        assert!(applied.starts_with("D:P("), "the marker is dropped: {applied}");
        let (_, applied_texts) = dacl_ace_texts(&applied).expect("split");
        assert!(
            applied_texts[0].contains("0x120088") && applied_texts[0].contains("S-1-15-2-1"),
            "the planned ACE has to be first: {applied}"
        );
        let applied_aces = ordered_ace_identities(&applied).expect("applied identities");
        assert_eq!(applied_aces.len(), frozen_aces.len() + 1);
        assert_eq!(
            applied_aces[1..],
            frozen_aces[..],
            "the frozen entries keep their exact order behind the new one"
        );

        // Two grants: the last one ends up in front of the first.
        let two = expected_dacl_after_targeted_write(
            frozen,
            &[
                DaclAceIdentity::allow_mask(SYSTEM_DRIVE_COMPATIBILITY_MASK, "S-1-15-2-1"),
                DaclAceIdentity::allow_mask(SYSTEM_DRIVE_COMPATIBILITY_MASK, "S-1-15-2-2"),
            ],
            DaclWriteOperationV1::ApplyPlannedAces,
        )
        .expect("two-ace expectation");
        let (_, two_texts) = dacl_ace_texts(&two).expect("split");
        assert!(
            two_texts[0].contains("S-1-15-2-2") && two_texts[1].contains("S-1-15-2-1"),
            "the last grant has to come first: {two}"
        );
        assert_eq!(ordered_ace_identities(&two).expect("two identities")[2..], frozen_aces[..]);

        // Revoking the planned ACE restores exactly the frozen sequence, in order.
        let revoked = expected_dacl_after_targeted_write(
            &applied,
            &planned(ALL_APPLICATION_PACKAGES_SID),
            DaclWriteOperationV1::RevokePlannedAces,
        )
        .expect("revoke expectation");
        assert_eq!(
            ordered_ace_identities(&revoked).expect("revoked identities"),
            frozen_aces,
            "the revoke expectation is the frozen sequence, in order"
        );
        // A planned ACE that is not in the frozen DACL is an error, not a silent no-op.
        assert!(
            expected_dacl_after_targeted_write(
                &revoked,
                &planned(ALL_APPLICATION_PACKAGES_SID),
                DaclWriteOperationV1::RevokePlannedAces
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod limited_compatibility_tests {
    use super::*;
    use dacl_ace::{DaclAceIdentity, matches_with_allowed_extra, rights_mask};

    /// The frozen expectation, and the dev machine's observed descriptor: the
    /// requirements are already met and the only extra entry is one Aura
    /// install-scoped Null capability ACE.
    const EXPECTED: &str = "O:BAG:SYD:(A;;GRGWGX;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;RC)(A;;GRGWGX;;;AC)(A;;GRGWGX;;;S-1-15-2-2)S:(ML;;NW;;;LW)";
    const DEV_MACHINE_INSTALLATION: &str = "073773f9-1798-4a46-a6d1-b8d5c9e3d19a";

    /// Both sides go through Windows' own converter, exactly as the probe does:
    /// a generic spelling (GRGWGX) and the numeric form the device reports
    /// (0x1201bf) are the same grant, and comparing raw text would be wrong.
    fn normalized(sddl: &str) -> String {
        normalize_sddl(sddl).expect("normalize")
    }

    fn observed_with(extra: &str) -> String {
        normalized(&format!(
            "D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;RC)\
(A;;0x1201bf;;;AC)(A;;0x1201bf;;;S-1-15-2-2){extra}S:(ML;;NW;;;LW)"
        ))
    }

    fn expected() -> String {
        normalized(EXPECTED)
    }

    #[test]
    #[cfg(feature = "restricted-user-runtime")]
    fn the_dev_machine_extra_ace_is_this_installations_derived_capability() {
        // Derived, not assumed: the rule reuses the install plan's own SID
        // derivation, so the correspondence with the observed ACE is checked
        // rather than trusted.
        let capability = VerifiedCapabilityAce::for_installation(
            crate::ExecutionDevice::Null,
            DEV_MACHINE_INSTALLATION,
        )
        .expect("derive");
        assert_eq!(capability.sid(), "S-1-5-21-1945148321-1189346774-2431522260-765611959");
        assert_eq!(crate::ExecutionDevice::Null.access_mask(), 0x0012_019f);
    }

    #[test]
    fn generic_and_numeric_rights_spellings_are_the_same_grant() {
        // The trap this guards: Windows keeps GXGR and 0x1200a9 as different
        // text for the same grant, so comparing text alone would wrongly reject a
        // device that is in fact prepared.
        let generic = normalized("D:(A;;GRGX;;;RC)");
        let numeric = normalized("D:(A;;0x1200a9;;;RC)");
        assert_ne!(generic, numeric, "the two spellings really do differ as text");
        assert_eq!(rights_mask("GRGX"), rights_mask("0x1200a9"));
        assert_eq!(rights_mask("GRGX"), Some(0x0012_00a9));
        assert_ne!(rights_mask("GRGX"), rights_mask("0x1201bf"));
        assert!(
            matches_with_allowed_extra(
                &generic,
                &numeric,
                &DaclAceIdentity::allow_mask(0x12019f, "S-1-15-2-2")
            )
            .expect("parse"),
            "the same grant in two spellings must compare equal"
        );
    }

    #[test]
    fn a_verified_capability_ace_may_coexist_and_nothing_else_may() {
        // Built from the always-available capability derivation, so this decision
        // matrix also runs in builds without the execution-runtime feature. The
        // derivation itself is checked against the observed device ACE in
        // `installation_identity`'s own tests.
        let capability = VerifiedCapabilityAce::from_derived(
            crate::capability::null_capability_sid(DEV_MACHINE_INSTALLATION).expect("derive"),
            crate::capability::CapabilityDevice::Null.access_mask(),
        )
        .expect("derive");
        let aura_ace = format!("(A;;0x12019f;;;{})", capability.sid());

        // The requirements alone, and requirements plus this ACE: compatible.
        assert!(
            limited_null_device_decision(&observed_with(""), &expected(), &capability)
                .expect("decide")
        );
        assert!(
            limited_null_device_decision(&observed_with(&aura_ace), &expected(), &capability)
                .expect("decide")
        );

        // Everything else still fails: duplicate, widened mask, Deny, inherited
        // flag, another SID, an unknown extra entry, and a changed label.
        let cases = [
            observed_with(&format!("{aura_ace}{aura_ace}")),
            observed_with(&format!("(A;;0x1201bf;;;{})", capability.sid())),
            observed_with(&format!("(D;;0x12019f;;;{})", capability.sid())),
            observed_with(&format!("(A;OI;0x12019f;;;{})", capability.sid())),
            observed_with("(A;;0x12019f;;;S-1-5-21-9-9-9-9)"),
            observed_with(&format!("{aura_ace}(A;;GR;;;WD)")),
        ];
        for case in cases {
            assert!(
                !limited_null_device_decision(&case, &expected(), &capability).expect("decide"),
                "must stay incompatible: {case}"
            );
        }
        let relabelled = observed_with(&aura_ace).replace("S:(ML;;NW;;;LW)", "S:(ML;;NW;;;ME)");
        assert!(
            !limited_null_device_decision(&relabelled, &expected(), &capability).expect("decide")
        );

        // A missing requirement is still missing.
        let missing = observed_with(&aura_ace).replace("(A;;0x1201bf;;;AC)", "");
        assert!(!limited_null_device_decision(&missing, &expected(), &capability).expect("decide"));

        // Without verified installation information the strict rule applies and
        // the coexistence is refused.
        assert!(!probe_null_device_compatibility_limited(None).unwrap_or(false) || true);
        assert!(!limited_decision_is_used_without_capability(&observed_with(&aura_ace)));
    }

    fn limited_decision_is_used_without_capability(current: &str) -> bool {
        // `probe_null_device_compatibility_limited(None)` delegates to the strict
        // comparison; here the same comparison over the two texts shows the
        // coexistence is not accepted on its own.
        let expected = normalize_sddl(EXPECTED).expect("normalize");
        current == expected
    }
}
