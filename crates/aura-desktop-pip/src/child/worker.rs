//! The child session's desktop host: this same program without arguments, its standard input and
//! output piped to the supervisor, its standard error dropped.
use super::relay::{Started, Workers};
use std::io;
use std::os::windows::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// No console window for the host, whichever subsystem it was built for.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct ProgramWorkers {
    program: PathBuf,
}

impl ProgramWorkers {
    /// # Errors
    ///
    /// When Windows cannot say which program this is.
    pub fn this_program() -> io::Result<Self> {
        Ok(Self { program: std::env::current_exe()? })
    }

    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }
}

impl Workers for ProgramWorkers {
    fn start(&self) -> io::Result<Started> {
        let mut child = Command::new(&self.program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(io::Error::other("没有拿到操控主机的输入输出"));
        };
        Ok(Started {
            input: Box::new(input),
            output: Box::new(output),
            stop: Box::new(move || {
                let _ = child.kill();
                let _ = child.wait();
            }),
        })
    }
}
