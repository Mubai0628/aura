use std::{io, io::Read, path::PathBuf};

use aura_domain_common::{ContentHash, ContentHashAlgorithm, RuntimeInstanceId};
use sha2::{Digest, Sha256};
use thiserror::Error;
use windows_sys::Win32::{
    Foundation::{GetLastError, HANDLE, STILL_ACTIVE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetExitCodeProcess, GetProcessId, INFINITE, TerminateProcess, WaitForSingleObject,
    },
    UI::{
        Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_HIDE,
    },
};

const FAILED_VERIFICATION_CLEANUP_TIMEOUT_MS: u32 = 5_000;

#[derive(Debug, Error)]
pub enum ElevatedHelperLaunchError {
    #[error(transparent)]
    Platform(#[from] io::Error),
    #[error(
        "cleanup_failed_state_unknown: launch verification failed ({verification_error}); elevated process cleanup could not be confirmed ({cleanup_error})"
    )]
    CleanupFailedStateUnknown { verification_error: io::Error, cleanup_error: io::Error },
}

impl ElevatedHelperLaunchError {
    #[must_use]
    pub const fn is_state_unknown(&self) -> bool {
        matches!(self, Self::CleanupFailedStateUnknown { .. })
    }
}

use crate::{
    handle::OwnedHandle,
    job::quote_windows_argument,
    path::{NativeFileIdentity, VerifiedFile, inspect_existing_path, open_verified_executable},
    process::{ProcessIdentity, snapshot_process},
};

#[derive(Clone, Debug)]
pub struct ElevatedHelperLaunch {
    pub executable: PathBuf,
    pub component_manifest_path: PathBuf,
    pub expected_content_hash: ContentHash,
    pub pipe_name: String,
    pub parent_process_id: u32,
    pub parent_runtime_instance_id: RuntimeInstanceId,
    pub helper_runtime_instance_id: RuntimeInstanceId,
    pub allow_dev_component_mismatch: bool,
}

pub struct ElevatedProcess {
    process: OwnedHandle,
    identity: ProcessIdentity,
    executable_identity: NativeFileIdentity,
}

