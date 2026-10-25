//! A long-lived program in its own Job. Core may queue a frame for
//! stdin at any time and receives stdout and stderr as they arrive. Stopping the session,
//! dropping it or the program exiting ends the whole process tree; nothing outlives it.
use crate::checked_program::{self, Links};
use anyhow::{Result, anyhow, ensure};
use aura_windows_platform::{JobLimits, NativeEnvironmentEntry, NativeSandboxProcess, RootJob};
use aura_windows_user_contract::ExecutionFileBinding;
use std::{
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// The largest single frame Core may queue for stdin.
pub const MAX_SESSION_FRAME_BYTES: usize = 8 * 1024 * 1024;
const QUEUED_FRAMES: usize = 64;
const READ_CHUNK: usize = 64 * 1024;

pub struct SessionSpec<'a> {
    /// The verified program, or the Windows CMD image when `cmd_script` is set.
    pub image: &'a ExecutionFileBinding,
    /// A Core-written `.cmd` file CMD runs; its arguments are inside the script.
    pub cmd_script: Option<&'a Path>,
    pub arguments: &'a [String],
    pub working_directory: &'a Path,
    pub environment: &'a [NativeEnvironmentEntry],
    /// Whether the program file may have other hard links (an MCP server's may). It is held
    /// against writes and verified again here either way.
    pub other_links_allowed: bool,
    /// What the whole tree may use: a plugin's limits; none for the user's own programs.
    pub limits: Option<JobLimits>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SessionEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    /// The program and everything it started are gone, or could not be confirmed gone.
    Ended {
        exit_code: Option<u32>,
        stop_confirmed: bool,
        /// The tree used (nearly) all the memory its limits allow.
        memory_limit_reached: bool,
    },
}

pub struct ProgramSession {
    frames: Option<mpsc::SyncSender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
    process_id: u32,
}

impl ProgramSession {
    /// Starts the program in a new Job and returns at once. Events arrive on `events`, in
    /// order, until exactly one `Ended`.
    pub fn start(spec: SessionSpec<'_>, events: mpsc::Sender<SessionEvent>) -> Result<Self> {
        ensure!(spec.image.sha256.is_some(), "program.image_hash_required");
        ensure!(spec.arguments.iter().all(|value| !value.contains('\0')), "program.nul_argument");
        // The user's own program: read in full the first time, and again
        // only after it changed or a launch of it failed.
        let links = if spec.other_links_allowed { Links::Allowed } else { Links::Single };
        let image = checked_program::hold(spec.image, links)?;
        let (directory, held_directory) =
            crate::file_binding::capture(spec.working_directory, false)?;
        ensure!(held_directory.info().is_directory, "program.directory_required");
        let job = RootJob::create()?;
        if let Some(limits) = spec.limits {
            job.limit(limits)?;
        }
        let arguments =
            spec.arguments.iter().map(|value| value.encode_utf16().collect()).collect::<Vec<_>>();
        let (mut process, timing) = aura_windows_platform::spawn_compat_process(
            &job,
            &aura_windows_platform::dos_drive_path(Path::new(&spec.image.path))?,
            &arguments,
            &aura_windows_platform::dos_drive_path(Path::new(&directory.path))?,
            spec.environment,
            spec.cmd_script,
        )
        .inspect_err(|_| {
            checked_program::check_after_failure(spec.image, links, "launch_failed")
        })?;
        if !timing.created_in_job {
            let _ = job.terminate_tree();
            return Err(anyhow!("program.not_started_in_job"));
        }
        let process_id = process.process_id();
        let Some(mut stdin) = process.take_stdin() else {
            let _ = job.terminate_tree();
            return Err(anyhow!("program.stdin_unavailable"));
        };
        let (frames, queued) = mpsc::sync_channel::<Vec<u8>>(QUEUED_FRAMES);
        let writer =
            std::thread::Builder::new().name("aura-session-stdin".into()).spawn(move || {
                for frame in queued {
                    if stdin.write_all(&frame).and_then(|()| stdin.flush()).is_err() {
                        return;
                    }
                }
            })?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let limits = spec.limits;
        let binding = spec.image.clone();
        let worker = std::thread::Builder::new().name("aura-session".into()).spawn(move || {
            // The program file and folder stay held until the tree is gone.
            let _held = (image, held_directory);
            let crashed = follow(&process, &job, limits, &flag, &events);
            if crashed {
                checked_program::check_after_failure(&binding, links, "exited_abnormally");
            }
        })?;
        Ok(Self {
            frames: Some(frames),
            stop,
            worker: Some(worker),
            writer: Some(writer),
            process_id,
        })
    }

