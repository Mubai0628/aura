//! Direct argv, creation-time Job membership, bounded streams and confirmed whole-tree stop.
use crate::checked_program::{self, Links};
use anyhow::{Result, ensure};
use aura_windows_platform::{NativeEnvironmentEntry, NativeSandboxProcess, RootJob};
use aura_windows_user_contract::ExecutionFileBinding;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct GitProcessResult {
    pub exit_code: Option<u32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stopped: bool,
    pub reason: &'static str,
}

#[derive(Clone, Copy)]
pub(super) enum Mode {
    IsolatedRead,
    Native,
}
pub(super) struct ProcessSpec<'a> {
    pub image: &'a ExecutionFileBinding,
    pub directory: &'a Path,
    pub args: &'a [String],
    pub environment: &'a [NativeEnvironmentEntry],
    pub cancelled: &'a AtomicBool,
    pub timeout: Duration,
    pub limit: usize,
    pub mode: Mode,
    pub on_started: Option<&'a mut dyn FnMut()>,
}
pub(super) fn run(spec: ProcessSpec<'_>) -> Result<GitProcessResult> {
    let ProcessSpec {
        image,
        directory,
        args,
        environment,
        cancelled,
        timeout,
        limit,
        mode,
        on_started,
    } = spec;
    ensure!(
        timeout > Duration::ZERO && limit > 0 && limit <= 64 * 1024 * 1024,
        "git.invalid_limits"
    );
    let _image = crate::file_binding::verify_git_image(image)?;
    let (work_binding, _directory) = crate::file_binding::capture(directory, false)?;
    ensure!(_directory.info().is_directory, "git.directory_required");
    if cancelled.load(Ordering::Acquire) {
        return Ok(GitProcessResult {
            exit_code: None,
            stdout: vec![],
            stderr: vec![],
            stopped: true,
            reason: "cancelled",
        });
    }
    ensure!(args.iter().all(|arg| !arg.contains('\0')), "git.nul_argument");
    let wide = args.iter().map(|arg| arg.encode_utf16().collect()).collect::<Vec<_>>();
    let _read_directory = if matches!(mode, Mode::IsolatedRead) {
        Some(aura_windows_platform::GitReadDirectory::open(directory)?)
    } else {
        None
    };
    let job = if matches!(mode, Mode::IsolatedRead) {
        RootJob::create_single_process()?
    } else {
        RootJob::create()?
    };
    let image_path = aura_windows_platform::dos_drive_path(Path::new(&image.path))?;
    let work_path = aura_windows_platform::dos_drive_path(Path::new(&work_binding.path))?;
    let (mut process, timing) = match mode {
        Mode::IsolatedRead => aura_windows_platform::spawn_git_read_process(
            &job,
            &image_path,
            &wide,
            &work_path,
            environment,
        ),
        Mode::Native => aura_windows_platform::spawn_compat_process(
            &job,
            &image_path,
            &wide,
            &work_path,
            environment,
            None,
        ),
    }
    .inspect_err(|_| {
        checked_program::check_after_failure(image, Links::Allowed, "launch_failed")
    })?;
    // The owning permission-start lease survives through CreateProcess and
    // creation-time Job membership. Only the actual native start releases it.
    if let Some(started) = on_started {
        started();
    }
    drop(process.take_stdin());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let observed = if timing.created_in_job {
        observe(&process, &job, cancelled, timeout, limit, &mut stdout, &mut stderr)
    } else {
        Err(anyhow::anyhow!("git.job_not_bound_at_creation"))
    };
    let mut reason = match observed {
        Ok(reason) => reason,
        Err(error) => {
            append_error(&mut stderr, &stdout, limit, &error);
            "process_error"
        }
    };
    // Every post-launch error reaches this cleanup, including failed pipe/wait queries.
    // A failed query cannot be used as evidence that the Job has stopped.
    if reason != "exited"
        && let Err(error) = job.terminate_tree()
    {
        append_error(&mut stderr, &stdout, limit, &error.into());
    }
    let stop_deadline = Instant::now() + Duration::from_secs(3);
    let stopped = loop {
        match job.active_processes() {
            Ok(0) => break true,
            _ if Instant::now() >= stop_deadline => break false,
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    match drain(&process, &mut stdout, &mut stderr, limit) {
        Ok(true) if reason == "exited" => reason = "output_limit",
        Err(error) => {
            append_error(&mut stderr, &stdout, limit, &error);
            reason = "process_error";
        }
        _ => {}
    }
    // Empty Job accounting can precede the root's wait object becoming signalled.
    // Observe both within the same stop budget before claiming termination.
    let exit_code = process
        .wait_timeout(stop_deadline.saturating_duration_since(Instant::now()))
        .unwrap_or_else(|error| {
            append_error(&mut stderr, &stdout, limit, &error.into());
            reason = "process_error";
            None
        });
    let stopped = stopped && exit_code.is_some() && timing.created_in_job;
    // Git failed in a way it does not report itself: the program is checked again.
    if reason == "process_error" {
        checked_program::check_after_failure(image, Links::Allowed, "process_error");
    } else if reason == "exited" && exit_code.is_some_and(checked_program::is_error_status) {
        checked_program::check_after_failure(image, Links::Allowed, "exited_abnormally");
    }
    Ok(GitProcessResult { exit_code, stdout, stderr, stopped, reason })
}

fn observe(
    process: &NativeSandboxProcess,
    job: &RootJob,
    cancelled: &AtomicBool,
    timeout: Duration,
    limit: usize,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> Result<&'static str> {
    let began = Instant::now();
    loop {
        let exceeded = drain(process, stdout, stderr, limit)?;
        if cancelled.load(Ordering::Acquire) {
            return Ok("cancelled");
        } else if exceeded {
            return Ok("output_limit");
        } else if began.elapsed() >= timeout {
            return Ok("timed_out");
        }
        if process.wait_timeout(Duration::ZERO)?.is_some() && job.active_processes()? == 0 {
            return Ok("exited");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn append_error(stderr: &mut Vec<u8>, stdout: &[u8], limit: usize, error: &anyhow::Error) {
    let message = format!("\n{error}");
    let remaining = limit.saturating_sub(stdout.len() + stderr.len());
    stderr.extend_from_slice(&message.as_bytes()[..message.len().min(remaining)]);
}

fn drain(
    process: &NativeSandboxProcess,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    limit: usize,
) -> Result<bool> {
    // A bounded turn keeps cancel/deadline observation responsive under continuous output.
    for _ in 0..16 {
        let remaining = limit.saturating_sub(stdout.len() + stderr.len());
        let out = process.drain_stdout((remaining + 1).min(8192))?;
        let err = process.drain_stderr((remaining.saturating_sub(out.len()) + 1).min(8192))?;
        let count = out.len() + err.len();
        let exceeded = count > remaining;
        stdout.extend_from_slice(&out[..out.len().min(remaining)]);
        let remaining = limit.saturating_sub(stdout.len() + stderr.len());
        stderr.extend_from_slice(&err[..err.len().min(remaining)]);
        if count == 0 {
            return Ok(false);
        }
        if exceeded {
            return Ok(true);
        }
    }
    Ok(false)
}
