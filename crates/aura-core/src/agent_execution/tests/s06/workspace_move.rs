//! A move points a Workspace at another folder. A conversation's file approvals follow a file's
//! identity, which a move on the same drive keeps, so what stops them must not be the withdrawal
//! that runs after the move commits: a Core that stops in between never runs it.
use super::consent_fixture::approve_file;
use super::*;
use aura_execution_contract::BackendDescriptor;
use aura_permission_domain::{
    ExternalGrantAvailability, ExternalGrantCapability as C, ExternalGrantTerm,
};
use aura_permission_engine::policy::{PolicyInput, authorize};
use aura_security_contract::{
    ApprovalDecisionKind, ApprovalState, AuthorizationApproval, PolicyAuthorization,
};
use aura_security_service::external_grants;
use aura_workspace_domain::WorkspaceLifecycleOperation;
use std::path::PathBuf;

fn commit_without_follow_up(f: &Fixture, operation: WorkspaceLifecycleOperation) {
    crate::r03_tests::workspace_moves::commit_without_follow_up(
        f.service(),
        f.turn.workspace_id,
        operation,
    );
}

fn move_without_withdrawal(f: &Fixture) -> PathBuf {
    crate::r03_tests::workspace_moves::move_without_withdrawal(
        f.service(),
        f.turn.workspace_id,
        &f.root,
    )
}

/// Runs `check` on the call's policy input as dispatch builds it: the session's approvals and
/// the Workspace as Core holds it now.
fn with_live_policy<T>(
    f: &Fixture,
    call: &CallContext,
    check: impl FnOnce(&PolicyInput<'_>, &BackendDescriptor) -> T,
) -> T {
    let engine = f.service().agent_execution_required();
    let grants = aura_persistence_v2::security::grants_for_call(
        f.service().broker.lock().expect("read the approvals").database.connection(),
        &call.operation.fingerprint,
        Some(f.turn.session_id),
    )
    .expect("the session's approvals");
    let mut policy = call.policy(UtcTimestamp::now(), engine.runtime);
    policy.grants = &grants;
    policy.workspace = engine.approval_workspace(&call.operation);
    check(&policy, &broker::descriptor(engine.runtime))
}

/// What the call still asks the user for, decided as dispatch decides it.
fn asked(f: &Fixture, call: &CallContext) -> Option<Vec<ResolvedCapability>> {
    match with_live_policy(f, call, |policy, backend| authorize(policy, backend, None)) {
        PolicyAuthorization::Ask { proposal } => proposal.requested_capabilities,
        other => panic!("the call is decided without asking: {other:?}"),
    }
}

/// Answers what the call still asks, as the approval route records an answer.
fn answer(f: &Fixture, call: &CallContext, decision: ApprovalDecisionKind) {
    let PolicyAuthorization::Ask { proposal } =
        with_live_policy(f, call, |policy, backend| authorize(policy, backend, None))
    else {
        panic!("the call has a question to answer");
    };
    let engine = f.service().agent_execution_required();
    let folder = f
        .service()
        .persistence
        .workspace(f.turn.workspace_id)
        .expect("read the Workspace the user answers in")
        .canonical_root;
    let now = UtcTimestamp::now();
    let request = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    f.service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &request)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                request.approval_id,
                request.revision,
                &call.operation,
                decision,
                broker::descriptor(engine.runtime).class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                Some(&folder),
                now,
            )
        })
        .expect("record the answer")
        .expect("an allowing answer issues a grant");
}

