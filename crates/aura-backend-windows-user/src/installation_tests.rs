#![allow(clippy::unwrap_used)]
use super::installation::{
    InstallationManifest, InstallationStages, applied_system_cmd_conversion, resolve_image,
    verify_system_cmd_conversion,
};
use aura_domain_common::{ContentHash, Revision, ToolWireVersionV1, jcs::canonical_sha256};
use aura_windows_user_contract::{
    ASSURANCE_VERSION, ExecutionAccount, ExecutionFileBinding, INSTALLATION_VERSION,
    SYSTEM_CMD_CONVERSION_VERSION, SystemCmdConversion, SystemCmdConversionState,
};

fn binding(tag: &[u8]) -> ExecutionFileBinding {
    ExecutionFileBinding {
        path: format!(r"\\?\C:\{}", String::from_utf8_lossy(tag)),
        volume_serial: "1".into(),
        file_id: [tag.first().copied().unwrap_or(1); 16],
        sha256: Some(ContentHash::sha256(tag)),
    }
}

fn account() -> ExecutionAccount {
    ExecutionAccount {
        name: "offline".into(),
        sid: "S-1-5-21-1-2-3-1002".into(),
        credential_sha256: ContentHash::sha256(b"secret"),
    }
}

fn manifest(revision: u64, cmd: &[u8]) -> InstallationManifest {
    InstallationManifest {
        schema_version: ToolWireVersionV1,
        installation_version: INSTALLATION_VERSION.into(),
        assurance_version: ASSURANCE_VERSION.into(),
        revision: Revision::new(revision),
        installation_id: "4b486eb4-dd7f-4681-b8ac-7eece6ce8d25".into(),
        owner_sid: "S-1-5-21-1-2-3-1001".into(),
        resource_record_hash: ContentHash::sha256(b"record"),
        maintenance_config_sha256: ContentHash::sha256(b"maint"),
        core_image: binding(b"core"),
        runner_image: binding(b"runner"),
        cmd_image: binding(cmd),
        powershell_image: binding(b"pwsh"),
        offline: account(),
        online: None,
    }
}

fn conversion(before: &InstallationManifest, after: &InstallationManifest) -> SystemCmdConversion {
    SystemCmdConversion {
        schema: SYSTEM_CMD_CONVERSION_VERSION.into(),
        installation_id: before.installation_id.clone(),
        from_revision: before.revision,
        to_revision: after.revision,
        from_cmd_image: before.cmd_image.clone(),
        to_cmd_image: after.cmd_image.clone(),
        from_fingerprint: canonical_sha256(before).unwrap(),
        to_fingerprint: canonical_sha256(after).unwrap(),
        core_sha256: before.core_image.sha256.clone().unwrap(),
        runner_sha256: before.runner_image.sha256.clone().unwrap(),
        powershell_sha256: before.powershell_image.sha256.clone().unwrap(),
        state: SystemCmdConversionState::Pending,
    }
}

#[test]
fn legal_cmd_revision_conversion_is_accepted() {
    let before = manifest(1, b"old-cmd");
    let after = manifest(2, b"new-cmd");
    verify_system_cmd_conversion(&before, &after, &conversion(&before, &after)).unwrap();
}

#[test]
fn conversion_rejects_core_runner_or_powershell_changes() {
    let before = manifest(1, b"old-cmd");
    let mut after = manifest(2, b"new-cmd");
    after.core_image = binding(b"other-core");
    assert!(verify_system_cmd_conversion(&before, &after, &conversion(&before, &after)).is_err());
    after = manifest(2, b"new-cmd");
    after.runner_image = binding(b"other-runner");
    assert!(verify_system_cmd_conversion(&before, &after, &conversion(&before, &after)).is_err());
    after = manifest(2, b"new-cmd");
    after.powershell_image = binding(b"other-pwsh");
    assert!(verify_system_cmd_conversion(&before, &after, &conversion(&before, &after)).is_err());
}

