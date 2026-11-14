//! The relay against a real named pipe served by this test process.
use aura_chrome_domain::protocol::{CHROME_PROTOCOL_VERSION, HostHello, pipe_name, plain_path};
use aura_chrome_host::{
    frame::{read_frame, write_frame},
    relay::{RelayTarget, relay, target_beside},
};
use std::sync::atomic::{AtomicU32, Ordering};

const ORIGIN: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop/";

fn pipe() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        r"\\.\pipe\aura-chrome-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// This test program, named as an installed relay names the Core beside it.
fn canonical_self() -> std::path::PathBuf {
    let canonical =
        std::fs::canonicalize(std::env::current_exe().expect("this test is the server"))
            .expect("canonical");
    plain_path(&canonical)
}

#[test]
fn an_installed_relay_names_the_core_beside_it_as_windows_reports_it() {
    let canonical =
        std::fs::canonicalize(std::env::current_exe().expect("exe")).expect("canonical");
    let target = target_beside(&canonical);
    let folder = canonical_self().parent().expect("folder").to_path_buf();
    assert_eq!(target.expected_server, folder.join("aura-core.exe"));
    assert!(!target.expected_server.to_string_lossy().starts_with(r"\\?\"));
    assert_eq!(target.pipe_name, pipe_name(&folder));
}

#[tokio::test]
async fn a_relay_names_its_caller_then_passes_frames_both_ways_until_chrome_closes() {
    let name = pipe();
    let mut server =
        aura_windows_platform::create_secure_byte_pipe_server(&name, true).expect("pipe server");
    let target = RelayTarget { pipe_name: name, expected_server: canonical_self() };
    let (mut chrome_writes, relay_reads) = tokio::io::duplex(1 << 16);
    let (relay_writes, mut chrome_reads) = tokio::io::duplex(1 << 16);
    let running =
        tokio::spawn(async move { relay(ORIGIN, &target, relay_reads, relay_writes).await });
    server.connect().await.expect("relay connects");
    let hello = read_frame(&mut server, 1 << 16).await.expect("frame").expect("hello");
    let hello: HostHello = serde_json::from_slice(&hello).expect("host hello");
    assert_eq!(hello, HostHello { origin: ORIGIN.to_owned(), protocol: CHROME_PROTOCOL_VERSION });
    write_frame(&mut chrome_writes, br#"{"type":"pair","code":"12345678"}"#, 1 << 16)
        .await
        .expect("up");
    let up = read_frame(&mut server, 1 << 16).await.expect("frame").expect("upward");
    assert_eq!(up, br#"{"type":"pair","code":"12345678"}"#.to_vec());
    write_frame(&mut server, br#"{"type":"pairing_required"}"#, 1 << 16).await.expect("down");
    let down = read_frame(&mut chrome_reads, 1 << 16).await.expect("frame").expect("downward");
    assert_eq!(down, br#"{"type":"pairing_required"}"#.to_vec());
    drop(chrome_writes);
    running.await.expect("join").expect("a closed Chrome side ends the relay cleanly");
}

#[tokio::test]
async fn a_relay_refuses_a_pipe_served_by_anything_but_the_installed_core() {
    let name = pipe();
    let mut server =
        aura_windows_platform::create_secure_byte_pipe_server(&name, true).expect("pipe server");
    let target = RelayTarget {
        pipe_name: name,
        expected_server: r"C:\Program Files\Elsewhere\aura-core.exe".into(),
    };
    let (_chrome_writes, relay_reads) = tokio::io::duplex(1 << 10);
    let (relay_writes, _chrome_reads) = tokio::io::duplex(1 << 10);
    let running =
        tokio::spawn(async move { relay(ORIGIN, &target, relay_reads, relay_writes).await });
    server.connect().await.expect("relay connects");
    let error = tokio::time::timeout(std::time::Duration::from_secs(10), running)
        .await
        .expect("the relay stops at once instead of serving an unknown server")
        .expect("join")
        .expect_err("refused");
    assert!(format!("{error:#}").contains("chrome_host.server_not_aura_core"), "{error:#}");
    assert_eq!(read_frame(&mut server, 1 << 10).await.ok().flatten(), None, "it said nothing");
}
