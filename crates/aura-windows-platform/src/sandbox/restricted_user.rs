//! Restricted-user process primitive shared by the formal Runner and historical
//! diagnostics. No database, approval policy or automatic fallback lives here.
use super::*;
mod namespace_probe;
mod private_ui;
pub use namespace_probe::{
    current_windows_session_id, execution_namespace_access, execution_namespace_dacl,
    execution_namespace_sid,
};
#[cfg(feature = "restricted-user-spike")]
mod runtime_probe;
#[cfg(feature = "restricted-user-spike")]
mod sync_probe;
mod token_objects;
use private_ui::PrivateDesktop;
#[cfg(feature = "restricted-user-spike")]
pub use runtime_probe::spike_runtime_dlls;
#[cfg(feature = "restricted-user-spike")]
pub use sync_probe::spike_synchronization;
use windows_sys::Win32::Security::{
    CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, IsTokenRestricted, SID_AND_ATTRIBUTES,
    TOKEN_ADJUST_DEFAULT, TOKEN_USER, TokenUser, WRITE_RESTRICTED,
};

pub struct RestrictedUserProcess {
    process: NativeSandboxProcess,
    _desktop: PrivateDesktop,
}

#[derive(Default)]
pub struct RestrictedUserLaunchOptions<'a> {
    pub device_install_id: Option<&'a str>,
    pub cmd_script: Option<&'a Path>,
}

/// Development Runner and its children must report loader failures instead of
/// waiting forever for error dialogs on the private, invisible desktop.
pub fn suppress_execution_error_dialogs() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        GetErrorMode, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
    };
    // SAFETY: process-local error mode only; children inherit it at creation.
    unsafe {
        SetErrorMode(GetErrorMode() | SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
}
impl std::ops::Deref for RestrictedUserProcess {
    type Target = NativeSandboxProcess;
    fn deref(&self) -> &Self::Target {
        &self.process
    }
}
impl std::ops::DerefMut for RestrictedUserProcess {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.process
    }
}

