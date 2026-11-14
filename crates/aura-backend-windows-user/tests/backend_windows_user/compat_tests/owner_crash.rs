use super::support::*;
use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn crashing_only_the_owner_closes_its_job_and_recovery_does_not_replay_it() {
    let root = tempfile::tempdir().expect("owned crash workspace");
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64");
    let mut owner = Command::new(fixture())
        .arg("owner")
        .arg(root.path())
        .arg(runtime.join("payload"))
        .arg(runtime.join("manifest.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000)
        .spawn()
        .expect("owned hidden fixture owner");
    let end = Instant::now() + Duration::from_secs(20);
    while !root.path().join("owner.ready").exists() {
        if let Some(status) = owner.try_wait().expect("owner status") {
            panic!("fixture owner exited {status}");
        }
        if Instant::now() >= end {
            let _ = owner.kill();
            let _ = owner.wait();
            panic!("owner did not publish receipt");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.path().join("owner.ready")).expect("owned crash receipt"),
    )
    .expect("parse receipt");
    let pid = u32::try_from(receipt["grandchild"]["pid"].as_u64().expect("grandchild PID"))
        .expect("32-bit PID");
    let retained = aura_windows_platform::retain_process(pid)
        .expect("retain actual grandchild before owner crash");
    assert_eq!(
        retained.identity().creation_time_100ns,
        receipt["grandchild"]["created"].as_u64().expect("creation identity")
    );
    assert!(retained.is_running().expect("live background fact"));
    owner.kill().expect("crash only owned fixture owner");
    owner.wait().expect("owner stopped");
    let end = Instant::now() + Duration::from_secs(5);
    while retained.is_running().expect("retained creation identity stop fact") {
        assert!(Instant::now() < end, "last-handle close did not stop grandchild");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        aura_windows_platform::RootJob::open_for_recovery(
            receipt["installation"].as_str().expect("installation"),
            receipt["nonce"].as_str().expect("nonce")
        )
        .expect("owner-verified absence")
        .is_none()
    );
    assert_eq!(std::fs::read_to_string(root.path().join("starts")).expect("no replay"), "1");
}
