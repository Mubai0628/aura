//! A service run as its plugin would run it, with a fake Aura on its standard streams that plays
//! the protocol line by line.
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct FakeAura {
    pub child: Child,
    input: ChildStdin,
    lines: mpsc::Receiver<String>,
    _folder: tempfile::TempDir,
}

impl FakeAura {
    /// `program` runs `script` saved as `file`, next to the helper `helper` copied in.
    pub fn start(program: &str, script: &str, file: &str, helper: &str) -> Self {
        let folder = tempfile::tempdir().expect("plugin folder");
        let name = helper.rsplit('/').next().expect("its name");
        std::fs::write(folder.path().join(name), super::sdk(helper)).expect("helper");
        std::fs::write(folder.path().join(file), script).expect("service");
        let mut child = Command::new(program)
            .arg(folder.path().join(file))
            .current_dir(folder.path())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("{program}: {error}"));
        let input = child.stdin.take().expect("stdin");
        let output = child.stdout.take().expect("stdout");
        let (send, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, input, lines, _folder: folder }
    }

    pub fn send(&mut self, message: &Value) {
        writeln!(self.input, "{message}").expect("write");
        self.input.flush().expect("flush");
    }

    /// The next line, which has to be a JSON object.
    pub fn next(&self, what: &str) -> Value {
        let line = self
            .lines
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("{what}: nothing in 10 s"));
        let value: Value =
            serde_json::from_str(&line).unwrap_or_else(|_| panic!("not a protocol line: {line}"));
        assert!(value.is_object(), "{line}");
        value
    }

    pub fn quiet_for(&self, time: Duration) {
        if let Ok(line) = self.lines.recv_timeout(time) {
            panic!("nothing more yet, but: {line}");
        }
    }

    /// The text of the next `log` line.
    pub fn logged(&self) -> String {
        let line = self.next("a log line");
        assert_eq!(line["type"], "log", "{line}");
        line["text"].as_str().expect("text").to_owned()
    }

    /// The next request, past the lines it logged before it.
    pub fn request(&self, what: &str) -> Value {
        loop {
            let line = self.next(what);
            if line["type"] != "log" {
                assert_eq!(line["type"], "request", "{what}: {line}");
                return line;
            }
        }
    }

    /// How the service ended, once it did by itself.
    pub fn ended(&mut self) -> std::process::ExitStatus {
        let since = Instant::now();
        while since.elapsed() < Duration::from_secs(10) {
            if let Some(status) = self.child.try_wait().expect("its state") {
                return status;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the service did not end by itself in 10 s");
    }
}

impl Drop for FakeAura {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
