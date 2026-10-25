//! A long-lived program answers frames sent at any time, and its whole tree ends when
//! the session stops or the program exits.
use super::{super::*, support::*};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn interactive(root: &std::path::Path) -> (ProgramSession, mpsc::Receiver<SessionEvent>) {
    let image = command_image();
    let environment = environment(root, false);
    // An interactive CMD without echo or prompt: each line written is one command.
    let arguments = vec!["/d".into(), "/q".into(), "/k".into(), "prompt $s".into()];
    let (events, received) = mpsc::channel();
    let session = ProgramSession::start(
        SessionSpec {
            image: &image,
            cmd_script: None,
            arguments: &arguments,
            working_directory: root,
            environment: environment.entries(),
            other_links_allowed: false,
            limits: None,
        },
        events,
    )
    .expect("session in a creation-time Job");
    (session, received)
}

/// Collects stdout until `wanted` appears or the session ends.
fn read_until(
    received: &mpsc::Receiver<SessionEvent>,
    wanted: &str,
) -> (String, Option<SessionEvent>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut text = String::new();
    while Instant::now() < deadline {
        match received.recv_timeout(Duration::from_millis(200)) {
            Ok(SessionEvent::Stdout(bytes)) => {
                text.push_str(&String::from_utf8_lossy(&bytes));
                if text.contains(wanted) {
                    return (text, None);
                }
            }
            Ok(SessionEvent::Stderr(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(ended @ SessionEvent::Ended { .. }) => return (text, Some(ended)),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    (text, None)
}

#[test]
fn frames_sent_at_any_time_are_answered_and_the_exit_code_is_reported() {
    let root = tempfile::tempdir().expect("temporary directory");
    let (session, received) = interactive(root.path());
    assert!(session.process_id() > 0);
    session.send(b"echo first-frame\r\n".to_vec()).expect("queued");
    let (text, ended) = read_until(&received, "first-frame");
    assert!(text.contains("first-frame") && ended.is_none(), "{text}");
    std::thread::sleep(Duration::from_millis(300));
    session.send(b"echo second-frame\r\n".to_vec()).expect("queued later");
    let (text, _) = read_until(&received, "second-frame");
    assert!(text.contains("second-frame"), "{text}");
    session.send(b"exit 7\r\n".to_vec()).expect("queued");
    let (_, ended) = read_until(&received, "never printed");
    assert_eq!(
        ended,
        Some(SessionEvent::Ended {
            exit_code: Some(7),
            stop_confirmed: true,
            memory_limit_reached: false
        })
    );
    assert!(
        session.send(b"echo late\r\n".to_vec()).is_err()
            || received.recv_timeout(Duration::from_millis(500)).is_err()
    );
}

#[test]
fn stopping_ends_the_program_and_its_children() {
    let root = tempfile::tempdir().expect("temporary directory");
    let (mut session, received) = interactive(root.path());
    // A child that would outlive its parent without the Job.
    session
        .send(b"start /b ping -n 60 127.0.0.1 >nul\r\necho child-started\r\n".to_vec())
        .expect("queued");
    let (text, _) = read_until(&received, "child-started");
    assert!(text.contains("child-started"), "{text}");
    let began = Instant::now();
    session.stop();
    assert!(began.elapsed() < Duration::from_secs(8), "a bounded stop");
    let ended = received
        .iter()
        .find(|event| matches!(event, SessionEvent::Ended { .. }))
        .expect("one end event");
    assert!(matches!(ended, SessionEvent::Ended { stop_confirmed: true, .. }), "{ended:?}");
    assert!(session.send(b"echo after-stop\r\n".to_vec()).is_err());
}

#[test]
fn oversized_frames_are_refused_before_they_reach_the_program() {
    let root = tempfile::tempdir().expect("temporary directory");
    let (session, _received) = interactive(root.path());
    let error = session.send(vec![b'x'; MAX_SESSION_FRAME_BYTES + 1]).expect_err("too large");
    assert!(error.to_string().contains("frame_too_large"), "{error}");
}

/// Only a session that allows other links (an MCP server's) starts a
/// program file that has them; every other runner keeps refusing such a file.
#[test]
fn a_program_with_another_hard_link_starts_only_where_other_links_are_allowed() {
    let root = tempfile::tempdir().expect("temporary directory");
    let program = root.path().join("program.exe");
    let system = aura_windows_platform::windows_directory().expect("Windows directory");
    std::fs::copy(system.join("System32/cmd.exe"), &program).expect("a copy of CMD");
    std::fs::hard_link(&program, root.path().join("other.exe")).expect("a second link");
    let (image, held) = crate::file_binding::capture_linked_image(&program).expect("bound");
    drop(held);
    let environment = environment(root.path(), false);
    let arguments = vec!["/d".into(), "/q".into(), "/k".into(), "prompt $s".into()];
    let start = |other_links_allowed| {
        let (events, received) = mpsc::channel();
        ProgramSession::start(
            SessionSpec {
                image: &image,
                cmd_script: None,
                arguments: &arguments,
                working_directory: root.path(),
                environment: environment.entries(),
                other_links_allowed,
                limits: None,
            },
            events,
        )
        .map(|session| (session, received))
    };
    let Err(refused) = start(false) else { panic!("another link is refused by default") };
    assert!(refused.to_string().contains("runner.aliased_file"), "{refused}");
    let (mut session, received) = start(true).expect("allowed for this session");
    session.send(b"echo linked-program\r\n".to_vec()).expect("queued");
    let (text, ended) = read_until(&received, "linked-program");
    assert!(text.contains("linked-program") && ended.is_none(), "{text}");
    session.stop();
}
