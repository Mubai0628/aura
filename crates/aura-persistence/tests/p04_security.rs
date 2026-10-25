use std::str::FromStr;

use aura_artifact_contract::{ArtifactAccess, ArtifactOwnerRef, ArtifactScope};
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ApprovalRequestId, CancellationId, ContentHash, ContentHashAlgorithm, CorrelationId,
    ExternalGrantId, FileIdentity, HostPreparationOperationId, NetworkRequestId, PermissionGrantId,
    PermissionSnapshotId, QuarantineRecordId, ReleaseBundleId, RendererInstanceId, Revision,
    RuntimeInstanceId, SandboxExecutionId, SecurityOperationId, SessionId, UtcTimestamp,
    WorkspaceId, WorkspaceLifecycleOperationId, WorkspacePath,
};
use aura_domain_common::{ToolId, ToolInstanceId, ToolRegistrySnapshotId};
use aura_permission_domain::{
    ApprovalRequestDraft, ApprovalView, CanonicalExecutableRef, ExternalGrantCapability,
    ExternalGrantLifecycle, ExternalGrantRecord, ExternalGrantTargetKind, ExternalGrantTerm,
    ExternalIsolationMode, ExternalProcessLaunchPlan, HostPreparationAction,
    HostPreparationBrokerPlan, HostPreparationOperationState, HostPreparationOperationView,
    HostPreparationOwnership, HostPreparationSummary, NetworkAccessRequirement,
    NetworkEnforcementKind, NetworkInspectionLevel, NetworkRequestAuditRecord,
    NetworkRequestAuditState, ProcessOutputLimits, QuarantineRecord, QuarantineRecordDetails,
    QuarantineReleaseBinding, QuarantineReleaseTarget, QuarantineState, ResolvedScope,
    RuntimeComponentState, SandboxExecutionState, SecurityRuntimeProjection, SecuritySource,
};
use aura_persistence::{
    ArtifactOverwriteJournalBinding, ArtifactOverwriteJournalState, ArtifactWriteRequest,
    HostPreparationOperationTransition, PersistenceCoordinator, PersistenceError,
    PersistenceOptions, PersistenceStartup, WorkspaceLifecycleRecord,
};
use aura_runtime_contract::{BundleNamespace, ReleaseDescriptor, ReleaseIdentity};
use aura_security_contract::{
    ApprovalDecisionKind, ApprovalState, CanonicalPath, InvocationIdentity, NetworkCallerKind,
    PermissionMode, ResolvedCapability,
};
use aura_tool_contract::{
    ToolDefinitionSnapshotRef, ToolRegistrySnapshotRef, ToolSourceContractFingerprintV1,
    ToolSourceKindV1, ToolVersion,
};
use aura_workspace_domain::{
    CatalogFreshness, SessionSummary, WorkspaceLifecycleOperation, WorkspaceLifecycleOperationKind,
    WorkspaceLifecycleState, WorkspaceLifecycleSummary, WorkspaceState, WorkspaceSummary,
};
use tempfile::TempDir;

fn start(directory: &TempDir, runtime: RuntimeInstanceId) -> PersistenceCoordinator {
    let options = PersistenceOptions::new(
        directory.path().join("data/aura.db"),
        directory.path().join("data/artifact-objects"),
        runtime,
    );
    match PersistenceCoordinator::start(options).expect("start persistence") {
        PersistenceStartup::Ready(coordinator) => coordinator,
        PersistenceStartup::RecoveryRequired(recovery) => {
            panic!("unexpected recovery requirement: {}", recovery.reason)
        }
    }
}