    #[must_use]
    pub const fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Queues one frame for stdin. A program that stopped reading is refused rather than
    /// waited on.
    pub fn send(&self, frame: Vec<u8>) -> Result<()> {
        ensure!(frame.len() <= MAX_SESSION_FRAME_BYTES, "program.frame_too_large");
        self.frames
            .as_ref()
            .ok_or_else(|| anyhow!("program.stdin_closed"))?
            .try_send(frame)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => anyhow!("program.stdin_backpressure"),
                mpsc::TrySendError::Disconnected(_) => anyhow!("program.stdin_closed"),
            })
    }

    /// Ends the program and its tree. Returns once the session thread finished, or after a
    /// bounded wait.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.frames.take();
        for handle in [self.worker.take(), self.writer.take()].into_iter().flatten() {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !handle.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for ProgramSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn drain(
    process: &NativeSandboxProcess,
    events: &mpsc::Sender<SessionEvent>,
) -> Result<bool, std::io::Error> {
    let out = process.drain_stdout(READ_CHUNK)?;
    let err = process.drain_stderr(READ_CHUNK)?;
    let active = !out.is_empty() || !err.is_empty();
    // A receiver that went away is Core no longer listening: the session stops.
    if !out.is_empty() && events.send(SessionEvent::Stdout(out)).is_err() {
        return Err(std::io::Error::other("session receiver closed"));
    }
    if !err.is_empty() && events.send(SessionEvent::Stderr(err)).is_err() {
        return Err(std::io::Error::other("session receiver closed"));
    }
    Ok(active)
}

/// Follows the program until it ends; true when it ended on its own with a Windows error status
/// (a crash or a failed load), not by a stop.
fn follow(
    process: &NativeSandboxProcess,
    job: &RootJob,
    limits: Option<JobLimits>,
    stop: &AtomicBool,
    events: &mpsc::Sender<SessionEvent>,
) -> bool {
    let mut idle = 0_u32;
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        match drain(process, events) {
            Err(_) => break,
            Ok(true) => {
                idle = 0;
                continue;
            }
            Ok(false) => {}
        }
        match (process.wait_timeout(Duration::ZERO), job.active_processes()) {
            (Ok(Some(_)), Ok(0)) | (Err(_), _) | (_, Err(_)) => break,
            _ => {}
        }
        idle = idle.saturating_add(1);
        // Quick while a conversation is going on, gentle while the server waits.
        std::thread::sleep(Duration::from_millis(if idle < 50 { 2 } else { 15 }));
    }
    let _ = job.terminate_tree();
    let deadline = Instant::now() + Duration::from_secs(3);
    let stopped = loop {
        match job.active_processes() {
            Ok(0) => break true,
            _ if Instant::now() >= deadline => break false,
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    for _ in 0..16 {
        if !matches!(drain(process, events), Ok(true)) {
            break;
        }
    }
    let exit_code = process.wait_timeout(Duration::from_millis(500)).ok().flatten();
    let _ = events.send(SessionEvent::Ended {
        exit_code,
        stop_confirmed: stopped && exit_code.is_some(),
        memory_limit_reached: memory_limit_reached(job, limits),
    });
    !stop.load(Ordering::Acquire) && exit_code.is_some_and(checked_program::is_error_status)
}

/// Whether the tree's memory came within 5% of its limit: an allocation that failed there is
/// the likely reason it ended.
pub(crate) fn memory_limit_reached(job: &RootJob, limits: Option<JobLimits>) -> bool {
    limits.is_some_and(|limits| {
        job.peak_memory()
            .is_ok_and(|peak| peak.saturating_mul(100) >= limits.memory_bytes.saturating_mul(95))
    })
}
