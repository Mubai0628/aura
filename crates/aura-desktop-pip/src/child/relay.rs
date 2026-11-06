//! Passing lines between Core's pipe and the host the supervisor runs. Core's orders and requests
//! are read on the calling thread, in order; the host's lines are written to the pipe by a thread
//! of the host's own. What the supervisor says comes before the lines it is about: `started`
//! before the host's first line, `ended` after its last.
use super::super::lines::{Line, next_line};
use aura_desktop_domain::MAX_LINE_BYTES;
use aura_desktop_domain::child::{
    CHILD_PROTOCOL_VERSION, ChildLine, MAX_CHILD_LINE_BYTES, SupervisorOrder, SupervisorSays,
    host_line, read_child_line, supervisor_line,
};
use std::io::{self, BufReader, Read, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

/// A host that was started: its standard input and output, and how to end it.
pub struct Started {
    pub input: Box<dyn Write + Send>,
    pub output: Box<dyn Read + Send>,
    pub stop: Box<dyn FnMut() + Send>,
}

/// Starts hosts: the real program, or a stand-in in tests.
pub trait Workers {
    /// # Errors
    ///
    /// Why the host could not be started, as the supervisor tells Core.
    fn start(&self) -> io::Result<Started>;
}

type Shared = Arc<Mutex<Box<dyn Write + Send>>>;

struct Running {
    input: Box<dyn Write + Send>,
    stop: Box<dyn FnMut() + Send>,
    lines: JoinHandle<()>,
}

impl Running {
    /// Ends the host and waits until its last line and `ended` are on the pipe.
    fn end(mut self) {
        (self.stop)();
        drop(self.input);
        let _ = self.lines.join();
    }
}

/// Says hello with `token`, then serves Core's lines from `pipe` until Core closes it, writing to
/// `out`.
///
/// # Errors
///
/// When the pipe can be neither read nor written.
pub fn relay(
    pipe: impl Read,
    out: Box<dyn Write + Send>,
    token: &str,
    workers: &dyn Workers,
) -> io::Result<()> {
    let out: Shared = Arc::new(Mutex::new(out));
    say(
        &out,
        &SupervisorSays::Hello { token: token.to_owned(), protocol: CHILD_PROTOCOL_VERSION },
    )?;
    let mut pipe = BufReader::new(pipe);
    let mut running: Option<Running> = None;
    let served = loop {
        let line = match next_line(&mut pipe, MAX_CHILD_LINE_BYTES) {
            Ok(Some(Line::Line(line))) => line,
            Ok(Some(Line::TooLong)) => continue,
            Ok(None) => break Ok(()),
            Err(error) => break Err(error),
        };
        match read_child_line::<SupervisorOrder>(&line) {
            Ok(ChildLine::Host(request)) => {
                // Without a host the request is dropped: Core heard `ended` or `not_started`.
                if let Some(host) = running.as_mut() {
                    let mut framed = request.to_vec();
                    framed.push(b'\n');
                    let _ = host.input.write_all(&framed).and_then(|()| host.input.flush());
                }
            }
            Ok(ChildLine::Supervisor(SupervisorOrder::Start)) => {
                // Core's order wins over a host it no longer counts on: that one ends first, so
                // its `ended` comes before the new host's `started`.
                if let Some(earlier) = running.take() {
                    earlier.end();
                }
                match workers.start() {
                    Ok(started) => {
                        if let Err(error) = say(&out, &SupervisorSays::Started) {
                            let mut stop = started.stop;
                            stop();
                            break Err(error);
                        }
                        let lines = forward(started.output, Arc::clone(&out));
                        running = Some(Running { input: started.input, stop: started.stop, lines });
                    }
                    Err(error) => {
                        let why = SupervisorSays::NotStarted { why: error.to_string() };
                        if let Err(error) = say(&out, &why) {
                            break Err(error);
                        }
                    }
                }
            }
            Ok(ChildLine::Supervisor(SupervisorOrder::Stop)) => {
                if let Some(host) = running.take() {
                    host.end();
                }
            }
            Err(_) => {}
        }
    };
    if let Some(host) = running.take() {
        host.end();
    }
    served
}

fn say(out: &Shared, message: &SupervisorSays) -> io::Result<()> {
    write_line(out, &supervisor_line(message))
}

fn write_line(out: &Shared, line: &[u8]) -> io::Result<()> {
    let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
    out.write_all(line)?;
    out.flush()
}

/// Writes the host's lines to the pipe until its output ends, then says it ended.
fn forward(output: Box<dyn Read + Send>, out: Shared) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut output = BufReader::new(output);
        while let Ok(Some(line)) = next_line(&mut output, MAX_LINE_BYTES) {
            if let Line::Line(line) = line
                && write_line(&out, &host_line(&line)).is_err()
            {
                break;
            }
        }
        let _ = say(&out, &SupervisorSays::Ended);
    })
}