fn seed(coordinator: &PersistenceCoordinator) -> (WorkspaceId, SessionId) {
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    coordinator
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: "Security".to_owned(),
                selected_root: r"C:\Aura Tests\Security".to_owned(),
                canonical_root: r"C:\Aura Tests\Security".to_owned(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id,
                workspace_id,
                title: "Permission".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("seed workspace/session");
    coordinator
        .ensure_session_permission_state(
            session_id,
            workspace_id,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("permission state");
    (workspace_id, session_id)
}

#[test]
fn mode_setter_waits_for_current_authority_use_then_invalidates_its_revision() {
    let root = TempDir::new().expect("isolated authority");
    let store = start(&root, RuntimeInstanceId::new());
    let (_, session) = seed(&store);
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    let writer = store.clone();
    let thread = store
        .with_session_permission_authority(session, |state| {
            assert_eq!(state.mode, PermissionMode::ReadOnly);
            let revision = state.revision;
            let thread = std::thread::spawn(move || {
                started_tx.send(()).expect("started");
                let updated = writer
                    .set_session_permission_mode(
                        session,
                        revision,
                        PermissionMode::FullAccess,
                        CorrelationId::new(),
                    )
                    .expect("setter");
                done_tx.send(updated).expect("committed");
            });
            started_rx.recv().expect("writer launched");
            assert!(
                done_rx.recv_timeout(std::time::Duration::from_millis(30)).is_err(),
                "UI cannot commit between authority read and reservation"
            );
            assert_eq!(
                store.session_permission_state(session).expect("same row").revision,
                revision
            );
            thread
        })
        .expect("authority");
    thread.join().expect("setter after barrier release");
    let updated = done_rx.recv().expect("updated authority");
    assert_eq!(updated.mode, PermissionMode::FullAccess);
    assert_eq!(updated.revision, 2);
}

fn built_in_tool_source() -> SecuritySource {
    SecuritySource::Tool {
        tool: Box::new(
            aura_permission_domain::ToolSecuritySourceRefV1::new(
                ToolDefinitionSnapshotRef {
                    tool_id: ToolId::new(),
                    tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
                    registry_snapshot_ref: ToolRegistrySnapshotRef {
                        tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                        registry_revision: Revision::INITIAL,
                        content_hash: CanonicalHashV1::sha256(b"p04-security-registry"),
                    },
                },
                ToolInstanceId::new(),
                ToolSourceKindV1::BuiltIn,
                CanonicalHashV1::sha256(b"p04-security-definition"),
                ToolSourceContractFingerprintV1::BuiltIn {
                    signed_builtin_tool_contract_fingerprint:
                        aura_tool_contract::SignedBuiltinToolContractFingerprintV1::new(
                            CanonicalHashV1::sha256(b"p04-security-source"),
                        ),
                },
            )
            .expect("built-in source"),
        ),
    }
}

fn draft(
    coordinator: &PersistenceCoordinator,
    workspace_id: WorkspaceId,
    session_id: SessionId,
) -> ApprovalRequestDraft {
    let snapshot = coordinator
        .create_permission_snapshot(session_id, None, CorrelationId::new())
        .expect("permission snapshot");
    ApprovalRequestDraft {
        identity: InvocationIdentity::runtime_user(workspace_id, session_id, CorrelationId::new()),
        operation_ids: vec![SecurityOperationId::new()],
        operation_fingerprints: vec![aura_domain_common::ContentHash::blake3(b"operation")],
        title: "Write file".to_owned(),
        model_purpose: None,
        actual_operation_summary: "write src/lib.rs".to_owned(),
        source_summary: "built-in fs tool".to_owned(),
        source: built_in_tool_source(),
        source_fingerprint: None,
        capabilities: vec![ResolvedCapability::WorkspaceFileWrite],
        scopes: vec![ResolvedScope::WorkspaceRoot { workspace_id }],
        risk_flags: Vec::new(),
        allowed_decisions: vec![ApprovalDecisionKind::AllowSession, ApprovalDecisionKind::Deny],
        permission_snapshot_id: snapshot.id,
        release_descriptor: None,
    }
}

fn external_grant_record(
    id: ExternalGrantId,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    permission_grant_id: Option<PermissionGrantId>,
    path: &str,
) -> ExternalGrantRecord {
    let now = UtcTimestamp::now();
    ExternalGrantRecord {
        id,
        session_id,
        workspace_id,
        permission_grant_id,
        selected_root: CanonicalPath {
            display: path.to_owned(),
            utf16: path.encode_utf16().collect(),
        },
        canonical_root: CanonicalPath {
            display: path.to_owned(),
            utf16: path.encode_utf16().collect(),
        },
        root_identity: FileIdentity {
            volume_serial_number: "1".to_owned(),
            file_id: format!(
                "{:032x}",
                id.to_string().bytes().fold(0_u128, |value, byte| {
                    value.wrapping_mul(257).wrapping_add(u128::from(byte))
                })
            ),
        },
        target_kind: ExternalGrantTargetKind::File,
        capabilities: vec![ExternalGrantCapability::Read],
        recursive_scope: false,
        lifecycle: ExternalGrantLifecycle::Active,
        revalidation_revision: 1,
        created_at: now,
        updated_at: now,
        suspended_at: None,
        revoked_at: None,
        expires_at: None,
        turn_task_id: None,
        term: ExternalGrantTerm::Session,
    }
}

#[test]
fn approval_has_one_terminal_decision_and_rejects_stale_or_duplicate_resolution() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);
    let request = coordinator
        .create_approval_request(
            draft(&coordinator, workspace_id, session_id),
            CorrelationId::new(),
        )
        .expect("approval request");

    assert!(matches!(
        coordinator.resolve_approval(
            request.id,
            request.request_revision + 1,
            ApprovalDecisionKind::AllowSession,
            CorrelationId::new(),
        ),
        Err(PersistenceError::ApprovalStale)
    ));
    let committed = coordinator
        .resolve_approval(
            request.id,
            request.request_revision,
            ApprovalDecisionKind::AllowSession,
            CorrelationId::new(),
        )
        .expect("resolve approval");
    assert_eq!(committed.resolution.state, ApprovalState::Approved);
    assert_eq!(committed.resolution.grant_ids.len(), 1);
    assert!(matches!(
        coordinator.resolve_approval(
            request.id,
            request.request_revision,
            ApprovalDecisionKind::Deny,
            CorrelationId::new(),
        ),
        Err(PersistenceError::ApprovalAlreadyResolved)
    ));
    assert!(coordinator.pending_approvals().expect("pending approvals").is_empty());
    assert_eq!(
        coordinator
            .session_permission_state(session_id)
            .expect("permission projection")
            .active_grant_count,
        1
    );
}

