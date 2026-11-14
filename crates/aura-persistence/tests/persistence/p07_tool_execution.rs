#[path = "p07_tool_execution/projections.rs"]
mod projections;
use aura_artifact_contract::{
    ArtifactAccess, ArtifactAccessLinkV1, ArtifactAccessSubjectV1, ArtifactOwnerRef, ArtifactScope,
    ToolArtifactOwnerRef,
};
use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::{CanonicalBlake3HashV1, CanonicalHashV1};
use aura_domain_common::{
    AgentRunId, CausationId, CommandInvocationId, CorrelationId, ErrorCategory, ErrorEnvelope,
    EventId, ModelStepId, Revision, RuntimeInstanceId, SessionId, SessionSequence, TurnTaskId,
    UtcTimestamp, WorkspaceId,
};
use aura_domain_common::{
    ToolBatchId, ToolCallId, ToolConformanceRunId, ToolExecutionAttemptId, ToolId, ToolInstanceId,
    ToolPackageId, ToolRegistrySnapshotId,
};
use aura_execution_contract::SideEffectStateV1;
use aura_permission_domain::{
    AuthorizedToolSandboxExecutionV1, CanonicalExecutableRef, NativeSandboxMode,
    NetworkAccessRequirement, NetworkEnforcementKind, NonSecretSandboxRuntimeFactsV1,
    NonSecretToolRuntimeFactsV1, ProcessOutputLimits, ResolvedScope, SandboxExecutionPlan,
    ToolPreparedRuntimeKindV1, ToolSandboxDispatchBindingV1, ToolSecuritySourceRefV1,
    W7ToolOperationFingerprintPreimageV1, WindowsOsString,
};
use aura_persistence::{
    ArtifactWriteDraftV1, ArtifactWriteRequest,
    AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1, ContextSourceEventCommitRequest,
    EventActor, PersistenceCoordinator, PersistenceError, PersistenceFaultPoint,
    PersistenceOptions, PersistenceStartup, PersistentEventDraft,
    StoredToolHistoryAtomicGroupAssociationV1, ToolActivityPageReadErrorV1,
    ToolBatchAdmissionCommitRequest, ToolExecutionAttemptCancellationRequest,
    ToolExecutionAttemptDispatchPreparedCommitRequest, ToolExecutionAttemptFinalizationRequest,
    ToolExecutionAttemptProgressRequest, ToolExecutionAttemptRetryableFailureRequest,
    ToolExecutionAttemptRuntimeStartedRequest, ToolExecutionAttemptTerminalFact,
    ToolHistoryAtomicGroupAssociationRequestV1, VerifiedToolPackageInstallRequest,
    tool_activity_visibility_context_hash_v1,
};
use aura_security_contract::{
    CanonicalPath, InvocationActor, InvocationIdentity, PermissionAllowAuthorizationBasisV1,
    PermissionMode, ResolvedCapability,
};
use aura_tool_contract::{
    CoreInvocationIdentityV1, EffectiveToolBindingSnapshot, NormalizedToolCallInputStateV1,
    NormalizedToolCallV1, PackageFingerprintV1, PackageKey, PackageRelativePath,
    PackageSecurityFingerprintV1, ToolAudienceV1, ToolBatchResultV1, ToolBatchStatusV1,
    ToolCallStatusV1, ToolCancellationReasonV1, ToolCanonicalName, ToolDefinitionSnapshotRef,
    ToolExecutionAttemptOutcomeRefV1, ToolExecutionAttemptRecordV1, ToolExecutionAttemptStatusV1,
    ToolExecutionPlanCallV1, ToolExecutionPlanV1, ToolExposureV1, ToolInputSchemaV1, ToolKey,
    ToolLoadStateV1, ToolNotExecutedReasonV1, ToolPurposeTextV1, ToolRegistryCatalogEntryV1,
    ToolRegistryScopeV1, ToolRegistrySnapshotV1, ToolResultContractV1, ToolResultOutcomeV1,
    ToolResultV1, ToolRuntimeDescriptorV1, ToolRuntimeErrorCode, ToolSchemaSnapshotV1,
    ToolSourceKindV1, ToolVersion,
};
use aura_tool_domain::{
    ToolDefinitionRecordV1, ToolExecutionSemanticsV1, ToolInstanceOverridesV1,
    ToolInstanceRecordV1, ToolPackageLifecycleV1, ToolPackageRecordV1, ToolPackageVersionRecordV1,
    ToolRuntimeHealthV1,
};
use aura_tool_projection::{
    SessionToolActivityProjection, ToolAttemptProgressView, ToolBatchActivitySummaryView,
    ToolBatchProjection, ToolCallActivitySummaryView, ToolCallProjection,
    ToolExecutionAttemptProjection, ToolProjectionErrorView, ToolResultOutcomeSummaryView,
};
use aura_tool_registry::{ToolSearchCorpusV1, ToolSearchDocumentV1};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use projections::{call_projection, call_summary, normalized_call};
use rusqlite::{Connection, params};
use serde_json::json;
use tempfile::TempDir;

fn timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:00Z".parse().expect("timestamp")
}

fn dispatch_timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:01Z".parse().expect("dispatch timestamp")
}

fn runtime_timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:02Z".parse().expect("runtime timestamp")
}

fn progress_timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:03Z".parse().expect("progress timestamp")
}

fn cancellation_timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:04Z".parse().expect("cancellation timestamp")
}

fn terminal_timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:05Z".parse().expect("terminal timestamp")
}

fn retry_timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:06Z".parse().expect("retry timestamp")
}

fn retry_dispatch_timestamp() -> UtcTimestamp {
    "2026-09-03T00:00:07Z".parse().expect("retry dispatch timestamp")
}

struct AdmissionFixture {
    _temp: TempDir,
    database_path: std::path::PathBuf,
    coordinator: PersistenceCoordinator,
    request: ToolBatchAdmissionCommitRequest,
}

impl AdmissionFixture {
    fn new(seed: u8) -> Self {
        Self::new_with_retry_policy(seed, false)
    }

    fn new_retryable(seed: u8) -> Self {
        Self::new_with_retry_policy(seed, true)
    }

    fn new_with_retry_policy(seed: u8, automatic_retry_allowed: bool) -> Self {
        let temp = tempfile::tempdir().expect("temporary directory");
        let database_path = temp.path().join("aura.db");
        let startup = PersistenceCoordinator::start(PersistenceOptions::new(
            database_path.clone(),
            temp.path().join("artifacts"),
            RuntimeInstanceId::from_bytes([seed; 16]),
        ))
        .expect("persistence startup");
        let PersistenceStartup::Ready(coordinator) = startup else {
            panic!("fixture database must be ready");
        };

        let workspace_id = WorkspaceId::from_bytes([seed.wrapping_add(1); 16]);
        let session_id = SessionId::from_bytes([seed.wrapping_add(2); 16]);
        let turn_task_id = TurnTaskId::from_bytes([seed.wrapping_add(3); 16]);
        let agent_run_id = AgentRunId::from_bytes([seed.wrapping_add(4); 16]);
        seed_scope(
            &coordinator,
            &database_path,
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
            seed,
        );

        let install = install_request(seed, automatic_retry_allowed);
        let definition = install.definitions[0].clone();
        let instance = install.instances[0].clone();
        let package_version = install.package_version.clone();
        coordinator
            .seed_verified_tool_package_for_test(install)
            .expect("install exact tool authority");

        let registry = registry_snapshot(
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
            &definition,
            &instance,
            seed,
        );
        let search_document = ToolSearchDocumentV1 {
            tool_id: definition.tool_id,
            tool_version: definition.tool_version.clone(),
            definition_fingerprint: definition.definition_fingerprint.clone(),
            model_description: definition.model_description.clone(),
        };
        coordinator
            .store_tool_registry_search_corpus(
                ToolSearchCorpusV1::new(registry.clone(), vec![search_document])
                    .expect("registry corpus"),
            )
            .expect("persist registry snapshot");
        coordinator
            .seed_tool_registry_runtime_authority_for_test(
                registry.snapshot_ref.clone(),
                definition.clone(),
                instance.clone(),
                package_version,
            )
            .expect("persist exact runtime payload authority");

        let invocation = CoreInvocationIdentityV1 {
            command_invocation_id: CommandInvocationId::from_bytes([seed.wrapping_add(10); 16]),
            session_id,
            turn_task_id,
            agent_run_id,
            model_step_id: ModelStepId::from_bytes([seed.wrapping_add(11); 16]),
        };
        let executable_call = normalized_call(
            ToolCallId::from_bytes([seed.wrapping_add(12); 16]),
            1,
            definition.tool_id,
            definition.tool_version.clone(),
            registry.snapshot_ref.clone(),
            "Read the fixture value",
            seed,
        );
        let unresolved_call = normalized_call(
            ToolCallId::from_bytes([seed.wrapping_add(13); 16]),
            2,
            ToolId::from_bytes([seed.wrapping_add(14); 16]),
            ToolVersion::new(None, Revision::INITIAL).expect("unresolved tool version"),
            registry.snapshot_ref.clone(),
            "Use a tool that is no longer registered",
            seed.wrapping_add(1),
        );
        let batch = aura_tool_contract::ToolBatchRequestV1::new(
            ToolBatchId::from_bytes([seed.wrapping_add(15); 16]),
            invocation,
            Revision::INITIAL,
            CanonicalHashV1::sha256(&[seed, 20]),
            registry.snapshot_ref.clone(),
            vec![executable_call.clone(), unresolved_call.clone()],
        )
        .expect("batch");
        let execution_plan = ToolExecutionPlanV1::new(
            batch.tool_batch_id,
            registry.snapshot_ref.clone(),
            Revision::INITIAL,
            CanonicalHashV1::sha256(&[seed, 21]),
            vec![ToolExecutionPlanCallV1 {
                tool_call_id: executable_call.tool_call_id,
                ordinal: executable_call.ordinal,
                dependency_tool_call_ids: Vec::new(),
                execution_atomic_group: None,
                resolved_resource_claims: Vec::new(),
                timeout_ms: 120_000,
                max_retries: 3,
                local_concurrency_limit: None,
                entity_revision: definition.entity_revision,
            }],
        )
        .expect("execution plan");
        let not_executed = ToolResultV1 {
            tool_call_id: unresolved_call.tool_call_id,
            outcome: ToolResultOutcomeV1::NotExecuted {
                reason: ToolNotExecutedReasonV1::ToolNotFound,
                error: None,
            },
            side_effect_state: SideEffectStateV1::NotStarted,
            attempts: Vec::new(),
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: None,
            started_at: None,
            completed_at: timestamp(),
        };
        let call_summaries = vec![
            call_summary(&batch, &executable_call, &definition.display_name, None),
            call_summary(
                &batch,
                &unresolved_call,
                "Unavailable fixture tool",
                Some(ToolNotExecutedReasonV1::ToolNotFound),
            ),
        ];
        let call_projections = vec![
            call_projection(&batch, &executable_call, None),
            call_projection(&batch, &unresolved_call, Some(ToolNotExecutedReasonV1::ToolNotFound)),
        ];
        let batch_projection = ToolBatchProjection {
            tool_batch_id: batch.tool_batch_id,
            batch_content_hash: batch.batch_content_hash.clone(),
            session_id,
            turn_task_id,
            agent_run_id,
            model_step_id: batch.invocation_identity.model_step_id,
            registry_snapshot_ref: registry.snapshot_ref,
            control_state_revision: batch.p08_state_revision,
            control_state_hash: batch.p08_state_hash.clone(),
            status: ToolBatchStatusV1::Admitted,
            calls: call_summaries.clone(),
            started_at: None,
            completed_at: None,
            safe_error: None,
            can_cancel: true,
        };
        let session_activity_projection = SessionToolActivityProjection {
            session_id,
            active_batches: vec![ToolBatchActivitySummaryView {
                tool_batch_id: batch.tool_batch_id,
                status: ToolBatchStatusV1::Admitted,
                call_count: 2,
                completed_call_count: 1,
                started_at: None,
                completed_at: None,
                can_cancel: true,
            }],
            recent_calls: call_summaries,
            older_activity_cursor: None,
        };
        let request = ToolBatchAdmissionCommitRequest {
            batch,
            execution_plan,
            not_executed_results: vec![not_executed],
            batch_projection,
            call_projections,
            session_activity_projection,
            admitted_at: timestamp(),
            correlation_id: CorrelationId::from_bytes([seed.wrapping_add(16); 16]),
        };
        Self { _temp: temp, database_path, coordinator, request }
    }
}

