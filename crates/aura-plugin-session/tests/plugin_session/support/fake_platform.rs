//! The Skill's fake chat platform, on a port of its own on this computer.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct FakePlatform {
    child: Child,
    pub port: u16,
}

impl FakePlatform {
    pub fn start() -> Self {
        let mut child = Command::new("python")
            .arg(super::skill_path("examples/fake-chat/platform/fake_platform.py"))
            .args(["--port", "0"])
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("python");
        let mut line = String::new();
        let mut output = BufReader::new(child.stdout.take().expect("stdout"));
        output.read_line(&mut line).expect("its address");
        let port = line.trim().rsplit(':').next().and_then(|port| port.parse().ok());
        let port = port.unwrap_or_else(|| panic!("no port in {line:?}"));
        Self { child, port }
    }

    pub fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Value {
        let body = body.map(Value::to_string).unwrap_or_default();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", self.port)).expect("platform");
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        let (_, body) = response.split_once("\r\n\r\n").expect("a body");
        serde_json::from_str(body).expect("JSON")
    }

    pub fn push(&self, user: &str, text: &str) {
        let message = json!({"chat_type": "private", "chat_id": user, "user_id": user,
            "user_name": "老王", "text": text});
        self.call("POST", "/push", Some(&message));
    }

    /// What the bot posted, oldest first.
    pub fn sent(&self) -> Vec<Value> {
        self.call("GET", "/sent", None)["sent"].as_array().cloned().unwrap_or_default()
    }

    /// What the bot posted, once it posted `count` messages.
    pub fn sent_at_least(&self, count: usize) -> Vec<Value> {
        for _ in 0..100 {
            let sent = self.sent();
            if sent.len() >= count {
                return sent;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the bot did not post {count} messages in 10 s: {:?}", self.sent());
    }
}

impl Drop for FakePlatform {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