#[test]
fn core_restart_cancels_pending_approval_and_revokes_network_side_effect_state() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);
    let request = coordinator
        .create_approval_request(
            draft(&coordinator, workspace_id, session_id),
            CorrelationId::new(),
        )
        .expect("approval request");
    let report = coordinator
        .reconcile_security_after_core_restart(CorrelationId::new())
        .expect("restart reconciliation");
    assert_eq!(report.cancelled_approvals, 1);
    let cancelled = coordinator.approval_details(request.id).expect("cancelled request");
    assert_eq!(cancelled.state, ApprovalState::Cancelled);
    assert_eq!(cancelled.invalidation_reason.as_deref(), Some("core_restarted"));
}

#[test]
fn core_restart_interrupts_quarantine_release_journal_without_replaying_it() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);
    let bytes = b"MZ executable fixture".to_vec();
    let artifact = coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Quarantine,
            media_type: "application/octet-stream".to_owned(),
            access: ArtifactAccess::USER_READ.union(ArtifactAccess::EXPORT),
            owner: ArtifactOwnerRef::Session { session_id },
            bytes: bytes.clone(),
        })
        .expect("quarantine artifact");
    let quarantine_id = QuarantineRecordId::new();
    let now = UtcTimestamp::now();
    coordinator
        .create_quarantine_record(
            QuarantineRecordDetails {
                record: QuarantineRecord {
                    id: quarantine_id,
                    original_name: "tool.exe".to_owned(),
                    source_url: "https://example.invalid/tool.exe".to_owned(),
                    media_type: Some("application/octet-stream".to_owned()),
                    content_disposition: None,
                    detected_kinds: vec!["windows_pe_extension".to_owned()],
                    content_hash: artifact.content_hash.clone(),
                    byte_length: u64::try_from(bytes.len()).expect("length"),
                    state: QuarantineState::Quarantined,
                    created_at: now,
                    resolved_at: None,
                },
                artifact: Some(artifact),
                storage_path: None,
            },
            CorrelationId::new(),
        )
        .expect("quarantine record");
    let target_path = WorkspacePath::from_str("bin/tool.exe").expect("workspace path");
    let binding = QuarantineReleaseBinding {
        quarantine_id,
        content_hash: ContentHash::blake3(&bytes),
        workspace_id,
        session_id,
        target: QuarantineReleaseTarget::Workspace { path: target_path.clone() },
        destination_parent_identity: FileIdentity {
            volume_serial_number: "1".to_owned(),
            file_id: "00".repeat(16),
        },
        final_destination: CanonicalPath {
            display: r"C:\Aura Tests\Security\bin\tool.exe".to_owned(),
            utf16: r"C:\Aura Tests\Security\bin\tool.exe".encode_utf16().collect(),
        },
    };
    let snapshot = coordinator
        .create_permission_snapshot(session_id, None, CorrelationId::new())
        .expect("snapshot");
    let approval = coordinator
        .create_approval_request(
            ApprovalRequestDraft {
                identity: InvocationIdentity::runtime_user(
                    workspace_id,
                    session_id,
                    CorrelationId::new(),
                ),
                operation_ids: vec![SecurityOperationId::new()],
                operation_fingerprints: vec![binding.operation_fingerprint().expect("fingerprint")],
                title: "Release executable".to_owned(),
                model_purpose: None,
                actual_operation_summary: "release tool.exe".to_owned(),
                source_summary: "Aura Quarantine".to_owned(),
                source: SecuritySource::UserInterface,
                source_fingerprint: None,
                capabilities: vec![ResolvedCapability::DownloadWindowsExecutable],
                scopes: vec![ResolvedScope::WorkspacePath {
                    workspace_id,
                    canonical_relative_path: target_path,
                }],
                risk_flags: vec![aura_security_contract::OperationRiskFlag::ExecutableArtifact],
                allowed_decisions: vec![ApprovalDecisionKind::AllowOnce],
                permission_snapshot_id: snapshot.id,
                release_descriptor: None,
            },
            CorrelationId::new(),
        )
        .expect("release approval");
    coordinator
        .resolve_approval(
            approval.id,
            approval.request_revision,
            ApprovalDecisionKind::AllowOnce,
            CorrelationId::new(),
        )
        .expect("approve release");
    coordinator
        .consume_quarantine_release_authorization(binding, approval.id, CorrelationId::new())
        .expect("consume authorization");
    coordinator.mark_quarantine_release_applying(quarantine_id).expect("mark applying");
    coordinator
        .reconcile_security_after_core_restart(CorrelationId::new())
        .expect("reconcile restart");
    assert_eq!(
        coordinator
            .quarantine_release_journal_state(quarantine_id)
            .expect("journal state")
            .as_deref(),
        Some("interrupted")
    );
    assert_eq!(
        coordinator.quarantine_details(quarantine_id).expect("quarantine").record.state,
        QuarantineState::Quarantined,
        "recovery must never replay or mark the external release completed"
    );
}