impl ElevatedProcess {
    pub const fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }

    pub const fn executable_identity(&self) -> &NativeFileIdentity {
        &self.executable_identity
    }

    pub fn wait(self) -> io::Result<u32> {
        // SAFETY: the owned process handle remains valid for the wait.
        if unsafe { WaitForSingleObject(self.process.as_raw(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut exit_code = 0_u32;
        // SAFETY: the process is signaled and output storage is writable.
        if unsafe { GetExitCodeProcess(self.process.as_raw(), &mut exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(exit_code)
    }
}

pub fn launch_verified_aura_elevated_helper(
    launch: &ElevatedHelperLaunch,
) -> Result<ElevatedProcess, ElevatedHelperLaunchError> {
    // Keep this handle alive through the synchronous UAC prompt and process
    // creation. FILE_SHARE_READ denies both writes and replacement while the
    // path-based Shell API resolves the approved executable.
    let pinned_executable = validate_launch(launch)?;
    let expected_identity = pinned_executable.info().identity.clone();
    let (pinned_executable_path, working_directory) =
        pinned_shell_launch_paths(&pinned_executable)?;
    let verb = wide_null("runas");
    let executable = wide_null(&pinned_executable_path.to_string_lossy());
    let mut arguments = vec![
        format!("--pipe-name={}", launch.pipe_name),
        format!("--parent-pid={}", launch.parent_process_id),
        format!("--parent-runtime-instance-id={}", launch.parent_runtime_instance_id),
        format!("--helper-runtime-instance-id={}", launch.helper_runtime_instance_id),
        format!("--component-manifest={}", launch.component_manifest_path.to_string_lossy()),
    ];
    if cfg!(debug_assertions) && launch.allow_dev_component_mismatch {
        arguments.push("--allow-dev-component-mismatch".to_owned());
    }
    let parameters = wide_null(
        &arguments
            .iter()
            .map(|argument| quote_windows_argument(argument))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let directory = wide_null(&working_directory.to_string_lossy());
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(std::mem::size_of::<SHELLEXECUTEINFOW>())
            .map_err(|_| io::Error::other("SHELLEXECUTEINFOW size overflow"))?,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: executable.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: directory.as_ptr(),
        nShow: SW_HIDE,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: all fields point to fixed, NUL-terminated Aura Helper launch
    // values and remain alive through ShellExecuteExW.
    if unsafe { ShellExecuteExW(&mut execute) } == 0 {
        // SAFETY: queried immediately after the failed Win32 call.
        let error = unsafe { GetLastError() };
        return Err(io::Error::from_raw_os_error(error as i32).into());
    }
    let process = OwnedHandle::from_raw(execute.hProcess)?;
    // SAFETY: hProcess is live because SEE_MASK_NOCLOSEPROCESS was requested.
    let process_id = unsafe { GetProcessId(process.as_raw()) };
    if process_id == 0 {
        let error = io::Error::last_os_error();
        return Err(failed_post_launch_verification(process.as_raw(), error));
    }
    let identity = match snapshot_process(process_id) {
        Ok(identity) => identity,
        Err(error) => {
            return Err(failed_post_launch_verification(process.as_raw(), error));
        }
    };
    let actual = match inspect_existing_path(&identity.executable_path, true) {
        Ok(actual) => actual,
        Err(error) => {
            return Err(failed_post_launch_verification(process.as_raw(), error));
        }
    };
    if actual.identity != expected_identity {
        return Err(failed_post_launch_verification(
            process.as_raw(),
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Aura Elevated Helper process image FileId did not match the pinned executable",
            ),
        ));
    }
    drop(pinned_executable);
    Ok(ElevatedProcess { process, identity, executable_identity: expected_identity })
}

fn failed_post_launch_verification(
    process: HANDLE,
    verification_error: io::Error,
) -> ElevatedHelperLaunchError {
    match terminate_and_confirm_exit(process) {
        Ok(()) => ElevatedHelperLaunchError::Platform(verification_error),
        Err(cleanup_error) => ElevatedHelperLaunchError::CleanupFailedStateUnknown {
            verification_error,
            cleanup_error,
        },
    }
}

fn pinned_shell_launch_paths(executable: &VerifiedFile) -> io::Result<(PathBuf, PathBuf)> {
    let executable_path = executable.info().final_path.clone();
    let working_directory = executable_path
        .parent()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Elevated Helper path has no parent")
        })?
        .to_path_buf();
    Ok((executable_path, working_directory))
}

fn terminate_and_confirm_exit(process: HANDLE) -> io::Result<()> {
    // SAFETY: callers pass the exact live process handle returned by
    // ShellExecuteExW (or a live child-process handle in the unit test).
    let terminated = unsafe { TerminateProcess(process, 1) };
    if terminated == 0 {
        let terminate_error = io::Error::last_os_error();
        let exit_code = process_exit_code(process).map_err(|state_error| {
            io::Error::other(format!(
                "TerminateProcess failed ({terminate_error}); process state query failed ({state_error})"
            ))
        })?;
        if exit_code == STILL_ACTIVE as u32 {
            return Err(io::Error::other(format!(
                "TerminateProcess failed and the elevated process remains active: {terminate_error}"
            )));
        }
        return Ok(());
    }

    // SAFETY: the process handle remains live for this bounded cleanup wait.
    match unsafe { WaitForSingleObject(process, FAILED_VERIFICATION_CLEANUP_TIMEOUT_MS) } {
        WAIT_OBJECT_0 => {}
        WAIT_TIMEOUT => {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "elevated process did not exit before the cleanup deadline",
            ));
        }
        WAIT_FAILED => return Err(io::Error::last_os_error()),
        result => {
            return Err(io::Error::other(format!(
                "unexpected process cleanup wait result: {result}"
            )));
        }
    }
    if process_exit_code(process)? == STILL_ACTIVE as u32 {
        return Err(io::Error::other(
            "elevated process remained active after successful cleanup wait",
        ));
    }
    Ok(())
}

fn process_exit_code(process: HANDLE) -> io::Result<u32> {
    let mut exit_code = 0_u32;
    // SAFETY: the process handle is live and output storage is writable.
    if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(exit_code)
}

