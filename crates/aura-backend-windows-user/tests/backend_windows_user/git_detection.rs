#![cfg(feature = "git-test-profile")]
use aura_backend_windows_user::git::GitRunner;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
fn real_native_fixture_image_obeys_minimum_version_gate_and_rebinds_after_upgrade() {
    let root =
        tempfile::Builder::new().prefix("aura-git-test-version-").tempdir().expect("fixture");
    let image = root.path().join("git.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_aura-git-version-fixture"), &image)
        .expect("temporary native executable");
    let global = root.path().join("global.cfg");
    let system = root.path().join("system.cfg");
    std::fs::write(&global, "[credential]\nhelper =\n").expect("global");
    std::fs::write(&system, "").expect("system");
    let profile = root.path().join("profile.json");
    std::fs::write(
        &profile,
        serde_json::to_vec(&serde_json::json!({"global_config":global,"system_config":system}))
            .expect("json"),
    )
    .expect("profile");
    let previous = GitRunner::isolated(&image, &profile).expect("initial bound image");
    for (version, expected) in [
        ("git version 2.39.0.windows.1\n", "too_old"),
        ("git version 2.40.1.windows.1\n", "available"),
        ("unrecognized image\n", "invalid"),
    ] {
        std::fs::write(root.path().join("version.txt"), version).expect("test version");
        let detected = GitRunner::isolated(&image, &profile)
            .expect("isolated image")
            .inspect_version()
            .expect("real version process");
        assert_eq!(detected.state, expected);
        assert_eq!(detected.runner.is_some(), expected == "available");
    }
    std::fs::remove_file(&image).expect("remove our temporary fixture image");
    std::fs::copy(env!("CARGO_BIN_EXE_aura-git-version-fixture"), &image)
        .expect("replace our image");
    let stale = previous.run(
        root.path(),
        &["--version".into()],
        &std::sync::atomic::AtomicBool::new(false),
        std::time::Duration::from_secs(5),
        65536,
    );
    assert!(stale.is_err(), "a replaced image is never launched using its old binding");
    let _rebound =
        GitRunner::isolated(&image, &profile).expect("replacement rebound without a restart");
}

/// While Git's program is as it was,
/// detection reads neither its bytes nor its `--version` again; a replaced program is detected
/// again. The fixture answers `--version` from version.txt, so a second answer shows a second run.
#[test]
fn detection_asks_an_unchanged_git_once_and_a_replaced_one_again() {
    let root = tempfile::Builder::new().prefix("aura-git-test-detect-").tempdir().expect("fixture");
    let image = root.path().join("git.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_aura-git-version-fixture"), &image).expect("image");
    let global = root.path().join("global.cfg");
    let system = root.path().join("system.cfg");
    std::fs::write(&global, "[credential]\nhelper =\n").expect("global");
    std::fs::write(&system, "").expect("system");
    let profile = root.path().join("profile.json");
    std::fs::write(
        &profile,
        serde_json::to_vec(&serde_json::json!({
            "global_config":global,"system_config":system,"git_path":image
        }))
        .expect("JSON"),
    )
    .expect("profile");
    let version = root.path().join("version.txt");
    std::fs::write(&version, "git version 2.40.1.windows.1\n").expect("version");
    let first = GitRunner::detect_with_profile(&profile).expect("first detection");
    assert_eq!(first.state, "available");
    assert_eq!(first.version.as_deref(), Some("git version 2.40.1.windows.1"));
    std::fs::write(&version, "git version 2.41.0.windows.1\n").expect("version");
    let again = GitRunner::detect_with_profile(&profile).expect("same program");
    assert_eq!(again.version, first.version, "an unchanged program is not asked again");
    assert!(again.runner.is_some());
    std::fs::remove_file(&image).expect("remove our temporary image");
    std::fs::copy(env!("CARGO_BIN_EXE_aura-git-version-fixture"), &image).expect("replace it");
    let replaced = GitRunner::detect_with_profile(&profile).expect("replaced program");
    assert_eq!(replaced.version.as_deref(), Some("git version 2.41.0.windows.1"));
}