#[test]
fn core_restart_marks_applying_artifact_overwrite_needs_reconcile_without_replay() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);
    let external_grant_id = ExternalGrantId::new();
    coordinator
        .create_external_grant(
            external_grant_record(
                external_grant_id,
                workspace_id,
                session_id,
                None,
                r"C:\Aura Tests\Attached\artifact.bin",
            ),
            CorrelationId::new(),
        )
        .expect("attached external grant");
    let operation_id = SecurityOperationId::new();
    coordinator
        .begin_artifact_overwrite(
            ArtifactOverwriteJournalBinding {
                operation_id,
                workspace_id,
                session_id,
                external_grant_id,
                relative_path: WorkspacePath::from_str("artifact.bin").expect("relative path"),
                final_destination: CanonicalPath {
                    display: r"C:\Aura Tests\Attached\artifact.bin".to_owned(),
                    utf16: r"C:\Aura Tests\Attached\artifact.bin".encode_utf16().collect(),
                },
                backup_file_name: format!(".aura-backup-{operation_id}.tmp"),
                expected_target_identity: FileIdentity {
                    volume_serial_number: "1".to_owned(),
                    file_id: "11".repeat(16),
                },
                expected_content_hash: ContentHash::blake3(b"old"),
                replacement_content_hash: ContentHash::blake3(b"new"),
            },
            CorrelationId::new(),
        )
        .expect("begin overwrite journal");
    coordinator
        .transition_artifact_overwrite(
            operation_id,
            ArtifactOverwriteJournalState::Applying,
            None,
            CorrelationId::new(),
        )
        .expect("mark applying");

    let report = coordinator
        .reconcile_security_after_core_restart(CorrelationId::new())
        .expect("restart reconciliation");
    assert_eq!(report.artifact_overwrites_needing_reconcile, 1);
    assert_eq!(report.artifact_overwrites_interrupted, 0);
    let journal = coordinator.artifact_overwrite_journal(operation_id).expect("journal record");
    assert_eq!(journal.state, ArtifactOverwriteJournalState::NeedsReconcile);
    assert_eq!(journal.error_code.as_deref(), Some("core_restarted_external_state_ambiguous"));
    assert_ne!(journal.state, ArtifactOverwriteJournalState::Completed);
}