#[test]
fn admission_commits_batch_calls_not_executed_result_events_and_projections_atomically() {
    let fixture = AdmissionFixture::new(30);
    let batch_id = fixture.request.batch.tool_batch_id;
    let unresolved_call_id = fixture.request.batch.calls[1].tool_call_id;

    let commit = fixture
        .coordinator
        .commit_tool_batch_admission(fixture.request)
        .expect("atomic admission commit");

    assert_eq!(commit.event_ids.len(), 2);
    assert_eq!(commit.projection_changes.len(), 4);
    assert_eq!(commit.projection_outbox_ids.len(), 4);
    let connection = Connection::open(&fixture.database_path).expect("open committed database");
    (|| -> rusqlite::Result<()> {
        assert_eq!(table_count(&connection, "tool_batches")?, 1);
        assert_eq!(table_count(&connection, "tool_calls")?, 2);
        assert_eq!(table_count(&connection, "tool_execution_attempts")?, 0);
        assert_eq!(table_count(&connection, "tool_results")?, 1);
        let unresolved_status = connection.query_row(
            "SELECT status FROM tool_calls WHERE tool_call_id=?1",
            [unresolved_call_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        assert_eq!(unresolved_status, "finalized");
        let batch_status = connection.query_row(
            "SELECT status FROM tool_batches WHERE tool_batch_id=?1",
            [batch_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        assert_eq!(batch_status, "admitted");
        let event_types = connection
            .prepare(
                "SELECT event_type FROM events
                     WHERE event_id IN (?1, ?2) ORDER BY global_sequence",
            )?
            .query_map(
                params![commit.event_ids[0].to_string(), commit.event_ids[1].to_string()],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(event_types, vec!["ToolBatchAdmitted", "ToolResultFinalized"]);
        Ok(())
    })()
    .expect("read committed authority");
}

#[test]
fn execution_entity_state_reports_exact_cas_revisions_and_attempt_sequence() {
    let fixture = AdmissionFixture::new(31);
    let admission = fixture.request.clone();
    let batch_id = admission.batch.tool_batch_id;
    let call_id = admission.execution_plan.calls[0].tool_call_id;
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");

    let admitted = fixture
        .coordinator
        .load_tool_execution_entity_state_v1(batch_id, call_id)
        .expect("load admitted state")
        .expect("admitted state");
    assert_eq!(admitted.batch_status, ToolBatchStatusV1::Admitted);
    assert_eq!(admitted.call_status, ToolCallStatusV1::Queued);
    assert_eq!(admitted.batch_revision, Revision::INITIAL);
    assert_eq!(admitted.call_revision, Revision::INITIAL);
    assert_eq!(admitted.next_attempt_number, 1);

    let dispatch = attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 31);
    let expected_attempt = dispatch.attempt.clone();
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch)
        .expect("persist first attempt");
    let running = fixture
        .coordinator
        .load_tool_execution_entity_state_v1(batch_id, call_id)
        .expect("load running state")
        .expect("running state");
    assert_eq!(running.batch_status, ToolBatchStatusV1::Running);
    assert_eq!(running.call_status, ToolCallStatusV1::Running);
    assert_eq!(running.batch_revision, Revision::new(2));
    assert_eq!(running.call_revision, Revision::new(2));
    assert_eq!(running.next_attempt_number, 2);
    assert_eq!(
        fixture
            .coordinator
            .load_tool_execution_attempt_records_v1(batch_id, call_id)
            .expect("load ordered attempt records"),
        vec![expected_attempt],
    );

    assert!(
        fixture
            .coordinator
            .load_tool_execution_entity_state_v1(batch_id, ToolCallId::new())
            .expect("missing state")
            .is_none()
    );
    assert!(
        fixture
            .coordinator
            .load_tool_execution_attempt_records_v1(batch_id, ToolCallId::new())
            .expect("missing attempt history")
            .is_empty()
    );
}

#[test]
fn injected_failure_rolls_back_every_admission_fact() {
    let fixture = AdmissionFixture::new(60);
    let baseline = authority_counts(
        &Connection::open(&fixture.database_path).expect("open baseline database"),
    )
    .expect("baseline counts");

    let error = fixture
        .coordinator
        .commit_tool_batch_admission_with_fault(
            fixture.request,
            Some(PersistenceFaultPoint::BeforeCommit),
        )
        .expect_err("fault must abort transaction");
    assert!(error.to_string().contains("injected tool admission persistence failure"));

    let after = authority_counts(
        &Connection::open(&fixture.database_path).expect("open database after rollback"),
    )
    .expect("counts after rollback");
    assert_eq!(after, baseline);
}

#[test]
fn attempt_start_and_dispatch_prepared_are_one_durable_pre_dispatch_transaction() {
    let fixture = AdmissionFixture::new(90);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch = attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 90);
    let attempt_id = dispatch.attempt.tool_execution_attempt_id;
    let call_id = dispatch.attempt.tool_call_id;
    let expected_evidence = dispatch.dispatch_evidence_hash.clone();
    let commit = fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch)
        .expect("persist atomic pre-dispatch boundaries");
    assert_eq!(commit.event_ids.len(), 2);
    assert_eq!(commit.projection_changes.len(), 4);

    let connection = Connection::open(&fixture.database_path).expect("open database");
    let row = connection
        .query_row(
            "SELECT status, side_effect_state, started_event_id,
                    dispatch_prepared_event_id, dispatch_prepared_at
             FROM tool_execution_attempts WHERE tool_execution_attempt_id=?1",
            [attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .expect("dispatch-prepared attempt row");
    assert_eq!(row.0, "dispatch_prepared");
    assert_eq!(row.1, "not_started");
    assert_eq!(row.2, commit.event_ids[0].to_string());
    assert_eq!(row.3, commit.event_ids[1].to_string());
    assert_eq!(row.4, dispatch_timestamp().to_string());
    assert_eq!(
        connection
            .query_row(
                "SELECT sequence FROM tool_attempt_operational_state
                 WHERE tool_execution_attempt_id=?1",
                [attempt_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("operational state"),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || entity_revision FROM tool_calls
                 WHERE tool_call_id=?1",
                [call_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("running call"),
        "running:2"
    );
    let payload_json = connection
        .query_row(
            "SELECT payload_json FROM events WHERE event_id=?1",
            [commit.event_ids[1].to_string()],
            |row| row.get::<_, String>(0),
        )
        .expect("dispatch event payload");
    let payload: aura_tool_contract::ToolExecutionAttemptDispatchPreparedPayloadV1 =
        serde_json::from_str(&payload_json).expect("dispatch payload");
    assert_eq!(payload.dispatch_evidence_hash, expected_evidence);
}

#[test]
fn attempt_boundary_faults_leave_no_partial_event_record_or_projection() {
    let fixture = AdmissionFixture::new(120);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 120);
    let baseline = authority_counts(
        &Connection::open(&fixture.database_path).expect("open pre-start database"),
    )
    .expect("pre-start counts");
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared_with_fault(
            dispatch.clone(),
            Some(PersistenceFaultPoint::BeforeCommit),
        )
        .expect_err("atomic pre-dispatch fault must roll back");
    assert_eq!(
        authority_counts(
            &Connection::open(&fixture.database_path).expect("open post-start-fault database"),
        )
        .expect("post-start-fault counts"),
        baseline
    );

    let exists = Connection::open(&fixture.database_path)
        .expect("open final database")
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM tool_execution_attempts
             WHERE tool_execution_attempt_id=?1)",
            [dispatch.attempt.tool_execution_attempt_id.to_string()],
            |row| row.get::<_, bool>(0),
        )
        .expect("attempt existence");
    assert!(!exists, "a failed pre-dispatch transaction must not consume a tea_");
}

#[test]
fn authorized_sandbox_binding_is_atomic_exact_and_single_use() {
    let fixture = AdmissionFixture::new(121);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 121);
    let authorized = authorized_sandbox_execution(&fixture.coordinator, &admission, &dispatch, 121);
    let request = AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1 {
        attempt_commit: dispatch.clone(),
        sandbox_dispatch_binding: authorized.binding.clone(),
    };

    fixture
        .coordinator
        .commit_authorized_tool_execution_attempt_dispatch_prepared_with_fault_v1(
            request.clone(),
            Some(PersistenceFaultPoint::BeforeCommit),
        )
        .expect_err("fault rolls back attempt and binding");
    let connection = Connection::open(&fixture.database_path).expect("open database");
    assert_eq!(table_count(&connection, "tool_execution_attempts").expect("attempts"), 0);
    assert_eq!(table_count(&connection, "tool_sandbox_dispatch_bindings").expect("bindings"), 0);
    drop(connection);

    fixture
        .coordinator
        .commit_authorized_tool_execution_attempt_dispatch_prepared_v1(request)
        .expect("atomic authorized pre-dispatch commit");
    let consumed = fixture
        .coordinator
        .consume_authorized_tool_sandbox_dispatch_v1(authorized.clone(), runtime_timestamp())
        .expect("first exact Host consume");
    assert_eq!(consumed, authorized.binding);
    assert!(matches!(
        fixture
            .coordinator
            .consume_authorized_tool_sandbox_dispatch_v1(authorized, progress_timestamp()),
        Err(PersistenceError::ToolSandboxDispatchBindingUnavailable)
    ));
    let connection = Connection::open(&fixture.database_path).expect("open consumed database");
    assert_eq!(
        connection
            .query_row("SELECT state FROM tool_sandbox_dispatch_bindings", [], |row| row
                .get::<_, String>(0),)
            .expect("binding state"),
        "consumed"
    );
    drop(connection);
    let recovery = fixture
        .coordinator
        .recover_incomplete_tool_executions()
        .expect("reconcile consumed dispatch after simulated Core/Host crash");
    assert_eq!(recovery.dispatch_unknown_attempts_terminalized, 1);
    let connection = Connection::open(&fixture.database_path).expect("open recovered database");
    assert_eq!(
        connection
            .query_row("SELECT state FROM tool_sandbox_dispatch_bindings", [], |row| row
                .get::<_, String>(0),)
            .expect("recovered binding state"),
        "recovery_required"
    );
}

#[test]
fn authorized_sandbox_binding_rejects_plan_mutation_without_consuming() {
    let fixture = AdmissionFixture::new(122);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 122);
    let authorized = authorized_sandbox_execution(&fixture.coordinator, &admission, &dispatch, 122);
    fixture
        .coordinator
        .commit_authorized_tool_execution_attempt_dispatch_prepared_v1(
            AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1 {
                attempt_commit: dispatch,
                sandbox_dispatch_binding: authorized.binding.clone(),
            },
        )
        .expect("persist binding");

    let mut changed = authorized;
    changed.plan.working_directory.display.push_str("-changed");
    assert!(matches!(
        fixture
            .coordinator
            .consume_authorized_tool_sandbox_dispatch_v1(changed, runtime_timestamp()),
        Err(PersistenceError::ToolSandboxDispatchBindingMismatch)
    ));
    let connection = Connection::open(&fixture.database_path).expect("open database");
    assert_eq!(
        connection
            .query_row("SELECT state FROM tool_sandbox_dispatch_bindings", [], |row| row
                .get::<_, String>(0),)
            .expect("binding state"),
        "dispatch_prepared"
    );
}

#[test]
fn authorized_sandbox_binding_rejects_a_superseded_permission_snapshot() {
    let fixture = AdmissionFixture::new(123);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 123);
    let authorized = authorized_sandbox_execution(&fixture.coordinator, &admission, &dispatch, 123);
    fixture
        .coordinator
        .commit_authorized_tool_execution_attempt_dispatch_prepared_v1(
            AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1 {
                attempt_commit: dispatch,
                sandbox_dispatch_binding: authorized.binding.clone(),
            },
        )
        .expect("persist binding");

    let state = fixture
        .coordinator
        .session_permission_state(admission.batch.invocation_identity.session_id)
        .expect("permission state");
    fixture
        .coordinator
        .set_session_permission_mode(
            admission.batch.invocation_identity.session_id,
            state.revision,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("change permission mode");

    assert!(matches!(
        fixture
            .coordinator
            .consume_authorized_tool_sandbox_dispatch_v1(authorized, runtime_timestamp()),
        Err(PersistenceError::ToolSandboxDispatchBindingUnavailable)
    ));
    let connection = Connection::open(&fixture.database_path).expect("open database");
    assert_eq!(
        connection
            .query_row("SELECT state FROM tool_sandbox_dispatch_bindings", [], |row| row
                .get::<_, String>(0),)
            .expect("binding state"),
        "dispatch_prepared"
    );
}

#[test]
fn runtime_progress_is_monotonic_operational_state_without_event_churn() {
    let fixture = AdmissionFixture::new(150);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 150);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");

    let runtime_started = runtime_started_request(&dispatch);
    let runtime_commit = fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(runtime_started.clone())
        .expect("persist runtime start");
    assert!(runtime_commit.event_ids.is_empty());
    assert_eq!(runtime_commit.projection_changes.len(), 1);

    let progress = progress_request(&runtime_started, 2);
    let progress_commit = fixture
        .coordinator
        .commit_tool_execution_attempt_progress(progress.clone())
        .expect("persist monotonic progress");
    assert!(progress_commit.event_ids.is_empty());
    assert_eq!(progress_commit.projection_changes.len(), 1);

    let connection = Connection::open(&fixture.database_path).expect("open progress database");
    let state = connection
        .query_row(
            "SELECT attempt.status, attempt.side_effect_state,
                    operational.sequence, operational.percentage,
                    json_extract(attempt.record_json, '$.last_progress_sequence')
             FROM tool_execution_attempts AS attempt
             JOIN tool_attempt_operational_state AS operational
               USING (tool_execution_attempt_id)
             WHERE attempt.tool_execution_attempt_id=?1",
            [runtime_started.attempt.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .expect("runtime operational state");
    assert_eq!(state, ("running".to_owned(), "unknown".to_owned(), 2, Some(25), 0));
    let counts_before_duplicate = authority_counts(&connection).expect("counts before duplicate");
    drop(connection);

    fixture
        .coordinator
        .commit_tool_execution_attempt_progress(progress)
        .expect_err("duplicate progress sequence must be rejected");
    let connection = Connection::open(&fixture.database_path).expect("reopen progress database");
    assert_eq!(
        authority_counts(&connection).expect("counts after duplicate"),
        counts_before_duplicate
    );
}

#[test]
fn restart_reconciles_dispatch_prepared_as_indeterminate_and_ignores_progress_completion() {
    let fixture = AdmissionFixture::new(161);
    let admission = fixture.request.clone();
    let batch_id = admission.batch.tool_batch_id;
    let call_id = admission.batch.calls[0].tool_call_id;
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 161);
    let attempt_id = dispatch.attempt.tool_execution_attempt_id;
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");
    let mut progress = progress_request(&running, 1);
    progress.attempt_projection.progress.as_mut().expect("progress").percentage = Some(100);
    fixture
        .coordinator
        .commit_tool_execution_attempt_progress(progress)
        .expect("persist misleading 100 percent progress");

    let database_path = fixture.database_path.clone();
    let artifact_root = fixture._temp.path().join("artifacts");
    drop(fixture.coordinator);
    let restarted = PersistenceCoordinator::start(PersistenceOptions::new(
        database_path.clone(),
        artifact_root,
        RuntimeInstanceId::from_bytes([162; 16]),
    ))
    .expect("restart recovery");
    let PersistenceStartup::Ready(restarted) = restarted else {
        panic!("dispatch outcome reconciliation should restore Ready");
    };

    let connection = Connection::open(&database_path).expect("open recovered database");
    let result_json = connection
        .query_row(
            "SELECT result_json FROM tool_results WHERE tool_call_id=?1",
            [call_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .expect("recovered logical result");
    let result: ToolResultV1 = serde_json::from_str(&result_json).expect("decode result");
    assert_eq!(result.side_effect_state, SideEffectStateV1::Unknown);
    assert!(matches!(
        result.outcome,
        ToolResultOutcomeV1::Indeterminate {
            reason: aura_tool_contract::ToolIndeterminateReasonV1::DispatchOutcomeUnknown,
            ..
        }
    ));
    assert_eq!(result.attempts.len(), 1, "restart must not consume a retry attempt");
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || side_effect_state FROM tool_execution_attempts
                 WHERE tool_execution_attempt_id=?1",
                [attempt_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("terminal recovered attempt"),
        "indeterminate:unknown"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status FROM tool_batches WHERE tool_batch_id=?1",
                [batch_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("completed recovered batch"),
        "completed"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT recovery_code FROM tool_execution_recovery_diagnostics
                 WHERE tool_execution_attempt_id=?1",
                [attempt_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("stable recovery diagnostic"),
        "dispatch_outcome_unknown"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM tool_batch_payload_leases
                 WHERE tool_batch_id=?1 AND released_at IS NULL",
                [batch_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("released payload lease"),
        0
    );
    let event_count = table_count(&connection, "events").expect("event count");
    drop(connection);
    drop(restarted);

    let restarted_again = PersistenceCoordinator::start(PersistenceOptions::new(
        database_path.clone(),
        fixture._temp.path().join("artifacts"),
        RuntimeInstanceId::from_bytes([163; 16]),
    ))
    .expect("idempotent restart");
    assert!(matches!(restarted_again, PersistenceStartup::Ready(_)));
    assert_eq!(
        table_count(&Connection::open(database_path).expect("open idempotent database"), "events")
            .expect("event count after second restart"),
        event_count,
        "recovery must not append a second terminal"
    );
}

#[test]
fn historical_started_only_attempt_blocks_ready_without_fabricating_dispatch() {
    let fixture = AdmissionFixture::new(164);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let call_id = admission.batch.calls[0].tool_call_id;
    let attempt = ToolExecutionAttemptRecordV1 {
        tool_execution_attempt_id: ToolExecutionAttemptId::from_bytes([181; 16]),
        tool_call_id: call_id,
        attempt_number: 1,
        status: ToolExecutionAttemptStatusV1::Started,
        side_effect_state: SideEffectStateV1::NotStarted,
        evidence_ref: None,
        started_at: timestamp(),
        dispatch_prepared_at: None,
        completed_at: None,
        last_progress_sequence: 0,
    };
    let record_json = String::from_utf8(
        aura_domain_common::jcs::canonical_json_jcs(&attempt).expect("canonical attempt"),
    )
    .expect("UTF-8 attempt");
    let record_hash_json = String::from_utf8(
        aura_domain_common::jcs::canonical_json_jcs(
            &aura_domain_common::jcs::canonical_sha256(&attempt).expect("attempt hash"),
        )
        .expect("canonical attempt hash"),
    )
    .expect("UTF-8 attempt hash");
    let connection = Connection::open(&fixture.database_path).expect("open historical database");
    connection.execute_batch("PRAGMA foreign_keys=ON;").expect("foreign keys");
    connection
        .execute(
            "UPDATE tool_batches SET status='running', entity_revision=2,
                started_at=?1, updated_at=?1 WHERE tool_batch_id=?2",
            params![timestamp().to_string(), admission.batch.tool_batch_id.to_string()],
        )
        .expect("seed running batch");
    connection
        .execute(
            "UPDATE tool_calls SET status='running', entity_revision=2,
                started_at=?1, updated_at=?1 WHERE tool_call_id=?2",
            params![timestamp().to_string(), call_id.to_string()],
        )
        .expect("seed running call");
    connection
        .execute(
            "INSERT INTO tool_execution_attempts(
                tool_execution_attempt_id, tool_call_id, tool_batch_id, attempt_number,
                execution_plan_hash_json, status, side_effect_state, record_json,
                record_hash_json, started_event_id, started_at, updated_at
             ) VALUES (?1, ?2, ?3, 1, ?4, 'started', 'not_started', ?5, ?6, ?7, ?8, ?8)",
            params![
                attempt.tool_execution_attempt_id.to_string(),
                call_id.to_string(),
                admission.batch.tool_batch_id.to_string(),
                serde_json::to_string(&admission.execution_plan.execution_plan_hash)
                    .expect("plan hash"),
                record_json,
                record_hash_json,
                admission_commit.event_ids[0].to_string(),
                timestamp().to_string(),
            ],
        )
        .expect("seed historical started-only attempt");
    connection
        .execute(
            "INSERT INTO tool_attempt_operational_state(
                tool_execution_attempt_id, sequence, percentage, progress_json, updated_at
             ) VALUES (?1, 0, NULL, NULL, ?2)",
            params![attempt.tool_execution_attempt_id.to_string(), timestamp().to_string()],
        )
        .expect("seed operational state");
    drop(connection);
    let database_path = fixture.database_path.clone();
    drop(fixture.coordinator);

    let error = match PersistenceCoordinator::start(PersistenceOptions::new(
        database_path.clone(),
        fixture._temp.path().join("artifacts"),
        RuntimeInstanceId::from_bytes([165; 16]),
    )) {
        Ok(_) => panic!("started-only history must block Ready"),
        Err(error) => error,
    };
    assert!(matches!(error, aura_persistence::PersistenceError::ToolExecutionRecoveryRequired));
    let connection = Connection::open(database_path).expect("open blocked database");
    let state = connection
        .query_row(
            "SELECT a.status, a.dispatch_prepared_event_id, b.status,
                    d.recovery_code
             FROM tool_execution_attempts a
             JOIN tool_batches b ON b.tool_batch_id=a.tool_batch_id
             JOIN tool_execution_recovery_diagnostics d
               ON d.tool_execution_attempt_id=a.tool_execution_attempt_id
             WHERE a.tool_execution_attempt_id=?1",
            [attempt.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .expect("historical recovery diagnostic");
    assert_eq!(
        state,
        (
            "started".to_owned(),
            None,
            "recovery_required".to_owned(),
            "historical_started_only".to_owned()
        )
    );
}

#[test]
fn execution_attempt_can_own_internal_evidence_artifact() {
    let fixture = AdmissionFixture::new(170);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 170);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist pre-dispatch attempt");
    let attempt_id = dispatch.attempt.tool_execution_attempt_id;
    let owner = ArtifactOwnerRef::Tool {
        owner: ToolArtifactOwnerRef::ExecutionAttempt { tool_execution_attempt_id: attempt_id },
    };

    let artifact = fixture
        .coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/json".to_owned(),
            access: ArtifactAccess::NONE,
            owner: owner.clone(),
            bytes: br#"{"evidence":"runtime-bound"}"#.to_vec(),
        })
        .expect("persist attempt-owned evidence");
    assert_eq!(artifact.owner, owner);

    let connection = Connection::open(&fixture.database_path).expect("open artifact database");
    let stored = connection
        .query_row(
            "SELECT owner_kind, owner_type, owner_id
             FROM artifact_owners WHERE artifact_id=?1",
            [artifact.artifact_id.to_string()],
            |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            },
        )
        .expect("stored tool artifact owner");
    assert_eq!(stored.0, "tool");
    assert_eq!(stored.1, "execution_attempt");
    let stored_owner: ToolArtifactOwnerRef =
        serde_json::from_str(&stored.2).expect("canonical tool owner");
    assert_eq!(
        stored_owner,
        ToolArtifactOwnerRef::ExecutionAttempt { tool_execution_attempt_id: attempt_id }
    );
}

#[test]
fn cancellation_request_is_durable_without_fabricating_a_terminal_result() {
    let fixture = AdmissionFixture::new(190);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 190);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");
    let cancellation = cancellation_request(&running, 190);

    let baseline = authority_counts(
        &Connection::open(&fixture.database_path).expect("open pre-cancellation database"),
    )
    .expect("pre-cancellation counts");
    fixture
        .coordinator
        .commit_tool_execution_attempt_cancellation_requested_with_fault(
            cancellation.clone(),
            Some(PersistenceFaultPoint::BeforeCommit),
        )
        .expect_err("cancellation fault must roll back");
    let connection = Connection::open(&fixture.database_path).expect("open rollback database");
    assert_eq!(authority_counts(&connection).expect("rollback counts"), baseline);
    assert_eq!(
        connection
            .query_row(
                "SELECT status FROM tool_execution_attempts
                 WHERE tool_execution_attempt_id=?1",
                [running.attempt.tool_execution_attempt_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("attempt status after rollback"),
        "running"
    );
    drop(connection);

    let commit = fixture
        .coordinator
        .commit_tool_execution_attempt_cancellation_requested(cancellation.clone())
        .expect("persist cancellation intent");
    assert_eq!(commit.event_ids.len(), 1);
    assert_eq!(commit.projection_changes.len(), 1);
    let connection = Connection::open(&fixture.database_path).expect("open cancellation database");
    let state = connection
        .query_row(
            "SELECT status, cancellation_requested_event_id,
                    cancellation_requested_at, terminal_event_id
             FROM tool_execution_attempts WHERE tool_execution_attempt_id=?1",
            [running.attempt.tool_execution_attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .expect("cancelling attempt");
    assert_eq!(state.0, "cancelling");
    assert_eq!(state.1, commit.event_ids[0].to_string());
    assert_eq!(state.2, cancellation_timestamp().to_string());
    assert_eq!(state.3, None);
    assert_eq!(table_count(&connection, "tool_results").expect("result count"), baseline.4);
    let payload_json = connection
        .query_row(
            "SELECT payload_json FROM events WHERE event_id=?1",
            [commit.event_ids[0].to_string()],
            |row| row.get::<_, String>(0),
        )
        .expect("cancellation event payload");
    let payload: aura_tool_contract::ToolExecutionAttemptCancellationRequestedPayloadV1 =
        serde_json::from_str(&payload_json).expect("cancellation payload");
    assert_eq!(payload.reason, ToolCancellationReasonV1::UserRequested);
    drop(connection);

    fixture
        .coordinator
        .commit_tool_execution_attempt_cancellation_requested(cancellation)
        .expect_err("a second cancellation event must be rejected");
}

#[test]
fn terminal_attempt_result_batch_and_projections_commit_atomically() {
    let fixture = AdmissionFixture::new(210);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 210);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");
    let request =
        successful_finalization_request(&fixture.coordinator, &admission, &dispatch, &running);
    let attempt_id = request.attempt.tool_execution_attempt_id;
    let call_id = request.result.tool_call_id;
    let batch_id = request.batch_projection.tool_batch_id;

    let baseline = authority_counts(
        &Connection::open(&fixture.database_path).expect("open pre-finalization database"),
    )
    .expect("pre-finalization counts");
    fixture
        .coordinator
        .commit_tool_execution_attempt_finalized_with_fault(
            request.clone(),
            Some(PersistenceFaultPoint::BeforeCommit),
        )
        .expect_err("terminal fault must roll back");
    let connection = Connection::open(&fixture.database_path).expect("open rollback database");
    assert_eq!(authority_counts(&connection).expect("rollback counts"), baseline);
    assert_eq!(
        connection
            .query_row(
                "SELECT status FROM tool_execution_attempts
                 WHERE tool_execution_attempt_id=?1",
                [attempt_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("attempt status after rollback"),
        "running"
    );
    drop(connection);

    let commit = fixture
        .coordinator
        .commit_tool_execution_attempt_finalized(request.clone())
        .expect("atomic finalization");
    assert_eq!(commit.event_ids.len(), 2);
    assert_eq!(commit.projection_changes.len(), 4);
    assert_eq!(commit.projection_outbox_ids.len(), 4);

    let connection = Connection::open(&fixture.database_path).expect("open finalized database");
    let finalized_counts = authority_counts(&connection).expect("finalized counts");
    assert_eq!(finalized_counts.7, baseline.7 + 4);
    assert_eq!(finalized_counts.8, baseline.8 + 2);
    let attempt = connection
        .query_row(
            "SELECT status, side_effect_state, evidence_artifact_id,
                    terminal_event_id, completed_at
             FROM tool_execution_attempts WHERE tool_execution_attempt_id=?1",
            [attempt_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .expect("terminal attempt");
    assert_eq!(attempt.0, "succeeded");
    assert_eq!(attempt.1, "no_effect");
    assert_eq!(attempt.2, request.attempt.evidence_ref.expect("attempt evidence").to_string());
    assert_eq!(attempt.3, commit.event_ids[0].to_string());
    assert_eq!(attempt.4, terminal_timestamp().to_string());
    assert_eq!(
        connection
            .query_row(
                "SELECT outcome_kind || ':' || side_effect_state
                 FROM tool_results WHERE tool_call_id=?1",
                [call_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("logical result"),
        "succeeded:no_effect"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || entity_revision FROM tool_calls
                 WHERE tool_call_id=?1",
                [call_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("finalized call"),
        "finalized:3"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || entity_revision FROM tool_batches
                 WHERE tool_batch_id=?1",
                [batch_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("completed batch"),
        "completed:3"
    );
    let event_types = connection
        .prepare(
            "SELECT event_type FROM events WHERE event_id IN (?1, ?2)
             ORDER BY global_sequence",
        )
        .expect("event query")
        .query_map(
            params![commit.event_ids[0].to_string(), commit.event_ids[1].to_string()],
            |row| row.get::<_, String>(0),
        )
        .expect("terminal events")
        .collect::<Result<Vec<_>, _>>()
        .expect("terminal event rows");
    assert_eq!(event_types, vec!["ToolExecutionAttemptSucceeded", "ToolResultFinalized"]);
    drop(connection);

    fixture
        .coordinator
        .commit_tool_execution_attempt_finalized(request)
        .expect_err("a terminal attempt cannot be finalized twice");
}

#[test]
fn activity_cursor_pages_exact_terminal_history_and_rechecks_session_visibility() {
    let fixture = AdmissionFixture::new(209);
    let admission = fixture.request.clone();
    let session_id = admission.batch.invocation_identity.session_id;
    let executable_call_id = admission.execution_plan.calls[0].tool_call_id;
    let unresolved_call_id = admission.not_executed_results[0].tool_call_id;
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 209);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");
    fixture
        .coordinator
        .commit_tool_execution_attempt_finalized(successful_finalization_request(
            &fixture.coordinator,
            &admission,
            &dispatch,
            &running,
        ))
        .expect("finalize executable call");

    let connection = Connection::open(&fixture.database_path).expect("open activity database");
    let executable_sequence = connection
        .query_row(
            "SELECT event.session_sequence
             FROM tool_results result
             JOIN events event ON event.event_id=result.finalized_event_id
             WHERE result.tool_call_id=?1",
            [executable_call_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("terminal session sequence");
    let (unresolved_event_id, unresolved_sequence) = connection
        .query_row(
            "SELECT event.event_id, event.session_sequence
             FROM tool_results result
             JOIN events event ON event.event_id=result.finalized_event_id
             WHERE result.tool_call_id=?1",
            [unresolved_call_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .expect("unresolved terminal session sequence");
    let executable_event_id = connection
        .query_row(
            "SELECT finalized_event_id FROM tool_results WHERE tool_call_id=?1",
            [executable_call_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .expect("executable terminal event");
    // Keep the invalid preflight Result as the already-rendered boundary and
    // page the exact admitted/executed Result below it. SessionSequence is the
    // only ordering fact changed in this isolated query test.
    connection
        .execute("UPDATE events SET session_sequence=-1 WHERE event_id=?1", [&unresolved_event_id])
        .expect("reserve unresolved sequence");
    connection
        .execute(
            "UPDATE events SET session_sequence=?1 WHERE event_id=?2",
            params![unresolved_sequence, executable_event_id],
        )
        .expect("move executable result below boundary");
    connection
        .execute(
            "UPDATE events SET session_sequence=?1 WHERE event_id=?2",
            params![executable_sequence, unresolved_event_id],
        )
        .expect("move unresolved result to boundary");
    drop(connection);
    let terminal_sequence = SessionSequence::new(
        u64::try_from(executable_sequence).expect("non-negative session sequence"),
    );
    let visibility = tool_activity_visibility_context_hash_v1(session_id);
    let cursor = fixture
        .coordinator
        .issue_tool_activity_cursor_v1(
            session_id,
            visibility.clone(),
            50,
            terminal_sequence,
            unresolved_call_id,
        )
        .expect("issue stable activity cursor");
    fixture
        .coordinator
        .read_tool_registry_search_corpus(&admission.batch.registry_snapshot_ref)
        .expect("read activity Registry corpus")
        .expect("activity Registry corpus remains available");
    assert!(matches!(
        fixture.coordinator.read_tool_activity_page_v1(
            &cursor,
            SessionId::from_bytes([208; 16]),
            &visibility,
        ),
        Err(ToolActivityPageReadErrorV1::CursorMismatch)
    ));
    let page = fixture
        .coordinator
        .read_tool_activity_page_v1(&cursor, session_id, &visibility)
        .expect("read exact activity page");
    assert_eq!(page.session_id, session_id);
    assert_eq!(page.cutoff_session_sequence, terminal_sequence);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].call.tool_call_id, executable_call_id);
    assert!(page.next_cursor.is_none());
}

#[test]
fn activity_cursor_expires_when_its_terminal_boundary_is_explicitly_removed() {
    let fixture = AdmissionFixture::new(208);
    let admission = fixture.request.clone();
    let session_id = admission.batch.invocation_identity.session_id;
    let unresolved_call_id = admission.not_executed_results[0].tool_call_id;
    fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let connection = Connection::open(&fixture.database_path).expect("open activity database");
    let terminal_sequence = connection
        .query_row(
            "SELECT event.session_sequence
             FROM tool_results result
             JOIN events event ON event.event_id=result.finalized_event_id
             WHERE result.tool_call_id=?1",
            [unresolved_call_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("terminal session sequence");
    let terminal_sequence = SessionSequence::new(
        u64::try_from(terminal_sequence).expect("non-negative session sequence"),
    );
    let visibility = tool_activity_visibility_context_hash_v1(session_id);
    let cursor = fixture
        .coordinator
        .issue_tool_activity_cursor_v1(
            session_id,
            visibility.clone(),
            50,
            terminal_sequence,
            unresolved_call_id,
        )
        .expect("issue stable activity cursor");
    connection
        .execute("DELETE FROM tool_results WHERE tool_call_id=?1", [unresolved_call_id.to_string()])
        .expect("simulate explicit history cleanup");
    drop(connection);

    assert!(matches!(
        fixture.coordinator.read_tool_activity_page_v1(&cursor, session_id, &visibility),
        Err(ToolActivityPageReadErrorV1::CursorExpired)
    ));
}

#[test]
fn completed_batch_finalizes_the_exact_pending_history_atomic_group() {
    let fixture = AdmissionFixture::new(211);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let assistant_content = "The fixture tool call is pending.";
    let assistant_event_id = EventId::from_bytes([212; 16]);
    fixture
        .coordinator
        .commit_context_source_event(ContextSourceEventCommitRequest {
            event: PersistentEventDraft {
                event_id: assistant_event_id,
                workspace_id: None,
                session_id: Some(admission.batch.invocation_identity.session_id),
                turn_task_id: Some(admission.batch.invocation_identity.turn_task_id),
                agent_run_id: Some(admission.batch.invocation_identity.agent_run_id),
                event_type: "AssistantResponse".to_owned(),
                event_version: 1,
                actor: EventActor::ParentAgent,
                correlation_id: CorrelationId::from_bytes([213; 16]),
                causation_id: None,
                atomic_group_id: None,
                payload_json: Some(json!({"content": assistant_content})),
                payload_artifact_ref: None,
                searchable_text: Some(assistant_content.to_owned()),
                created_at: timestamp(),
            },
            canonical_content: assistant_content.to_owned(),
            model_visible: true,
            explicitly_user_authored: false,
        })
        .expect("persist assistant response event");
    let assistant_hash =
        aura_domain_common::jcs::CanonicalSha256HashV1::sha256(assistant_content.as_bytes());
    let pending = fixture
        .coordinator
        .commit_pending_tool_history_atomic_group_v1(ToolHistoryAtomicGroupAssociationRequestV1 {
            tool_batch_id: admission.batch.tool_batch_id,
            assistant_response_event_id: assistant_event_id,
            assistant_response_content_hash: assistant_hash.clone(),
            created_at: timestamp(),
        })
        .expect("persist pending history association");
    assert!(matches!(pending, StoredToolHistoryAtomicGroupAssociationV1::Pending { .. }));

    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 211);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");
    let finalization =
        successful_finalization_request(&fixture.coordinator, &admission, &dispatch, &running);
    fixture
        .coordinator
        .commit_tool_execution_attempt_finalized_with_fault(
            finalization.clone(),
            Some(PersistenceFaultPoint::BeforeCommit),
        )
        .expect_err("terminal Result and history-group finalization must roll back together");
    assert!(matches!(
        fixture
            .coordinator
            .load_tool_history_atomic_group_v1(admission.batch.tool_batch_id)
            .expect("load rolled-back history association")
            .expect("pending history association remains"),
        StoredToolHistoryAtomicGroupAssociationV1::Pending { .. }
    ));
    let connection = Connection::open(&fixture.database_path).expect("open rolled-back database");
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM tool_results WHERE tool_call_id=?1",
                [dispatch.attempt.tool_call_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("rolled-back result count"),
        0
    );
    drop(connection);

    fixture
        .coordinator
        .commit_tool_execution_attempt_finalized(finalization)
        .expect("finalize batch and history group");

    let stored = fixture
        .coordinator
        .load_tool_history_atomic_group_v1(admission.batch.tool_batch_id)
        .expect("load history association")
        .expect("history association");
    let StoredToolHistoryAtomicGroupAssociationV1::Finalized { preimage, group_ref, .. } = stored
    else {
        panic!("completed batch must finalize its history association");
    };
    assert_eq!(group_ref.assistant_response_event_id, assistant_event_id);
    assert_eq!(preimage.assistant_response_content_hash, assistant_hash);
    assert_eq!(preimage.calls.len(), admission.batch.calls.len());
    assert!(preimage.calls.windows(2).all(|pair| pair[0].ordinal < pair[1].ordinal));
}

#[test]
fn completion_can_win_after_cancel_intent_without_becoming_cancelled() {
    let fixture = AdmissionFixture::new(230);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 230);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");
    let cancelling = cancellation_request(&running, 230);
    fixture
        .coordinator
        .commit_tool_execution_attempt_cancellation_requested(cancelling.clone())
        .expect("persist cancellation intent");

    let mut request =
        successful_finalization_request(&fixture.coordinator, &admission, &dispatch, &running);
    request.attempt_projection.progress = cancelling.attempt_projection.progress;
    let commit = fixture
        .coordinator
        .commit_tool_execution_attempt_finalized(request)
        .expect("completion is the first durable terminal");
    assert_eq!(commit.event_ids.len(), 2);
    assert_eq!(
        Connection::open(&fixture.database_path)
            .expect("open completed database")
            .query_row(
                "SELECT status FROM tool_execution_attempts
                 WHERE tool_execution_attempt_id=?1",
                [running.attempt.tool_execution_attempt_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("terminal status"),
        "succeeded"
    );
}

#[test]
fn cancelled_terminal_requires_the_exact_persisted_core_intent() {
    let fixture = AdmissionFixture::new(245);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 245);
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");
    let cancelled = cancelled_finalization_request(successful_finalization_request(
        &fixture.coordinator,
        &admission,
        &dispatch,
        &running,
    ));

    fixture
        .coordinator
        .commit_tool_execution_attempt_finalized(cancelled.clone())
        .expect_err("cancelled cannot be inferred without a persisted Core intent");
    let cancellation = cancellation_request(&running, 245);
    fixture
        .coordinator
        .commit_tool_execution_attempt_cancellation_requested(cancellation)
        .expect("persist exact cancellation intent");
    let commit = fixture
        .coordinator
        .commit_tool_execution_attempt_finalized(cancelled)
        .expect("persist verified cancelled terminal");
    let event_type = Connection::open(&fixture.database_path)
        .expect("open cancelled database")
        .query_row(
            "SELECT event_type FROM events WHERE event_id=?1",
            [commit.event_ids[0].to_string()],
            |row| row.get::<_, String>(0),
        )
        .expect("cancelled terminal event");
    assert_eq!(event_type, "ToolExecutionAttemptCancelled");
}

#[test]
fn retryable_failure_terminates_only_the_attempt_before_a_new_attempt_starts() {
    let fixture = AdmissionFixture::new_retryable(251);
    let admission = fixture.request.clone();
    let admission_commit =
        fixture.coordinator.commit_tool_batch_admission(fixture.request).expect("admit batch");
    let first_dispatch =
        attempt_dispatch_prepared_request(&admission, admission_commit.event_ids[0], 251);
    let dispatch = first_dispatch.clone();
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(dispatch.clone())
        .expect("persist dispatch preparation");
    let running = runtime_started_request(&dispatch);
    fixture
        .coordinator
        .commit_tool_execution_attempt_runtime_started(running.clone())
        .expect("persist runtime start");

    let retryable_failure = retryable_failure_request(&fixture.coordinator, &running, 251);
    let unsafe_failure =
        retryable_failure_with_side_effect(retryable_failure.clone(), SideEffectStateV1::Unknown);
    let baseline = authority_counts(
        &Connection::open(&fixture.database_path).expect("open pre-failure database"),
    )
    .expect("pre-failure counts");
    fixture
        .coordinator
        .commit_tool_execution_attempt_failed_for_retry(unsafe_failure)
        .expect_err("unknown side effects must prevent transparent retry");
    assert_eq!(
        authority_counts(
            &Connection::open(&fixture.database_path).expect("open rejected-failure database"),
        )
        .expect("rejected failure counts"),
        baseline
    );

    fixture
        .coordinator
        .commit_tool_execution_attempt_failed_for_retry_with_fault(
            retryable_failure.clone(),
            Some(PersistenceFaultPoint::BeforeCommit),
        )
        .expect_err("retryable failure fault must roll back");
    assert_eq!(
        authority_counts(
            &Connection::open(&fixture.database_path).expect("open rollback database"),
        )
        .expect("rollback counts"),
        baseline
    );

    let failure_commit = fixture
        .coordinator
        .commit_tool_execution_attempt_failed_for_retry(retryable_failure.clone())
        .expect("persist retryable attempt failure");
    assert_eq!(failure_commit.event_ids.len(), 1);
    assert_eq!(failure_commit.projection_changes.len(), 1);
    let connection =
        Connection::open(&fixture.database_path).expect("open failed-attempt database");
    let failed_counts = authority_counts(&connection).expect("failed attempt counts");
    assert_eq!(failed_counts.7, baseline.7 + 1);
    assert_eq!(failed_counts.8, baseline.8);
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || side_effect_state FROM tool_execution_attempts
                 WHERE tool_execution_attempt_id=?1",
                [retryable_failure.attempt.tool_execution_attempt_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("failed attempt"),
        "failed:no_effect"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || entity_revision FROM tool_calls
                 WHERE tool_call_id=?1",
                [retryable_failure.attempt.tool_call_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("still-running logical call"),
        "running:2"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || entity_revision FROM tool_batches
                 WHERE tool_batch_id=?1",
                [admission.batch.tool_batch_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("still-running batch"),
        "running:2"
    );
    assert_eq!(table_count(&connection, "tool_results").expect("logical result count"), 1);
    assert_eq!(
        connection
            .query_row(
                "SELECT event_type FROM events WHERE event_id=?1",
                [failure_commit.event_ids[0].to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("attempt failure event"),
        "ToolExecutionAttemptFailed"
    );
    drop(connection);

    let second_dispatch = retry_attempt_dispatch_prepared_request(&first_dispatch, 251);
    let second_attempt_id = second_dispatch.attempt.tool_execution_attempt_id;
    fixture
        .coordinator
        .commit_tool_execution_attempt_dispatch_prepared(second_dispatch)
        .expect("persist second attempt pre-dispatch boundaries");
    let connection = Connection::open(&fixture.database_path).expect("open retry database");
    let attempts = connection
        .prepare(
            "SELECT tool_execution_attempt_id, attempt_number, status
             FROM tool_execution_attempts WHERE tool_call_id=?1 ORDER BY attempt_number",
        )
        .expect("attempt query")
        .query_map([retryable_failure.attempt.tool_call_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
        })
        .expect("attempt rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("attempt records");
    assert_eq!(
        attempts,
        vec![
            (
                retryable_failure.attempt.tool_execution_attempt_id.to_string(),
                1,
                "failed".to_owned(),
            ),
            (second_attempt_id.to_string(), 2, "dispatch_prepared".to_owned()),
        ]
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT status || ':' || entity_revision FROM tool_calls
                 WHERE tool_call_id=?1",
                [retryable_failure.attempt.tool_call_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("retried logical call"),
        "running:3"
    );
    assert_eq!(table_count(&connection, "tool_results").expect("logical result count"), 1);
}

fn attempt_dispatch_prepared_request(
    admission: &ToolBatchAdmissionCommitRequest,
    admitted_event_id: aura_domain_common::EventId,
    seed: u8,
) -> ToolExecutionAttemptDispatchPreparedCommitRequest {
    let call = &admission.batch.calls[0];
    let attempt = ToolExecutionAttemptRecordV1 {
        tool_execution_attempt_id: ToolExecutionAttemptId::from_bytes([seed.wrapping_add(17); 16]),
        tool_call_id: call.tool_call_id,
        attempt_number: 1,
        status: ToolExecutionAttemptStatusV1::DispatchPrepared,
        side_effect_state: SideEffectStateV1::NotStarted,
        evidence_ref: None,
        started_at: timestamp(),
        dispatch_prepared_at: Some(dispatch_timestamp()),
        completed_at: None,
        last_progress_sequence: 0,
    };
    let mut batch_projection = admission.batch_projection.clone();
    batch_projection.status = ToolBatchStatusV1::Running;
    batch_projection.started_at = Some(timestamp());
    let batch_call = batch_projection
        .calls
        .iter_mut()
        .find(|summary| summary.tool_call_id == call.tool_call_id)
        .expect("batch call summary");
    batch_call.status = ToolCallStatusV1::Running;
    batch_call.started_at = Some(timestamp());

    let mut call_projection = admission.call_projections[0].clone();
    call_projection.status = ToolCallStatusV1::Running;
    call_projection.attempt_ids = vec![attempt.tool_execution_attempt_id];
    call_projection.started_at = Some(timestamp());

    let attempt_projection = ToolExecutionAttemptProjection {
        tool_execution_attempt_id: attempt.tool_execution_attempt_id,
        tool_call_id: attempt.tool_call_id,
        attempt_number: attempt.attempt_number,
        status: ToolExecutionAttemptStatusV1::DispatchPrepared,
        side_effect_state: SideEffectStateV1::NotStarted,
        progress: None,
        started_at: attempt.started_at,
        dispatch_prepared_at: Some(dispatch_timestamp()),
        completed_at: None,
        safe_error: None,
        user_readable_artifacts: Vec::new(),
        can_cancel: true,
    };
    let mut session_activity_projection = admission.session_activity_projection.clone();
    let active_batch = session_activity_projection
        .active_batches
        .iter_mut()
        .find(|summary| summary.tool_batch_id == admission.batch.tool_batch_id)
        .expect("active batch summary");
    active_batch.status = ToolBatchStatusV1::Running;
    active_batch.started_at = Some(timestamp());
    let recent_call = session_activity_projection
        .recent_calls
        .iter_mut()
        .find(|summary| summary.tool_call_id == call.tool_call_id)
        .expect("recent call summary");
    recent_call.status = ToolCallStatusV1::Running;
    recent_call.started_at = Some(timestamp());

    ToolExecutionAttemptDispatchPreparedCommitRequest {
        attempt,
        execution_plan_hash: admission.execution_plan.execution_plan_hash.clone(),
        dispatch_evidence_hash: CanonicalHashV1::sha256(&[seed, 22]),
        expected_batch_revision: Revision::INITIAL,
        expected_call_revision: Revision::INITIAL,
        admitted_event_id,
        causation_id: CausationId::from_bytes([seed.wrapping_add(18); 16]),
        batch_projection,
        call_projection,
        attempt_projection,
        session_activity_projection,
        correlation_id: admission.correlation_id,
    }
}

fn authorized_sandbox_execution(
    coordinator: &PersistenceCoordinator,
    admission: &ToolBatchAdmissionCommitRequest,
    dispatch: &ToolExecutionAttemptDispatchPreparedCommitRequest,
    seed: u8,
) -> AuthorizedToolSandboxExecutionV1 {
    let normalized = &admission.batch.calls[0];
    let reference = normalized.definition_ref.as_ref().expect("resolved sandbox call");
    let corpus = coordinator
        .read_tool_registry_search_corpus(&admission.batch.registry_snapshot_ref)
        .expect("load Registry corpus")
        .expect("Registry corpus");
    let exact = coordinator
        .describe_tool_definition_exact(reference)
        .expect("describe exact Tool")
        .expect("exact Tool");
    let effective = corpus
        .snapshot
        .effective_bindings
        .iter()
        .find(|binding| binding.tool_id == reference.tool_id)
        .expect("effective binding");
    let source = ToolSecuritySourceRefV1::new(
        reference.clone(),
        effective.tool_instance_id,
        effective.source_kind,
        effective.definition_fingerprint.clone(),
        effective.source_contract_fingerprint.clone(),
    )
    .expect("exact source");
    let snapshot = coordinator
        .create_permission_snapshot(
            admission.batch.invocation_identity.session_id,
            Some(admission.batch.invocation_identity.turn_task_id),
            admission.correlation_id,
        )
        .expect("permission snapshot");
    let identity = InvocationIdentity {
        principal_id: aura_domain_common::PrincipalId::from_bytes([seed.wrapping_add(40); 16]),
        actor: InvocationActor::ParentAgent,
        workspace_id: Some(snapshot.workspace_id),
        session_id: Some(snapshot.session_id),
        turn_task_id: Some(admission.batch.invocation_identity.turn_task_id),
        agent_run_id: Some(admission.batch.invocation_identity.agent_run_id),
        parent_agent_run_id: None,
        correlation_id: admission.correlation_id,
        permission_snapshot_id: Some(snapshot.id),
    };
    let executable = CanonicalExecutableRef {
        path: CanonicalPath {
            display: format!(r"C:\Aura Tests\P07\{seed}\fixture.exe"),
            utf16: format!(r"C:\Aura Tests\P07\{seed}\fixture.exe").encode_utf16().collect(),
        },
        identity: aura_domain_common::FileIdentity {
            volume_serial_number: format!("volume-{seed}"),
            file_id: format!("file-{seed}"),
        },
        content_hash: aura_domain_common::ContentHash::sha256(&[seed, 41]),
    };
    let working_directory = CanonicalPath {
        display: format!(r"C:\Aura Tests\P07\{seed}"),
        utf16: format!(r"C:\Aura Tests\P07\{seed}").encode_utf16().collect(),
    };
    let arguments = match &normalized.input {
        NormalizedToolCallInputStateV1::Ready { arguments, .. } => arguments.clone(),
        NormalizedToolCallInputStateV1::InvalidInput { .. } => panic!("ready call"),
    };
    let purpose = match &normalized.input {
        NormalizedToolCallInputStateV1::Ready { purpose, .. } => purpose.clone(),
        NormalizedToolCallInputStateV1::InvalidInput { .. } => panic!("ready call"),
    };
    let runtime = NonSecretToolRuntimeFactsV1::Sandbox {
        facts: Box::new(NonSecretSandboxRuntimeFactsV1 {
            runtime_kind: ToolPreparedRuntimeKindV1::DeclarativeCommandArgv,
            executable: executable.clone(),
            arguments: vec![WindowsOsString { utf16: "--fixture".encode_utf16().collect() }],
            working_directory: working_directory.clone(),
            environment: vec![],
            filesystem_grants: vec![],
            credential: None,
            success_exit_codes: vec![0],
            stdio_protocol_version: None,
            aura_stdio_bootstrap: None,
            aura_stdio_output: None,
            timeout_ms: 5_000,
            output_limits: ProcessOutputLimits {
                stdout_bytes: 1_024,
                stderr_bytes: 1_024,
                combined_bytes: 2_048,
            },
        }),
    };
    let operation = W7ToolOperationFingerprintPreimageV1::new(
        admission.batch.invocation_identity.clone(),
        admission.batch.tool_batch_id,
        normalized.tool_call_id,
        source.clone(),
        aura_domain_common::ContentHash::blake3(&[seed, 42]),
        admission.execution_plan.execution_plan_hash.clone(),
        vec![ResolvedCapability::ProcessExecute],
        vec![ResolvedScope::WorkspaceRoot { workspace_id: snapshot.workspace_id }],
        vec![],
        purpose,
        arguments.clone(),
        aura_domain_common::jcs::canonical_sha256(&arguments).expect("arguments hash"),
        runtime,
        Some(NativeSandboxMode::CompatibilityAppContainer),
        NetworkAccessRequirement::NotRequired,
    )
    .expect("operation preimage");
    assert_eq!(exact.definition.tool_id, source.definition_ref.tool_id);
    let plan = SandboxExecutionPlan {
        execution_id: aura_domain_common::SandboxExecutionId::from_bytes(
            [seed.wrapping_add(43); 16],
        ),
        identity: identity.clone(),
        permission_snapshot_id: snapshot.id,
        mode: NativeSandboxMode::CompatibilityAppContainer,
        network_access: NetworkAccessRequirement::NotRequired,
        network_enforcement: NetworkEnforcementKind::AppContainerNetworkDenied,
        executable,
        args: vec![WindowsOsString { utf16: "--fixture".encode_utf16().collect() }],
        working_directory,
        environment: vec![],
        filesystem_grants: vec![],
        network_grant_ids: vec![],
        timeout_ms: 5_000,
        output_limits: ProcessOutputLimits {
            stdout_bytes: 1_024,
            stderr_bytes: 1_024,
            combined_bytes: 2_048,
        },
        cancellation_id: aura_domain_common::CancellationId::from_bytes(
            [seed.wrapping_add(44); 16],
        ),
        plan_hash: aura_domain_common::ContentHash::blake3(&[seed, 45]),
    };
    let binding = ToolSandboxDispatchBindingV1::new(
        aura_domain_common::SecurityOperationId::from_bytes([seed.wrapping_add(46); 16]),
        identity,
        operation.fingerprint().expect("operation fingerprint"),
        snapshot.clone(),
        PermissionAllowAuthorizationBasisV1::PermissionMode { mode: snapshot.mode },
        admission.batch.tool_batch_id,
        normalized.tool_call_id,
        dispatch.attempt.tool_execution_attempt_id,
        plan.execution_id,
        admission.execution_plan.execution_plan_hash.clone(),
        operation.runtime_binding_hash.clone(),
        source,
        operation.source_security_fingerprint.clone(),
        plan.mode,
        plan.network_access,
        plan.network_enforcement,
        plan.plan_hash.clone(),
        None,
        None,
        dispatch_timestamp(),
    )
    .expect("dispatch binding");
    AuthorizedToolSandboxExecutionV1::new(operation, binding, plan)
        .expect("authorized sandbox execution")
}

fn retry_attempt_dispatch_prepared_request(
    first_start: &ToolExecutionAttemptDispatchPreparedCommitRequest,
    seed: u8,
) -> ToolExecutionAttemptDispatchPreparedCommitRequest {
    let attempt = ToolExecutionAttemptRecordV1 {
        tool_execution_attempt_id: ToolExecutionAttemptId::from_bytes([seed.wrapping_add(25); 16]),
        tool_call_id: first_start.attempt.tool_call_id,
        attempt_number: 2,
        status: ToolExecutionAttemptStatusV1::DispatchPrepared,
        side_effect_state: SideEffectStateV1::NotStarted,
        evidence_ref: None,
        started_at: retry_timestamp(),
        dispatch_prepared_at: Some(retry_dispatch_timestamp()),
        completed_at: None,
        last_progress_sequence: 0,
    };
    let mut call_projection = first_start.call_projection.clone();
    call_projection.attempt_ids.push(attempt.tool_execution_attempt_id);
    let attempt_projection = ToolExecutionAttemptProjection {
        tool_execution_attempt_id: attempt.tool_execution_attempt_id,
        tool_call_id: attempt.tool_call_id,
        attempt_number: attempt.attempt_number,
        status: attempt.status,
        side_effect_state: attempt.side_effect_state,
        progress: None,
        started_at: attempt.started_at,
        dispatch_prepared_at: Some(retry_dispatch_timestamp()),
        completed_at: None,
        safe_error: None,
        user_readable_artifacts: Vec::new(),
        can_cancel: true,
    };
    ToolExecutionAttemptDispatchPreparedCommitRequest {
        attempt,
        execution_plan_hash: first_start.execution_plan_hash.clone(),
        dispatch_evidence_hash: CanonicalHashV1::sha256(&[seed, 29]),
        expected_batch_revision: Revision::new(2),
        expected_call_revision: Revision::new(2),
        admitted_event_id: first_start.admitted_event_id,
        causation_id: CausationId::from_bytes([seed.wrapping_add(26); 16]),
        batch_projection: first_start.batch_projection.clone(),
        call_projection,
        attempt_projection,
        session_activity_projection: first_start.session_activity_projection.clone(),
        correlation_id: first_start.correlation_id,
    }
}

fn runtime_started_request(
    dispatch: &ToolExecutionAttemptDispatchPreparedCommitRequest,
) -> ToolExecutionAttemptRuntimeStartedRequest {
    let mut attempt = dispatch.attempt.clone();
    attempt.status = ToolExecutionAttemptStatusV1::Running;
    attempt.side_effect_state = SideEffectStateV1::Unknown;
    let mut attempt_projection = dispatch.attempt_projection.clone();
    attempt_projection.status = ToolExecutionAttemptStatusV1::Running;
    attempt_projection.side_effect_state = SideEffectStateV1::Unknown;
    ToolExecutionAttemptRuntimeStartedRequest {
        attempt,
        attempt_projection,
        updated_at: runtime_timestamp(),
    }
}

fn progress_request(
    runtime_started: &ToolExecutionAttemptRuntimeStartedRequest,
    sequence: u64,
) -> ToolExecutionAttemptProgressRequest {
    let mut attempt_projection = runtime_started.attempt_projection.clone();
    attempt_projection.progress = Some(ToolAttemptProgressView {
        sequence,
        percentage: Some(25),
        message: Some("fixture progress".to_owned()),
        updated_at: progress_timestamp(),
    });
    ToolExecutionAttemptProgressRequest { attempt_projection }
}

fn cancellation_request(
    runtime_started: &ToolExecutionAttemptRuntimeStartedRequest,
    seed: u8,
) -> ToolExecutionAttemptCancellationRequest {
    let mut attempt = runtime_started.attempt.clone();
    attempt.status = ToolExecutionAttemptStatusV1::Cancelling;
    let mut attempt_projection = runtime_started.attempt_projection.clone();
    attempt_projection.status = ToolExecutionAttemptStatusV1::Cancelling;
    attempt_projection.can_cancel = false;
    ToolExecutionAttemptCancellationRequest {
        attempt,
        reason: ToolCancellationReasonV1::UserRequested,
        requested_at: cancellation_timestamp(),
        attempt_projection,
        causation_id: CausationId::from_bytes([seed.wrapping_add(23); 16]),
        correlation_id: CorrelationId::from_bytes([seed.wrapping_add(24); 16]),
    }
}

fn retryable_failure_request(
    _coordinator: &PersistenceCoordinator,
    running: &ToolExecutionAttemptRuntimeStartedRequest,
    seed: u8,
) -> ToolExecutionAttemptRetryableFailureRequest {
    let attempt_evidence_artifact = ArtifactWriteDraftV1::new(
        ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.tool-attempt-evidence.v1+json".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: running.attempt.tool_execution_attempt_id,
                },
            },
            bytes: br#"{"runtime_terminated":true,"persistent_effect":false}"#.to_vec(),
        },
        terminal_timestamp(),
    )
    .expect("retryable attempt evidence draft");
    let evidence = attempt_evidence_artifact.record().expect("retryable attempt evidence");
    let correlation_id = CorrelationId::from_bytes([seed.wrapping_add(28); 16]);
    let error = ErrorEnvelope::new(
        ToolRuntimeErrorCode::DispatchFailed.error_code(),
        ErrorCategory::Unavailable,
        "The tool runtime dispatch failed.",
        true,
        correlation_id,
        None,
    );
    let mut attempt = running.attempt.clone();
    attempt.status = ToolExecutionAttemptStatusV1::Failed;
    attempt.side_effect_state = SideEffectStateV1::NoEffect;
    attempt.evidence_ref = Some(evidence.artifact_id);
    attempt.completed_at = Some(terminal_timestamp());
    let mut attempt_projection = running.attempt_projection.clone();
    attempt_projection.status = attempt.status;
    attempt_projection.side_effect_state = attempt.side_effect_state;
    attempt_projection.completed_at = attempt.completed_at;
    attempt_projection.safe_error = Some(ToolProjectionErrorView {
        code: error.code.clone(),
        category: error.category,
        message: error.message.clone(),
        retryable: error.retryable,
    });
    attempt_projection.can_cancel = false;
    ToolExecutionAttemptRetryableFailureRequest {
        attempt,
        error,
        attempt_projection,
        attempt_evidence_artifact,
        causation_id: CausationId::from_bytes([seed.wrapping_add(27); 16]),
        correlation_id,
    }
}

fn retryable_failure_with_side_effect(
    mut request: ToolExecutionAttemptRetryableFailureRequest,
    side_effect_state: SideEffectStateV1,
) -> ToolExecutionAttemptRetryableFailureRequest {
    request.attempt.side_effect_state = side_effect_state;
    request.attempt_projection.side_effect_state = side_effect_state;
    request
}

fn successful_finalization_request(
    _coordinator: &PersistenceCoordinator,
    admission: &ToolBatchAdmissionCommitRequest,
    dispatch: &ToolExecutionAttemptDispatchPreparedCommitRequest,
    runtime_started: &ToolExecutionAttemptRuntimeStartedRequest,
) -> ToolExecutionAttemptFinalizationRequest {
    let attempt_evidence_artifact = ArtifactWriteDraftV1::new(
        ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.tool-attempt-evidence.v1+json".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: runtime_started.attempt.tool_execution_attempt_id,
                },
            },
            bytes: br#"{"runtime_terminated":true,"persistent_effect":false}"#.to_vec(),
        },
        terminal_timestamp(),
    )
    .expect("attempt terminal evidence draft");
    let attempt_evidence = attempt_evidence_artifact.record().expect("attempt terminal evidence");
    let aggregate_side_effect_evidence_artifact = ArtifactWriteDraftV1::new(
        ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.tool-side-effect-aggregate.v1+json".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ToolCall {
                    tool_call_id: runtime_started.attempt.tool_call_id,
                },
            },
            bytes: br#"{"aggregate":"no_effect","attempts":1}"#.to_vec(),
        },
        terminal_timestamp(),
    )
    .expect("aggregate side-effect evidence draft");
    let aggregate_evidence =
        aggregate_side_effect_evidence_artifact.record().expect("aggregate side-effect evidence");
    let model_visible_result_artifact = ArtifactWriteDraftV1::new(
        ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.model-visible-tool-result.v1+json".to_owned(),
            access: ArtifactAccess::MODEL_READ,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ToolCall {
                    tool_call_id: runtime_started.attempt.tool_call_id,
                },
            },
            bytes: br#"{"schema_version":1,"fixture":true}"#.to_vec(),
        },
        terminal_timestamp(),
    )
    .expect("model-visible result envelope draft");
    let model_visible =
        model_visible_result_artifact.record().expect("model-visible result envelope");
    let user_audit_result_artifact = ArtifactWriteDraftV1::new(
        ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: "application/vnd.aura.user-audit-tool-result.v1+json".to_owned(),
            access: ArtifactAccess::USER_READ,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ToolCall {
                    tool_call_id: runtime_started.attempt.tool_call_id,
                },
            },
            bytes: br#"{"schema_version":1,"fixture":true}"#.to_vec(),
        },
        terminal_timestamp(),
    )
    .expect("user-audit result envelope draft");
    let user_audit = user_audit_result_artifact.record().expect("user-audit result envelope");

    let mut attempt = runtime_started.attempt.clone();
    attempt.status = ToolExecutionAttemptStatusV1::Succeeded;
    attempt.side_effect_state = SideEffectStateV1::NoEffect;
    attempt.evidence_ref = Some(attempt_evidence.artifact_id);
    attempt.completed_at = Some(terminal_timestamp());
    let result = ToolResultV1 {
        tool_call_id: attempt.tool_call_id,
        outcome: ToolResultOutcomeV1::Succeeded,
        side_effect_state: SideEffectStateV1::NoEffect,
        attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
            tool_execution_attempt_id: attempt.tool_execution_attempt_id,
            attempt_number: attempt.attempt_number,
            side_effect_state: SideEffectStateV1::NoEffect,
            side_effect_evidence_ref: Some(attempt_evidence.artifact_id),
            dispatched: true,
        }],
        model_visible_result_ref: Some(model_visible.artifact_id),
        user_audit_result_ref: Some(user_audit.artifact_id),
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: Some(aggregate_evidence.artifact_id),
        started_at: Some(attempt.started_at),
        completed_at: terminal_timestamp(),
    };
    let batch_result = ToolBatchResultV1 {
        tool_batch_id: admission.batch.tool_batch_id,
        status: ToolBatchStatusV1::Completed,
        results: vec![result.clone(), admission.not_executed_results[0].clone()],
        completed_at: terminal_timestamp(),
    };

    let mut attempt_projection = runtime_started.attempt_projection.clone();
    attempt_projection.status = ToolExecutionAttemptStatusV1::Succeeded;
    attempt_projection.side_effect_state = SideEffectStateV1::NoEffect;
    attempt_projection.completed_at = Some(terminal_timestamp());
    attempt_projection.can_cancel = false;

    let mut call_projection = dispatch.call_projection.clone();
    call_projection.status = ToolCallStatusV1::Finalized;
    call_projection.outcome = Some(ToolResultOutcomeSummaryView::Succeeded);
    call_projection.side_effect_state = Some(SideEffectStateV1::NoEffect);
    call_projection.completed_at = Some(terminal_timestamp());
    call_projection.model_visible_result = Some(aura_tool_contract::UserReadableToolArtifactView {
        artifact_id: model_visible.artifact_id,
        media_type: model_visible.media_type.clone(),
        byte_length: model_visible.byte_length,
    });
    call_projection.user_audit_result = Some(aura_tool_contract::UserReadableToolArtifactView {
        artifact_id: user_audit.artifact_id,
        media_type: user_audit.media_type.clone(),
        byte_length: user_audit.byte_length,
    });
    call_projection.can_cancel = false;

    let mut batch_projection = dispatch.batch_projection.clone();
    batch_projection.status = ToolBatchStatusV1::Completed;
    batch_projection.completed_at = Some(terminal_timestamp());
    batch_projection.can_cancel = false;
    let batch_call = batch_projection
        .calls
        .iter_mut()
        .find(|summary| summary.tool_call_id == attempt.tool_call_id)
        .expect("batch call summary");
    batch_call.status = ToolCallStatusV1::Finalized;
    batch_call.outcome = Some(ToolResultOutcomeSummaryView::Succeeded);
    batch_call.side_effect_state = Some(SideEffectStateV1::NoEffect);
    batch_call.completed_at = Some(terminal_timestamp());
    batch_call.can_cancel = false;

    let mut session_activity_projection = dispatch.session_activity_projection.clone();
    session_activity_projection
        .active_batches
        .retain(|summary| summary.tool_batch_id != admission.batch.tool_batch_id);
    let recent_call = session_activity_projection
        .recent_calls
        .iter_mut()
        .find(|summary| summary.tool_call_id == attempt.tool_call_id)
        .expect("session call summary");
    recent_call.status = ToolCallStatusV1::Finalized;
    recent_call.outcome = Some(ToolResultOutcomeSummaryView::Succeeded);
    recent_call.side_effect_state = Some(SideEffectStateV1::NoEffect);
    recent_call.completed_at = Some(terminal_timestamp());
    recent_call.can_cancel = false;

    ToolExecutionAttemptFinalizationRequest {
        attempt,
        terminal: ToolExecutionAttemptTerminalFact::Succeeded,
        result,
        batch_result: Some(batch_result),
        expected_batch_revision: Revision::new(2),
        expected_call_revision: Revision::new(2),
        batch_projection,
        call_projection,
        attempt_projection,
        session_activity_projection,
        attempt_evidence_artifact,
        aggregate_side_effect_evidence_artifact,
        model_visible_result_artifact,
        user_audit_result_artifact,
        result_artifact_access_links: vec![
            ArtifactAccessLinkV1::new(
                model_visible.artifact_id,
                ArtifactAccessSubjectV1::Session {
                    session_id: admission.batch.invocation_identity.session_id,
                },
                ArtifactAccess::MODEL_READ,
                terminal_timestamp(),
            )
            .expect("model access link"),
            ArtifactAccessLinkV1::new(
                user_audit.artifact_id,
                ArtifactAccessSubjectV1::Session {
                    session_id: admission.batch.invocation_identity.session_id,
                },
                ArtifactAccess::USER_READ,
                terminal_timestamp(),
            )
            .expect("user access link"),
        ],
        causation_id: CausationId::from_bytes([241; 16]),
        correlation_id: admission.correlation_id,
    }
}

fn cancelled_finalization_request(
    mut request: ToolExecutionAttemptFinalizationRequest,
) -> ToolExecutionAttemptFinalizationRequest {
    let reason = ToolCancellationReasonV1::UserRequested;
    request.attempt.status = ToolExecutionAttemptStatusV1::Cancelled;
    request.terminal = ToolExecutionAttemptTerminalFact::Cancelled { reason };
    request.result.outcome = ToolResultOutcomeV1::Cancelled { reason };
    request.attempt_projection.status = ToolExecutionAttemptStatusV1::Cancelled;
    request.call_projection.outcome = Some(ToolResultOutcomeSummaryView::Cancelled { reason });
    let batch_call = request
        .batch_projection
        .calls
        .iter_mut()
        .find(|summary| summary.tool_call_id == request.result.tool_call_id)
        .expect("batch call summary");
    batch_call.outcome = Some(ToolResultOutcomeSummaryView::Cancelled { reason });
    let session_call = request
        .session_activity_projection
        .recent_calls
        .iter_mut()
        .find(|summary| summary.tool_call_id == request.result.tool_call_id)
        .expect("session call summary");
    session_call.outcome = Some(ToolResultOutcomeSummaryView::Cancelled { reason });
    request.batch_result.as_mut().expect("completed batch result").results[0].outcome =
        ToolResultOutcomeV1::Cancelled { reason };
    request
}

fn seed_scope(
    coordinator: &PersistenceCoordinator,
    database_path: &std::path::Path,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    turn_task_id: TurnTaskId,
    agent_run_id: AgentRunId,
    seed: u8,
) {
    coordinator
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: format!("P07 Tool Workspace {seed}"),
                selected_root: format!(r"C:\Aura Tests\P07\{seed}"),
                canonical_root: format!(r"C:\Aura Tests\P07\{seed}"),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id,
                workspace_id,
                title: format!("P07 Tool Session {seed}"),
                state: "running".to_owned(),
            },
            CorrelationId::from_bytes([seed.wrapping_add(5); 16]),
        )
        .expect("seed workspace and session");
    coordinator
        .ensure_session_permission_state(
            session_id,
            workspace_id,
            PermissionMode::ReadOnly,
            CorrelationId::from_bytes([seed.wrapping_add(6); 16]),
        )
        .expect("seed permission state");
    let connection = Connection::open(database_path).expect("open fixture database");
    connection.execute_batch("PRAGMA foreign_keys=ON;").expect("foreign keys");
    let now = timestamp().to_string();
    connection
        .execute(
            "INSERT INTO turn_tasks(turn_task_id, session_id, state, created_at, updated_at)
             VALUES (?1, ?2, 'running', ?3, ?3)",
            params![turn_task_id.to_string(), session_id.to_string(), now],
        )
        .expect("seed turn task");
    connection
        .execute(
            "INSERT INTO agent_runs(
                agent_run_id, turn_task_id, parent_agent_run_id, state, created_at, updated_at
             ) VALUES (?1, ?2, NULL, 'running', ?3, ?3)",
            params![agent_run_id.to_string(), turn_task_id.to_string(), now],
        )
        .expect("seed agent run");
}

fn install_request(seed: u8, automatic_retry_allowed: bool) -> VerifiedToolPackageInstallRequest {
    let package_id = ToolPackageId::from_bytes([seed; 16]);
    let tool_id = ToolId::from_bytes([seed.wrapping_add(1); 16]);
    let instance_id = ToolInstanceId::from_bytes([seed.wrapping_add(2); 16]);
    let package_version = CanonicalSemVer::new("1.0.0").expect("package version");
    let tool_version =
        ToolVersion::new(Some(package_version.clone()), Revision::INITIAL).expect("tool version");
    let package_key = PackageKey::new(format!("fixture.package{seed}")).expect("package key");
    let tool_key = ToolKey::new(format!("tool{seed}")).expect("tool key");
    let package_fingerprint = PackageFingerprintV1(CanonicalHashV1::sha256(&[seed, 1]));
    let security_fingerprint =
        PackageSecurityFingerprintV1(CanonicalBlake3HashV1::blake3(&[seed, 2]));
    let definition = ToolDefinitionRecordV1::new(
        tool_id,
        tool_version.clone(),
        Some(package_id),
        Some(package_key.clone()),
        Some(tool_key),
        ToolCanonicalName::new(format!("fixture.tool{seed}")).expect("canonical name"),
        format!("Fixture Tool {seed}"),
        format!("Execute fixture tool {seed}"),
        format!("Fixture tool {seed} summary"),
        ToolInputSchemaV1::new(json!({
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"],
            "additionalProperties": false
        }))
        .expect("input schema"),
        false,
        ToolResultContractV1::Unit,
        ToolRuntimeDescriptorV1::ExternalSource {
            source_contract_key: format!("fixture.package.{seed}.v1"),
        },
        Vec::new(),
        Vec::new(),
        ToolAudienceV1::SubagentConfigurable,
        ToolSourceKindV1::Imported,
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed,
            repeat_is_idempotent: true,
            may_create_persistent_effects: false,
        },
        CanonicalHashV1::sha256(&[seed, 4]),
        Revision::INITIAL,
        timestamp(),
    )
    .expect("definition");
    let instance = ToolInstanceRecordV1 {
        tool_instance_id: instance_id,
        tool_id,
        tool_version,
        source_kind: ToolSourceKindV1::Imported,
        source_contract_fingerprint: aura_tool_contract::ToolSourceContractFingerprintV1::package(
            ToolSourceKindV1::Imported,
            aura_tool_contract::ToolContractFingerprintV1 {
                package_runtime_closure_hash: aura_tool_contract::PackageRuntimeClosureHashV1(
                    CanonicalHashV1::sha256(&[seed, 4]),
                ),
                fingerprint: CanonicalHashV1::sha256(&[seed, 5]),
            },
        )
        .expect("package source fingerprint"),
        enabled: true,
        runtime_health: ToolRuntimeHealthV1::Healthy,
        overrides: ToolInstanceOverridesV1 {
            timeout_ms: None,
            max_retries: None,
            max_concurrent_attempts: None,
            subagent_enabled: None,
        },
        entity_revision: Revision::INITIAL,
        created_at: timestamp(),
        updated_at: timestamp(),
    };
    VerifiedToolPackageInstallRequest {
        package: ToolPackageRecordV1 {
            tool_package_id: package_id,
            package_key: package_key.clone(),
            active_version: Some(package_version.clone()),
            lifecycle: ToolPackageLifecycleV1::Installed,
            deletion_pending: false,
            entity_revision: Revision::INITIAL,
            created_at: timestamp(),
            updated_at: timestamp(),
        },
        package_version: ToolPackageVersionRecordV1 {
            tool_package_id: package_id,
            package_version,
            package_fingerprint,
            package_security_fingerprint: security_fingerprint,
            manifest_hash: CanonicalHashV1::sha256(&[seed, 3]),
            package_store_relative_path: PackageRelativePath::new(format!(
                "packages/fixture-package{seed}/1.0.0"
            ))
            .expect("store path"),
            conformance_run_ids: vec![ToolConformanceRunId::from_bytes([seed.wrapping_add(3); 16])],
            installed_at: timestamp(),
            payload_strong_reference_count: 1,
            payload_gc_eligible_at: None,
        },
        definitions: vec![definition],
        instances: vec![instance],
        correlation_id: CorrelationId::from_bytes([seed.wrapping_add(4); 16]),
    }
}

fn registry_snapshot(
    workspace_id: WorkspaceId,
    session_id: SessionId,
    turn_task_id: TurnTaskId,
    agent_run_id: AgentRunId,
    definition: &ToolDefinitionRecordV1,
    instance: &ToolInstanceRecordV1,
    seed: u8,
) -> ToolRegistrySnapshotV1 {
    let schema = ToolSchemaSnapshotV1::new(
        definition.tool_id,
        definition.tool_version.clone(),
        definition.canonical_name.clone(),
        definition.model_description.clone(),
        definition.input_schema.clone(),
        definition.strict,
    )
    .expect("schema snapshot");
    let catalog = ToolRegistryCatalogEntryV1 {
        tool_id: definition.tool_id,
        tool_version: definition.tool_version.clone(),
        tool_instance_id: instance.tool_instance_id,
        canonical_name: definition.canonical_name.clone(),
        display_name: definition.display_name.clone(),
        summary: definition.model_summary.clone(),
        source: definition.source_kind,
        definition_fingerprint: definition.definition_fingerprint.clone(),
        source_contract_fingerprint: instance.source_contract_fingerprint.clone(),
        load_state: ToolLoadStateV1::Ephemeral,
    };
    let binding = EffectiveToolBindingSnapshot {
        tool_id: definition.tool_id,
        tool_version: definition.tool_version.clone(),
        tool_instance_id: instance.tool_instance_id,
        source_kind: definition.source_kind,
        exposure: ToolExposureV1::EphemeralLoad,
        definition_fingerprint: definition.definition_fingerprint.clone(),
        source_contract_fingerprint: instance.source_contract_fingerprint.clone(),
    };
    ToolRegistrySnapshotV1::new(
        ToolRegistrySnapshotId::from_bytes([seed.wrapping_add(6); 16]),
        Revision::INITIAL,
        ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, agent_run_id },
        None,
        None,
        CanonicalHashV1::sha256(&[seed, 6]),
        vec![schema],
        vec![catalog],
        vec![binding],
        timestamp(),
    )
    .expect("registry snapshot")
}

fn table_count(connection: &Connection, table: &str) -> rusqlite::Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    connection.query_row(&sql, [], |row| row.get(0))
}

type AuthorityCounts = (i64, i64, i64, i64, i64, i64, i64, i64, i64);

fn authority_counts(connection: &Connection) -> rusqlite::Result<AuthorityCounts> {
    Ok((
        table_count(connection, "tool_batches")?,
        table_count(connection, "tool_calls")?,
        table_count(connection, "tool_execution_attempts")?,
        table_count(connection, "tool_attempt_operational_state")?,
        table_count(connection, "tool_results")?,
        table_count(connection, "events")?,
        table_count(connection, "projection_outbox")?,
        table_count(connection, "artifacts")?,
        table_count(connection, "artifact_access_links")?,
    ))
}
