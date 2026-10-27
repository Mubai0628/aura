//! The owner outlives the direct result while descendants or stream writers remain.
use super::output::Output;
use anyhow::{Result, ensure};
use aura_windows_platform::{
    JobLaunchTiming, NativeSandboxProcess, ProcessIdentity, RootJob, VerifiedFile,
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) struct ExecutionResources {
    pub scratch: tempfile::TempDir,
    pub work_file: VerifiedFile,
    pub image_file: VerifiedFile,
    pub input: Option<JoinHandle<std::io::Result<()>>>,
    pub excluded_environment_names: Vec<String>,
}

pub(super) struct ExecutionSetup {
    pub installation: String,
    pub nonce: String,
    pub entered: Instant,
    pub prepared: Duration,
    pub timing: JobLaunchTiming,
    pub timeout: Duration,
    pub output_limit: usize,
    /// Hides the user's credentials in stdout and in stderr.
    pub hidden: [super::redaction::Redactor; 2],
}

pub struct CompatExecution {
    job: RootJob,
    process: NativeSandboxProcess,
    root: ProcessIdentity,
    resources: ExecutionResources,
    setup: ExecutionSetup,
    output: Output,
    input_error: Option<String>,
}

pub struct JobReceipt {
    pub installation: String,
    pub nonce: String,
    pub root: ProcessIdentity,
}

#[derive(Debug)]
pub struct JobObservation {
    pub root_exit: Option<u32>,
    pub active_processes: u32,
    pub members: Vec<ProcessIdentity>,
}

#[derive(Debug)]
pub struct StopObservation {
    pub confirmed: bool,
    pub active_processes: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct DirectResult {
    pub exit_code: Option<u32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub output_truncated: bool,
    pub active_processes: u32,
    pub stop_reason: &'static str,
    pub stop_confirmed: bool,
    pub stdin_completed: bool,
    pub stdin_error: Option<String>,
    pub elapsed: Duration,
    pub marker: Option<Duration>,
}

impl CompatExecution {
    pub(super) fn new(
        job: RootJob,
        process: NativeSandboxProcess,
        resources: ExecutionResources,
        mut setup: ExecutionSetup,
    ) -> Result<Self> {
        let root = setup
            .timing
            .root_identity
            .clone()
            .ok_or_else(|| anyhow::anyhow!("compat missing creation identity"))?;
        ensure!(root.process_id == process.process_id(), "compat launch identity mismatch");
        let hidden = std::mem::replace(
            &mut setup.hidden,
            [super::redaction::Redactor::none(), super::redaction::Redactor::none()],
        );
        let output = Output::new(setup.output_limit, hidden);
        Ok(Self { job, process, root, resources, setup, output, input_error: None })
    }

    pub fn root_identity(&self) -> &ProcessIdentity {
        &self.root
    }
    pub fn receipt(&self) -> JobReceipt {
        JobReceipt {
            installation: self.setup.installation.clone(),
            nonce: self.setup.nonce.clone(),
            root: self.root.clone(),
        }
    }
    pub fn launch_timing(&self) -> (&JobLaunchTiming, Duration) {
        (&self.setup.timing, self.setup.prepared)
    }
    pub fn marker_delay(&self) -> Option<Duration> {
        self.output.marker
    }
    pub fn working_directory(&self) -> &std::path::Path {
        &self.resources.work_file.info().final_path
    }
    pub fn retained_image(&self) -> &std::path::Path {
        &self.resources.image_file.info().final_path
    }
    pub fn scratch_directory(&self) -> &std::path::Path {
        self.resources.scratch.path()
    }
    pub fn excluded_environment_names(&self) -> &[String] {
        &self.resources.excluded_environment_names
    }
    pub fn output_since(
        &mut self,
        stdout: usize,
        stderr: usize,
    ) -> Result<(Vec<u8>, Vec<u8>, bool)> {
        self.drain()?;
        Ok((
            self.output.stdout.get(stdout..).unwrap_or_default().to_vec(),
            self.output.stderr.get(stderr..).unwrap_or_default().to_vec(),
            self.output.truncated,
        ))
    }
    /// Releases output held back as the possible start of a hidden value; no process may write.
    pub fn finish_output(&mut self) {
        self.output.finish();
    }
    pub fn active_processes(&self) -> Result<u32> {
        Ok(self.job.active_processes()?)
    }