fn fixture() -> (tempfile::TempDir, GitRunner) {
    let root =
        tempfile::Builder::new().prefix("aura-git-test-read-process-").tempdir().expect("fixture");
    let image = root.path().join("git.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_aura-git-version-fixture"), &image).expect("image");
    let global = root.path().join("global.cfg");
    let system = root.path().join("system.cfg");
    std::fs::write(&global, "[credential]\nhelper =\n").expect("global");
    std::fs::write(&system, "").expect("system");
    let profile = root.path().join("profile.json");
    std::fs::write(
        &profile,
        serde_json::to_vec(&serde_json::json!({
            "global_config":global,"system_config":system
        }))
        .expect("JSON"),
    )
    .expect("profile");
    let runner = GitRunner::isolated(&image, &profile).expect("read runner");
    (root, runner)
}

#[test]
fn single_process_read_timeout_and_cancel_are_confirmed_without_starting_children() {
    let (root, runner) = fixture();
    for cancel in [false, true] {
        let signal = Arc::new(AtomicBool::new(false));
        let trigger = cancel.then(|| {
            let signal = signal.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(250));
                signal.store(true, Ordering::Release);
            })
        });
        let began = Instant::now();
        let result = runner
            .run(
                root.path(),
                &["fixture-wait".into()],
                &signal,
                if cancel { Duration::from_secs(5) } else { Duration::from_millis(300) },
                65536,
            )
            .expect("read");
        if let Some(trigger) = trigger {
            trigger.join().expect("trigger");
        }
        assert_eq!(result.reason, if cancel { "cancelled" } else { "timed_out" });
        assert!(result.stopped && result.exit_code.is_some(), "{result:?}");
        assert_eq!(result.stdout, b"ready\n");
        assert!(began.elapsed() < Duration::from_secs(5));
    }
}

#[test]
fn single_process_read_has_a_combined_output_limit_and_empty_trace_environment() {
    let (root, runner) = fixture();
    let result = runner
        .run(
            root.path(),
            &["fixture-flood".into()],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            1024,
        )
        .expect("read quota");
    assert_eq!(result.reason, "output_limit");
    assert!(result.stopped && result.exit_code.is_some(), "{result:?}");
    assert_eq!(result.stdout.len() + result.stderr.len(), 1024);
    let result = runner
        .run(
            root.path(),
            &["fixture-trace-env".into()],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            1024,
        )
        .expect("trace environment");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, b"GIT_TRACE2=\nGIT_TRACE2_EVENT=\nGIT_TRACE2_PERF=\n");
    let result = runner
        .run(
            root.path(),
            &["fixture-spawn-child".into()],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            65536,
        )
        .expect("read child limit");
    assert!(result.stopped && result.exit_code == Some(0), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stdout).starts_with("child-denied:"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("child-started"));
}

#[test]
fn read_token_denies_a_direct_existing_medium_file_write_without_any_git_lock() {
    let (root, runner) = fixture();
    let file = root.path().join("medium-worktree-file.txt");
    std::fs::write(&file, b"before\n").expect("own temporary file");
    let args = ["fixture-write-file".into(), file.to_string_lossy().into_owned()];
    let native = runner
        .run_native(root.path(), &args, &AtomicBool::new(false), Duration::from_secs(5), 65536)
        .expect("positive control in the user's normal token");
    assert!(native.stopped && native.exit_code == Some(0), "{native:?}");
    assert_eq!(std::fs::read(&file).expect("positive control bytes"), b"forbidden\n");
    std::fs::write(&file, b"before\n").expect("reset only our temporary fixture");
    let isolated = runner
        .run(root.path(), &args, &AtomicBool::new(false), Duration::from_secs(5), 65536)
        .expect("the same executable under Low integrity");
    assert!(isolated.stopped && isolated.exit_code.is_some_and(|code| code != 0), "{isolated:?}");
    assert_eq!(std::fs::read(&file).expect("unchanged Medium bytes"), b"before\n");
}
