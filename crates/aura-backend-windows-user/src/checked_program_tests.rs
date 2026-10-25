#![allow(clippy::unwrap_used)]
//! A program from the user's machine is checked in full the first time it is used; later launches
//! compare its identity, size and write time, and a difference or a failed launch checks it again.
use super::checked_program::{self, Links};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

fn program(directory: &Path, bytes: &[u8]) -> PathBuf {
    let path = directory.join("program.exe");
    fs::write(&path, bytes).unwrap();
    path
}

fn expect_err<T>(result: anyhow::Result<T>) -> anyhow::Error {
    match result {
        Ok(_) => panic!("expected error"),
        Err(error) => error,
    }
}

/// The records this process logged about one program, as (event, result, failure).
fn logged(path: &str) -> Vec<(String, String, String)> {
    checked_program::logged_for_tests()
        .into_iter()
        .filter(|(_, fields)| fields["program"] == path)
        .map(|(event, fields)| {
            (
                event,
                fields["result"].as_str().unwrap_or_default().to_owned(),
                fields["failure"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_program_is_read_in_full_once_while_it_stays_as_it_was_checked() {
    let temp = tempfile::tempdir().unwrap();
    let path = program(temp.path(), b"program-bytes");
    let (binding, held) = checked_program::capture(&path, Links::Allowed).unwrap();
    assert!(binding.sha256.is_some());
    drop(held);
    assert_eq!(checked_program::full_checks(&binding), 1);
    for _ in 0..3 {
        drop(checked_program::hold(&binding, Links::Allowed).unwrap());
    }
    // Asking for the same file by its path again does not read it either.
    let (again, held) = checked_program::capture(&path, Links::Allowed).unwrap();
    drop(held);
    assert_eq!(again, binding);
    assert_eq!(checked_program::full_checks(&binding), 1);
    assert!(checked_program::is_unchanged(&binding));
    assert!(logged(&binding.path).is_empty(), "{:?}", logged(&binding.path));
}

#[test]
fn a_held_program_cannot_be_written_while_it_starts() {
    let temp = tempfile::tempdir().unwrap();
    let path = program(temp.path(), b"program-bytes");
    let (binding, held) = checked_program::capture(&path, Links::Allowed).unwrap();
    drop(held);
    let held = checked_program::hold(&binding, Links::Allowed).unwrap();
    let refused = fs::OpenOptions::new().write(true).open(&path);
    assert_eq!(refused.unwrap_err().raw_os_error(), Some(32), "sharing violation");
    drop(held);
}

#[test]
fn a_changed_program_is_checked_again_refused_and_logged() {
    let temp = tempfile::tempdir().unwrap();
    let path = program(temp.path(), b"program-bytes");
    let (binding, held) = checked_program::capture(&path, Links::Allowed).unwrap();
    drop(held);
    fs::write(&path, b"program-bytez").unwrap();
    // A rewrite in the same clock tick keeps the old write time (the limit the quick check has):
    // the rewrite here is dated later, as one at any later moment is.
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    assert!(!checked_program::is_unchanged(&binding));
    let error = expect_err(checked_program::hold(&binding, Links::Allowed));
    assert!(error.to_string().contains("runner.file_binding_changed"), "{error}");
    assert_eq!(checked_program::full_checks(&binding), 2);
    assert_eq!(
        logged(&binding.path),
        [("program.checked_again".into(), "changed".into(), String::new())]
    );
    // Refused until it is captured again: a later launch checks it in full once more.
    expect_err(checked_program::hold(&binding, Links::Allowed));
    assert_eq!(checked_program::full_checks(&binding), 3);
}

#[test]
fn a_program_whose_write_time_alone_changed_is_checked_again_and_kept() {
    let temp = tempfile::tempdir().unwrap();
    let path = program(temp.path(), b"program-bytes");
    let (binding, held) = checked_program::capture(&path, Links::Allowed).unwrap();
    drop(held);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    drop(checked_program::hold(&binding, Links::Allowed).unwrap());
    assert_eq!(checked_program::full_checks(&binding), 2);
    drop(checked_program::hold(&binding, Links::Allowed).unwrap());
    assert_eq!(checked_program::full_checks(&binding), 2, "recorded as it is now");
    assert_eq!(
        logged(&binding.path),
        [("program.checked_again".into(), "unchanged".into(), String::new())]
    );
}

#[test]
fn a_failed_launch_checks_the_program_again_and_logs_what_it_found() {
    let temp = tempfile::tempdir().unwrap();
    let path = program(temp.path(), b"program-bytes");
    let (binding, held) = checked_program::capture(&path, Links::Allowed).unwrap();
    drop(held);
    checked_program::check_after_failure(&binding, Links::Allowed, "launch_failed");
    assert_eq!(checked_program::full_checks(&binding), 2);
    drop(checked_program::hold(&binding, Links::Allowed).unwrap());
    assert_eq!(checked_program::full_checks(&binding), 2, "still as checked");
    fs::remove_file(&path).unwrap();
    checked_program::check_after_failure(&binding, Links::Allowed, "exited_abnormally");
    assert_eq!(
        logged(&binding.path),
        [
            ("program.checked_after_failure".into(), "unchanged".into(), "launch_failed".into()),
            (
                "program.checked_after_failure".into(),
                "unavailable".into(),
                "exited_abnormally".into()
            ),
        ]
    );
    assert!(!checked_program::is_unchanged(&binding));
}

#[test]
fn a_program_with_another_link_needs_links_allowed() {
    let temp = tempfile::tempdir().unwrap();
    let path = program(temp.path(), b"program-bytes");
    fs::hard_link(&path, temp.path().join("other.exe")).unwrap();
    let error = expect_err(checked_program::capture(&path, Links::Single));
    assert!(error.to_string().contains("runner.aliased_file"), "{error}");
    let (binding, held) = checked_program::capture(&path, Links::Allowed).unwrap();
    drop(held);
    let error = expect_err(checked_program::hold(&binding, Links::Single));
    assert!(error.to_string().contains("runner.aliased_file"), "{error}");
}
