use super::*;
use aura_permission_domain::{ExternalGrantCapability as C, ExternalGrantTerm};
use aura_security_service::external_grants;
use std::path::Path;

fn grant(f: &Fixture, path: &Path, capabilities: Vec<C>) {
    let info = aura_windows_platform::inspect_existing_path(path, true)
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
                capabilities,
                term: ExternalGrantTerm::Session,
                turn_task_id: None,
                expires_at: None,
            },
            ApprovalRequestId::new(),
            "isolated directory approval",
            CorrelationId::new(),
        )
        .expect("record the explicit directory scope for this isolated session");
}

#[tokio::test]
async fn s06_live_directory_revoke_preserves_an_independent_approval_of_the_same_root() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.list"]).await;
    let directory = f._host.root.path().join("two-authorities");
    std::fs::create_dir(&directory).expect("create the isolated directory fixture");
    let batch = f.admit(&[("fs.list", json!({"path":directory,"limit":100,"cursor":null}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let call = &batch.calls[&id];
    let consent = super::consent_fixture::approve_file(&f, call);
    grant(&f, &directory, vec![C::Read]);
    let root = aura_windows_platform::inspect_existing_path(&directory, true)
        .expect("verify the native identity of the fixture target");
    let display = root.final_path.display().to_string();
    let companion = f
        .service()
        .agent_execution_required()
        .external_grants
        .issue_approved_directory(
            external_grants::ApprovedDirectoryGrant {
                session_id: f.turn.session_id,
                workspace_id: f.turn.workspace_id,
                canonical_root: CanonicalPath { utf16: display.encode_utf16().collect(), display },
                root_identity: aura_workspace::domain_identity(&root.identity),
                recursive_scope: true,
                capabilities: vec![C::Read],
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
    crate::r03_tests::success(&f.runtime, "security.file_session_consent.revoke", json!({
        "session_id":f.turn.session_id,"grant_id":consent.grant_id,"expected_revision":consent.revision,
    })).await;
    assert!(
        f.service().agent_execution_required().external_grants.covers_capabilities(
            companion.id,
            f.turn.session_id,
            &[ResolvedCapability::ExternalFileRead]
        ),
        "withdrawal of one approval must preserve independent live authority"
    );
    f.execute(batch).await;
    f.output(id);
}

#[tokio::test]
async fn s06_b3_c1_approved_directory_covers_deep_files_created_after_approval() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let directory = f._host.root.path().join("D");
    std::fs::create_dir(&directory).expect("create the isolated directory fixture");
    grant(&f, &directory, vec![C::Read, C::Write, C::Create, C::Delete]);
    std::fs::create_dir_all(directory.join("future/deep"))
        .expect("create the isolated directory fixture");
    let target = directory.join("future/deep/new.txt");
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":target,"text":"approved future file",
        "expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(matches!(
        batch.calls[&id].operation.content.resources[0],
        ResourceDescriptor::ExternalGrant { .. }
    ));
    f.execute(batch).await;
    f.output(id);
    assert_eq!(
        std::fs::read(target).expect("read the actual file effect after dispatch"),
        b"approved future file"
    );
}

#[tokio::test]
async fn s06_b3_similar_prefix_and_junction_outside_are_not_inherited() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let directory = f._host.root.path().join("D");
    let sibling = f._host.root.path().join("D2");
    let outside = f._host.root.path().join("outside");
    for path in [&directory, &sibling, &outside] {
        std::fs::create_dir(path).expect("create the isolated directory fixture");
    }
    grant(&f, &directory, vec![C::Write, C::Create]);
    junction::create(&outside, directory.join("link"))
        .expect("create the junction that escapes the approved directory");
    let targets = [sibling.join("new.txt"), directory.join("link/new.txt")];
    let batch = f.admit(
        &targets
            .iter()
            .map(|path| {
                (
                    "fs.write",
                    json!({"path":path,"text":"must ask",
        "expected":{"expected_kind":"absent"}}),
                )
            })
            .collect::<Vec<_>>(),
    );
    let ids = batch.batch.calls.iter().map(|c| c.tool_call_id).collect::<Vec<_>>();
    for id in &ids {
        assert!(
            batch.calls[id]
                .operation
                .content
                .resources
                .iter()
                .all(|r| !matches!(r, ResourceDescriptor::ExternalGrant { .. }))
        );
    }
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    for path in targets {
        assert!(!path.exists());
    }
    let store =
        f.service().broker.lock().expect("lock the existing authority store for inspection");
    for id in ids {
        assert!(
            aura_persistence_v2::tool::execution::call(store.database.connection(), id)
                .expect("read the durable tool call at this dispatch boundary")
                .attempt_id
                .is_none()
        );
    }
}

#[tokio::test]
async fn s06_d1_parent_directory_grant_does_not_cover_a_system_child() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.list"]).await;
    let selected = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").expect("Windows supplies the actual SystemRoot fixture"),
    );
    grant(
        &f,
        selected.parent().expect("the verified fixture path has its parent directory"),
        vec![C::Read],
    );
    let batch = f.admit(&[("fs.list", json!({"path":selected,"limit":1,"cursor":null}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(matches!(f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { call_id, .. }) if call_id == id));
    assert!(
        aura_persistence_v2::tool::execution::call(
            f.service()
                .broker
                .lock()
                .expect("lock the existing authority store for inspection")
                .database
                .connection(),
            id
        )
        .expect("read the durable tool call at this dispatch boundary")
        .attempt_id
        .is_none()
    );
}

#[tokio::test]
async fn s06_live_directory_file_revocation_withdraws_its_companion_authority() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.list"]).await;
    let directory = f._host.root.path().join("approved-directory");
    std::fs::create_dir(&directory).expect("create the isolated directory fixture");
    let batch = f.admit(&[("fs.list", json!({"path":directory,"limit":100,"cursor":null}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let call = &batch.calls[&id];
    let consent = super::consent_fixture::approve_file(&f, call);
    let reference =
        consent.scope.file_consent.as_ref().expect("the approved grant contains its file scope");
    let root = aura_windows_platform::inspect_existing_path(&directory, true)
        .expect("verify the native identity of the fixture target");
    let display = root.final_path.display().to_string();
    let companion = f
        .service()
        .agent_execution_required()
        .external_grants
        .issue_approved_directory(
            external_grants::ApprovedDirectoryGrant {
                session_id: f.turn.session_id,
                workspace_id: f.turn.workspace_id,
                canonical_root: CanonicalPath { utf16: display.encode_utf16().collect(), display },
                root_identity: aura_workspace::domain_identity(&root.identity),
                recursive_scope: true,
                capabilities: vec![C::Read],
                term: ExternalGrantTerm::Session,
                turn_task_id: None,
                expires_at: None,
            },
            reference.approval_id,
            call.operation.fingerprint.0.hex(),
            CorrelationId::new(),
        )
        .expect("record the companion directory grant bound to this file approval");
    assert!(f.service().agent_execution_required().external_grants.covers_capabilities(
        companion.id,
        f.turn.session_id,
        &[ResolvedCapability::ExternalFileRead]
    ));
    crate::r03_tests::success(&f.runtime, "security.file_session_consent.revoke", json!({
        "session_id":f.turn.session_id,"grant_id":consent.grant_id,"expected_revision":consent.revision,
    })).await;
    assert!(
        !f.service().agent_execution_required().external_grants.covers_capabilities(
            companion.id,
            f.turn.session_id,
            &[ResolvedCapability::ExternalFileRead]
        ),
        "fresh directory authority cannot outlive the approval that issued it"
    );
    // This prepared call is bound to the now withdrawn directory decision. It may be
    // refused instead of reopened as a fresh request, but it must never reach an Attempt.
    let result = f.execute_rejecting(batch).await;
    assert!(matches!(
        result,
        Some(BatchOutcome::Completed(_)) | Some(BatchOutcome::AwaitingApproval { .. })
    ));
    assert!(
        aura_persistence_v2::tool::execution::call(
            f.service()
                .broker
                .lock()
                .expect("lock the existing authority store for inspection")
                .database
                .connection(),
            id
        )
        .expect("read the durable tool call at this dispatch boundary")
        .attempt_id
        .is_none()
    );
}
