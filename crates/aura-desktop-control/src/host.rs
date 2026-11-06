//! The request loop: one request per line in, one reply per line out, in order. A line that is
//! not a request gets an error reply with the id it carries (0 when it carries none) and the loop
//! goes on; the loop ends when Core closes the host's standard input.
use aura_desktop_domain::{
    DESKTOP_PROTOCOL_VERSION, Delivery, HostCommand, HostError, HostErrorCode, HostMessage,
    HostOutcome, HostRequest, MAX_LINE_BYTES,
};
use std::io::{self, BufRead, Read, Write};
use std::sync::{Arc, Mutex};

/// What carries out the requests: the real desktop, or a stand-in in tests.
pub trait Desktop {
    fn run(&mut self, command: HostCommand) -> HostOutcome;
    fn request(&mut self, request: HostRequest) -> HostOutcome {
        self.run(request.command)
    }
}

/// Where the host's lines go. Shared, so that what the user does can be told between replies.
pub type Output = Arc<Mutex<dyn Write + Send>>;

/// What the host says by itself, queued by threads that must never wait for the output (the
/// hooks'). Written before the next line, so that Core reads it in the order it happened, and
/// by a thread of its own when no line comes.
static SAID: Mutex<Vec<HostMessage>> = Mutex::new(Vec::new());

/// Writes one message as one line, after what was said before it.
pub fn write(output: &Output, message: &HostMessage) -> io::Result<()> {
    write_lines(output, Some(message))
}

/// Says `message` without waiting for the output.
pub fn say(output: &Output, message: HostMessage) {
    SAID.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(message);
    let output = Arc::clone(output);
    let _ = std::thread::Builder::new().name("aura-desktop-say".into()).spawn(move || {
        let _ = write_lines(&output, None);
    });
}

fn write_lines(output: &Output, message: Option<&HostMessage>) -> io::Result<()> {
    let mut output = output.lock().map_err(|_| io::Error::other("host output poisoned"))?;
    let said = std::mem::take(&mut *SAID.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    for message in said.iter().chain(message) {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        output.write_all(&line)?;
    }
    output.flush()
}

pub fn serve(
    desktop: &mut dyn Desktop,
    mut input: impl BufRead,
    output: &Output,
) -> io::Result<()> {
    write(output, &HostMessage::Ready { protocol: DESKTOP_PROTOCOL_VERSION })?;
    let mut line = Vec::new();
    loop {
        line.clear();
        if Read::take(&mut input, MAX_LINE_BYTES as u64 + 1).read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        if line.len() > MAX_LINE_BYTES {
            skip_rest_of_line(&mut input)?;
            write(output, &refused(0, "这一行太长"))?;
            continue;
        }
        let text = String::from_utf8_lossy(&line);
        if text.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<HostRequest>(&text) {
            Ok(request) => HostMessage::Reply { id: request.id, outcome: desktop.request(request) },
            Err(error) => refused(id_of(&text), &format!("不是一个请求：{error}")),
        };
        write(output, &reply)?;
    }
}

fn skip_rest_of_line(input: &mut impl BufRead) -> io::Result<()> {
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return Ok(());
        }
        if let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            input.consume(end + 1);
            return Ok(());
        }
        let length = buffer.len();
        input.consume(length);
    }
}

fn id_of(text: &str) -> u64 {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value.get("id").and_then(serde_json::Value::as_u64))
        .unwrap_or(0)
}

fn refused(id: u64, message: &str) -> HostMessage {
    HostMessage::Reply {
        id,
        outcome: HostOutcome::Failed(HostError::new(
            HostErrorCode::InvalidRequest,
            message,
            Delivery::NotDelivered,
        )),
    }
}