/// Caller owns the root and must journal/reconcile the exact DACL change.
/// Modify includes rename/delete, but never WRITE_DAC or WRITE_OWNER.
pub fn grant_empty_execution_write_root(path: &Path, sid: &str) -> io::Result<()> {
    validate_sid(sid)?;
    // This prototype never grants a user's existing Workspace recursively.
    // Pre-existing aliases need the later handle/identity-aware ACL protocol.
    let info = crate::inspect_existing_path(path, false)?;
    if !info.is_directory || info.is_reparse_point || std::fs::read_dir(path)?.next().is_some() {
        return Err(io::Error::other(
            "Spike grant requires an empty, owned directory without aliases",
        ));
    }
    let encoded = wide_os_null(path.as_os_str());
    let mut descriptor = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    // SAFETY: all outputs are initialized and encoded is NUL terminated.
    let status = unsafe {
        GetNamedSecurityInfoW(
            encoded.as_ptr(),
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
    let _descriptor = LocalAllocation(descriptor.cast());
    let (entry, _sid) = explicit_access_for_sid(
        sid,
        0x0013_01ff,
        GRANT_ACCESS,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )?;
    let mut updated = ptr::null_mut();
    // SAFETY: entry, SID and existing ACL remain alive through the call.
    let status = unsafe { SetEntriesInAclW(1, &entry, dacl, &mut updated) };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _updated = LocalAllocation(updated.cast());
    // SAFETY: updated is a valid ACL and only the named root DACL is changed.
    let status = unsafe {
        SetNamedSecurityInfoW(
            encoded.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            updated,
            ptr::null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

/// Creates a restricted version of this non-elevated caller's token, with a
/// private desktop, explicit streams, suspended launch and a kill-on-close Job.
pub fn spawn_restricted_user_process(
    write_sid: &str,
    executable: &Path,
    arguments: &[Vec<u16>],
    directory: &Path,
    environment: &[NativeEnvironmentEntry],
    secret: Option<&NativeSecretEnvironmentEntry>,
    options: RestrictedUserLaunchOptions<'_>,
) -> io::Result<RestrictedUserProcess> {
    validate_sid(write_sid)?;
    if current_process_is_elevated()? {
        return Err(io::Error::other("R06 Runner must be non-elevated"));
    }
    let token = create_attempt_token(write_sid, options.device_install_id)?;
    let desktop = PrivateDesktop::create(write_sid)?;
    let process = spawn_restricted_process_inner(
        NativeRestrictedMode::StandardUser,
        None,
        executable,
        arguments,
        directory,
        environment,
        secret,
        LaunchOverrides {
            token: Some(&token),
            desktop: Some(&desktop.name),
            cmd_script: options.cmd_script,
            ..LaunchOverrides::default()
        },
    )?;
    Ok(RestrictedUserProcess { process, _desktop: desktop })
}

fn create_attempt_token(
    write_sid: &str,
    device_install_id: Option<&str>,
) -> io::Result<OwnedHandle> {
    let mut device_sids = Vec::new();
    if let Some(id) = device_install_id {
        for device in crate::ExecutionDevice::EXECUTION {
            device_sids.push(device.sid(id)?);
        }
    }
    if let Some(id) = device_install_id {
        device_sids.push(execution_namespace_sid(id)?);
    }
    let token = create_write_token(write_sid, &device_sids)?;
    if device_install_id.is_some() {
        verify_fixed_resource_access(&token)?;
    }
    Ok(token)
}

/// Impersonates only the token just derived from this dedicated Runner, opens
/// the fixed resources without I/O, then always reverts before process launch.
/// No READ_CONTROL grant, supplied resource path or client token is introduced.
fn verify_fixed_resource_access(token: &OwnedHandle) -> io::Result<()> {
    use windows_sys::Win32::Security::{ImpersonateLoggedOnUser, RevertToSelf};
    if unsafe { ImpersonateLoggedOnUser(token.as_raw()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            // Failure cannot safely return to work under an unexpected token.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    let _revert = Revert;
    let observations = crate::ExecutionDevice::EXECUTION
        .into_iter()
        .map(crate::ExecutionDevice::access_probe)
        .chain(std::iter::once(execution_namespace_access()));
    for observation in observations {
        if observation["required"]["opened"] != true
            || ["write_dac", "write_owner", "delete"]
                .into_iter()
                .any(|right| observation[right]["opened"] != false)
        {
            return Err(io::Error::other("runner.fixed_resource_access_changed"));
        }
    }
    Ok(())
}

fn create_write_token(write_sid: &str, device_sids: &[String]) -> io::Result<OwnedHandle> {
    if device_sids.iter().enumerate().any(|(i, s)| s == write_sid || device_sids[..i].contains(s)) {
        return Err(io::Error::other("Device and per-attempt write capabilities must differ"));
    }
    let current = current_process_token(
        TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
    )?;
    let (entry, _sid) = explicit_access_for_sid(write_sid, 0, GRANT_ACCESS, 0)?;
    let restrict = SID_AND_ATTRIBUTES { Sid: entry.Trustee.ptstrName.cast(), Attributes: 0 };
    let device_entries = device_sids
        .iter()
        .map(|sid| explicit_access_for_sid(sid, 0, GRANT_ACCESS, 0))
        .collect::<io::Result<Vec<_>>>()?;
    let mut restricting = vec![restrict];
    for (entry, _allocation) in &device_entries {
        restricting.push(SID_AND_ATTRIBUTES { Sid: entry.Trustee.ptstrName.cast(), Attributes: 0 });
    }
    let mut token = ptr::null_mut();
    // SAFETY: token and SID are retained; zero-count arrays are null. Never set
    // SANDBOX_INERT: application control remains effective.
    if unsafe {
        CreateRestrictedToken(
            current.as_raw(),
            WRITE_RESTRICTED | DISABLE_MAX_PRIVILEGE,
            0,
            ptr::null(),
            0,
            ptr::null(),
            restricting.len() as u32,
            restricting.as_ptr(),
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle::from_raw(token)?;
    token_objects::allow_attempt_objects(&token, write_sid)?;
    Ok(token)
}

pub fn current_user_token_identity() -> io::Result<(String, bool)> {
    let token = current_process_token(TOKEN_QUERY)?;
    let sid = token_user_sid(&token)?;
    Ok((sid, unsafe { IsTokenRestricted(token.as_raw()) } != 0))
}

#[cfg(test)]
mod file_inheritance_tests {
    use super::*;
    #[test]
    fn attempt_can_reopen_a_file_created_under_the_granted_directory() {
        use windows_sys::Win32::Security::{ImpersonateLoggedOnUser, RevertToSelf};
        let temporary = tempfile::tempdir().expect("private test directory");
        let sid = "S-1-5-21-171717-282828-393939-404040";
        let root = crate::ExecutionFileSecurity::open(temporary.path(), true).expect("root handle");
        let before = root.capture().expect("before ACL");
        let after = root.plan_with_inheritance(&[(sid, 0x0013_01bf)], true).expect("grant");
        root.apply_exact(&before, &after).expect("apply grant");
        let token = create_attempt_token(sid, None).expect("restricted token");
        assert_ne!(unsafe { ImpersonateLoggedOnUser(token.as_raw()) }, 0);
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                assert_ne!(unsafe { RevertToSelf() }, 0);
            }
        }
        let revert = Revert;
        let path = temporary.path().join("created.txt");
        let created = std::fs::write(&path, b"first write");
        let reopened =
            if created.is_ok() { std::fs::write(&path, b"second write") } else { Ok(()) };
        drop(revert);
        root.apply_exact(&after, &before).expect("restore ACL");
        assert!(created.is_ok(), "create: {created:?}");
        assert!(reopened.is_ok(), "reopen: {reopened:?}");
    }
}

pub(crate) fn token_user_sid(token: &OwnedHandle) -> io::Result<String> {
    let mut needed = 0;
    // SAFETY: sizing query has no output buffer.
    unsafe {
        GetTokenInformation(token.as_raw(), TokenUser, ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned storage covers the reported size and token is queryable.
    if unsafe {
        GetTokenInformation(
            token.as_raw(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful TokenUser query yields TOKEN_USER followed by its SID.
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    sid_to_string(user.User.Sid)
}

fn validate_sid(sid: &str) -> io::Result<()> {
    let values = sid.strip_prefix("S-1-5-21-").map(|s| s.split('-').collect::<Vec<_>>());
    if !values
        .is_some_and(|v| v.len() == 4 && v.iter().all(|s| s.parse::<u32>().is_ok_and(|n| n > 0)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "R06 requires an installation/attempt-specific synthetic SID",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_existing_alias_tree_without_changing_its_dacl() {
        let root = tempfile::tempdir().expect("owned root");
        std::fs::write(root.path().join("existing.txt"), b"outside fixture").expect("fixture");
        let before = capture_file_dacl(root.path()).expect("DACL");
        assert!(grant_empty_execution_write_root(root.path(), "S-1-5-21-1-2-3-4").is_err());
        assert_eq!(capture_file_dacl(root.path()).expect("unchanged DACL"), before);
        assert!(validate_sid("S-1-1-0").is_err());
    }
}