#[test]
fn relocate_revokes_path_grants_and_invalidates_pending_path_approvals_atomically() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);

    let approved = coordinator
        .create_approval_request(
            draft(&coordinator, workspace_id, session_id),
            CorrelationId::new(),
        )
        .expect("approved request");
    let resolution = coordinator
        .resolve_approval(
            approved.id,
            approved.request_revision,
            ApprovalDecisionKind::AllowSession,
            CorrelationId::new(),
        )
        .expect("create path grant");
    let grant_id = resolution.resolution.grant_ids[0];
    let pending = coordinator
        .create_approval_request(
            draft(&coordinator, workspace_id, session_id),
            CorrelationId::new(),
        )
        .expect("pending request");

    let external_path = CanonicalPath {
        display: r"C:\Aura Tests\External\shared.txt".to_owned(),
        utf16: r"C:\Aura Tests\External\shared.txt".encode_utf16().collect(),
    };
    let mut external_authorization_draft = draft(&coordinator, workspace_id, session_id);
    external_authorization_draft.capabilities = vec![ResolvedCapability::ExternalFileRead];
    external_authorization_draft.scopes = vec![ResolvedScope::ExternalPath {
        canonical_path: external_path.clone(),
        identity: Some(FileIdentity {
            volume_serial_number: "1".to_owned(),
            file_id: "22".repeat(16),
        }),
        recursive: false,
    }];
    external_authorization_draft.allowed_decisions =
        vec![ApprovalDecisionKind::AllowWorkspace, ApprovalDecisionKind::Deny];
    let external_authorization = coordinator
        .create_approval_request(external_authorization_draft, CorrelationId::new())
        .expect("external authorization approval");
    let external_authorization_grant_id = coordinator
        .resolve_approval(
            external_authorization.id,
            external_authorization.request_revision,
            ApprovalDecisionKind::AllowWorkspace,
            CorrelationId::new(),
        )
        .expect("workspace external authorization")
        .resolution
        .grant_ids[0];

    let rooted_external_grant_id = ExternalGrantId::new();
    coordinator
        .create_external_grant(
            external_grant_record(
                rooted_external_grant_id,
                workspace_id,
                session_id,
                Some(external_authorization_grant_id),
                &external_path.display,
            ),
            CorrelationId::new(),
        )
        .expect("authorization-epoch-bound external grant");
    let attached_external_grant_id = ExternalGrantId::new();
    coordinator
        .create_external_grant(
            external_grant_record(
                attached_external_grant_id,
                workspace_id,
                session_id,
                None,
                r"C:\Aura Tests\Attached\notes.txt",
            ),
            CorrelationId::new(),
        )
        .expect("independent attached external grant");

    let mut external_scoped_draft = draft(&coordinator, workspace_id, session_id);
    external_scoped_draft.capabilities = vec![ResolvedCapability::ExternalFileRead];
    external_scoped_draft.scopes =
        vec![ResolvedScope::ExternalGrant { grant_id: rooted_external_grant_id }];
    external_scoped_draft.allowed_decisions =
        vec![ApprovalDecisionKind::AllowWorkspace, ApprovalDecisionKind::Deny];
    let external_scoped_approval = coordinator
        .create_approval_request(external_scoped_draft, CorrelationId::new())
        .expect("ExternalGrant-scoped approval");
    let external_scoped_permission_grant_id = coordinator
        .resolve_approval(
            external_scoped_approval.id,
            external_scoped_approval.request_revision,
            ApprovalDecisionKind::AllowWorkspace,
            CorrelationId::new(),
        )
        .expect("ExternalGrant-scoped permission grant")
        .resolution
        .grant_ids[0];
    let mut pending_external_draft = draft(&coordinator, workspace_id, session_id);
    pending_external_draft.capabilities = vec![ResolvedCapability::ExternalFileRead];
    pending_external_draft.scopes =
        vec![ResolvedScope::ExternalGrant { grant_id: rooted_external_grant_id }];
    let pending_external = coordinator
        .create_approval_request(pending_external_draft, CorrelationId::new())
        .expect("pending ExternalGrant-scoped approval");

    let operation_id = WorkspaceLifecycleOperationId::new();
    let operation = WorkspaceLifecycleOperation::Relocate {
        selected_target_root: r"C:\Aura Tests\Security Relocated".to_owned(),
        confirmed_same_logical_project: true,
    };
    let committing = WorkspaceLifecycleRecord {
        summary: WorkspaceLifecycleSummary {
            approval_follow_up: Vec::new(),
            operation_id,
            workspace_id,
            renderer_instance_id: RendererInstanceId::new(),
            operation: WorkspaceLifecycleOperationKind::Relocate,
            lifecycle_revision: Revision::INITIAL,
            state: WorkspaceLifecycleState::Committing,
            ack_deadline_at: None,
            preflight: None,
            error_code: None,
        },
        operation: operation.clone(),
    };
    coordinator.create_workspace_lifecycle_operation(committing).expect("relocate journal");
    let mut workspace = coordinator.workspace(workspace_id).expect("workspace");
    workspace.selected_root = r"C:\Aura Tests\Security Relocated".to_owned();
    workspace.canonical_root = workspace.selected_root.clone();
    workspace.revision = Revision::new(2);
    workspace.authorization_epoch = Revision::new(2);
    coordinator
        .commit_workspace_lifecycle(
            workspace,
            WorkspaceLifecycleRecord {
                summary: WorkspaceLifecycleSummary {
                    approval_follow_up: Vec::new(),
                    operation_id,
                    workspace_id,
                    renderer_instance_id: RendererInstanceId::new(),
                    operation: WorkspaceLifecycleOperationKind::Relocate,
                    lifecycle_revision: Revision::new(2),
                    state: WorkspaceLifecycleState::Completed,
                    ack_deadline_at: None,
                    preflight: None,
                    error_code: None,
                },
                operation,
            },
            "workspace.relocated",
            CorrelationId::new(),
            true,
        )
        .expect("commit relocate");

    assert!(coordinator.permission_grant(grant_id).expect("grant").revoked_at.is_some());
    assert_eq!(
        coordinator
            .external_grant(rooted_external_grant_id)
            .expect("root-bound external grant")
            .lifecycle,
        ExternalGrantLifecycle::Revoked
    );
    assert_eq!(
        coordinator
            .external_grant(attached_external_grant_id)
            .expect("attached external grant")
            .lifecycle,
        ExternalGrantLifecycle::Active,
        "independent attached external files must survive Workspace relocation"
    );
    assert!(
        coordinator
            .permission_grant(external_scoped_permission_grant_id)
            .expect("ExternalGrant-scoped permission grant")
            .revoked_at
            .is_some(),
        "PermissionGrants referring to a revoked ExternalGrantId must be revoked atomically"
    );
    assert!(
        coordinator
            .permission_grant(external_authorization_grant_id)
            .expect("independent external-path authorization")
            .revoked_at
            .is_none(),
        "independent external-path authorization must not be over-revoked"
    );
    let invalidated = coordinator.approval_details(pending.id).expect("pending approval");
    assert_eq!(invalidated.state, ApprovalState::Invalidated);
    assert_eq!(
        invalidated.invalidation_reason.as_deref(),
        Some("workspace_relocated_authorization_epoch_changed")
    );
    let invalidated_external = coordinator
        .approval_details(pending_external.id)
        .expect("pending ExternalGrant-scoped approval");
    assert_eq!(invalidated_external.state, ApprovalState::Invalidated);
    assert_eq!(
        invalidated_external.invalidation_reason.as_deref(),
        Some("workspace_relocated_authorization_epoch_changed")
    );
}