#[test]
fn conversion_rejects_wrong_source_cmd_or_missing_step() {
    let before = manifest(1, b"old-cmd");
    let after = manifest(2, b"new-cmd");
    let mut wrong = conversion(&before, &after);
    wrong.from_cmd_image = binding(b"not-old");
    assert!(verify_system_cmd_conversion(&before, &after, &wrong).is_err());
    let skipped = manifest(3, b"new-cmd");
    assert!(
        verify_system_cmd_conversion(&before, &skipped, &conversion(&before, &skipped)).is_err()
    );
}

#[test]
fn recorded_conversion_reconstructs_the_previous_revision_and_marks_applied() {
    let before = manifest(1, b"old-cmd");
    let after = manifest(2, b"new-cmd");
    let pending = conversion(&before, &after);
    let applied = applied_system_cmd_conversion(&after, pending.clone()).unwrap();
    assert_eq!(applied.state, SystemCmdConversionState::Applied);
    assert_eq!(applied.from_fingerprint, pending.from_fingerprint);
    assert_eq!(applied.to_fingerprint, pending.to_fingerprint);
    let mut forged = pending;
    forged.from_fingerprint = canonical_sha256(&after).unwrap();
    assert!(applied_system_cmd_conversion(&after, forged).is_err());
}

/// The manifest decides where an image is read, and the root is the fallback.
///
/// This is the location contract the completion review asked to pin: a manifest that
/// declares an absolute path outside the installation root is read there, a relative
/// declaration is resolved against the root, and a declared location that does not exist
/// falls back to the same file name in the root — which is where this repository's
/// publisher writes the binaries. The hash comparison in the caller is unchanged, so the
/// fallback can only change where a file is looked for, never which file is accepted.
#[test]
fn a_declared_image_is_read_where_the_manifest_says_and_falls_back_to_the_root() {
    let root = tempfile::tempdir().expect("root");
    let release = tempfile::tempdir().expect("release");
    std::fs::create_dir_all(root.path().join("runtime")).expect("runtime");
    std::fs::write(root.path().join("aura-core.exe"), "root core").expect("root core");
    std::fs::write(release.path().join("aura-core.exe"), "release core").expect("release core");
    std::fs::write(root.path().join("runtime/pwsh.exe"), "root pwsh").expect("root pwsh");

    // An absolute declaration is used as written.
    let declared = release.path().join("aura-core.exe").display().to_string();
    assert_eq!(resolve_image(root.path(), &declared), release.path().join("aura-core.exe"));

    // A relative declaration is resolved against the installation root.
    assert_eq!(
        resolve_image(root.path(), "runtime/pwsh.exe"),
        root.path().join("runtime/pwsh.exe")
    );

    // A declaration this installation does not have falls back to the root by name.
    let missing = release.path().join("gone/aura-core.exe").display().to_string();
    assert_eq!(resolve_image(root.path(), &missing), root.path().join("aura-core.exe"));

    // A bare file name resolves to the root either way.
    assert_eq!(resolve_image(root.path(), "aura-core.exe"), root.path().join("aura-core.exe"));
}

/// Stage durations add up, and a stage that was not measured is zero.
///
/// The real numbers come from a real installation (recorded by the open itself), so what
/// a unit test can pin is the arithmetic and the default: the total is the sum of the
/// four stages, a path that measured nothing reports zeros, and nothing saturates into a
/// wrong total. The measurement boundaries are asserted by construction: each stage is
/// timed around exactly one step of `open`.
#[test]
fn installation_stage_durations_add_up() {
    let measured = InstallationStages {
        published_ms: 3,
        package_ms: 5_486,
        images_ms: 2_228,
        cmd_ms: 7,
        verify_ms: 0,
        wait_ms: 0,
    };
    assert_eq!(measured.total_ms(), 7_724);
    assert_eq!(InstallationStages::default().total_ms(), 0);
    let large = InstallationStages {
        published_ms: u64::MAX,
        package_ms: 1,
        images_ms: 0,
        cmd_ms: 0,
        verify_ms: 0,
        wait_ms: 0,
    };
    assert_eq!(large.total_ms(), u64::MAX, "a saturated total must not wrap");
}

