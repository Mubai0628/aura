//! Direct process completion and later Job output are independent observations.
use anyhow::{Result, ensure};
use aura_domain_common::{UtcTimestamp, jcs::canonical_sha256};
use aura_windows_user_contract::*;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

pub(crate) fn execute(
    envelope: RunnerDispatchEnvelope,
    preparation: WindowsUserPreparation,
    installation: super::installation::Installation,
    cancel: Arc<AtomicU8>,
    events: tokio::sync::mpsc::UnboundedSender<RunnerCoreMessage>,
) -> Result<()> {
    use aura_windows_platform::{RestrictedUserLaunchOptions, spawn_restricted_user_process};
    use std::io::Write;
    let command: ManagedShellCommand =
        serde_json::from_value(envelope.dispatch.operation.content.action.clone())?;
    let scratch = Path::new(&envelope.scratch_directory.path);
    let _scratch = super::file_binding::verify(&envelope.scratch_directory)?;
    let _working = super::file_binding::verify(&preparation.working_directory)?;
    let _image = match command.shell {
        ManagedShell::Cmd => super::file_binding::verify_system_cmd(&preparation.shell_image)?,
        ManagedShell::PowerShell => super::file_binding::verify(&preparation.shell_image)?,
    };
    let scratch_root = aura_windows_platform::inspect_existing_path(
        &installation.root.join("execution-scratch"),
        false,
    )?;
    ensure!(
        aura_windows_platform::final_path_within(&scratch_root, _scratch.info()),
        "runner.scratch_escape"
    );
    let expected_image = match command.shell {
        ManagedShell::PowerShell => &installation.manifest.powershell_image,
        ManagedShell::Cmd => &installation.manifest.cmd_image,
    };
    ensure!(&preparation.shell_image == expected_image, "runner.shell_image_changed");
    ensure!(
        cancel.load(Ordering::Acquire) == 0 && envelope.expires_at > UtcTimestamp::now(),
        "runner.cancelled_before_launch"
    );
    let script_path = scratch.join(if command.shell == ManagedShell::Cmd {
        "command.cmd"
    } else {
        "command.ps1"
    });
    let script = match command.shell {
        ManagedShell::PowerShell => {
            super::environment::write_powershell_script(scratch, &command.script)?;
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
    let (_script, _held_script) = super::file_binding::capture(&script_path, true)?;
    // The model's script beside it is held the same way, and both are the evidence.
    let (_model_script, script) = match command.shell {
        ManagedShell::PowerShell => (
            Some(super::file_binding::capture(
                &scratch.join(super::environment::POWERSHELL_SCRIPT_FILE),
                true,
            )?),
            super::environment::submitted_powershell(&script, &command.script),
        ),
        ManagedShell::Cmd => (None, script),
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
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let environment =
        super::environment::baseline(scratch, Some(&installation.root.join("runtime")))?;
    let started_at = UtcTimestamp::now();
    let attempt = envelope.dispatch.binding.attempt.clone();
    events.send(RunnerCoreMessage::Progress {
        update: RunnerProgress {
            attempt: attempt.clone(),
            sequence: 1,
            observed_at: started_at,
            submitted_script: Some(script.clone()),
            stdout: vec![],
            stderr: vec![],
            output_truncated: false,
        },
    })?;
    let mut process = spawn_restricted_user_process(
        &envelope.attempt_write_sid,
        Path::new(&preparation.shell_image.path),
        &arguments.iter().map(|s| s.encode_utf16().collect()).collect::<Vec<_>>(),
        Path::new(&preparation.working_directory.path),
        &environment,
        None,
        RestrictedUserLaunchOptions {
            device_install_id: Some(&installation.manifest.installation_id),
            cmd_script: (command.shell == ManagedShell::Cmd).then_some(script_path.as_path()),
        },
    )?;
    let process_id = process.process_id();
    events.send(RunnerCoreMessage::Started { attempt: attempt.clone(), process_id })?;
    let input = process.take_stdin().ok_or_else(|| anyhow::anyhow!("runner.stdin_missing"))?;
    let limit = command.max_output_bytes as usize;
    let begun = std::time::Instant::now();
    std::thread::scope(move |scope| -> Result<()> {
        scope.spawn(move || {
            let mut input = input;
            let _ = input.write_all(command.stdin.as_bytes());
        });
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut retained = 0usize;
        let mut truncated = false;
        let mut direct_done = false;
        let mut sequence = 0;
        let mut progress_sequence = 1;
        loop {
            let mut out = process.drain_stdout(limit.saturating_sub(retained) + 1)?;
            let mut err = process.drain_stderr(limit.saturating_sub(retained + out.len()) + 1)?;
            let was_truncated = truncated;
            truncated |= retained.saturating_add(out.len()).saturating_add(err.len()) > limit;
            out.truncate(limit.saturating_sub(retained));
            retained += out.len();
            err.truncate(limit.saturating_sub(retained));
            retained += err.len();
            let requested = cancel.load(Ordering::Acquire);
            let reason = if requested == 1 {
                RunnerStopReason::Cancelled
            } else if requested != 0 {
                RunnerStopReason::CoreDisconnected
            } else if !direct_done && begun.elapsed() >= Duration::from_millis(command.timeout_ms) {
                RunnerStopReason::TimedOut
            } else if !direct_done && truncated {
                RunnerStopReason::OutputLimit
            } else {
                RunnerStopReason::Exited
            };
            if reason != RunnerStopReason::Exited {
                process.terminate(1)?;
            }
            let exit = process.wait_timeout(Duration::from_millis(10))?;
            if !direct_done {
                if !out.is_empty() || !err.is_empty() || truncated != was_truncated {
                    progress_sequence += 1;
                    events.send(RunnerCoreMessage::Progress {
                        update: RunnerProgress {
                            attempt: attempt.clone(),
                            sequence: progress_sequence,
                            observed_at: UtcTimestamp::now(),
                            submitted_script: None,
                            stdout: out.clone(),
                            stderr: err.clone(),
                            output_truncated: truncated,
                        },
                    })?;
                }
                stdout.extend(out);
                stderr.extend(err);
                if exit.is_some() || reason != RunnerStopReason::Exited {
                    if reason != RunnerStopReason::Exited {
                        ensure!(
                            process.wait_for_process_tree_exit(Duration::from_secs(5))?,
                            "runner.tree_not_stopped"
                        );
                    } else {
                        // Job ActiveProcesses can lag the root wait handle.
                        // Publish Completed only after the tree is empty, or
                        // after this bound wait if descendants remain.
                        let _ = process.wait_for_process_tree_exit(Duration::from_secs(1))?;
                    }
                    // Drain presently available bytes, never wait for descendant EOF.
                    let tail = process.drain_stdout(limit.saturating_sub(retained) + 1)?;
                    truncated |= tail.len() > limit.saturating_sub(retained);
                    let keep = tail.len().min(limit.saturating_sub(retained));
                    stdout.extend(&tail[..keep]);
                    retained += keep;
                    let tail = process.drain_stderr(limit.saturating_sub(retained) + 1)?;
                    truncated |= tail.len() > limit.saturating_sub(retained);
                    let keep = tail.len().min(limit.saturating_sub(retained));
                    stderr.extend(&tail[..keep]);
                    retained += keep;
                    let stopped = process.active_processes_in_job()? == 0;
                    events.send(RunnerCoreMessage::Completed {
                        observation: Box::new(RunnerObservation {
                            attempt: attempt.clone(),
                            binding_fingerprint: canonical_sha256(&envelope.dispatch.binding)?,
                            account_sid: preparation.account_sid.clone(),
                            process_id,
                            shell_image: preparation.shell_image.clone(),
                            started_at,
                            finished_at: UtcTimestamp::now(),
                            exit_code: process.wait_timeout(Duration::from_millis(1))?,
                            stop_reason: reason,
                            process_tree_stopped: stopped,
                            stdout: std::mem::take(&mut stdout),
                            stderr: std::mem::take(&mut stderr),
                            submitted_script: script.clone(),
                            output_truncated: truncated,
                        }),
                    })?;
                    direct_done = true;
                    if stopped {
                        break;
                    }
                }
            } else {
                let stopped = process.active_processes_in_job()? == 0;
                if stopped || !out.is_empty() || !err.is_empty() || truncated != was_truncated {
                    sequence += 1;
                    events.send(RunnerCoreMessage::Background {
                        update: RunnerBackgroundUpdate {
                            attempt: attempt.clone(),
                            sequence,
                            observed_at: UtcTimestamp::now(),
                            state: if reason != RunnerStopReason::Exited {
                                BackgroundState::Interrupted
                            } else if stopped {
                                BackgroundState::Exited
                            } else {
                                BackgroundState::Running
                            },
                            stdout: out,
                            stderr: err,
                            output_truncated: truncated,
                        },
                    })?;
                }
                if stopped {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    })
}
