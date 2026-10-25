use super::input::Input;
use crate::checked_program::{self, Links};
use anyhow::{Result, ensure};
use aura_windows_platform::{JobLimits, NativeEnvironmentEntry, NativeSandboxProcess, RootJob};
use aura_windows_user_contract::ExecutionFileBinding;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramStop {
    Exited,
    Cancelled,
    TimedOut,
    OutputLimit,
    ProtocolError,
    IoError,
}

#[derive(Debug)]
pub struct ProgramOutput {
    pub started: bool,
    pub process_id: Option<u32>,
    pub exit_code: Option<u32>,
    pub reason: ProgramStop,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stop_confirmed: bool,
    /// The tree used (nearly) all the memory its limits allow.
    pub memory_limit_reached: bool,
}

/// What the program reads on stdin.
#[derive(Clone, Copy, Debug)]
pub enum ProgramInput<'a> {
    /// Stdin is closed when the program starts.
    Closed,
    /// Core answers the program's stdout frames (the Aura stdio protocol).
    Frames,
    /// One document is written and stdin is then closed, so the program can read to its
    /// end. A program that exits without reading it is not a protocol error.
    Document(&'a [u8]),
}

pub struct ProgramSpec<'a> {
    pub image: &'a ExecutionFileBinding,
    pub arguments: &'a [String],
    pub working_directory: &'a Path,
    pub environment: &'a [NativeEnvironmentEntry],
    pub input: ProgramInput<'a>,
    pub cancelled: &'a AtomicBool,
    pub timeout: Duration,
    pub max_output_bytes: usize,
    /// What the whole tree may use: a plugin's limits; none for the user's own programs.
    pub limits: Option<JobLimits>,
}

