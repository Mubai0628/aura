//! Verified current-user startup chain; product policy and journaling belong to the adapter.
use super::{CompatExecution, environment};
use crate::checked_program::{self, Links};
use anyhow::{Result, ensure};
use aura_windows_platform::{RootJob, VerifiedFile};
use aura_windows_user_contract::{ExecutionFileBinding, ManagedShell, ManagedShellCommand};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

#[cfg(any(test, feature = "compat-adoption"))]
pub use aura_windows_platform::JobLaunchFault as LaunchFault;

pub struct CompatBackend {
    _runtime_files: Vec<VerifiedFile>,
    _cmd_file: VerifiedFile,
    pub(super) powershell: ExecutionFileBinding,
    pub(super) cmd: ExecutionFileBinding,
    pub(super) trusted_environment: Vec<(OsString, OsString)>,
    pub(super) refresh_environment: bool,
    installation: String,
    image_verification: Duration,
}

impl CompatBackend {
    pub fn new(payload: &Path, manifest: &Path) -> Result<Self> {
        let mut backend = Self::with_environment(
            payload,
            manifest,
            aura_windows_platform::current_user_environment()?,
        )?;
        backend.refresh_environment = true;
        Ok(backend)
    }

    pub fn with_environment(
        payload: &Path,
        manifest: &Path,
        trusted: Vec<(OsString, OsString)>,
    ) -> Result<Self> {
        let began = Instant::now();
        ensure!(
            payload.is_absolute() && manifest.is_absolute(),
            "absolute managed runtime required"
        );
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(manifest)?)?;
        let runtime_files = crate::runtime_package::verify_managed_payload(payload, &manifest)?;
        // Aura's own PowerShell is checked like the user's programs: in full here, then only
        // compared until it changed or a launch of it failed.
        let (powershell, _) = checked_program::capture(&payload.join("pwsh.exe"), Links::Single)?;
        let cmd = crate::file_binding::capture_trusted_system_cmd()?;
        // The system CMD is the user's own program: checked in full here, then again only after
        // it changed or a launch of it failed.
        let cmd_file = checked_program::hold(&cmd, Links::Single)?;
        environment::build(trusted.clone(), None)?;
        Ok(Self {
            _runtime_files: runtime_files,
            _cmd_file: cmd_file,
            powershell,
            cmd,
            trusted_environment: trusted,
            refresh_environment: false,
            installation: installation_binding()?,
            image_verification: began.elapsed(),
        })
    }

    pub fn image_verification(&self) -> Duration {
        self.image_verification
    }
    pub fn powershell_path(&self) -> &Path {
        Path::new(&self.powershell.path)
    }
    pub fn cmd_path(&self) -> &Path {
        Path::new(&self.cmd.path)
    }
    pub fn removed_environment_names(&self) -> Vec<String> {
        environment::excluded_names(&self.trusted_environment)
    }

    pub fn launch(
        &self,
        command: &ManagedShellCommand,
        offline_proxy: Option<&str>,
    ) -> Result<CompatExecution> {
        self.launch_inner(command, offline_proxy, aura_windows_platform::spawn_compat_process)
    }

    #[cfg(any(test, feature = "compat-adoption"))]
    pub fn launch_with_fault(
        &self,
        command: &ManagedShellCommand,
        fault: LaunchFault,
    ) -> Result<CompatExecution> {
        self.launch_inner(command, None, |job, image, arguments, directory, environment, script| {
            aura_windows_platform::spawn_compat_process_with_fault(
                job,
                image,
                arguments,
                directory,
                environment,
                script,
                fault,
            )
        })
    }

    fn launch_inner(
        &self,
        command: &ManagedShellCommand,
        offline_proxy: Option<&str>,
        launch: impl FnOnce(
            &RootJob,
            &Path,
            &[Vec<u16>],
            &Path,
            &[aura_windows_platform::NativeEnvironmentEntry],
            Option<&Path>,
        ) -> std::result::Result<
            (aura_windows_platform::NativeSandboxProcess, aura_windows_platform::JobLaunchTiming),
            aura_windows_platform::JobLaunchError,
        >,
    ) -> Result<CompatExecution> {
        let entered = Instant::now();
        command.validate()?;
        let work = Path::new(&command.working_directory);
        ensure!(work.is_absolute(), "compat.relative_working_directory");
        let (work_binding, work_file) = crate::file_binding::capture(work, false)?;
        ensure!(work_file.info().is_directory, "compat.directory_required");
        let image = match command.shell {
            ManagedShell::PowerShell => &self.powershell,
            ManagedShell::Cmd => &self.cmd,
        };
        let cmd = command.shell == ManagedShell::Cmd;
        let image_file = checked_program::hold(image, Links::Single)?;
        let selected = if self.refresh_environment {
            aura_windows_platform::current_user_environment()?
        } else {
            self.trusted_environment.clone()
        };
        let excluded_environment_names = environment::excluded_names(&selected);
        let environment = environment::build(selected, offline_proxy)?;
        let scratch = tempfile::Builder::new().prefix("aura-compat-").tempdir()?;
        let script_path = scratch.path().join(if command.shell == ManagedShell::Cmd {
            "command.cmd"
        } else {
            "command.ps1"
        });
        let script = match command.shell {
            ManagedShell::PowerShell => {
                crate::environment::write_powershell_script(scratch.path(), &command.script)?;
                crate::environment::powershell_command_file()
            }
            ManagedShell::Cmd => {
                let codepage = aura_windows_platform::dos_drive_path(
                    &Path::new(&self.cmd.path).with_file_name("chcp.com"),
                )?;
                format!(
                    "@echo off\r\n@\"{}\" 65001 >nul\r\n{}\r\n",
                    codepage.display(),
                    command.script.replace("\r\n", "\n").replace('\n', "\r\n")
                )
            }
        };
        let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&script_path)?;
        file.write_all(script.as_bytes())?;
        file.sync_all()?;
        drop(file);
        let args = if command.shell == ManagedShell::PowerShell {
            crate::environment::POWERSHELL_ARGUMENTS
                .iter()
                .map(|v| (*v).to_owned())
                .chain([
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-File".into(),
                    script_path.to_string_lossy().into_owned(),
                ])
                .map(|v| v.encode_utf16().collect())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let nonce = nonce()?;
        let job = RootJob::create_recoverable(&self.installation, &nonce)?;
        let prepared = entered.elapsed();
        let (mut process, timing) = launch(
            &job,
            Path::new(&image.path),
            &args,
            Path::new(&work_binding.path),
            &environment,
            cmd.then_some(script_path.as_path()),
        )
        .inspect_err(|_| {
            checked_program::check_after_failure(image, Links::Single, "launch_failed")
        })?;
        let input = process
            .take_stdin()
            .map(|mut writer| {
                let input = command.stdin.clone().into_bytes();
                std::thread::Builder::new()
                    .name("aura-compat-stdin".into())
                    .spawn(move || writer.write_all(&input))
            })
            .transpose()?;
        CompatExecution::new(
            job,
            process,
            super::job_owner::ExecutionResources {
                scratch,
                work_file,
                image_file,
                input,
                excluded_environment_names,
            },
            super::job_owner::ExecutionSetup {
                installation: self.installation.clone(),
                nonce,
                entered,
                prepared,
                timing,
                timeout: Duration::from_millis(command.timeout_ms),
                output_limit: command.max_output_bytes as usize,
            },
        )
    }
}

fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("compat randomness: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn installation_binding() -> Result<String> {
    let value = nonce()?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &value[..8],
        &value[8..12],
        &value[12..16],
        &value[16..20],
        &value[20..]
    ))
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
