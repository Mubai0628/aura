//! O-1 uses the real save route and a failing transaction after native publication.
use super::*;

#[tokio::test]
async fn s07_o1_saved_file_is_successful_when_consent_follow_up_cannot_commit() {
    let DirtyEditorFixture { fixture: f, status, .. } = dirty_editor_fixture().await;
    f.service().broker.lock().expect("isolated approval database").database.connection()
        .execute_batch("CREATE TEMP TRIGGER fail_editor_follow BEFORE UPDATE ON authorization_grants BEGIN SELECT RAISE(ABORT, 's07 isolated consent storage failure'); END;")
        .expect("inject failure only into consent follow-up writes");
    let saved = crate::r03_tests::success(&f.runtime, crate::method::BUFFER_SAVE, json!({
        "workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id,"path":"editor.txt",
        "reservation_id":status["reservation_id"],"reservation_epoch":status["reservation_epoch"],
        "expected_buffer_revision":status["buffer_revision"],
    })).await;
    assert_eq!(
        std::fs::read(f.root.join("editor.txt")).expect("read published file"),
        b"editor saved"
    );
    assert_eq!(
        saved["approval_follow_up"][0]["requires_reapproval"], true,
        "successful save must explicitly warn that this file needs a fresh approval: {saved}"
    );
    assert_eq!(saved["approval_follow_up"][0]["path"], "editor.txt");
}