fn validate_launch(launch: &ElevatedHelperLaunch) -> io::Result<VerifiedFile> {
    if !launch.executable.is_absolute()
        || !launch.component_manifest_path.is_absolute()
        || launch.parent_process_id == 0
        || launch.executable.file_name().is_none_or(|name| {
            !name.to_string_lossy().eq_ignore_ascii_case("aura-elevated-helper.exe")
        })
        || !valid_pipe_name(&launch.pipe_name)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Aura Elevated Helper launch contract is invalid",
        ));
    }
    let mut executable = open_verified_executable(&launch.executable)?;
    if executable.info().is_directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Aura Elevated Helper executable is a directory",
        ));
    }
    let mut executable_bytes = Vec::new();
    executable.file_mut().read_to_end(&mut executable_bytes)?;
    let actual = match launch.expected_content_hash.algorithm {
        ContentHashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(&executable_bytes)),
        ContentHashAlgorithm::Blake3 => blake3::hash(&executable_bytes).to_hex().to_string(),
    };
    if actual != launch.expected_content_hash.hex
        && !(cfg!(debug_assertions) && launch.allow_dev_component_mismatch)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Aura Elevated Helper failed Release Bundle hash verification",
        ));
    }
    Ok(executable)
}

fn valid_pipe_name(value: &str) -> bool {
    let Some(scope) = value.strip_prefix(r"\\.\pipe\Aura\") else {
        return false;
    };
    let parts = scope.split('\\').collect::<Vec<_>>();
    parts.len() == 3 && lower_hex_128(parts[0]) && parts[1] == "Elevated" && lower_hex_128(parts[2])
}

fn lower_hex_128(value: &str) -> bool {
    value.len() == 32
        && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::{
        os::windows::io::AsRawHandle,
        path::PathBuf,
        process::{Command, Stdio},
    };

    use super::*;

    #[test]
    fn rejects_any_executable_that_is_not_the_fixed_aura_helper() {
        let launch = ElevatedHelperLaunch {
            executable: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            component_manifest_path: PathBuf::from(r"C:\Aura\component-manifest.json"),
            expected_content_hash: ContentHash::blake3(b"not cmd"),
            pipe_name: format!(r"\\.\pipe\Aura\{}\Elevated\{}", "b".repeat(32), "a".repeat(32)),
            parent_process_id: 42,
            parent_runtime_instance_id: RuntimeInstanceId::new(),
            helper_runtime_instance_id: RuntimeInstanceId::new(),
            allow_dev_component_mismatch: true,
        };
        assert!(validate_launch(&launch).is_err());
    }

    #[test]
    fn elevated_pipe_name_has_a_fixed_namespace_and_128_bit_suffix() {
        assert!(valid_pipe_name(&format!(
            r"\\.\pipe\Aura\{}\Elevated\{}",
            "abcdef0123456789".repeat(2),
            "0123456789abcdef".repeat(2)
        )));
        assert!(!valid_pipe_name(&format!(r"\\.\pipe\Aura\{}\Elevated\short", "a".repeat(32))));
        assert!(!valid_pipe_name(&format!(r"\\.\pipe\Other\{}", "a".repeat(32))));
    }

    #[test]
    fn failed_post_launch_verification_terminates_and_waits_for_the_child() {
        let mut child = Command::new("cmd.exe")
            .args(["/d", "/s", "/c", "ping.exe -n 30 127.0.0.1 >nul"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn long-running verification fixture");
        let process = child.as_raw_handle().cast();

        terminate_and_confirm_exit(process).expect("terminate and confirm child exit");

        assert!(child.try_wait().expect("query child state").is_some());
    }

    #[test]
    fn cleanup_failure_is_reported_as_state_unknown() {
        let error = failed_post_launch_verification(
            std::ptr::null_mut(),
            io::Error::new(io::ErrorKind::PermissionDenied, "verification fixture"),
        );
        assert!(error.is_state_unknown());
        assert!(matches!(error, ElevatedHelperLaunchError::CleanupFailedStateUnknown { .. }));
    }

    #[test]
    fn shell_launch_paths_are_derived_from_the_pinned_final_path() {
        let directory = tempfile::tempdir().expect("temporary helper directory");
        let caller_path = directory.path().join("aura-elevated-helper.exe");
        std::fs::write(&caller_path, b"helper fixture").expect("write helper fixture");
        let pinned = open_verified_executable(&caller_path).expect("pin helper fixture");

        let (executable, working_directory) =
            pinned_shell_launch_paths(&pinned).expect("derive pinned shell paths");

        assert_eq!(executable, pinned.info().final_path);
        assert_eq!(working_directory, executable.parent().expect("helper parent"));
    }
}
