use super::*;
use aura_execution_contract::{ExecutionOutcome, RollbackEvidence};

#[tokio::test]
async fn r05_editor_save_preserves_reservation_and_verifies_disk_bytes() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    fs::write(root.join("buffer.txt"), "before").expect("file");
    let opened = success(
        &state,
        method::BUFFER_OPEN,
        json!({"workspace_id":w.workspace_id,"session_id":s,"path":"buffer.txt","writable":true}),
    )
    .await;
    let edited=success(&state,method::BUFFER_EDIT,json!({"workspace_id":w.workspace_id,"session_id":s,"path":"buffer.txt",
        "reservation_id":opened["reservation_id"],"reservation_epoch":opened["reservation_epoch"],"buffer_revision":aura_domain_common::Revision::new(serde_json::from_value::<aura_domain_common::Revision>(opened["buffer_revision"].clone()).expect("revision").value()+1),
        "base_core_revision":opened["core_acknowledged_revision"],"content":"saved","active_editing":false,"ime_composition_active":false})).await;
    let status = &edited["status"];
    success(&state,method::BUFFER_SAVE,json!({"workspace_id":w.workspace_id,"session_id":s,"path":"buffer.txt","reservation_id":status["reservation_id"],
        "reservation_epoch":status["reservation_epoch"],"expected_buffer_revision":status["buffer_revision"]})).await;
    assert_eq!(fs::read(root.join("buffer.txt")).expect("saved bytes"), b"saved");
    assert_eq!(
        results(&state).last().expect("receipt").side_effect,
        SideEffectStateV1::FullyApplied
    );
}

#[tokio::test]
async fn r05_partial_changeset_compensates_and_records_real_rollback() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    fs::write(root.join("first.txt"), b"before").expect("initial");
    fs::create_dir(root.join("source-dir")).expect("source");
    fs::write(root.join("source-dir/normal.txt"), "normal").expect("normal");
    fs::create_dir(root.join("real-dir")).expect("real dir");
    fs::write(root.join("real-dir/linked.txt"), "linked").expect("linked");
    junction::create(root.join("real-dir"), root.join("source-dir/link"))
        .expect("fixture junction");
    let response=request(&state,method::FS_APPLY_CHANGESET,json!({"workspace_id":w.workspace_id,"session_id":s,"operations":[
        {"operation":"write_file","target":{"path":"first.txt"},"content":"after","expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"before")},"encoding":null,"line_ending":null},
        {"operation":"copy","source":{"path":"source-dir"},"destination":{"path":"copied-dir"},"source_expected":{"expected_kind":"directory"},"destination_expected":{"expected_kind":"absent"}}
    ]})).await;
    assert!(response.get("error").is_some());
    assert_eq!(fs::read(root.join("first.txt")).expect("restored"), b"before");
    assert!(!root.join("copied-dir").exists());
    let completed = results(&state);
    let result = completed.last().expect("rollback receipt");
    assert_eq!(result.outcome, ExecutionOutcome::Failed);
    assert_eq!(result.side_effect, SideEffectStateV1::Compensated);
    assert!(matches!(result.receipt.rollback, RollbackEvidence::Completed { .. }));
    junction::delete(root.join("source-dir/link")).expect("remove only fixture link");
}