/// Core's decoder may answer a valid hello with its bound Execute frame. The
/// callback cannot start a process, publish a Result or grant resource access.
pub trait ProgramIo {
    fn stdout(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>>;
}
impl<F> ProgramIo for F
where
    F: FnMut(&[u8]) -> Result<Vec<Vec<u8>>>,
{
    fn stdout(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
        self(bytes)
    }
}

pub fn run(
    spec: ProgramSpec<'_>,
    io: &mut dyn ProgramIo,
    on_started: &mut dyn FnMut(u32),
) -> Result<ProgramOutput> {
    ensure!(
        spec.timeout > Duration::ZERO && (1..=64 * 1024 * 1024).contains(&spec.max_output_bytes),
        "package.invalid_process_limits"
    );
    ensure!(spec.image.sha256.is_some(), "package.image_hash_required");
    ensure!(spec.arguments.iter().all(|value| !value.contains('\0')), "package.nul_argument");
    // A package's or a hook's program is the user's own: read in full the first time, then only
    // compared until it changed, failed to start or crashed.
    let _image = checked_program::hold(spec.image, Links::Single)?;
    let (directory, _directory) = crate::file_binding::capture(spec.working_directory, false)?;
    ensure!(_directory.info().is_directory, "package.directory_required");
    if spec.cancelled.load(Ordering::Acquire) {
        return Ok(ProgramOutput {
            started: false,
            process_id: None,
            exit_code: None,
            reason: ProgramStop::Cancelled,
            stdout: vec![],
            stderr: vec![],
            stop_confirmed: true,
            memory_limit_reached: false,
        });
    }
    let job = RootJob::create()?;
    if let Some(limits) = spec.limits {
        job.limit(limits)?;
    }
    let args =
        spec.arguments.iter().map(|value| value.encode_utf16().collect()).collect::<Vec<_>>();
    let (mut process, timing) = aura_windows_platform::spawn_compat_process(
        &job,
        &aura_windows_platform::dos_drive_path(Path::new(&spec.image.path))?,
        &args,
        &aura_windows_platform::dos_drive_path(Path::new(&directory.path))?,
        spec.environment,
        None,
    )
    .inspect_err(|_| {
        checked_program::check_after_failure(spec.image, Links::Single, "launch_failed")
    })?;
    // From here every error goes through whole-tree stop. Dropping a caller's
    // waiter cannot abandon a running Job: the blocking worker owns it.
    let mut input = None;
    let mut reason = ProgramStop::Exited;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if timing.created_in_job {
        on_started(process.process_id());
        match spec.input {
            ProgramInput::Closed => drop(process.take_stdin()),
            ProgramInput::Frames | ProgramInput::Document(_) => {
                match process.take_stdin().map(Input::open).transpose() {
                    Ok(value) => input = value,
                    Err(_) => reason = ProgramStop::IoError,
                }
            }
        }
        if let (ProgramInput::Document(bytes), Some(writer)) = (spec.input, input.as_mut()) {
            if writer.send(bytes.to_vec()).is_err() {
                reason = ProgramStop::IoError;
            }
            writer.close();
        }
    } else {
        reason = ProgramStop::IoError;
    }
    let began = Instant::now();
    while reason == ProgramStop::Exited {
        match drain(&process, &mut stdout, &mut stderr, spec.max_output_bytes, io, &mut input) {
            Ok(true) => reason = ProgramStop::OutputLimit,
            Err(DrainError::Protocol) => reason = ProgramStop::ProtocolError,
            Err(DrainError::Io) => reason = ProgramStop::IoError,
            Ok(false) => {}
        }
        if spec.cancelled.load(Ordering::Acquire) {
            reason = ProgramStop::Cancelled;
        } else if began.elapsed() >= spec.timeout && reason == ProgramStop::Exited {
            reason = ProgramStop::TimedOut;
        }
        if reason != ProgramStop::Exited {
            break;
        }
        match (process.wait_timeout(Duration::ZERO), job.active_processes()) {
            (Ok(Some(_)), Ok(0)) => break,
            (Err(_), _) | (_, Err(_)) => {
                reason = ProgramStop::IoError;
                break;
            }
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    if reason != ProgramStop::Exited {
        let _ = job.terminate_tree();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let stopped = loop {
        match job.active_processes() {
            Ok(0) => break true,
            _ if Instant::now() >= deadline => break false,
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    match drain(&process, &mut stdout, &mut stderr, spec.max_output_bytes, io, &mut input) {
        Ok(true) if reason == ProgramStop::Exited => reason = ProgramStop::OutputLimit,
        Err(DrainError::Protocol) if reason == ProgramStop::Exited => {
            reason = ProgramStop::ProtocolError
        }
        Err(DrainError::Io) if reason == ProgramStop::Exited => reason = ProgramStop::IoError,
        _ => {}
    }
    let exit_code =
        process.wait_timeout(deadline.saturating_duration_since(Instant::now())).ok().flatten();
    if exit_code.is_some_and(checked_program::is_error_status) {
        checked_program::check_after_failure(spec.image, Links::Single, "exited_abnormally");
    }
    let memory_limit_reached = super::session::memory_limit_reached(&job, spec.limits);
    // Close the owning Job before waiting for a blocked input writer. Even an
    // accounting/query failure still closes the kill-on-close boundary here.
    drop(job);
    let (input_stopped, input_written) = input.map_or((true, true), Input::finish);
    if reason == ProgramStop::Exited && !input_written && matches!(spec.input, ProgramInput::Frames)
    {
        reason = ProgramStop::ProtocolError;
    }
    Ok(ProgramOutput {
        started: true,
        process_id: Some(process.process_id()),
        exit_code,
        reason,
        stdout,
        stderr,
        stop_confirmed: stopped && exit_code.is_some() && timing.created_in_job && input_stopped,
        memory_limit_reached,
    })
}

enum DrainError {
    Io,
    Protocol,
}
fn drain(
    process: &NativeSandboxProcess,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    limit: usize,
    io: &mut dyn ProgramIo,
    input: &mut Option<Input>,
) -> Result<bool, DrainError> {
    for _ in 0..16 {
        let remaining = limit.saturating_sub(stdout.len() + stderr.len());
        let out = process.drain_stdout((remaining + 1).min(8192)).map_err(|_| DrainError::Io)?;
        let err = process
            .drain_stderr((remaining.saturating_sub(out.len()) + 1).min(8192))
            .map_err(|_| DrainError::Io)?;
        let count = out.len() + err.len();
        let retain = out.len().min(remaining);
        stdout.extend_from_slice(&out[..retain]);
        let left = limit.saturating_sub(stdout.len() + stderr.len());
        stderr.extend_from_slice(&err[..err.len().min(left)]);
        if count > remaining {
            return Ok(true);
        }
        if !out.is_empty() {
            for response in io.stdout(&out).map_err(|_| DrainError::Protocol)? {
                input
                    .as_mut()
                    .ok_or(DrainError::Protocol)?
                    .send(response)
                    .map_err(|_| DrainError::Protocol)?;
            }
        }
        if count == 0 {
            return Ok(false);
        }
    }
    Ok(false)
}
