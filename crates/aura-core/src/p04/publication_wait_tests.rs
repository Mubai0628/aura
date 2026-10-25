//! Actual async route retries; approval, cancellation and unrelated publications are measured.
use super::*;
use aura_artifact_contract::{ArtifactOwnerRef, ArtifactScope};
use aura_network_control::QuarantineWriteRequest;

fn hold_target(
    p04: &P04Services,
    target: &Target,
    path: &str,
) -> aura_workspace_domain::ResourceCommit {
    let facts = p04.workspace.resource_facts(target.workspace, target.session).unwrap();
    let path = WorkspacePath::parse(path).unwrap();
    let mut requests =
        vec![ResourceRequest::new(facts.slot(&path).unwrap(), ResourceAccess::Write)];
    if let Some(object) = facts.object(&path).unwrap() {
        requests.push(ResourceRequest::new(object, ResourceAccess::Write));
    }
    p04.workspace
        .eligibility_owner()
        .commit(&CommitRequest::per_resource(requests), target.session, ExecutionId::next(), None)
        .unwrap()
}

fn export_request(
    p04: &P04Services,
    target: &Target,
    path: &str,
    expected: FsExpectedState,
) -> Value {
    let artifact = p04
        .persistence
        .write_artifact(aura_persistence::ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "text/plain".into(),
            access: ArtifactAccess::USER_READ.union(ArtifactAccess::EXPORT),
            owner: ArtifactOwnerRef::Session { session_id: target.session },
            bytes: b"new".to_vec(),
        })
        .unwrap();
    serde_json::to_value(ArtifactExportRequest {
        artifact_id: artifact.artifact_id,
        workspace_id: target.workspace,
        session_id: target.session,
        target: ArtifactExportTarget::ExternalGrant {
            external_grant_id: target.grant,
            relative_path: WorkspacePath::parse(path).unwrap(),
        },
        expected,
    })
    .unwrap()
}

async fn refused(p04: &P04Services, initial: u64) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while p04.publication_workers.counters().completed <= initial {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the actual publisher tried and refused common admission");
}

fn spawn_route(
    p04: &P04Services,
    method_name: &'static str,
    params: Value,
    id: CorrelationId,
) -> tokio::task::JoinHandle<Result<Value, IpcErrorEnvelope>> {
    let p04 = p04.clone();
    tokio::spawn(async move {
        p04.dispatch_main_command(method_name, &params, id).await.expect("registered route")
    })
}

#[tokio::test]
async fn async_export_wait_frees_workers_and_rechecks_the_original_expected() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    let hold = hold_target(&p04, &target, "target.txt");
    let started = p04.publication_workers.counters().completed;
    let task = spawn_route(
        &p04,
        method::ARTIFACT_EXPORT,
        export_request(
            &p04,
            &target,
            "target.txt",
            FsExpectedState::FileHash { hash: ContentHash::blake3(b"old") },
        ),
        CorrelationId::new(),
    );
    refused(&p04, started).await;
    assert!(!task.is_finished());
    assert_eq!(std::fs::read(&target.path).unwrap(), b"old");
    let sibling = spawn_route(
        &p04,
        method::ARTIFACT_EXPORT,
        export_request(&p04, &target, "sibling.txt", FsExpectedState::Absent),
        CorrelationId::new(),
    );
    tokio::time::timeout(std::time::Duration::from_secs(1), sibling)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(target.path.with_file_name("sibling.txt")).unwrap(), b"new");
    std::fs::write(&target.path, b"external change").unwrap();
    p04.workspace.eligibility_owner().release_commit(hold);
    let error = tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code.as_str(), "artifact.expected_hash_mismatch");
    assert_eq!(std::fs::read(&target.path).unwrap(), b"external change");
    assert_eq!(p04.workspace.eligibility_owner().snapshot().commits, 0);
}

