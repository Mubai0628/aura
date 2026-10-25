//! Inspect mandatory labels through retained handles. Unlabelled objects are Medium in Windows MIC.
use crate::VerifiedFile;
use std::{io, mem::offset_of, os::windows::io::AsRawHandle, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_SUCCESS, LocalFree},
    Security::{
        ACL,
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        GetAce, GetSidIdentifierAuthority, GetSidSubAuthority, GetSidSubAuthorityCount, IsValidSid,
        LABEL_SECURITY_INFORMATION, SYSTEM_MANDATORY_LABEL_ACE,
    },
    Storage::FileSystem::{FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL},
    System::SystemServices::{SYSTEM_MANDATORY_LABEL_ACE_TYPE, SYSTEM_MANDATORY_LABEL_NO_WRITE_UP},
};
pub struct GitReadDirectory {
    file: VerifiedFile,
}
impl GitReadDirectory {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = crate::path::open_native_with_desired_access(
            path,
            FILE_READ_ATTRIBUTES | READ_CONTROL,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )?;
        if !file.info().is_directory || file.info().is_reparse_point {
            return Err(io::Error::other("git.repository_directory_invalid: 仓库目录身份不可用"));
        }
        let value = Self { file };
        value.recheck()?;
        Ok(value)
    }
    pub fn recheck(&self) -> io::Result<()> {
        let mut descriptor = ptr::null_mut();
        let mut sacl: *mut ACL = ptr::null_mut();
        // SAFETY: query only the mandatory label of this retained file object; no SACL privilege or mutation.
        let status = unsafe {
            GetSecurityInfo(
                self.file.file().as_raw_handle().cast(),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut sacl,
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let result = inspect_label(sacl);
        // SAFETY: the allocation came from GetSecurityInfo; every ACE was read before freeing it.
        unsafe {
            LocalFree(descriptor);
        }
        result
    }
}
fn inspect_label(sacl: *const ACL) -> io::Result<()> {
    if sacl.is_null() {
        return Ok(());
    }
    // SAFETY: the SACL and all ACEs belong to the live descriptor returned by Windows.
    for index in 0..unsafe { (*sacl).AceCount } {
        let mut raw = ptr::null_mut();
        if unsafe { GetAce(sacl, u32::from(index), &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let ace = raw.cast::<SYSTEM_MANDATORY_LABEL_ACE>();
        if unsafe { u32::from((*ace).Header.AceType) } != SYSTEM_MANDATORY_LABEL_ACE_TYPE {
            continue;
        }
        if unsafe { usize::from((*ace).Header.AceSize) }
            < offset_of!(SYSTEM_MANDATORY_LABEL_ACE, SidStart) + 12
        {
            return Err(io::Error::other("git.integrity_label_invalid"));
        }
        let sid = unsafe { (&raw const (*ace).SidStart).cast_mut().cast() };
        if unsafe { IsValidSid(sid) } == 0
            || unsafe { *GetSidSubAuthorityCount(sid) } != 1
            || unsafe { (*GetSidIdentifierAuthority(sid)).Value } != [0, 0, 0, 0, 0, 16]
        {
            return Err(io::Error::other("git.integrity_label_invalid"));
        }
        let level = unsafe { *GetSidSubAuthority(sid, 0) };
        if level <= 4096 || unsafe { (*ace).Mask } & SYSTEM_MANDATORY_LABEL_NO_WRITE_UP == 0 {
            return Err(io::Error::other(
                "git.low_integrity_repository: 仓库工作树、git-dir和common-dir必须高于Low并禁止向上写入；请在终端处理此仓库",
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "git-read-test-profile")]
pub fn set_git_read_fixture_integrity(path: &Path, low: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            SetNamedSecurityInfoW,
        },
        GetSecurityDescriptorSacl,
    };
    let path = std::fs::canonicalize(path)?;
    let temporary = std::fs::canonicalize(std::env::temp_dir())?;
    let relative = path
        .strip_prefix(temporary)
        .map_err(|_| io::Error::other("git.test_label_outside_temporary"))?;
    if !relative
        .components()
        .next()
        .is_some_and(|part| part.as_os_str().to_string_lossy().starts_with("aura-git-test-"))
    {
        return Err(io::Error::other("git.test_label_outside_fixture"));
    }
    let sddl = if low { "S:(ML;;NW;;;LW)" } else { "S:(ML;;NW;;;ME)" };
    let sddl = sddl.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut descriptor = ptr::null_mut();
    // SAFETY: this seam only relabels explicitly owned temporary fixture objects.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut present = 0;
    let mut defaulted = 0;
    let mut sacl = ptr::null_mut();
    let result = (|| {
        if unsafe { GetSecurityDescriptorSacl(descriptor, &mut present, &mut sacl, &mut defaulted) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let wide = path.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
        let status = unsafe {
            SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                sacl,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}