#[test]
fn host_preparation_decision_is_revision_checked_and_denied_is_terminal() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let runtime = security_runtime();
    let probed = coordinator
        .record_host_preparation_probe(runtime.clone(), CorrelationId::new())
        .expect("record probe");
    let now = UtcTimestamp::now();
    let operation_id = HostPreparationOperationId::new();
    let runtime_instance_id = RuntimeInstanceId::new();
    let release_bundle_id = ReleaseBundleId::new();
    let component_manifest_hash =
        ContentHash::new(ContentHashAlgorithm::Sha256, "11".repeat(32)).expect("manifest hash");
    let bundle_namespace: BundleNamespace =
        "aura.desktop.test.persistence".parse().expect("test namespace");
    let release_descriptor = ReleaseDescriptor {
        bundle_namespace,
        identity: ReleaseIdentity { release_bundle_id, component_manifest_hash },
    };
    let expires_at = UtcTimestamp::from_value(now.value() + time::Duration::minutes(3));
    let plan_hash = ContentHash::blake3(b"host preparation plan");
    let plan = HostPreparationBrokerPlan {
        operation_id,
        action: HostPreparationAction::Prepare,
        host_preparation_revision: probed.status.revision,
        runtime_instance_id,
        release_descriptor: release_descriptor.clone(),
        plan_nonce_hash: ContentHash::blake3(b"nonce"),
        expires_at,
        system_drive_root: CanonicalPath {
            display: r"C:\".to_owned(),
            utf16: r"C:\".encode_utf16().collect(),
        },
        ownership_before: HostPreparationOwnership::default(),
        add_all_application_packages_ace: true,
        add_all_restricted_application_packages_ace: false,
        update_null_device_security: false,
        plan_hash: plan_hash.clone(),
    };
    let operation = HostPreparationOperationView {
        operation_id,
        operation_revision: 1,
        action: HostPreparationAction::Prepare,
        state: HostPreparationOperationState::Prepared,
        host_preparation_revision: probed.status.revision,
        runtime_instance_id,
        release_descriptor: release_descriptor.clone(),
        plan_hash,
        expires_at,
        requires_elevation: true,
        add_all_application_packages_ace: true,
        add_all_restricted_application_packages_ace: false,
        update_null_device_security: false,
        created_at: now,
        updated_at: now,
        error_code: None,
    };
    let prepared = coordinator
        .begin_host_preparation_operation(
            probed.status.revision,
            operation,
            plan,
            ApprovalView {
                session_id: None,
                id: ApprovalRequestId::new(),
                title: "配置 Host Preparation".to_owned(),
                model_purpose: None,
                actual_operation_summary: "应用冻结的兼容配置".to_owned(),
                source_summary: "Aura Core Host Preparation".to_owned(),
                risk_flags: vec![
                    aura_security_contract::OperationRiskFlag::SystemConfigurationMutation,
                    aura_security_contract::OperationRiskFlag::Elevation,
                ],
                allowed_decisions: vec![
                    ApprovalDecisionKind::AllowOnce,
                    ApprovalDecisionKind::Deny,
                ],
                request_revision: 1,
                created_at: now,
                release_descriptor: Some(release_descriptor),
            },
            runtime.clone(),
            CorrelationId::new(),
        )
        .expect("begin Host Preparation");
    assert_eq!(prepared.status.revision, probed.status.revision + 1);
    let journal = coordinator
        .host_preparation_operation(operation_id)
        .expect("load Host Preparation approval journal");
    let approval = journal.approval.expect("persisted Main-only ApprovalView");
    assert_eq!(
        approval.allowed_decisions,
        [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    );

    assert!(matches!(
        coordinator.transition_host_preparation_operation(HostPreparationOperationTransition {
            operation_id,
            expected_state: HostPreparationOperationState::Prepared,
            expected_operation_revision: 2,
            expected_host_preparation_revision: prepared.status.revision,
            next_state: HostPreparationOperationState::Denied,
            ownership_after: None,
            error_code: Some("sandbox.host_preparation_denied".to_owned()),
            runtime: runtime.clone(),
            correlation_id: CorrelationId::new(),
        },),
        Err(PersistenceError::HostPreparationStateConflict)
    ));

    let denied = coordinator
        .transition_host_preparation_operation(HostPreparationOperationTransition {
            operation_id,
            expected_state: HostPreparationOperationState::Prepared,
            expected_operation_revision: 1,
            expected_host_preparation_revision: prepared.status.revision,
            next_state: HostPreparationOperationState::Denied,
            ownership_after: None,
            error_code: Some("sandbox.host_preparation_denied".to_owned()),
            runtime,
            correlation_id: CorrelationId::new(),
        })
        .expect("deny Host Preparation");
    assert!(denied.status.active_operation.is_none());
    assert!(
        coordinator
            .pending_host_preparation_operations()
            .expect("pending Host Preparation")
            .is_empty()
    );
    let journal =
        coordinator.host_preparation_operation(operation_id).expect("Host Preparation journal");
    assert_eq!(journal.operation.state, HostPreparationOperationState::Denied);
    assert_eq!(journal.operation.operation_revision, 2);
}