/// Concurrent readiness queries share one failed check, and the failure is not a verdict.
///
/// The open is driven by the test, so the count below is the number of checks that really
/// ran rather than an inference. Callers that arrive while an installation is not ready
/// must see the check that was already running instead of each paying for the same
/// verification, and the failed check has to report the stage timings it measured. Once
/// the short sharing window is over, the next caller checks again: an installation that
/// becomes ready a moment later has to be readable as soon as it is, which is exactly what
/// a cached failure would prevent.
#[test]
fn concurrent_readiness_queries_share_one_failed_check() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let opens = std::sync::Arc::new(AtomicUsize::new(0));
    let root = tempfile::tempdir().expect("temporary installation");
    let binding = std::sync::Arc::new(super::installation::InstallationBinding::with_opener(
        root.path().to_path_buf(),
        {
            let opens = std::sync::Arc::clone(&opens);
            std::sync::Arc::new(move |_root: &std::path::Path| {
                opens.fetch_add(1, Ordering::SeqCst);
                // A real check takes real time; the waiters below arrive inside it.
                std::thread::sleep(std::time::Duration::from_millis(250));
                Err(anyhow::anyhow!("runner.installation_not_ready"))
            })
        },
    ));
    let waiters: Vec<_> = (0..8)
        .map(|_| {
            let binding = std::sync::Arc::clone(&binding);
            std::thread::spawn(move || binding.advertised().is_err())
        })
        .collect();
    for waiter in waiters {
        assert!(waiter.join().expect("readiness waiter"), "the check fails for everyone");
    }
    assert_eq!(opens.load(Ordering::SeqCst), 1, "one check, shared by everyone who asked");
    let (stages, succeeded) =
        binding.last_open_stages().expect("a failed check still reports the stages it measured");
    assert!(!succeeded);
    assert_eq!(stages.published_ms, 0);
    assert_eq!(stages.package_ms, 0);
    assert_eq!(stages.images_ms, 0);
    assert_eq!(stages.cmd_ms, 0);
    assert!(
        stages.verify_ms >= 200,
        "the shared check records the time it spent, not a zero total: {}ms",
        stages.verify_ms
    );

    // Past the window the next caller really checks again.
    std::thread::sleep(
        super::installation::FAILURE_SHARE_WINDOW + std::time::Duration::from_millis(50),
    );
    assert!(binding.advertised().is_err());
    assert_eq!(opens.load(Ordering::SeqCst), 2, "a failure is deduplicated, never cached");
}

/// A cache hit keeps the resource check it ran and does not copy another open's stages.
#[test]
fn a_cache_hit_log_records_the_resource_check_only() {
    let root = tempfile::tempdir().expect("log directory");
    let path = root.path().join("core.jsonl");
    let measured = InstallationStages {
        published_ms: 3,
        package_ms: 5_486,
        images_ms: 2_228,
        cmd_ms: 7,
        verify_ms: 19,
        wait_ms: 0,
    };
    super::installation::write_installation_open_line(
        &path,
        &measured,
        true,
        true,
        Some("4b486eb4-dd7f-4681-b8ac-7eece6ce8d25"),
    )
    .expect("cache-hit line");
    super::installation::write_installation_open_line(&path, &measured, false, false, None)
        .expect("miss line");
    let text = std::fs::read_to_string(&path).expect("log");
    let lines: Vec<serde_json::Value> =
        text.lines().map(|line| serde_json::from_str(line).expect("log line")).collect();
    assert_eq!(lines.len(), 2);
    let hit = &lines[0]["fields"];
    assert_eq!(lines[0]["event"], "tool.installation_open");
    assert_eq!(hit["ok"], true);
    assert_eq!(hit["cache_hit"], true);
    assert_eq!(hit["verify_ms"], 19);
    assert_eq!(hit["published_ms"], 0);
    assert_eq!(hit["package_ms"], 0);
    assert_eq!(hit["images_ms"], 0);
    assert_eq!(hit["cmd_ms"], 0);
    assert_eq!(hit["installation_id"], "4b486eb4-dd7f-4681-b8ac-7eece6ce8d25");
    assert_eq!(hit["shared_failure"], false);
    let miss = &lines[1]["fields"];
    assert_eq!(miss["ok"], false);
    assert_eq!(miss["cache_hit"], false);
    assert_eq!(miss["verify_ms"], 19);
    assert_eq!(miss["package_ms"], 5_486);
    assert_eq!(miss["published_ms"], 3);
    assert_eq!(miss["images_ms"], 2_228);
    assert_eq!(miss["cmd_ms"], 7);
}

