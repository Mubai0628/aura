//! The relay program as Chrome runs it. Chrome keeps the program's input open for as long as the
//! program runs, so the program has to end by itself once Core's side is gone; otherwise it keeps
//! Chrome's connection, and the files of its version, for good.
#![cfg(feature = "test-server")]
use aura_chrome_domain::protocol::plain_path;
use aura_chrome_host::frame::read_frame;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

#[tokio::test]
async fn the_program_ends_when_core_closes_although_chrome_keeps_its_input_open() {
    let name = format!(r"\\.\pipe\aura-chrome-end-test-{}", std::process::id());
    let server_image =
        plain_path(&std::fs::canonicalize(std::env::current_exe().expect("test")).expect("path"));
    let mut server =
        aura_windows_platform::create_secure_byte_pipe_server(&name, true).expect("pipe server");
    let mut program = Command::new(env!("CARGO_BIN_EXE_aura-chrome-host"))
        .arg(ORIGIN)
        .env("AURA_CHROME_HOST_TEST_PIPE", &name)
        .env("AURA_CHROME_HOST_TEST_SERVER", &server_image)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the relay program starts");
    // Chrome's side stays open: its input handle is held here until the end of the test.
    let _chrome_input = program.stdin.take().expect("input");
    server.connect().await.expect("the relay connects");
    read_frame(&mut server, 1 << 16).await.expect("frame").expect("hello");
    // Core goes away, as when Aura quits, restarts or switches to another version.
    drop(server);
    let started = Instant::now();
    let ended = loop {
        if let Some(status) = program.try_wait().expect("status") {
            break Some(status);
        }
        if started.elapsed() > Duration::from_secs(10) {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    if ended.is_none() {
        let _ = program.kill();
    }
    assert!(ended.is_some(), "the relay program still runs after its Core closed");
}