#[test]
fn network_request_audit_records_only_core_control_plane_enforcement() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);
    let identity = InvocationIdentity::runtime_user(workspace_id, session_id, CorrelationId::new());
    let now = UtcTimestamp::now();
    let record = NetworkRequestAuditRecord {
        request_id: NetworkRequestId::new(),
        identity: identity.clone(),
        caller: NetworkCallerKind::AuraTool,
        network_enforcement: NetworkEnforcementKind::CoreControlPlane,
        inspection_level: NetworkInspectionLevel::StructuredHttp,
        method: "GET".to_owned(),
        origin: None,
        destination_classes: Vec::new(),
        data_classes: Vec::new(),
        upload_bytes: 0,
        download_bytes: 0,
        state: NetworkRequestAuditState::Started,
        error_code: None,
        started_at: now,
        finished_at: None,
    };
    let started = coordinator
        .start_network_request_audit(record, CorrelationId::new())
        .expect("start Core network audit");
    let finished = coordinator
        .finish_network_request_audit(
            started.request_id,
            NetworkRequestAuditState::Finished,
            42,
            None,
            CorrelationId::new(),
        )
        .expect("finish Core network audit");
    assert_eq!(finished.network_enforcement, NetworkEnforcementKind::CoreControlPlane);
    assert_eq!(finished.download_bytes, 42);

    let rejected = NetworkRequestAuditRecord {
        request_id: NetworkRequestId::new(),
        identity,
        caller: NetworkCallerKind::SandboxProcess,
        network_enforcement: NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
        inspection_level: NetworkInspectionLevel::ObservedNativeOnly,
        method: "GET".to_owned(),
        origin: None,
        destination_classes: Vec::new(),
        data_classes: Vec::new(),
        upload_bytes: 0,
        download_bytes: 0,
        state: NetworkRequestAuditState::Started,
        error_code: None,
        started_at: now,
        finished_at: None,
    };
    assert!(matches!(
        coordinator.start_network_request_audit(rejected, CorrelationId::new()),
        Err(PersistenceError::InvalidRequest(_))
    ));
}