/// A caller that reuses a recent failure records its own wait, not the first check.
#[test]
fn a_shared_failure_log_records_this_wait_only() {
    let root = tempfile::tempdir().expect("log directory");
    let path = root.path().join("core.jsonl");
    let stages = InstallationStages { wait_ms: 41, ..InstallationStages::default() };
    super::installation::write_shared_installation_failure(&path, &stages).expect("shared line");
    let text = std::fs::read_to_string(&path).expect("log");
    let entry: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
    assert_eq!(entry["event"], "tool.installation_open");
    let fields = &entry["fields"];
    assert_eq!(fields["ok"], false);
    assert_eq!(fields["shared_failure"], true);
    assert_eq!(fields["cache_hit"], false);
    assert_eq!(fields["wait_ms"], 41);
    assert_eq!(fields["verify_ms"], 0);
    assert_eq!(fields["package_ms"], 0);
    assert_eq!(fields["published_ms"], 0);
    assert_eq!(fields["images_ms"], 0);
    assert_eq!(fields["cmd_ms"], 0);
}

/// Only Core says where installation records go (`installation::route_log`). A process Core
/// has not set up, like this test binary, records nothing, and never into the user's profile.
#[test]
fn installation_records_have_no_destination_until_core_routes_them() {
    assert_eq!(super::installation::installation_log_path(), None);
}

#[test]
fn the_running_process_matches_only_its_own_core_binding() {
    let path = std::env::current_exe().expect("test executable");
    let (actual, _) = super::file_binding::capture(&path, false).expect("capture");
    let identity = aura_windows_platform::snapshot_process(std::process::id()).expect("process");
    let owner = aura_windows_platform::process_account_sid(&identity).expect("owner");
    let binding = super::installation::InstallationBinding::new(std::path::PathBuf::from(
        r"Z:\binding-cache-does-not-open",
    ));
    let same = canonical_sha256(b"this-process").expect("fingerprint");
    assert_eq!(binding.core_binding_reject(&same, &actual, &owner).expect("check"), None);
    assert_eq!(
        binding.core_binding_reject(&same, &actual, &owner).expect("cached"),
        None,
        "the same install generation reuses the first answer"
    );
    let mut other = actual.clone();
    other.path = r"C:\work\aura\target\execution-candidates\not-this-core\aura-core.exe".into();
    let changed = canonical_sha256(b"other-core").expect("fingerprint");
    let reason =
        binding.core_binding_reject(&changed, &other, &owner).expect("mismatch").expect("reason");
    assert!(reason.starts_with("runner.core_binding_mismatch"), "{reason}");
    assert!(reason.contains("path"), "{reason}");
    let text = super::installation::InstallationBinding::core_binding_user_text(&reason);
    assert!(text.contains("路径"));
    assert!(text.contains("更换脚本或 Shell 不会修复"));
    assert!(text.contains("debug"));
}
