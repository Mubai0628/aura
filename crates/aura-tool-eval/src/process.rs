//! Bounded harness/scorer process trees. This is an eval consumer of the existing Job API.
use anyhow::{Result, ensure};
use std::{process::Command, time::Duration};

pub(crate) struct Captured {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

#[cfg(windows)]
pub(crate) fn run(command: &Command, timeout: Duration) -> Result<Captured> {
    use aura_windows_platform::{NativeEnvironmentEntry, RootJob, spawn_compat_process};
    use std::{
        path::{Path, PathBuf},
        time::Instant,
    };
    let program = Path::new(command.get_program());
    let executable = if program.is_absolute() {
        program.to_path_buf()
    } else {
        let paths =
            std::env::var_os("PATH").ok_or_else(|| anyhow::anyhow!("eval.path_unavailable"))?;
        std::env::split_paths(&paths)
            .map(|p| p.join(format!("{}.exe", program.to_string_lossy())))
            .find(|p| p.is_file())
            .ok_or_else(|| anyhow::anyhow!("eval.local_tool_unavailable"))?
    };
    let directory =
        command.get_current_dir().map(PathBuf::from).unwrap_or(std::env::current_dir()?);
    // Windows' hidden `=C:`-style entries of a process started from a shell are not variables.
    let mut env = std::env::vars()
        .filter(|(name, _)| !name.starts_with('=') && !secret_name(name))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (name, value) in command.get_envs() {
        let name = name.to_string_lossy().into_owned();
        env.retain(|key, _| !key.eq_ignore_ascii_case(&name));
        if let Some(value) = value {
            env.insert(name, value.to_string_lossy().into_owned());
        }
    }
    let environment = env
        .into_iter()
        .map(|(name, value)| NativeEnvironmentEntry {
            name,
            value_utf16: value.encode_utf16().collect(),
        })
        .collect::<Vec<_>>();
    let arguments = command
        .get_args()
        .map(|arg| arg.to_string_lossy().encode_utf16().collect())
        .collect::<Vec<_>>();
    let job = RootJob::create()?;
    let (process, _) =
        spawn_compat_process(&job, &executable, &arguments, &directory, &environment, None)?;
    let started = Instant::now();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit_code;
    let timed_out;
    loop {
        append(&mut stdout, process.drain_stdout(65536)?)?;
        append(&mut stderr, process.drain_stderr(65536)?)?;
        if let Some(code) = process.wait_timeout(Duration::ZERO)? {
            // The root exit code decides the result. Toolchain helpers such as vctip.exe can
            // outlive a build inside this Job; like the product Shell, give them a short grace
            // period and then end the Job instead of waiting for them until the timeout.
            if !process.wait_for_process_tree_exit(Duration::from_secs(5))? {
                process.terminate(code)?;
                ensure!(
                    process.wait_for_process_tree_exit(Duration::from_secs(5))?,
                    "eval.harness_stop_unconfirmed"
                );
            }
            append(&mut stdout, process.drain_stdout(65536)?)?;
            append(&mut stderr, process.drain_stderr(65536)?)?;
            exit_code = Some(i32::from_ne_bytes(code.to_ne_bytes()));
            timed_out = false;
            break;
        }
        if started.elapsed() >= timeout {
            process.terminate(124)?;
            ensure!(
                process.wait_for_process_tree_exit(Duration::from_secs(5))?,
                "eval.harness_stop_unconfirmed"
            );
            exit_code = None;
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(Captured { exit_code, stdout, stderr, timed_out })
}

#[cfg(not(windows))]
pub(crate) fn run(_command: &Command, _timeout: Duration) -> Result<Captured> {
    anyhow::bail!("eval.windows_job_runner_required")
}

#[cfg(windows)]
fn append(buffer: &mut Vec<u8>, chunk: Vec<u8>) -> Result<()> {
    ensure!(buffer.len() + chunk.len() <= 16 * 1024 * 1024, "eval.harness_output_limit");
    buffer.extend(chunk);
    Ok(())
}
#[cfg(windows)]
fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.contains("API_KEY")
        || name.split('_').any(|part| ["TOKEN", "KEY", "SECRET", "PASSWORD"].contains(&part))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_lingering_helper_neither_hangs_scoring_nor_hides_the_root_exit_code() {
        let mut command = Command::new("powershell");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Process -WindowStyle Hidden -FilePath powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 60'; exit 3",
        ]);
        let started = std::time::Instant::now();
        let output = run(&command, Duration::from_secs(45)).expect("bounded scorer run");
        assert_eq!(output.exit_code, Some(3), "the root process decides the result");
        assert!(!output.timed_out);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "a process left in the Job is ended after the grace period"
        );
    }
}
