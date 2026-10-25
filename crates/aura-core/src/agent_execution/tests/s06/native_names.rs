use super::consent_fixture::approve_file;
use super::*;

async fn distinct_names(first: &str, second: &str, sensitive: bool) {
    let f = Fixture::with_exposure_setup(PermissionMode::ReadOnly, &["fs.write"], |root| {
        if sensitive {
            let enabled = std::process::Command::new("fsutil.exe")
                .args(["file", "setCaseSensitiveInfo"])
                .arg(root)
                .arg("enable")
                .output()
                .expect(
                    "invoke fsutil to enable case sensitivity on the isolated fixture directory",
                );
            assert!(
                enabled.status.success(),
                "fixture-only case-sensitive directory: {} {}",
                String::from_utf8_lossy(&enabled.stdout),
                String::from_utf8_lossy(&enabled.stderr)
            );
        }
        std::fs::write(root.join(first), "first object")
            .expect("write the fixture content for this permission boundary");
        std::fs::write(root.join(second), "second object")
            .expect("write the fixture content for this permission boundary");
    })
    .await;
    let first_info = aura_windows_platform::inspect_existing_path(&f.root.join(first), true)
        .expect("verify the native identity of the fixture target");
    let second_info = aura_windows_platform::inspect_existing_path(&f.root.join(second), true)
        .expect("verify the native identity of the fixture target");
    assert_ne!(first_info.identity, second_info.identity, "two actual native objects");
    let batch = f.admit(&[
        (
            "fs.write",
            json!({"path":first,"text":"approved", "expected":{"expected_kind":"file_hash",
            "hash":ContentHash::blake3(b"first object")}}),
        ),
        (
            "fs.write",
            json!({"path":second,"text":"must ask", "expected":{"expected_kind":"file_hash",
            "hash":ContentHash::blake3(b"second object")}}),
        ),
    ]);
    let first_id = batch.batch.calls[0].tool_call_id;
    let second_id = batch.batch.calls[1].tool_call_id;
    approve_file(&f, &batch.calls[&first_id]);
    assert!(matches!(f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { call_id, .. }) if call_id == second_id));
    assert_eq!(
        std::fs::read(f.root.join(second)).expect("read the actual file effect after dispatch"),
        b"second object"
    );
    assert!(
        aura_persistence_v2::tool::execution::call(
            f.service()
                .broker
                .lock()
                .expect("lock the existing authority store for inspection")
                .database
                .connection(),
            second_id
        )
        .expect("read the durable tool call at this dispatch boundary")
        .attempt_id
        .is_none()
    );
}

#[tokio::test]
async fn s06_d5_unicode_objects_use_native_identity_through_dispatch() {
    distinct_names("\u{0130}.txt", "i\u{0307}.txt", false).await;
}

#[tokio::test]
async fn s06_d5_sensitive_a_and_a_are_separate_native_objects_through_dispatch() {
    distinct_names("A.txt", "a.txt", true).await;
}
