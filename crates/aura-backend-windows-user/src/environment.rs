//! Aura-owned baseline; never merge the launching user's environment.
//! External command capabilities will be bound explicitly by the orchestrator.
use anyhow::{Result, ensure};
use aura_windows_platform::NativeEnvironmentEntry;
use std::path::Path;

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;

pub const REVISION: &str = aura_windows_user_contract::SHELL_ENVIRONMENT_VERSION;
pub const POWERSHELL_BOOTSTRAP: &str = include_str!("powershell-bootstrap.ps1");
pub const POWERSHELL_ARGUMENTS: [&str; 4] = ["-MTA", "-NoLogo", "-NoProfile", "-NonInteractive"];
/// The model's own script, beside the command file that runs it.
pub const POWERSHELL_SCRIPT_FILE: &str = "script.ps1";
const POWERSHELL_RUNNER: &str = include_str!("powershell-runner.ps1");

/// The command file a PowerShell run executes - the
/// bootstrap, then the runner that reads [`POWERSHELL_SCRIPT_FILE`] beside it. A parse error is
/// reported by the script's own line before anything runs, and the script runs as a script block,
/// so its errors name its own lines, in UTF-8 plain text; its exit code is the one it had inline.
#[must_use]
pub fn powershell_command_file() -> String {
    format!("{}\n{POWERSHELL_RUNNER}", POWERSHELL_BOOTSTRAP.trim_end())
}

/// Writes the model's script into the run's private directory, for [`powershell_command_file`].
pub fn write_powershell_script(directory: &Path, script: &str) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(POWERSHELL_SCRIPT_FILE))?;
    file.write_all(script.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

/// What a PowerShell run executed, for its evidence: the command file, then the script.
#[must_use]
pub fn submitted_powershell(command_file: &str, script: &str) -> String {
    format!("{command_file}\n# {POWERSHELL_SCRIPT_FILE}\n{script}")
}

pub fn powershell_command(script: &str) -> Vec<String> {
    POWERSHELL_ARGUMENTS
        .into_iter()
        .map(str::to_owned)
        .chain(["-Command".into(), format!("{POWERSHELL_BOOTSTRAP}\n{script}")])
        .collect()
}

pub fn baseline(work: &Path, runtime: Option<&Path>) -> Result<Vec<NativeEnvironmentEntry>> {
    let system = aura_windows_platform::windows_directory()?;
    let work = aura_windows_platform::dos_drive_path(work)?;
    let runtime = runtime.map(aura_windows_platform::dos_drive_path).transpose()?;
    ensure!(
        work.is_absolute() && runtime.as_deref().is_none_or(Path::is_absolute),
        "absolute execution paths required"
    );
    let system_path = format!("{};{}", system.join("System32").display(), system.display());
    // Bare `pwsh` in child scripts resolves to this same verified package.
    let path = runtime
        .as_ref()
        .map_or(system_path.clone(), |runtime| format!("{};{system_path}", runtime.display()));
    let mut values = vec![
        ("AURA_EXECUTION_ENVIRONMENT", REVISION.into()),
        ("SystemRoot", system.display().to_string()),
        ("WINDIR", system.display().to_string()),
        ("PATH", path),
        ("PATHEXT", ".COM;.EXE;.BAT;.CMD".into()),
        ("TEMP", work.display().to_string()),
        ("TMP", work.display().to_string()),
        ("USERPROFILE", work.display().to_string()),
        ("HOME", work.display().to_string()),
        ("APPDATA", work.display().to_string()),
        ("LOCALAPPDATA", work.display().to_string()),
        ("COMSPEC", system.join("System32").join("cmd.exe").display().to_string()),
        ("POWERSHELL_TELEMETRY_OPTOUT", "1".into()),
        ("POWERSHELL_UPDATECHECK", "Off".into()),
    ];
    if let Some(runtime) = runtime {
        values.push(("PSModulePath", runtime.join("Modules").display().to_string()));
    }
    Ok(values
        .into_iter()
        .map(|(name, value)| NativeEnvironmentEntry {
            name: name.into(),
            value_utf16: value.encode_utf16().collect(),
        })
        .collect())
}