#[tokio::test]
async fn a_moved_workspace_asks_again_for_a_file_approved_at_its_old_folder() {
    let mut f = Fixture::new(PermissionMode::ReadOnly).await;
    let first = f.admit(&[(
        "fs.write",
        json!({"path":"approved.txt","text":"first","expected":{"expected_kind":"absent"}}),
    )]);
    approve_file(&f, first.calls.values().next().expect("the write is admitted"));
    let batch = first.batch.tool_batch_id;
    let group = f.execute(first).await;
    let moved = move_without_withdrawal(&f);
    f.continue_after_tools(batch, &group);
    let next = f.admit(&[(
        "fs.write",
        json!({"path":"approved.txt","text":"after the move",
        "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"first")}}),
    )]);
    assert!(
        matches!(f.execute_rejecting(next).await, Some(BatchOutcome::AwaitingApproval { .. })),
        "the approval given at the old folder still covers the moved file"
    );
    assert_eq!(std::fs::read(moved.join("approved.txt")).expect("read the moved file"), b"first");
}

/// A download is decided capability by capability, so the file it writes is covered by the
/// file's approval and its network by another answer.
#[tokio::test]
async fn a_download_after_a_move_asks_again_for_the_file_approved_at_the_old_folder() {
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::ReadOnly,
        &["fs.write", "http.download"],
        |root| std::fs::write(root.join("approved.txt"), "before").expect("create the file"),
    )
    .await;
    let write = f.admit(&[(
        "fs.write",
        json!({"path":"approved.txt","text":"approved",
        "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"before")}}),
    )]);
    approve_file(&f, write.calls.values().next().expect("the write is admitted"));
    let batch = write.batch.tool_batch_id;
    let group = f.execute(write).await;
    f.continue_after_tools(batch, &group);
    // Nothing listens there: the download is only decided, never sent.
    let download = f.admit(&[(
        "http.download",
        json!({"url":"http://127.0.0.1:9/file","path":"approved.txt","overwrite":true}),
    )]);
    let call = &download.calls[&download.batch.calls[0].tool_call_id];
    assert_eq!(
        asked(&f, call),
        Some(vec![ResolvedCapability::OrdinaryNetwork]),
        "the write's answer covers the file"
    );
    answer(&f, call, ApprovalDecisionKind::AllowOnce);
    let PolicyAuthorization::Allow { authorization } =
        with_live_policy(&f, call, |policy, backend| authorize(policy, backend, None))
    else {
        panic!("the two answers cover the download");
    };
    move_without_withdrawal(&f);
    let file: Vec<_> = call
        .operation
        .content
        .capabilities
        .iter()
        .copied()
        .filter(|capability| *capability != ResolvedCapability::OrdinaryNetwork)
        .collect();
    assert_eq!(
        asked(&f, call),
        Some(file),
        "the file approved at the old folder still covers the download after the move"
    );
    assert!(
        !with_live_policy(&f, call, |policy, backend| {
            aura_permission_engine::network::validate_composite(policy, backend, &authorization)
        }),
        "the download decided before the move is still reserved after it"
    );
}

#[tokio::test]
async fn archiving_and_restoring_a_workspace_keep_its_file_approvals() {
    let mut f = Fixture::new(PermissionMode::ReadOnly).await;
    let first = f.admit(&[(
        "fs.write",
        json!({"path":"approved.txt","text":"first","expected":{"expected_kind":"absent"}}),
    )]);
    approve_file(&f, first.calls.values().next().expect("the write is admitted"));
    let batch = first.batch.tool_batch_id;
    let group = f.execute(first).await;
    commit_without_follow_up(&f, WorkspaceLifecycleOperation::Archive);
    crate::r03_tests::success(
        &f.runtime,
        crate::method::WORKSPACE_RESTORE,
        json!({"workspace_id":f.turn.workspace_id}),
    )
    .await;
    f.continue_after_tools(batch, &group);
    let next = f.admit(&[(
        "fs.write",
        json!({"path":"approved.txt","text":"after the restore",
        "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"first")}}),
    )]);
    let id = next.batch.calls[0].tool_call_id;
    assert!(
        matches!(f.execute_rejecting(next).await, Some(BatchOutcome::Completed(_))),
        "an archived and restored Workspace is where it was, so its approvals still cover"
    );
    f.output(id);
    assert_eq!(
        std::fs::read(f.root.join("approved.txt")).expect("read the written file"),
        b"after the restore"
    );
}

/// Approves listing a folder outside the Workspace and records the folder the approval reaches,
/// as the approval route does after the answer.
fn approve_outside_folder(f: &Fixture) -> aura_domain_common::ExternalGrantId {
    let outside = f._host.root.path().join("outside");
    std::fs::create_dir(&outside).expect("create the outside folder");
    let batch = f.admit(&[("fs.list", json!({"path":outside,"limit":100,"cursor":null}))]);
    let call = &batch.calls[&batch.batch.calls[0].tool_call_id];
    let consent = approve_file(f, call);
    let info = aura_windows_platform::inspect_existing_path(&outside, true)
        .expect("verify the outside folder");
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
                capabilities: vec![C::Read],
                term: ExternalGrantTerm::Session,
                turn_task_id: None,
                expires_at: None,
            },
            consent.scope.file_consent.as_ref().expect("a file approval").approval_id,
            call.operation.fingerprint.0.hex(),
            CorrelationId::new(),
        )
        .expect("record the folder the approval reaches")
        .id
}

#[tokio::test]
async fn an_outside_folder_approved_before_a_move_is_not_reachable_after_it() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.list"]).await;
    let companion = approve_outside_folder(&f);
    let grants = &f.service().agent_execution_required().external_grants;
    let reachable = || {
        grants.covers_capabilities(
            companion,
            f.turn.session_id,
            &[ResolvedCapability::ExternalFileRead],
        )
    };
    assert!(reachable(), "the folder is reachable while the Workspace stays where it is");
    move_without_withdrawal(&f);
    assert!(!reachable(), "the folder approved before the move is still reachable after it");
}

/// A Core that stopped before the withdrawal after a move binds its journal at the next start,
/// as here, and withdraws then the approval and the folder record that came with it.
#[tokio::test]
async fn the_next_start_withdraws_an_outside_folder_approved_before_a_move() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.list"]).await;
    let companion = approve_outside_folder(&f);
    move_without_withdrawal(&f);
    let engine = f.service().agent_execution_required();
    let folder = || {
        engine
            .external_grants
            .list_session(f.turn.session_id)
            .expect("list the session's folders")
            .into_iter()
            .find(|view| view.id == companion)
            .expect("the folder the approval reached")
            .availability
    };
    // The folder outside stays where it was, so both are still listed in force, though neither
    // covers anything after the move.
    assert_eq!(folder(), ExternalGrantAvailability::InForce);
    assert_eq!(listed_approval(&f).await, json!("in_force"));
    f.service().p04_required().bind_turn_journal(std::sync::Arc::clone(&engine.journal));
    assert_eq!(
        folder(),
        ExternalGrantAvailability::Revoked,
        "the folder approved before the move is still listed in force"
    );
    assert_eq!(
        listed_approval(&f).await,
        json!("revoked"),
        "the approval given before the move is still listed in force"
    );
    assert_eq!(
        engine
            .external_grants
            .withdraw_out_of_workspace_file_approvals(None, CorrelationId::new())
            .expect("withdraw again"),
        0,
        "a second run finds more to withdraw"
    );
}

/// How the settings list the session's one file approval.
async fn listed_approval(f: &Fixture) -> serde_json::Value {
    crate::r03_tests::success(
        &f.runtime,
        "security.file_session_consent.list",
        json!({"session_id":f.turn.session_id,"include_inactive":true}),
    )
    .await[0]["availability"]
        .clone()
}
