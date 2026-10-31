use super::*;
use aura_permission_domain::{ExternalGrantCapability, ExternalGrantTerm};
use aura_security_service::external_grants;

#[tokio::test]
async fn s06_b6_a_read_directory_grant_asks_before_writing_without_an_attempt() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.write"]).await;
    let directory = f._host.root.path().join("read-approved");
    std::fs::create_dir(&directory).expect("isolated external directory");
    let info =
        aura_windows_platform::inspect_existing_path(&directory, true).expect("verified root");
    let display = info.final_path.display().to_string();
    f.service()
        .agent_execution_required()
        .external_grants
        .issue_approved_directory(
            external_grants::ApprovedDirectoryGrant {
                session_id: f.turn.session_id,
                workspace_id: f.turn.workspace_id,
                canonical_root: CanonicalPath { utf16: display.encode_utf16().collect(), display },
                root_identity: aura_workspace::domain_identity(&info.identity),
                recursive_scope: true,
                capabilities: vec![ExternalGrantCapability::Read],
                term: ExternalGrantTerm::Session,
                turn_task_id: None,
                expires_at: None,
            },
            ApprovalRequestId::new(),
            "read approval",
            CorrelationId::new(),
        )
        .expect("durable read grant");
    let target = directory.join("must-ask.txt");
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":target,"text":"must wait",
        "expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let outcome = f.execute_rejecting(batch).await;
    assert!(
        matches!(outcome, Some(BatchOutcome::AwaitingApproval { .. })),
        "a read grant must ask for its missing write capabilities before dispatch"
    );
    let store = f.service().broker.lock().expect("journal");
    let call =
        aura_persistence_v2::tool::execution::call(store.database.connection(), id).expect("call");
    assert!(call.attempt_id.is_none(), "no attempt while missing capabilities await approval");
    assert!(!target.exists(), "no file effect before approval");
}

#[tokio::test]
async fn s06_b6_create_only_grant_asks_only_for_the_missing_write_capability() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.write"]).await;
    let directory = f._host.root.path().join("create-approved");
    std::fs::create_dir(&directory).expect("create the isolated directory fixture");
    let info = aura_windows_platform::inspect_existing_path(&directory, true)
        .expect("verify the native identity of the fixture target");
    let display = info.final_path.display().to_string();
    f.service()
        .agent_execution_required()
        .external_grants
        .issue_approved_directory(
            external_grants::ApprovedDirectoryGrant {
                session_id: f.turn.session_id,
                workspace_id: f.turn.workspace_id,
                canonical_root: CanonicalPath { utf16: display.encode_utf16().collect(), display },
                root_identity: aura_workspace::domain_identity(&info.identity),
                recursive_scope: true,
                capabilities: vec![ExternalGrantCapability::Create],
                term: ExternalGrantTerm::Session,
                turn_task_id: None,
                expires_at: None,
            },
            ApprovalRequestId::new(),
            "create approval",
            CorrelationId::new(),
        )
        .expect("record the external grant with create capability only");
    let target = directory.join("still-needs-write.txt");
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":target,"text":"must wait",
        "expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let Some(BatchOutcome::AwaitingApproval { proposal, .. }) = f.execute_rejecting(batch).await
    else {
        panic!("missing write capability must ask before dispatch");
    };
    assert_eq!(
        proposal.requested_capabilities,
        Some(vec![ResolvedCapability::ExternalFileWrite]),
        "the already approved create capability must not be requested again"
    );
    assert!(!target.exists());
    assert!(
        aura_persistence_v2::tool::execution::call(
            f.service()
                .broker
                .lock()
                .expect("lock the existing authority store for inspection")
                .database
                .connection(),
            id,
        )
        .expect("read the durable tool call at this dispatch boundary")
        .attempt_id
        .is_none()
    );
}
