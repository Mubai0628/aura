use super::*;
use std::sync::atomic::AtomicBool;

#[test]
fn actual_git_uses_isolation_after_reading_the_windows_user_environment_block() {
    let root = tempfile::Builder::new().prefix("aura-git-test-").tempdir().expect("fixture");
    let global = root.path().join("global.cfg");
    let system = root.path().join("system.cfg");
    std::fs::write(&global, "[credential]\nhelper =\n[user]\nname = IsolatedFixture\n")
        .expect("global");
    std::fs::write(&system, "[credential]\nhelper = MUST_NOT_USE_SYSTEM_HELPER\n")
        .expect("poisoned system");
    let profile = root.path().join("profile.json");
    std::fs::write(
        &profile,
        serde_json::to_vec(&serde_json::json!({"global_config":global,"system_config":system}))
            .expect("json"),
    )
    .expect("profile");
    let git = std::env::var_os("AURA_GIT_TEST_BINARY")
        .unwrap_or_else(|| "C:/Program Files/Git/cmd/git.exe".into());
    let runner = GitRunner::isolated(Path::new(&git), &profile).expect("bind native Git");
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
        assert!(
            runner
                .environment
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(name))
                .expect("temporary proxy override")
                .value_utf16
                .is_empty()
        );
    }
    let invoke = |args: &[&str]| {
        runner
            .run(
                root.path(),
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                &AtomicBool::new(false),
                Duration::from_secs(5),
                65536,
            )
            .expect("real argv Job")
    };
    let helper = invoke(&["config", "--get-all", "credential.helper"]);
    assert_eq!(helper.exit_code, Some(0), "temporary config query: {helper:?}");
    assert_eq!(helper.stdout, b"\n", "exactly one empty helper, no manager or system config");
    assert!(helper.stopped && helper.reason == "exited");
    let identity = invoke(&["config", "--show-origin", "--get", "user.name"]);
    assert_eq!(identity.exit_code, Some(0));
    let text = String::from_utf8(identity.stdout).expect("config");
    assert!(text.contains("IsolatedFixture") && text.contains("global.cfg"));
}

pub(super) fn fixture() -> (tempfile::TempDir, GitRunner) {
    let root =
        tempfile::Builder::new().prefix("aura-git-test-process-").tempdir().expect("fixture");
    let global = root.path().join("global.cfg");
    let system = root.path().join("system.cfg");
    std::fs::write(&global, "[credential]\nhelper =\n").expect("config");
    std::fs::write(&system, "").expect("config");
    let profile = root.path().join("profile.json");
    std::fs::write(
        &profile,
        serde_json::to_vec(&serde_json::json!({
            "global_config":global,"system_config":system
        }))
        .expect("json"),
    )
    .expect("profile");
    let git = std::env::var_os("AURA_GIT_TEST_BINARY")
        .unwrap_or_else(|| "C:/Program Files/Git/cmd/git.exe".into());
    let runner = GitRunner::isolated(Path::new(&git), &profile).expect("isolated Git");
    (root, runner)
}

#[test]
fn isolated_reads_are_os_write_denied_even_without_read_flags() {
    let (root, runner) = fixture();
    let native = |args: &[&str]| {
        let output = std::process::Command::new(&runner.image_binding().path)
            .current_dir(root.path())
            .args(args)
            .env("GIT_CONFIG_GLOBAL", root.path().join("global.cfg"))
            .env("GIT_CONFIG_SYSTEM", root.path().join("system.cfg"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("HOME", root.path())
            .output()
            .expect("temporary setup Git");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    };
    native(&["init", "--initial-branch=main"]);
    std::fs::write(root.path().join("medium.txt"), b"before\n").expect("tracked file");
    native(&["add", "medium.txt"]);
    std::fs::write(root.path().join("medium.txt"), b"after\n").expect("changed file");
    let local = std::fs::read(root.path().join(".git/config")).expect("local config");
    let global = std::fs::read(root.path().join("global.cfg")).expect("global config");
    let index = std::fs::read(root.path().join(".git/index")).expect("index");
    // Deliberately omit --no-optional-locks and every Core read-hardening -c.
    // Denial must come from Windows, rather than from the read command selection.
    for args in [
        vec!["config", "--local", "fixture.forbidden", "value"],
        vec!["config", "--global", "fixture.forbidden", "value"],
        vec!["add", "medium.txt"],
        vec!["checkout", "--", "medium.txt"],
    ] {
        let result = runner
            .run(
                root.path(),
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                &AtomicBool::new(false),
                Duration::from_secs(5),
                65536,
            )
            .expect("isolated native process");
        assert!(
            result.stopped && result.exit_code.is_some_and(|code| code != 0),
            "write must be OS-denied: {result:?}"
        );
    }
    assert_eq!(std::fs::read(root.path().join(".git/config")).expect("local unchanged"), local);
    assert_eq!(std::fs::read(root.path().join("global.cfg")).expect("global unchanged"), global);
    assert_eq!(std::fs::read(root.path().join(".git/index")).expect("index unchanged"), index);
    assert_eq!(
        std::fs::read(root.path().join("medium.txt")).expect("worktree unchanged"),
        b"after\n"
    );
    let result = runner
        .run(
            root.path(),
            &[
                "-c".into(),
                "alias.forbidden=!echo forbidden > child-canary".into(),
                "forbidden".into(),
            ],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            65536,
        )
        .expect("single-process Job");
    assert!(result.exit_code.is_some_and(|code| code != 0));
    assert!(!root.path().join("child-canary").exists());
}

#[test]
fn argv_quotes_spaces_and_shell_metacharacters_are_literal() {
    let (root, runner) = fixture();
    let config = root.path().join("汉字 space config.cfg");
    let payload = "space ' quote \" slash\\ $HOME & echo forbidden > canary ; tail\\";
    let write = runner
        .run_native(
            root.path(),
            &[
                "config".into(),
                "--file".into(),
                config.to_string_lossy().into_owned(),
                "fixture.value".into(),
                payload.into(),
            ],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            65536,
        )
        .expect("argv write");
    assert_eq!(write.exit_code, Some(0));
    let read = runner
        .run(
            root.path(),
            &[
                "config".into(),
                "--file".into(),
                config.to_string_lossy().into_owned(),
                "--get".into(),
                "fixture.value".into(),
            ],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            65536,
        )
        .expect("argv read");
    assert_eq!(String::from_utf8(read.stdout).expect("UTF8"), format!("{payload}\n"));
    assert!(!root.path().join("canary").exists());
    assert!(read.stopped);
}
