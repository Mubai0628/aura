//! Starting the desktop host: the program `aura-desktop-host.exe` beside Core's own, as the user,
//! in a Job of its own that ends with it; its standard output split into lines.
use aura_backend_windows_user::checked_program::{self, Links};
use aura_backend_windows_user::program::{
    ProgramEnvironment, ProgramSession, SessionEvent, SessionSpec,
};
use aura_desktop_domain::MAX_LINE_BYTES;
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc;

/// The host program's file name, beside `aura-core.exe`.
pub const HOST_PROGRAM: &str = "aura-desktop-host.exe";

/// What a host says, line by line, until it ends.
#[derive(Debug, Eq, PartialEq)]
pub enum HostEvent {
    Line(Vec<u8>),
    /// A line longer than the protocol allows; it was dropped.
    TooLong,
    Ended,
}

/// A started host: lines go to its standard input; stopping it ends its whole process tree.
pub trait HostProcess: Send {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()>;
    fn stop(&mut self);
}

pub trait HostLauncher: Send + Sync + 'static {
    fn start(
        &self,
        events: mpsc::UnboundedSender<HostEvent>,
    ) -> Result<Box<dyn HostProcess>, String>;
}

/// Starts the real host program.
pub struct ProgramHost {
    pub program: PathBuf,
    /// Where it runs and keeps its TEMP.
    pub folder: PathBuf,
}

impl ProgramHost {
    /// The host beside the running Core.
    pub fn beside_core(folder: PathBuf) -> Result<Self, String> {
        let core = std::env::current_exe().map_err(|error| error.to_string())?;
        let program = core
            .parent()
            .map(|dir| dir.join(HOST_PROGRAM))
            .ok_or_else(|| "找不到 Core 所在的文件夹".to_owned())?;
        Ok(Self { program, folder })
    }
}

impl HostProcess for ProgramSession {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()> {
        ProgramSession::send(self, line)
    }

    fn stop(&mut self) {
        ProgramSession::stop(self);
    }
}

impl HostLauncher for ProgramHost {
    fn start(
        &self,
        events: mpsc::UnboundedSender<HostEvent>,
    ) -> Result<Box<dyn HostProcess>, String> {
        self.start_arguments(events, &[])
    }
}

impl ProgramHost {
    pub(crate) fn start_arguments(
        &self,
        events: mpsc::UnboundedSender<HostEvent>,
        arguments: &[String],
    ) -> Result<Box<dyn HostProcess>, String> {
        let temp = self.folder.join("temp");
        std::fs::create_dir_all(&temp)
            .map_err(|error| format!("无法建立文件夹 {}：{error}", temp.display()))?;
        // A development build hard-links its programs; the file is held against writes and
        // checked again when the session starts either way.
        let (binding, held) = checked_program::capture(&self.program, Links::Allowed)
            .map_err(|error| format!("{} 无法读取：{error}", self.program.display()))?;
        let environment = ProgramEnvironment::current(&[], false, &temp)
            .map_err(|error| format!("环境变量不对：{error}"))?;
        let (raw, session_events) = std_mpsc::channel();
        let session = ProgramSession::start(
            SessionSpec {
                image: &binding,
                cmd_script: None,
                arguments,
                working_directory: &self.folder,
                environment: environment.entries(),
                other_links_allowed: true,
                limits: None,
            },
            raw,
        )
        .map_err(|error| format!("没能启动 {}：{error}", self.program.display()))?;
        drop(held);
        std::thread::Builder::new()
            .name("aura-desktop-host-lines".into())
            .spawn(move || forward(&session_events, &events))
            .map_err(|error| error.to_string())?;
        Ok(Box::new(session))
    }
}

/// Splits standard output into lines; standard error is not read by Core.
fn forward(session: &std_mpsc::Receiver<SessionEvent>, events: &mpsc::UnboundedSender<HostEvent>) {
    let mut lines = Lines::default();
    while let Ok(event) = session.recv() {
        match event {
            SessionEvent::Stdout(chunk) => {
                for line in lines.push(&chunk) {
                    if events.send(line).is_err() {
                        return;
                    }
                }
            }
            SessionEvent::Stderr(_) => {}
            SessionEvent::Ended { .. } => {
                let _ = events.send(HostEvent::Ended);
                return;
            }
        }
    }
    let _ = events.send(HostEvent::Ended);
}

/// Whole lines out of chunks: a line over [`MAX_LINE_BYTES`], or the limit given, is dropped up to
/// its line break.
pub struct Lines {
    buffer: Vec<u8>,
    skipping: bool,
    limit: usize,
}

impl Default for Lines {
    fn default() -> Self {
        Self::with_limit(MAX_LINE_BYTES)
    }
}

impl Lines {
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self { buffer: Vec::new(), skipping: false, limit }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<HostEvent> {
        let mut out = Vec::new();
        for part in chunk.split_inclusive(|byte| *byte == b'\n') {
            let ends = part.last() == Some(&b'\n');
            let content = if ends { &part[..part.len() - 1] } else { part };
            if self.skipping {
                self.skipping = !ends;
                continue;
            }
            self.buffer.extend_from_slice(content);
            if self.buffer.len() > self.limit {
                self.buffer.clear();
                self.skipping = !ends;
                out.push(HostEvent::TooLong);
            } else if ends {
                let mut line = std::mem::take(&mut self.buffer);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if !line.iter().all(u8::is_ascii_whitespace) {
                    out.push(HostEvent::Line(line));
                }
            }
        }
        out
    }
}

/// A launcher for a host that cannot be started, saying why.
pub(crate) struct Missing(pub String);

impl HostLauncher for Missing {
    fn start(&self, _: mpsc::UnboundedSender<HostEvent>) -> Result<Box<dyn HostProcess>, String> {
        Err(self.0.clone())
    }
}
