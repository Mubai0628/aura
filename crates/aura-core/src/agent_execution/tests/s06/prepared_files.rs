use super::consent_fixture::set_mode;
use super::*;

#[tokio::test]
async fn s06_e2_prepared_file_keeps_its_admission_when_permissions_loosen() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"prepared.txt","text":"published",
        "expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    set_mode(&f, PermissionMode::FullAccess).await;
    f.execute(batch).await;
    f.output(id);
    assert_eq!(
        std::fs::read(f.root.join("prepared.txt"))
            .expect("read the actual file effect after dispatch"),
        b"published"
    );
}
