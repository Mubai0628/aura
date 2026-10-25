//! The repository's HTTP test MCP server (`aura-mcp-http-fixture`), started from the build
//! folder for one test on 127.0.0.1 and stopped when the test ends.
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) struct HttpServer {
    child: Child,
    /// `http://127.0.0.1:<port>/mcp`
    pub(crate) url: String,
    marker: PathBuf,
}

impl HttpServer {
    pub(crate) fn start(folder: &Path) -> Self {
        Self::start_with(folder, &[])
    }

    /// [`HttpServer::start`] with more arguments (for example `--require-auth`).
    pub(crate) fn start_with(folder: &Path, arguments: &[&str]) -> Self {
        let debug = std::env::current_exe()
            .expect("test image")
            .parent()
            .expect("deps")
            .parent()
            .expect("debug")
            .to_owned();
        let marker = folder.join("http-fixture-marker.txt");
        let mut child = Command::new(debug.join("aura-mcp-http-fixture.exe"))
            .arg("--marker")
            .arg(&marker)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("build current aura-tool-fixtures --bins first");
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("standard output"))
            .read_line(&mut line)
            .expect("the server's address");
        Self { child, url: line.trim().to_owned(), marker }
    }

    /// Stops the server now; its address no longer answers.
    pub(crate) fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// The address with another path on the same server.
    pub(crate) fn at(&self, path: &str) -> String {
        self.url.replace("/mcp", path)
    }

    /// What the server saw, one line per event.
    pub(crate) fn marks(&self) -> Vec<String> {
        std::fs::read_to_string(&self.marker)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    pub(crate) fn count(&self, prefix: &str) -> usize {
        self.marks().iter().filter(|line| line.starts_with(prefix)).count()
    }

    /// Waits up to ten seconds for a line starting with `prefix`.
    pub(crate) fn saw(&self, prefix: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.count(prefix) > 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop();
    }
}
