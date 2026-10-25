//! ConPTY and an owned non-breakaway Job. Native resources never serialize terminal input.
use super::*;
mod console;
mod ctrl_c;
mod wait;
use crate::{ProcessIdentity, RootJob};
use console::ConsoleSession;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::System::Console::COORD;

pub type TerminalOutputCallback = Arc<dyn Fn(&[u8]) -> io::Result<()> + Send + Sync>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}
impl TerminalSize {
    fn coord(self) -> io::Result<COORD> {
        if self.columns == 0
            || self.rows == 0
            || self.columns > i16::MAX as u16
            || self.rows > i16::MAX as u16
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "terminal.size must be between 1 and 32767 cells",
            ));
        }
        Ok(COORD { X: self.columns as i16, Y: self.rows as i16 })
    }
}
pub struct NativeTerminalLaunch<'a> {
    pub executable: &'a Path,
    pub arguments: &'a [Vec<u16>],
    pub directory: &'a Path,
    pub environment: &'a [NativeEnvironmentEntry],
    pub size: TerminalSize,
}
pub struct NativeTerminal {
    process: OwnedHandle,
    job: RootJob,
    console: ConsoleSession,
    identity: ProcessIdentity,
    closed: AtomicBool,
    close_result: Mutex<Option<Result<Option<u32>, String>>>,
    wake: Arc<wait::TerminalWake>,
}
impl NativeTerminal {
    pub fn spawn(
        spec: NativeTerminalLaunch<'_>,
        sink: TerminalOutputCallback,
    ) -> Result<Self, JobLaunchError> {
        Self::spawn_traced(spec, sink, LaunchOverrides::default())
    }
    #[cfg(any(test, feature = "compat-adoption"))]
    pub fn spawn_with_fault(
        spec: NativeTerminalLaunch<'_>,
        sink: TerminalOutputCallback,
        fault: JobLaunchFault,
    ) -> Result<Self, JobLaunchError> {
        Self::spawn_traced(spec, sink, LaunchOverrides { fault: Some(fault), ..Default::default() })
    }
    fn spawn_traced(
        spec: NativeTerminalLaunch<'_>,
        sink: TerminalOutputCallback,
        overrides: LaunchOverrides<'_>,
    ) -> Result<Self, JobLaunchError> {
        let mut timing = JobLaunchTiming::default();
        let began = Instant::now();
        let result = (|| {
            let directory = crate::dos_drive_path(spec.directory)?;
            if !directory.is_absolute() || !spec.executable.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "terminal.absolute_paths_required",
                ));
            }
            let environment = environment_block(spec.environment, None, &directory)?;
            let directory = wide_os_null(directory.as_os_str());
            // PSHOME inherits the executable spelling. A Win32 extended prefix is
            // mistaken for a UNC module path by interactive PowerShell autoload.
            let executable = crate::dos_drive_path(spec.executable)?;
            let application = wide_os_null(executable.as_os_str());
            let mut command_line = quote_windows_argument_wide(
                &executable.as_os_str().encode_wide().collect::<Vec<_>>(),
            );
            for argument in spec.arguments {
                if argument.contains(&0) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "terminal.argument_contains_nul",
                    ));
                }
                command_line.push(u16::from(b' '));
                command_line.extend(quote_windows_argument_wide(argument));
            }
            command_line.push(0);
            let token =
                current_process_is_elevated()?.then(standard_user_primary_token).transpose()?;
            let job = RootJob::create()?;
            let job_handle = job.duplicate_for_launch()?;
            let wake = Arc::new(wait::TerminalWake::new()?);
            let console = ConsoleSession::create(spec.size, sink, Arc::clone(&wake))?;
            let mut attributes =
                ProcessAttributeList::with_pseudoconsole(console.handle()?, job_handle.as_raw())?;
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            // Explicit NULL standard handles prevent Windows from duplicating the parent's
            // redirected IPC/stdout pipes. ConPTY supplies the replacement console handles.
            // https://github.com/microsoft/terminal/discussions/15814
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.lpAttributeList = attributes.as_raw();
            ctrl_c::let_terminal_programs_receive_ctrl_c()?;
            timing.before_create = began.elapsed();
            let information = launch::create_native_process(launch::NativeStartup {
                application: &application,
                command_line: &mut command_line,
                directory: &directory,
                environment: &environment,
                startup: &startup,
                token: token.as_ref(),
                as_user: false,
                inherit_handles: false,
                flags: CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
            })?;
            let process = {
                let mut options = LaunchOverrides {
                    job_at_creation: true,
                    trace: Some((&mut timing, began)),
                    #[cfg(any(test, feature = "compat-adoption"))]
                    fault: overrides.fault,
                    ..Default::default()
                };
                launch::adopt_created_process(&job_handle, information, &mut options)?
            };
            #[cfg(not(any(test, feature = "compat-adoption")))]
            let _ = overrides;
            console.release_creation_peers()?;
            let identity = timing
                .root_identity
                .clone()
                .ok_or_else(|| io::Error::other("terminal.identity_missing"))?;
            Ok(Self {
                process,
                job,
                console,
                identity,
                closed: AtomicBool::new(false),
                close_result: Mutex::new(None),
                wake,
            })
        })();
        result.map_err(|source| JobLaunchError { source, timing: Box::new(timing) })
    }
    /// Test support: puts this process in the state a Core inherits from a parent that
    /// ignores Ctrl+C (for example one started in a new process group).
    #[cfg(any(test, feature = "compat-adoption"))]
    #[doc(hidden)]
    pub fn inherit_ignored_ctrl_c_for_test(ignore: bool) -> io::Result<()> {
        ctrl_c::set_ignored(ignore)
    }
    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    pub fn process_identities(&self) -> io::Result<Vec<ProcessIdentity>> {
        self.job.process_identities()
    }
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::Error::other("terminal.closed"));
        }
        self.console.write(bytes)
    }
    pub fn interrupt(&self) -> io::Result<()> {
        self.write(&[3])
    }
    pub fn resize(&self, size: TerminalSize) -> io::Result<bool> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::Error::other("terminal.closed"));
        }
        self.console.resize(size)
    }
    pub fn output_error(&self) -> Option<String> {
        self.console.output_error()
    }
    /// Blocks without periodic wakeups. False denotes a close signal; output errors are errors.
    pub fn wait_for_exit(&self) -> io::Result<bool> {
        let exited = self.wake.wait(&self.process)?;
        if !exited && !self.closed.load(Ordering::Acquire) {
            return Err(io::Error::other(
                self.output_error().unwrap_or_else(|| "terminal.output_failed".into()),
            ));
        }
        Ok(exited)
    }
    pub fn try_exit_code(&self) -> io::Result<Option<u32>> {
        // SAFETY: retained process handle. Waiting first distinguishes a real exit code 259.
        let waited = unsafe { WaitForSingleObject(self.process.as_raw(), 0) };
        if waited == WAIT_TIMEOUT {
            return Ok(None);
        }
        if waited != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.process.as_raw(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(code))
    }
    pub fn close(&self) -> io::Result<Option<u32>> {
        let mut cached =
            self.close_result.lock().map_err(|_| io::Error::other("terminal.close_lock"))?;
        if let Some(result) = cached.as_ref() {
            return result.clone().map_err(io::Error::other);
        }
        self.closed.store(true, Ordering::Release);
        let woken = self.wake.signal();
        let stopped = self.job.duplicate_for_launch().and_then(|job| {
            crate::job::terminate_job(&job, 1)?;
            if !crate::job::wait_for_job_empty(&job, Duration::from_secs(5))? {
                return Err(io::Error::other("terminal.stop_unconfirmed"));
            }
            // The Job's active count reaches zero slightly before the root process object is
            // signaled. A confirmed stop also means the root has exited and has an exit code.
            // SAFETY: the retained root process handle is owned by this terminal.
            if unsafe { WaitForSingleObject(self.process.as_raw(), 2_000) } != WAIT_OBJECT_0 {
                return Err(io::Error::other("terminal.stop_unconfirmed"));
            }
            Ok(())
        });
        let drained = self.console.close();
        let result = stopped
            .and(drained)
            .and(woken)
            .and_then(|()| self.try_exit_code())
            .map_err(|error| error.to_string());
        *cached = Some(result.clone());
        result.map_err(io::Error::other)
    }
}
impl Drop for NativeTerminal {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
