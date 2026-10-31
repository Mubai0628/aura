//! The real directory decision's companion authority is checked on frozen-batch recovery.
use super::*;
use aura_permission_domain::{ExternalGrantCapability, ExternalGrantTerm};
use aura_security_service::external_grants;
use std::time::Duration;

#[tokio::test]
async fn s06_b6_directory_answer_resumes_without_reentering_the_journal_lock() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.list"]).await;
    let directory = f._host.root.path().join("resume-directory");
    std::fs::create_dir(&directory).expect("create the isolated directory fixture");
    std::fs::write(directory.join("visible.txt"), "ordinary fixture")
        .expect("write the fixture content for this permission boundary");
    let batch = f.admit(&[("fs.list", json!({"path":directory,"limit":100,"cursor":null}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let call = &batch.calls[&id];
    let consent = super::consent_fixture::approve_file(&f, call);
    let native = aura_windows_platform::inspect_existing_path(&directory, true)
        .expect("verify the native identity of the fixture target");
    let display = native.final_path.display().to_string();
    let engine = f.service().agent_execution_required();
    engine
        .external_grants
        .issue_approved_directory(
            external_grants::ApprovedDirectoryGrant {
                session_id: f.turn.session_id,
                workspace_id: f.turn.workspace_id,
                canonical_root: CanonicalPath { utf16: display.encode_utf16().collect(), display },
                root_identity: aura_workspace::domain_identity(&native.identity),
                recursive_scope: true,
                capabilities: vec![ExternalGrantCapability::Read],
                term: ExternalGrantTerm::Session,
                turn_task_id: None,
                expires_at: None,
            },
            consent
                .scope
                .file_consent
                .as_ref()
                .expect("the approved grant contains its file scope")
                .approval_id,
            call.operation.fingerprint.0.hex(),
            CorrelationId::new(),
        )
        .expect("record the explicit external directory approval");
    // A plain test thread keeps a regression bounded even if the implementation
    // recursively locks. The normal production admission entry owns this sync section.
    let batch_id = batch.batch.tool_batch_id;
    let (sender, received) = tokio::sync::oneshot::channel();
    let reader = std::thread::spawn(move || {
        let _ = sender.send(engine.resume_batch(batch_id));
    });
    let resumed = tokio::time::timeout(Duration::from_secs(3), received)
        .await
        .expect("a directory answer must not lock Core's journal indefinitely")
        .expect("the managed-stop observation completes within its budget")
        .expect("the managed-stop observation completes within its budget");
    reader.join().expect("join the fixture worker after its real execution settles");
    f.execute(resumed).await;
    let output = f.output(id);
    assert!(
        output["entries"]
            .as_array()
            .expect("the real tool result contains the expected collection")
            .iter()
            .any(|entry| entry["name"] == "visible.txt")
    );
}