#[test]
fn external_networked_execution_audit_is_durable_and_cannot_claim_core_enforcement() {
    let directory = tempfile::tempdir().expect("temp root");
    let coordinator = start(&directory, RuntimeInstanceId::new());
    let (workspace_id, session_id) = seed(&coordinator);
    let identity = InvocationIdentity::runtime_user(workspace_id, session_id, CorrelationId::new());
    let mut plan = ExternalProcessLaunchPlan {
        execution_id: SandboxExecutionId::new(),
        identity,
        permission_snapshot_id: PermissionSnapshotId::new(),
        source: SecuritySource::McpServer { server_id: "audit-fixture".to_owned() },
        source_fingerprint: ContentHash::blake3(b"audit external source"),
        isolation: ExternalIsolationMode::ManagedUserNetworked,
        network_access: NetworkAccessRequirement::Required,
        network_enforcement: NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
        executable: CanonicalExecutableRef {
            path: CanonicalPath {
                display: r"C:\Windows\System32\cmd.exe".to_owned(),
                utf16: r"C:\Windows\System32\cmd.exe".encode_utf16().collect(),
            },
            identity: FileIdentity {
                volume_serial_number: "1".to_owned(),
                file_id: "00".repeat(16),
            },
            content_hash: ContentHash::blake3(b"cmd fixture"),
        },
        args: Vec::new(),
        working_directory: CanonicalPath {
            display: r"C:\Windows".to_owned(),
            utf16: r"C:\Windows".encode_utf16().collect(),
        },
        environment: Vec::new(),
        timeout_ms: 1_000,
        output_limits: ProcessOutputLimits {
            stdout_bytes: 1_024,
            stderr_bytes: 1_024,
            combined_bytes: 2_048,
        },
        cancellation_id: CancellationId::new(),
        plan_hash: ContentHash::blake3(b"external plan"),
    };
    let before = coordinator.event_count().expect("event count before audit");
    coordinator
        .record_external_process_execution_audit(
            &plan,
            SandboxExecutionState::Planned,
            None,
            None,
            None,
            CorrelationId::new(),
        )
        .expect("persist external execution audit");
    assert_eq!(coordinator.event_count().expect("event count after audit"), before + 1);

    plan.network_enforcement = NetworkEnforcementKind::CoreControlPlane;
    assert!(matches!(
        coordinator.record_external_process_execution_audit(
            &plan,
            SandboxExecutionState::Planned,
            None,
            None,
            None,
            CorrelationId::new(),
        ),
        Err(PersistenceError::InvalidRequest(_))
    ));
}

fn security_runtime() -> SecurityRuntimeProjection {
    SecurityRuntimeProjection {
        sandbox_available: false,
        host_preparation: HostPreparationSummary::default(),
        sandbox_host_state: RuntimeComponentState::NotStarted,
        egress_proxy_state: RuntimeComponentState::NotStarted,
        elevated_helper_state: RuntimeComponentState::NotStarted,
        quarantine_count: 0,
    }
}
