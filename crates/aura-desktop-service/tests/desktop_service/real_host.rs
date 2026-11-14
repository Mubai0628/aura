//! The real host program, started the way Core starts it: it says it is ready, answers, and ends
//! with its Job when Core stops it. It is built by `cargo build -p aura-desktop-control --bin
//! aura-desktop-host` into the same target folder as this test. Listing the monitors touches no
//! window, pointer or key.
use aura_desktop_domain::{HostCommand, HostOutcome, HostResult};
use aura_desktop_service::{HOST_PROGRAM, HostLink, ProgramHost};
use std::sync::Arc;
use std::time::Duration;

fn host_program() -> std::path::PathBuf {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_real_host_starts_answers_and_starts_again_after_it_was_stopped() {
    let temp = tempfile::tempdir().expect("TEMP");
    let launcher = ProgramHost { program: host_program(), folder: temp.path().to_path_buf() };
    let link = HostLink::new(Arc::new(launcher));
    for round in 0..2 {
        let outcome = link.request(HostCommand::Displays, Duration::from_secs(5)).await;
        let Ok(HostOutcome::Done(HostResult::Displays { monitors })) = outcome else {
            panic!("round {round}: {outcome:?}");
        };
        assert!(!monitors.is_empty(), "round {round}: at least one monitor");
        assert!(link.running().await);
        link.stop().await;
        assert!(!link.running().await);
    }
}