    fn drain(&mut self) -> Result<()> {
        self.output.drain(&self.process, self.setup.entered)?;
        if self.resources.input.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(input) = self.resources.input.take()
        {
            match input.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => self.input_error = Some(error.to_string()),
                Err(_) => self.input_error = Some("input writer panicked".into()),
            }
        }
        Ok(())
    }

    pub fn observe(&mut self) -> Result<JobObservation> {
        self.drain()?;
        Ok(JobObservation {
            root_exit: self.process.wait_timeout(Duration::ZERO)?,
            active_processes: self.job.active_processes()?,
            members: self.job.process_identities()?,
        })
    }

    pub fn stop(&mut self, bound: Duration) -> StopObservation {
        let start = Instant::now();
        if matches!(self.job.active_processes(), Ok(0)) {
            return StopObservation { confirmed: true, active_processes: Some(0), error: None };
        }
        if let Err(error) = self.job.terminate_tree() {
            return StopObservation {
                confirmed: false,
                active_processes: self.job.active_processes().ok(),
                error: Some(error.to_string()),
            };
        }
        loop {
            match self.job.active_processes() {
                Ok(0) => {
                    return StopObservation {
                        confirmed: true,
                        active_processes: Some(0),
                        error: None,
                    };
                }
                Ok(active) if start.elapsed() >= bound => {
                    return StopObservation {
                        confirmed: false,
                        active_processes: Some(active),
                        error: Some("stop deadline: Job not confirmed empty".into()),
                    };
                }
                Ok(_) => std::thread::sleep(
                    Duration::from_millis(5).min(bound.saturating_sub(start.elapsed())),
                ),
                Err(error) => {
                    return StopObservation {
                        confirmed: false,
                        active_processes: None,
                        error: Some(error.to_string()),
                    };
                }
            }
        }
    }

    pub fn run_direct(&mut self, cancelled: &AtomicBool) -> Result<DirectResult> {
        self.run_direct_until(|| cancelled.load(Ordering::Acquire))
    }
    pub fn run_direct_until(&mut self, cancelled: impl Fn() -> bool) -> Result<DirectResult> {
        let mut reason = "exited";
        let mut stop_confirmed = false;
        loop {
            self.drain()?;
            if self.output.truncated {
                reason = "output_limit";
            } else if cancelled() {
                reason = "cancelled";
            } else if self.setup.entered.elapsed() >= self.setup.timeout {
                reason = "timed_out";
            }
            if reason != "exited" {
                stop_confirmed = self.stop(Duration::from_secs(3)).confirmed;
                break;
            }
            if self.process.wait_timeout(Duration::ZERO)?.is_some() {
                // CMD's console helpers can be exiting after the root is signaled.
                // During that window their image query may return access denied.
                // Only accounting proves emptiness; a still active Job after the
                // bounded grace remains owned background work, never ignored.
                let end = Instant::now() + Duration::from_millis(150);
                while self.job.active_processes()? > 0 && Instant::now() < end {
                    self.drain()?;
                    std::thread::sleep(Duration::from_millis(2));
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        // A root exit is a direct result even when descendants still own pipe endpoints.
        self.drain()?;
        let active = self.job.active_processes()?;
        stop_confirmed |= active == 0;
        if active == 0 {
            self.output.finish();
        }
        if stop_confirmed {
            let end = Instant::now() + Duration::from_millis(100);
            while self.resources.input.is_some() && Instant::now() < end {
                self.drain()?;
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(DirectResult {
            exit_code: self.process.wait_timeout(Duration::ZERO)?,
            stdout: self.output.stdout.clone(),
            stderr: self.output.stderr.clone(),
            output_truncated: self.output.truncated,
            active_processes: active,
            stop_reason: reason,
            stop_confirmed,
            stdin_completed: self.resources.input.is_none(),
            stdin_error: self.input_error.clone(),
            elapsed: self.setup.entered.elapsed(),
            marker: self.output.marker,
        })
    }
}

impl Drop for CompatExecution {
    fn drop(&mut self) {
        let _ = self.stop(Duration::from_millis(500));
    }
}
