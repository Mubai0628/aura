//! The child session's pipe with the real host program as its supervisor, run in this session for
//! the test, as `aura-desktop-host.exe --child <pipe> <token>`: Core lets it in only when it is the
//! host program, in the session expected, saying the opening's token; a link over it then starts
//! the host, which answers, and starts it again after it was stopped. Closing ends the supervisor.
//! The host is built by `cargo build -p aura-desktop-control --bin aura-desktop-host` into the
//! same target folder as this test. Listing the monitors touches no window, pointer or key.
use aura_desktop_domain::{HostCommand, HostOutcome, HostResult};
use aura_desktop_service::child::{ChildHost, ChildOpening, Expected, listen};
use aura_desktop_service::{HOST_PROGRAM, HostLink, LinkError};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn host_program() -> PathBuf {
    let test = std::env::current_exe().expect("test program");
    let folder = test.parent().and_then(std::path::Path::parent).expect("target folder");
    let program = folder.join(HOST_PROGRAM);
    assert!(
        program.is_file(),
        "{} is missing: build it first with cargo build -p aura-desktop-control --bin aura-desktop-host",
        program.display()
    );
    program
}

fn this_session() -> u32 {
    aura_windows_platform::snapshot_process(std::process::id())
        .expect("this process")
        .windows_session_id
}

fn expected(session: Option<u32>) -> Expected {
    Expected { program: host_program(), session: Arc::new(move || session) }
}

fn supervisor(opening: &ChildOpening, token: &str) -> Child {
    Command::new(host_program())
        .args(["--child", opening.pipe(), token])
        .spawn()
        .expect("the supervisor")
}

/// Waits for the supervisor to end by itself.
fn ends_within(child: &mut Child, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        if child.try_wait().expect("wait").is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    false
}

async fn displays(link: &HostLink) -> Result<usize, LinkError> {
    match link.request(HostCommand::Displays, Duration::from_secs(5)).await? {
        HostOutcome::Done(HostResult::Displays { monitors }) => Ok(monitors.len()),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_supervisor_let_in_runs_the_host_and_starts_it_again_until_closed() {
    let opening = ChildOpening::new().expect("opening");
    let listening = listen(&opening).expect("pipe");
    let mut process = supervisor(&opening, opening.token());
    let admitted = listening
        .accept(&expected(Some(this_session())), Duration::from_secs(20))
        .await
        .expect("let in");
    let child = ChildHost::new();
    child.attach(admitted);
    assert!(child.connected());

    let link = HostLink::new(Arc::new(child.clone()));
    for round in 0..2 {
        let monitors =
            displays(&link).await.unwrap_or_else(|error| panic!("round {round}: {error:?}"));
        assert!(monitors > 0, "round {round}: at least one monitor");
        assert!(link.running().await);
        link.stop().await;
        assert!(!link.running().await);
    }
    assert_eq!(
        displays(&link).await.expect("after two stops"),
        displays(&link).await.expect("again")
    );

    child.close();
    assert!(!child.connected());
    assert!(
        ends_within(&mut process, Duration::from_secs(10)),
        "the supervisor ends when Core closes"
    );
    match link.request(HostCommand::Displays, Duration::from_secs(5)).await {
        Err(LinkError::Ended | LinkError::NotStarted(_)) => {}
        other => panic!("after closing: {other:?}"),
    }
    match link.request(HostCommand::Displays, Duration::from_secs(5)).await {
        Err(LinkError::NotStarted(why)) => assert!(why.contains("小窗没有连上子会话"), "{why}"),
        other => panic!("no supervisor: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_supervisor_with_another_token_is_turned_away() {
    let opening = ChildOpening::new().expect("opening");
    let other = ChildOpening::new().expect("other");
    let listening = listen(&opening).expect("pipe");
    let mut process = supervisor(&opening, other.token());
    let refused = listening
        .accept(&expected(Some(this_session())), Duration::from_secs(3))
        .await
        .err()
        .expect("turned away");
    assert!(refused.contains("口令"), "{refused}");
    assert!(ends_within(&mut process, Duration::from_secs(10)), "a turned-away supervisor ends");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_supervisor_outside_the_child_session_is_turned_away() {
    let opening = ChildOpening::new().expect("opening");
    let listening = listen(&opening).expect("pipe");
    let mut process = supervisor(&opening, opening.token());
    let refused = listening
        .accept(&expected(Some(this_session() + 1000)), Duration::from_secs(3))
        .await
        .err()
        .expect("turned away");
    assert!(refused.contains("不在子会话里"), "{refused}");
    assert!(ends_within(&mut process, Duration::from_secs(10)), "a turned-away supervisor ends");

    let listening = listen(&opening).expect("pipe again");
    let mut process = supervisor(&opening, opening.token());
    let refused = listening
        .accept(&expected(None), Duration::from_secs(3))
        .await
        .err()
        .expect("no child session");
    assert!(refused.contains("不在子会话里"), "{refused}");
    assert!(ends_within(&mut process, Duration::from_secs(10)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_is_not_the_host_program_is_turned_away_even_with_the_token() {
    let opening = ChildOpening::new().expect("opening");
    let listening = listen(&opening).expect("pipe");
    let token = opening.token().to_owned();
    let name = opening.pipe().to_owned();
    let impostor = tokio::spawn(async move {
        let mut client =
            aura_windows_platform::open_secure_byte_pipe_client(&name).expect("connect");
        let hello = format!("S{{\"says\":\"hello\",\"token\":\"{token}\",\"protocol\":1}}\n");
        let _ = client.write_all(hello.as_bytes()).await;
        let mut byte = [0_u8; 1];
        // Turned away before anything is read: the pipe closes on it.
        client.read(&mut byte).await.unwrap_or(0)
    });
    let refused = listening
        .accept(&expected(Some(this_session())), Duration::from_secs(3))
        .await
        .err()
        .expect("turned away");
    assert!(refused.contains("不是 Aura 的操控主机"), "{refused}");
    assert_eq!(impostor.await.expect("impostor"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pipe_nobody_waits_on_ends_the_supervisor_at_once() {
    let opening = ChildOpening::new().expect("opening");
    let mut process = supervisor(&opening, opening.token());
    assert!(ends_within(&mut process, Duration::from_secs(10)));
    assert_eq!(process.wait().expect("status").code(), Some(0));
}

#[test]
fn the_command_line_runs_the_host_program_as_this_opening_s_supervisor() {
    let opening = ChildOpening::new().expect("opening");
    let line = opening.command_line(&PathBuf::from(r"C:\Program Files\Aura\aura-desktop-host.exe"));
    assert_eq!(
        line,
        format!(
            r#""C:\Program Files\Aura\aura-desktop-host.exe" --child {} {}"#,
            opening.pipe(),
            opening.token()
        )
    );
    assert!(aura_desktop_domain::child::valid_child_pipe(opening.pipe()));
    assert!(aura_desktop_domain::child::valid_child_token(opening.token()));
    assert_ne!(opening, ChildOpening::new().expect("another"));
}
