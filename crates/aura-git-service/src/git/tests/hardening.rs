use super::support::Fixture;
use aura_core_base::command_settlement;
use aura_workspace_domain::*;

#[test]
fn unconfirmed_git_stop_keeps_the_existing_broker_unknown_semantics() {
    let error = super::super::envelope(
        aura_domain_common::CorrelationId::new(),
        super::super::StopUnconfirmed.into(),
    );
    assert!(command_settlement::envelope_is_unconfirmed(&error));
    assert!(!error.retryable);
}
#[test]
fn all_scope_programs_are_disabled_and_required_filters_cannot_force_execution() {
    let mut fixture = Fixture::new(
        "[filter \"Trusted\"]\nclean = MUST_NOT_RUN_GLOBAL_FILTER\nrequired = true\n[core]\nfsmonitor = MUST_NOT_RUN_GLOBAL_FSMONITOR\n[protocol]\nallow = always\n",
    );
    fixture.file("tracked", b"one\n");
    fixture.file("global-only", b"before\n");
    fixture.commit("base");
    fixture.file("tracked", b"two\n");
    fixture.file("global-only", b"after\n");
    let canary = fixture.root.path().join("repo-program-ran");
    let command = format!("echo forbidden > '{}'", canary.display().to_string().replace('\\', "/"));
    for key in [
        "filter.Trusted.clean",
        "filter.Bad.clean",
        "filter.Bad.smudge",
        "filter.Bad.process",
        "diff.Bad.command",
        "diff.Bad.textconv",
        "core.fsmonitor",
        "core.pager",
        "core.editor",
    ] {
        fixture.git(&["config", key, &command]);
    }
    fixture.git(&["config", "filter.Bad.required", "true"]);
    for key in [
        "filter.GlobalOnly.clean",
        "filter.GlobalOnly.smudge",
        "filter.GlobalOnly.process",
        "diff.GlobalOnly.textconv",
        "diff.GlobalOnly.command",
        "core.fsmonitor",
        "core.pager",
        "core.editor",
    ] {
        fixture.git(&["config", "--global", key, &command]);
    }
    fixture.git(&["config", "--global", "filter.GlobalOnly.required", "true"]);
    fixture.file(
        ".gitattributes",
        b"tracked filter=Trusted diff=Bad\nglobal-only filter=GlobalOnly diff=GlobalOnly\n",
    );
    fixture.file(".git/hooks/post-index-change", command.as_bytes());
    fixture.refresh();
    for driver in ["Trusted", "Bad", "GlobalOnly"] {
        for (key, value) in [("clean", ""), ("smudge", ""), ("process", ""), ("required", "false")]
        {
            let expected = format!("filter.{driver}.{key}={value}");
            assert!(fixture.reader.config.windows(2).any(|pair| pair == ["-c", &expected]));
        }
    }
    assert!(!fixture.reader.config.iter().any(|arg| arg.contains("MUST_NOT_RUN_GLOBAL_FSMONITOR")));
    let index = std::fs::read(fixture.root.path().join(".git/index")).expect("index");
    let status = fixture.reader.status(&GitStatusInput::default()).expect("hardened status");
    assert!(
        status
            .entries
            .iter()
            .filter(|entry| ["tracked", "global-only"].contains(&entry.path.as_str()))
            .all(|entry| entry.comparison_note.as_deref()
                == Some(super::super::attributes::RAW_COMPARISON))
    );
    fixture
        .reader
        .diff(&serde_json::from_value(serde_json::json!({})).expect("diff"))
        .expect("hardened diff");
    fixture
        .reader
        .log(&serde_json::from_value(serde_json::json!({})).expect("log"))
        .expect("hardened log");
    assert_eq!(std::fs::read(fixture.root.path().join(".git/index")).expect("index"), index);
    assert!(!canary.exists());
    fixture.file(".gitattributes", b"tracked filter=Bad diff=Bad\n");
    let downgraded = fixture
        .reader
        .diff(&serde_json::from_value(serde_json::json!({})).expect("diff input"))
        .expect("untrusted driver diff");
    assert!(!downgraded.warnings.is_empty());
    assert!(downgraded.patch.contains("two"));
    assert!(!canary.exists());
}

#[test]
fn core_default_native_git_entry_has_exactly_the_test_empty_credential_helper() {
    let root = tempfile::Builder::new()
        .prefix("aura-git-test-core-")
        .tempdir()
        .expect("temporary working directory");
    let runner = aura_backend_windows_user::git::GitRunner::detect()
        .expect("test-gated default detection")
        .runner
        .expect("test Git >=2.40");
    let result = runner
        .run(
            root.path(),
            &["config".into(), "--get-all".into(), "credential.helper".into()],
            &std::sync::atomic::AtomicBool::new(false),
            std::time::Duration::from_secs(5),
            65536,
        )
        .expect("actual Core Git process");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, b"\n");
    assert!(result.stopped);
}

#[test]
fn physical_repository_facts_are_canonical_without_losing_volume_identity_bits() {
    let fixture = Fixture::new("");
    let facts = fixture.reader.facts().expect("physical facts");
    assert!(facts["repository"]["volume"].is_string());
    aura_domain_common::jcs::canonical_sha256(&facts)
        .expect("execution plans use I-JSON safe facts");
}

#[test]
fn native_config_include_errors_are_reported_and_url_userinfo_redacted() {
    let fixture = Fixture::new("");
    let outside = fixture.root.path().join("broken-outside.cfg");
    std::fs::write(&outside, "[broken\n").expect("broken include");
    fixture.git(&["config", "include.path", outside.to_str().expect("path")]);
    let failure = fixture
        .reader
        .status(&GitStatusInput::default())
        .expect_err("native Git must report malformed include");
    assert!(failure.to_string().contains("git.config_invalid"));
    let redacted =
        super::super::config::redact("fatal https://user:temporary-value@example.invalid/path");
    assert!(!redacted.contains("temporary-value") && !redacted.contains("user:"));
}