fn approved_release(
    p04: &P04Services,
    target: &Target,
) -> (ReleaseQuarantineInput, aura_domain_common::PermissionGrantId) {
    let record = p04
        .quarantine
        .quarantine_if_executable(QuarantineWriteRequest {
            original_name: "fixture.exe".into(),
            source_url: "https://fixture.invalid/file.exe".into(),
            media_type: None,
            content_disposition: None,
            bytes: b"synthetic never executed bytes",
            session_id: target.session,
            correlation_id: CorrelationId::new(),
        })
        .unwrap()
        .unwrap();
    let destination =
        QuarantineReleaseTarget::Workspace { path: WorkspacePath::parse("release.exe").unwrap() };
    let preview = p04
        .prepare_quarantine_release(
            PrepareQuarantineReleaseInput {
                quarantine_id: record.record.id,
                workspace_id: target.workspace,
                session_id: target.session,
                target: destination.clone(),
            },
            CorrelationId::new(),
        )
        .unwrap();
    let approved = p04
        .persistence
        .resolve_approval(
            preview.approval_id,
            preview.approval_request_revision,
            ApprovalDecisionKind::AllowOnce,
            CorrelationId::new(),
        )
        .unwrap();
    (
        ReleaseQuarantineInput {
            quarantine_id: record.record.id,
            approval_id: preview.approval_id,
            workspace_id: target.workspace,
            session_id: target.session,
            target: destination,
        },
        approved.resolution.grant_ids[0],
    )
}

#[tokio::test]
async fn quarantine_admission_wait_does_not_consume_once_approval_and_releases_once() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    let (request, grant) = approved_release(&p04, &target);
    let hold = hold_target(&p04, &target, "release.exe");
    let initial = p04.publication_workers.counters().completed;
    let task = spawn_route(
        &p04,
        method::SECURITY_QUARANTINE_RELEASE,
        serde_json::to_value(&request).unwrap(),
        CorrelationId::new(),
    );
    refused(&p04, initial).await;
    assert!(!task.is_finished());
    assert!(p04.persistence.permission_grant(grant).unwrap().consumed_at.is_none());
    assert!(
        p04.persistence.quarantine_release_journal_state(request.quarantine_id).unwrap().is_none()
    );
    assert!(!target.path.with_file_name("release.exe").exists());
    p04.workspace.eligibility_owner().release_commit(hold);
    tokio::time::timeout(std::time::Duration::from_secs(2), task).await.unwrap().unwrap().unwrap();
    assert_eq!(
        std::fs::read(target.path.with_file_name("release.exe")).unwrap(),
        b"synthetic never executed bytes"
    );
    assert!(p04.persistence.permission_grant(grant).unwrap().consumed_at.is_some());
    assert_eq!(
        p04.persistence.quarantine_release_journal_state(request.quarantine_id).unwrap().as_deref(),
        Some("completed")
    );
    let second = p04.release_quarantine(request, CorrelationId::new()).unwrap_err();
    assert_eq!(second.code.as_str(), "network.quarantine_not_found");
}

#[tokio::test]
async fn cancelled_quarantine_wait_preserves_approval_and_cannot_publish_later() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = fixture.start().await;
    let p04 = services(&state).p04_required();
    let target = seed(&p04, fixture.root.path());
    let (request, grant) = approved_release(&p04, &target);
    let hold = hold_target(&p04, &target, "release.exe");
    let initial = p04.publication_workers.counters().completed;
    let id = CorrelationId::new();
    let task = spawn_route(
        &p04,
        method::SECURITY_QUARANTINE_RELEASE,
        serde_json::to_value(&request).unwrap(),
        id,
    );
    refused(&p04, initial).await;
    aura_fs_tools::cancel_resource_wait(id);
    let error = tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code.as_str(), "fs.search_cancelled");
    p04.workspace.eligibility_owner().release_commit(hold);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!target.path.with_file_name("release.exe").exists());
    assert!(p04.persistence.permission_grant(grant).unwrap().consumed_at.is_none());
    assert!(
        p04.persistence.quarantine_release_journal_state(request.quarantine_id).unwrap().is_none()
    );
    assert!(
        !aura_fs_tools::resource_wait_cancelled(id),
        "the completed owner reclaims cancellation state"
    );
}
