//! Current-user process completion for NativeApproved. Core still publishes
//! the terminal Result; this module only observes the Job it launched.
use super::controller::{PortSection, WindowsUserCorePort, WindowsUserCorePortSection};
use anyhow::{Result, ensure};
use aura_domain_common::{UtcTimestamp, jcs::canonical_sha256};
use aura_execution_contract::{DispatchRequest, TerminationEvidence};
use aura_windows_user_contract::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Semaphore, watch};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run(
    request: DispatchRequest,
    prepared: WindowsUserPreparation,
    runtime: PathBuf,
    expiry: UtcTimestamp,
    mut cancelled: watch::Receiver<bool>,
    gate: Arc<Semaphore>,
    port: Arc<dyn WindowsUserCorePort>,
    observation: crate::observation::ExecutionObservation,
) -> Result<TerminationEvidence> {
    // Boundary: every terminal/diagnostic record below is a real journal
    // transaction, so each one is handed to the port's bounded owner instead of running on
    // the reactor that polls this future. The launch observation itself keeps its own
    // `spawn_blocking` carrier further down.
    let run = request.operation.content.identity.agent_run_id;
    let record = move |name: &'static str| PortSection::write(name, run);
    let permit = tokio::select! {
        value = gate.acquire_owned() => value.ok(),
        _ = cancelled.wait_for(|v| *v) => None,
    };
    if permit.is_none()
        || *cancelled.borrow()
        || UtcTimestamp::now() >= expiry.min(prepared.valid_until)
    {
        let reason = if *cancelled.borrow() {
            super::controller::NotStarted::Cancelled
        } else {
            super::controller::NotStarted::Expired
        };
        let completes = Arc::clone(&port);
        return port
            .section(
                record("native.complete"),
                Box::new(move || {
                    completes.complete(&request, None, true, true, Some(reason), None)
                }),
            )
            .await;
    }
    let _permit = permit;
    let launch = match port.begin_launch(&request).await {
        Ok(lease) => lease,
        Err(error) => {
            let code = super::controller::bounded_failure_code(&error);
            if let Some(code) = &code {
                let diagnostics = Arc::clone(&port);
                let owned = request.clone();
                let name = code.clone();
                let _ = port
                    .section(
                        record("native.diagnostic"),
                        Box::new(move || diagnostics.diagnostic(&owned, &name)),
                    )
                    .await;
            }
            let completes = Arc::clone(&port);
            return port
                .section(
                    record("native.complete"),
                    Box::new(move || {
                        completes.complete(
                            &request,
                            None,
                            true,
                            true,
                            Some(super::controller::NotStarted::Rejected),
                            code.as_deref(),
                        )
                    }),
                )
                .await;
        }
    };
    let port_started = port.clone();
    let request_started = request.clone();
    let observation = tokio::task::spawn_blocking(move || {
        // The blocking worker owns this even if the async waiter is dropped.
        let _execution_observation = observation;
        observe(
            &request_started,
            &prepared,
            &runtime,
            expiry,
            &cancelled,
            port_started.as_ref(),
            launch,
            &_execution_observation,
        )
    })
    .await;
    match observation {
        Ok(Ok(value)) => {
            let stopped = value.process_tree_stopped;
            let completes = Arc::clone(&port);
            port.section(
                record("native.complete"),
                Box::new(move || {
                    completes.complete(&request, Some(&value), stopped, true, None, None)
                }),
            )
            .await
        }
        Ok(Err(error)) => {
            let code = super::controller::bounded_failure_code(&error);
            if let Some(code) = &code {
                let diagnostics = Arc::clone(&port);
                let owned = request.clone();
                let name = code.clone();
                let _ = port
                    .section(
                        record("native.diagnostic"),
                        Box::new(move || diagnostics.diagnostic(&owned, &name)),
                    )
                    .await;
            }
            let completes = Arc::clone(&port);
            port.section(
                record("native.complete"),
                Box::new(move || {
                    completes.complete(
                        &request,
                        None,
                        true,
                        true,
                        Some(super::controller::NotStarted::Rejected),
                        code.as_deref(),
                    )
                }),
            )
            .await
        }
        Err(_) => {
            let completes = Arc::clone(&port);
            port.section(
                record("native.complete"),
                Box::new(move || {
                    completes.complete(
                        &request,
                        None,
                        true,
                        true,
                        Some(super::controller::NotStarted::Rejected),
                        None,
                    )
                }),
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)] // The owned launch lease must end at actual process start.
fn observe(
    request: &DispatchRequest,
    prepared: &WindowsUserPreparation,
    runtime: &Path,
    expiry: UtcTimestamp,
    cancelled: &watch::Receiver<bool>,
    port: &dyn WindowsUserCorePort,
    launch: super::controller::LaunchLease,
    observation: &crate::observation::ExecutionObservation,
) -> Result<RunnerObservation> {
    let command: ManagedShellCommand =
        serde_json::from_value(request.operation.content.action.clone())?;
    command.validate()?;
    ensure!(
        !*cancelled.borrow() && UtcTimestamp::now() < expiry.min(prepared.valid_until),
        "native.cancelled_before_launch"
    );
    let scratch = tempfile::tempdir()?;
    let environment =
        super::environment::baseline(scratch.path(), runtime.is_dir().then_some(runtime))?;
    let script_path = scratch.path().join(if command.shell == ManagedShell::Cmd {
        "command.cmd"
    } else {
        "command.ps1"
    });
    let script = match command.shell {
        ManagedShell::PowerShell => {
            super::environment::write_powershell_script(scratch.path(), &command.script)?;
            super::environment::powershell_command_file()
        }
        ManagedShell::Cmd => format!(
            "@echo off\r\n@chcp 65001 >nul\r\n{}\r\n",
            command.script.replace("\r\n", "\n").replace('\n', "\r\n")
        ),
    };
    let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&script_path)?;
    file.write_all(script.as_bytes())?;
    file.sync_all()?;
    drop(file);
    // The evidence names everything run: for PowerShell, the command file and the script.
    let script = match command.shell {
        ManagedShell::PowerShell => {
            super::environment::submitted_powershell(&script, &command.script)
        }
        ManagedShell::Cmd => script,
    };
    let arguments = if command.shell == ManagedShell::PowerShell {
        super::environment::POWERSHELL_ARGUMENTS
            .iter()
            .map(|s| (*s).to_owned())
            .chain([
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                script_path.to_string_lossy().into_owned(),
            ])
            .map(|s| s.encode_utf16().collect())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let started_at = UtcTimestamp::now();
    let mut process = aura_windows_platform::spawn_standard_user_process(
        Path::new(&prepared.shell_image.path),
        &arguments,
        Path::new(&prepared.working_directory.path),
        &environment,
        (command.shell == ManagedShell::Cmd).then_some(script_path.as_path()),
    )?;
    let process_id = process.process_id();
    observation.started();
    // This call is already on a real blocking carrier - `observe` runs
    // inside the `tokio::task::spawn_blocking` at the top of `run` - so the journal write it
    // performs cannot park a reactor worker and it stays exactly as it is.
    port.started(request, process_id, started_at)?;
    drop(launch);
    if let Some(mut input) = process.take_stdin() {
        let _ = input.write_all(command.stdin.as_bytes());
    }
    let limit = command.max_output_bytes as usize;
    let begun = std::time::Instant::now();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut retained = 0usize;
    let mut truncated = false;
    let mut reason;
    let mut exit;
    loop {
        let mut out = process.drain_stdout(limit.saturating_sub(retained) + 1)?;
        let mut err = process.drain_stderr(limit.saturating_sub(retained + out.len()) + 1)?;
        truncated |= retained.saturating_add(out.len()).saturating_add(err.len()) > limit;
        out.truncate(limit.saturating_sub(retained));
        retained += out.len();
        err.truncate(limit.saturating_sub(retained));
        retained += err.len();
        stdout.extend(out);
        stderr.extend(err);
        reason = if *cancelled.borrow() {
            RunnerStopReason::Cancelled
        } else if begun.elapsed() >= Duration::from_millis(command.timeout_ms) {
            RunnerStopReason::TimedOut
        } else if truncated {
            RunnerStopReason::OutputLimit
        } else {
            RunnerStopReason::Exited
        };
        if reason != RunnerStopReason::Exited {
            process.terminate(1)?;
        }
        exit = process.wait_timeout(Duration::from_millis(10))?;
        if exit.is_some() || reason != RunnerStopReason::Exited {
            if reason != RunnerStopReason::Exited {
                ensure!(
                    process.wait_for_process_tree_exit(Duration::from_secs(5))?,
                    "native.tree_not_stopped"
                );
            } else {
                let _ = process.wait_for_process_tree_exit(Duration::from_secs(1))?;
            }
            let tail = process.drain_stdout(limit.saturating_sub(retained) + 1)?;
            truncated |= tail.len() > limit.saturating_sub(retained);
            let keep = tail.len().min(limit.saturating_sub(retained));
            stdout.extend(&tail[..keep]);
            retained += keep;
            let tail = process.drain_stderr(limit.saturating_sub(retained) + 1)?;
            truncated |= tail.len() > limit.saturating_sub(retained);
            let keep = tail.len().min(limit.saturating_sub(retained));
            stderr.extend(&tail[..keep]);
            break;
        }
    }
    let stopped = process.active_processes_in_job()? == 0;
    Ok(RunnerObservation {
        attempt: request.binding.attempt.clone(),
        binding_fingerprint: canonical_sha256(&request.binding)?,
        account_sid: prepared.account_sid.clone(),
        process_id,
        shell_image: prepared.shell_image.clone(),
        started_at,
        finished_at: UtcTimestamp::now(),
        exit_code: exit.or(process.wait_timeout(Duration::from_millis(1))?),
        stop_reason: reason,
        process_tree_stopped: stopped,
        stdout,
        stderr,
        submitted_script: script,
        output_truncated: truncated,
    })
}
