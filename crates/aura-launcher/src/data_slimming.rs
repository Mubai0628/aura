//! The execution store keeps what older versions wrote and nothing reads again until the
//! version's own Core clears it (`aura-core --store-slim`, `aura_persistence_v2::slimming`). The
//! launcher runs it before a newer version's first start, and at a later start while a run is still
//! owed (the marker beside the store), with a progress window, before the desktop opens the store.
//! A run that fails never keeps Aura from starting; after three runs that never ended only a newer
//! version's first start tries again.
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use aura_windows_platform::Progress;
use serde_json::Value;

const STORE: &str = "data/v2-execution/aura-v2.db";
/// The first version whose Core has `--store-slim`; an older one refuses the argument.
const SINCE: &str = "0.1.10";
const MARKER: &str = "data/v2-execution/store-slimming.json";
/// Runs that started on the marker without ending it, after which only a newer version's first
/// start tries again.
const RUNS: u64 = 3;
/// Core reports at least every second while it works; this long without a word is a run that hung.
const SILENCE: Duration = Duration::from_secs(120);

/// Whether a run is owed before `version` starts: at a version's first start whenever there is a
/// store, otherwise only while the marker says one is.
pub fn owed(data_root: &Path, version: &str, first_start: bool) -> bool {
    if crate::install_state::newer(SINCE, version) {
        return false;
    }
    if first_start {
        return data_root.join(STORE).is_file();
    }
    let Ok(bytes) = std::fs::read(data_root.join(MARKER)) else {
        return false;
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|marker| marker["runs"].as_u64())
        .is_some_and(|runs| runs < RUNS)
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Core's report of the run, `null` when there was no store.
    Report(Value),
    Failed(String),
}

/// Applies one line of Core's answer to `progress`; the outcome when the line ends the run.
fn read_line(line: &str, progress: &Progress) -> Option<Outcome> {
    let value: Value = serde_json::from_str(line).ok()?;
    match value["step"].as_str() {
        Some("deleting") => {
            progress.set_stage("删除旧记录", false);
            progress.set_total(value["total"].as_u64().unwrap_or(0));
            progress.set_done(value["done"].as_u64().unwrap_or(0));
            None
        }
        Some("rebuilding") => {
            progress.set_stage("整理数据库文件", true);
            None
        }
        _ => match (value.get("report"), value["error"].as_str()) {
            (Some(report), _) => Some(Outcome::Report(report.clone())),
            (None, Some(error)) => Some(Outcome::Failed(error.to_owned())),
            (None, None) => None,
        },
    }
}

/// Runs the Core at `core` on the store in `data_root`, reporting to `progress` as it goes.
pub fn run(core: &Path, data_root: &Path, progress: &Progress) -> Outcome {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let child = Command::new(core)
        .arg("--store-slim")
        .arg(data_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => return Outcome::Failed(format!("Core did not start: {error}")),
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Outcome::Failed("Core's output is not readable".to_owned());
    };
    let (lines, heard) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let mut outcome = None;
    loop {
        match heard.recv_timeout(SILENCE) {
            Ok(line) => {
                if let Some(ended) = read_line(&line, progress) {
                    outcome = Some(ended);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                return Outcome::Failed(format!("Core said nothing for {} s", SILENCE.as_secs()));
            }
        }
    }
    let status = child.wait();
    match (outcome, status) {
        (Some(outcome), _) => outcome,
        (None, Ok(status)) => Outcome::Failed(format!("Core ended without a report: {status}")),
        (None, Err(error)) => Outcome::Failed(format!("Core's end is unknown: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_owed_at_a_first_start_or_while_the_marker_says_so_and_not_after_three() {
        let root = tempfile::tempdir().expect("data");
        let owed = |first_start| owed(root.path(), "0.1.10", first_start);
        assert!(!owed(true), "no store yet");
        std::fs::create_dir_all(root.path().join("data/v2-execution")).expect("folder");
        std::fs::write(root.path().join(STORE), b"").expect("store");
        assert!(owed(true));
        assert!(!owed(false));
        let marker = |runs: u32| {
            std::fs::write(
                root.path().join(MARKER),
                format!("{{\"schema_version\":1,\"stage\":\"rebuild\",\"runs\":{runs}}}\n"),
            )
            .expect("marker");
        };
        marker(0);
        assert!(owed(false));
        marker(2);
        assert!(owed(false));
        marker(3);
        assert!(!owed(false));
        assert!(owed(true), "a newer version's first start tries again");
        // A version before slimming refuses the argument, so it is never asked.
        assert!(!super::owed(root.path(), "0.1.9", true));
        assert!(super::owed(root.path(), "0.1.11", true));
    }

    #[test]
    fn cores_lines_move_the_window_and_the_last_one_ends_the_run() {
        let progress = Progress::default();
        assert_eq!(read_line(r#"{"step":"deleting","done":50,"total":200}"#, &progress), None);
        assert_eq!(read_line(r#"{"step":"rebuilding","kept":1000}"#, &progress), None);
        assert_eq!(read_line("not json", &progress), None);
        assert_eq!(
            read_line(r#"{"report":{"rebuild":{"state":"done"}}}"#, &progress),
            Some(Outcome::Report(serde_json::json!({ "rebuild": { "state": "done" } })))
        );
        assert_eq!(
            read_line(r#"{"error":"database is locked"}"#, &progress),
            Some(Outcome::Failed("database is locked".to_owned()))
        );
        assert_eq!(read_line(r#"{"report":null}"#, &progress), Some(Outcome::Report(Value::Null)));
    }
}
