//! Verified images/directories and interactive shell policy; no user input enters the argv.
use super::{TerminalKind, TerminalOptions, TerminalSession};
use crate::checked_program::{self, Links};
use crate::compat::{CompatBackend, environment};
use anyhow::{Result, ensure};
use aura_windows_platform::{
    NativeEnvironmentEntry, NativeTerminal, NativeTerminalLaunch, TerminalOutputCallback,
};
use aura_windows_user_contract::ManagedShell;
use std::{ffi::OsString, os::windows::ffi::OsStrExt, path::Path};

impl CompatBackend {
    pub fn launch_terminal(
        &self,
        options: &TerminalOptions,
        proxy: Option<&str>,
    ) -> Result<TerminalSession> {
        self.launch_terminal_inner(options, proxy, |spec, output| {
            NativeTerminal::spawn(spec, output)
        })
    }
    #[cfg(any(test, feature = "compat-adoption"))]
    pub fn launch_terminal_with_fault(
        &self,
        options: &TerminalOptions,
        proxy: Option<&str>,
        fault: crate::compat::LaunchFault,
    ) -> Result<TerminalSession> {
        self.launch_terminal_inner(options, proxy, |spec, output| {
            NativeTerminal::spawn_with_fault(spec, output, fault)
        })
    }
    fn launch_terminal_inner(
        &self,
        options: &TerminalOptions,
        proxy: Option<&str>,
        spawn: impl FnOnce(
            NativeTerminalLaunch<'_>,
            TerminalOutputCallback,
        )
            -> std::result::Result<NativeTerminal, aura_windows_platform::JobLaunchError>,
    ) -> Result<TerminalSession> {
        ensure!(
            options.working_directory.is_absolute(),
            "terminal.absolute_working_directory_required"
        );
        let (directory, directory_file) =
            crate::file_binding::capture(&options.working_directory, false)?;
        ensure!(directory_file.info().is_directory, "terminal.directory_required");
        let image = match options.shell {
            ManagedShell::PowerShell => &self.powershell,
            ManagedShell::Cmd => &self.cmd,
        };
        // The system CMD and Aura's own PowerShell are
        // read in full only the first time and after a change or a failure.
        let image_file = checked_program::hold(image, Links::Single)?;
        #[cfg(feature = "terminal-test-profile")]
        let isolated = options.kind == TerminalKind::User
            && std::env::var_os("AURA_TERMINAL_TEST_NO_PROFILE").is_some();
        #[cfg(not(feature = "terminal-test-profile"))]
        let isolated = false;
        let trusted = if isolated {
            crate::environment::baseline(
                &options.working_directory,
                Path::new(&image.path).parent(),
            )?
            .into_iter()
            .map(|entry| {
                Ok((
                    OsString::from(entry.name),
                    OsString::from(String::from_utf16(&entry.value_utf16)?),
                ))
            })
            .collect::<Result<Vec<_>>>()?
        } else if self.refresh_environment {
            aura_windows_platform::current_user_environment()?
        } else {
            self.trusted_environment.clone()
        };
        let environment = terminal_environment(trusted, options.kind, proxy)?;
        let arguments = terminal_arguments(options.shell, options.kind)
            .into_iter()
            .map(|value| value.encode_utf16().collect())
            .collect::<Vec<_>>();
        TerminalSession::start(options, vec![directory_file, image_file], |output| {
            Ok(spawn(
                NativeTerminalLaunch {
                    executable: Path::new(&image.path),
                    arguments: &arguments,
                    directory: Path::new(&directory.path),
                    environment: &environment,
                    size: options.size,
                },
                output,
            )
            .inspect_err(|_| {
                checked_program::check_after_failure(image, Links::Single, "launch_failed")
            })?)
        })
    }
}
pub(super) fn terminal_arguments(shell: ManagedShell, kind: TerminalKind) -> Vec<String> {
    match shell {
        ManagedShell::Cmd => ["/d", "/q", "/e:on", "/v:off", "/k", "chcp 65001 >nul"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ManagedShell::PowerShell => {
            let mut arguments = vec!["-NoLogo".into()];
            // Test-only: isolated Electron scenarios never load the owner's real profile.
            #[cfg(feature = "terminal-test-profile")]
            if kind == TerminalKind::User
                && std::env::var_os("AURA_TERMINAL_TEST_NO_PROFILE").is_some()
            {
                arguments.push("-NoProfile".into());
            }
            if matches!(kind, TerminalKind::Agent { .. }) {
                arguments.extend([
                    "-MTA".into(),
                    "-NoProfile".into(),
                    "-NoExit".into(),
                    "-Command".into(),
                    crate::environment::POWERSHELL_BOOTSTRAP.into(),
                ]);
            }
            arguments
        }
    }
}
pub(super) fn terminal_environment(
    trusted: Vec<(OsString, OsString)>,
    kind: TerminalKind,
    proxy: Option<&str>,
) -> Result<Vec<NativeEnvironmentEntry>> {
    match kind {
        TerminalKind::Agent { requires_network } => {
            if !requires_network {
                ensure!(proxy.is_some(), "terminal.owned_offline_proxy_required");
            }
            environment::build(trusted, if requires_network { None } else { proxy })
        }
        TerminalKind::User => {
            let mut entries = Vec::new();
            for (name, value) in trusted {
                let name = name
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("terminal.environment_name_invalid"))?;
                let upper = name.to_ascii_uppercase();
                if upper.starts_with("AURA_")
                    || upper.starts_with("CODEX_")
                    || upper == "__COMPAT_LAYER"
                {
                    continue;
                }
                ensure!(
                    !name.is_empty() && !name.contains(['=', '\0']),
                    "terminal.environment_name_invalid"
                );
                let value_utf16 = value.encode_wide().collect::<Vec<_>>();
                ensure!(!value_utf16.contains(&0), "terminal.environment_value_invalid");
                entries.push(NativeEnvironmentEntry { name: name.into(), value_utf16 });
            }
            // The native environment packer enforces Windows' case-insensitive uniqueness.
            Ok(entries)
        }
    }
}
