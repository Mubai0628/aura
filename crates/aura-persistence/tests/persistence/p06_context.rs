use std::collections::BTreeSet;

use aura_artifact_contract::{ArtifactAccess, ArtifactOwnerRef, ArtifactScope};
use aura_context_domain::{
    CompactionAnchorKind, CompactionModelAttemptState, ContextCompactionStateView,
    ContextOverflowDiagnosticView, ContextPrivacyClass, ContextRemediationClearReason,
    ContextRemediationPlan, ContextRemediationState, ContextSearchFilters, ContextSourceRef,
    ManualCompactionRequestState, StructuredCompactionRecord,
};
use aura_context_fixtures::{
    FixtureArtifact, P06PreparedFixture, p06_followup_prepared_context_fixture,
    p06_prepared_context_fixture,
};
use aura_context_runtime::{
    CompactionSourceDraft, build_anchor_manifest, parse_and_validate_compaction_output,
};
use aura_domain_common::{
    ArtifactId, CommandInvocationId, ContentHash, CorrelationId, EventId, ModelContentBlockId,
    NetworkRequestId, Revision, RuntimeInstanceId, TransportAttemptId, UtcTimestamp,
    canonical_json_v1,
};
use aura_model_domain::{
    AssistantContentBlock, ModelAdapterIdentity, ModelApiFormat, ModelUsage, NormalizedModelOutput,
    NormalizedStopReason, ProviderRequestEvidence, ProviderRequestHeaderClass,
    ProviderRequestHeaderMetadata, ProviderRequestTargetMetadata, TransportAttemptKind,
    TransportAttemptRecord, TransportAttemptState, TransportOperationContext, UsageSource,
};
use aura_permission_domain::NormalizedOrigin;
use aura_persistence::{
    ArtifactWriteRequest, ContextArtifactDraft, ContextCompactionCompletedRequest,
    ContextCompactionCreateRequest, ContextCompactionRunningRequest, ContextCompactionSourceDraft,
    ContextCompactionTerminalRequest, ContextManualCompactionCreateRequest,
    ContextManualCompactionRunningRequest, ContextProjectionCommitRequest,
    ContextRemediationClearedRequest, ContextRemediationRequiredRequest, ContextSearchChunkDraft,
    ContextSearchStoredQuery, ContextSourceEventCommitRequest, EventActor, PersistenceCoordinator,
    PersistenceError, PersistenceFaultPoint, PersistenceOptions, PersistenceStartup,
    PersistentEventDraft, PromptBundleCommitRequest, StoredContextProjectionSnapshot,
};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use rusqlite::{Connection, params};

fn start(
    database_path: std::path::PathBuf,
    artifact_root: std::path::PathBuf,
) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        database_path,
        artifact_root,
        RuntimeInstanceId::new(),
    ))
    .expect("start persistence")
    {
        PersistenceStartup::Ready(coordinator) => coordinator,
        PersistenceStartup::RecoveryRequired(recovery) => {
            panic!("unexpected recovery requirement: {}", recovery.reason)
        }
    }
}

fn seed_scope(
    persistence: &PersistenceCoordinator,
    database_path: &std::path::Path,
    fixture: &P06PreparedFixture,
) {
    persistence
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id: fixture.workspace_id,
                display_name: "P06 Context Workspace".to_owned(),
                selected_root: r"C:\Aura Tests\P06 Context".to_owned(),
                canonical_root: r"C:\Aura Tests\P06 Context".to_owned(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id: fixture.session_id,
                workspace_id: fixture.workspace_id,
                title: "P06 Context".to_owned(),
                state: "running".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("seed workspace and session");
    let connection = Connection::open(database_path).expect("open test database");
    connection.execute_batch("PRAGMA foreign_keys=ON;").expect("foreign keys");
    let now = UtcTimestamp::now().to_string();
    connection
        .execute(
            "INSERT INTO turn_tasks(turn_task_id, session_id, state, created_at, updated_at)
             VALUES (?1, ?2, 'running', ?3, ?3)",
            params![fixture.turn_task_id.to_string(), fixture.session_id.to_string(), now,],
        )
        .expect("seed turn task");
    connection
        .execute(
            "INSERT INTO agent_runs(
                agent_run_id, turn_task_id, parent_agent_run_id, state, created_at, updated_at
             ) VALUES (?1, ?2, NULL, 'running', ?3, ?3)",
            params![fixture.agent_run_id.to_string(), fixture.turn_task_id.to_string(), now,],
        )
        .expect("seed agent run");
}

fn context_artifact(artifact: FixtureArtifact) -> ContextArtifactDraft {
    ContextArtifactDraft {
        artifact_id: artifact.artifact_id,
        media_type: artifact.media_type,
        bytes: artifact.bytes,
    }
}

fn manual_compaction_request(
    fixture: &P06PreparedFixture,
    command_invocation_id: CommandInvocationId,
    focus: Option<&str>,
) -> ContextManualCompactionCreateRequest {
    let command = focus.map_or_else(|| "/compact".to_owned(), |focus| format!("/compact {focus}"));
    ContextManualCompactionCreateRequest {
        session_id: fixture.session_id,
        agent_run_id: fixture.agent_run_id,
        command_invocation_id,
        command_event: PersistentEventDraft {
            event_id: EventId::new(),
            workspace_id: Some(fixture.workspace_id),
            session_id: Some(fixture.session_id),
            turn_task_id: Some(fixture.turn_task_id),
            agent_run_id: Some(fixture.agent_run_id),
            event_type: "ManualCompactionRequested".to_owned(),
            event_version: 1,
            actor: EventActor::User,
            correlation_id: CorrelationId::new(),
            causation_id: None,
            atomic_group_id: None,
            payload_json: None,
            payload_artifact_ref: None,
            searchable_text: None,
            created_at: UtcTimestamp::now(),
        },
        command_artifact: ContextArtifactDraft {
            artifact_id: ArtifactId::new(),
            media_type: "text/plain; charset=utf-8".to_owned(),
            bytes: command.into_bytes(),
        },
        focus_artifact: focus.map(|focus| ContextArtifactDraft {
            artifact_id: ArtifactId::new(),
            media_type: "text/plain; charset=utf-8".to_owned(),
            bytes: focus.as_bytes().to_vec(),
        }),
    }
}

fn commit_prompt_bundle(persistence: &PersistenceCoordinator, fixture: &P06PreparedFixture) {
    persistence
        .commit_prompt_bundle_snapshot(PromptBundleCommitRequest {
            session_id: fixture.session_id,
            snapshot: fixture.prompt_bundle_snapshot.clone(),
            artifacts: fixture
                .prompt_bundle_artifacts
                .clone()
                .into_iter()
                .map(context_artifact)
                .collect(),
            correlation_id: CorrelationId::new(),
        })
        .expect("commit prompt bundle");
}

fn commit_context_fixture(persistence: &PersistenceCoordinator, fixture: &P06PreparedFixture) {
    persistence
        .commit_context_source_event(ContextSourceEventCommitRequest {
            event: PersistentEventDraft {
                event_id: fixture.current_user_event_id,
                workspace_id: Some(fixture.workspace_id),
                session_id: Some(fixture.session_id),
                turn_task_id: Some(fixture.turn_task_id),
                agent_run_id: Some(fixture.agent_run_id),
                event_type: "RootUserMessageAccepted".to_owned(),
                event_version: 1,
                actor: EventActor::User,
                correlation_id: CorrelationId::new(),
                causation_id: None,
                atomic_group_id: None,
                payload_json: None,
                payload_artifact_ref: None,
                searchable_text: Some(fixture.current_user_content.clone()),
                created_at: UtcTimestamp::now(),
            },
            canonical_content: fixture.current_user_content.clone(),
            model_visible: true,
            explicitly_user_authored: true,
        })
        .expect("commit source event and authorization fact");
    persistence
        .commit_context_projection_snapshot(ContextProjectionCommitRequest {
            snapshot: fixture.prepared.context.snapshot.clone(),
            execution_snapshot: fixture.execution_snapshot.clone(),
            instruction_envelope: fixture.prepared.context.instruction_envelope.clone(),
            model_egress_manifest: fixture.prepared.context.model_egress_manifest.clone(),
            artifacts: fixture
                .prepared
                .artifacts
                .clone()
                .into_iter()
                .map(|artifact| ContextArtifactDraft {
                    artifact_id: artifact.artifact_id,
                    media_type: artifact.media_type,
                    bytes: artifact.bytes,
                })
                .collect(),
            prompt_inspector: fixture.prepared.prompt_inspector.clone(),
            search_chunks: Vec::new(),
            correlation_id: CorrelationId::new(),
        })
        .expect("commit context fixture graph");
}

fn compaction_projection(
    source: &P06PreparedFixture,
    state: ContextCompactionStateView,
    active_model_step_id: Option<aura_domain_common::ModelStepId>,
) -> aura_context_domain::PromptInspectorProjection {
    let mut projection = source.prepared.prompt_inspector.clone();
    projection.active_model_step_id = active_model_step_id;
    projection.context_snapshot_id = Some(source.prepared.context.snapshot.id);
    projection.compaction.state = state;
    projection.compaction.source_context_snapshot_id = Some(source.prepared.context.snapshot.id);
    projection
}

fn request_evidence(fixture: &P06PreparedFixture) -> ProviderRequestEvidence {
    let body = br#"{"stream":true}"#;
    ProviderRequestEvidence {
        body_sha256: ContentHash::sha256(body),
        body_byte_length: u64::try_from(body.len()).expect("body length"),
        target: ProviderRequestTargetMetadata {
            method: "POST".to_owned(),
            origin: NormalizedOrigin {
                scheme: "https".to_owned(),
                host: "api.example.test".to_owned(),
                port: 443,
            },
            path: "/v1/responses".to_owned(),
            query_parameter_names: BTreeSet::new(),
        },
        headers: vec![ProviderRequestHeaderMetadata {
            name: "content-type".to_owned(),
            class: ProviderRequestHeaderClass::ContentMetadata,
            non_secret_value: Some("application/json".to_owned()),
        }],
        compatibility_removals: BTreeSet::new(),
        applied_output_token_field_alias: None,
        adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses),
        operation_context: TransportOperationContext::AgentModelAttempt {
            model_step_id: fixture.execution_snapshot.model_step_id,
            model_execution_snapshot_id: fixture.execution_snapshot.id,
            context_snapshot_id: fixture.execution_snapshot.context_snapshot_id,
            prompt_bundle_snapshot_id: fixture.execution_snapshot.prompt_bundle_snapshot_id,
        },
        tool_registry_snapshot_ref: Some(
            fixture.prepared.context.tool_schema_set.registry_snapshot_ref.clone(),
        ),
        tool_name_bindings: Vec::new(),
        historical_tool_call_wire_bindings: Vec::new(),
        canonical_body_artifact_id: None,
    }
}

fn completed_transport_attempt(
    persistence: &PersistenceCoordinator,
    fixture: &P06PreparedFixture,
    output_text: &str,
) -> TransportAttemptRecord {
    let raw_stream = persistence
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "text/event-stream".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Session { session_id: fixture.session_id },
            bytes: output_text.as_bytes().to_vec(),
        })
        .expect("write compaction raw stream");
    let mut attempt = TransportAttemptRecord {
        id: TransportAttemptId::new(),
        attempt_number: 1,
        attempt_kind: TransportAttemptKind::Initial,
        client_request_id: NetworkRequestId::new().to_string(),
        provider_request_id: None,
        endpoint_id: fixture.execution_snapshot.endpoint_id,
        endpoint_fingerprint: fixture.execution_snapshot.endpoint_fingerprint.clone(),
        request_evidence: request_evidence(fixture),
        state: TransportAttemptState::QueuedForEndpoint,
        started_at: UtcTimestamp::now(),
        first_byte_at: None,
        first_model_event_at: None,
        finished_at: None,
        raw_stream_artifact_id: None,
        failure_id: None,
    };
    persistence
        .create_transport_attempt(CorrelationId::new(), &attempt)
        .expect("create compaction transport attempt");
    for state in [
        TransportAttemptState::AcquiringPermit,
        TransportAttemptState::PreparingRequest,
        TransportAttemptState::AuthorizingNetwork,
        TransportAttemptState::Authorized,
        TransportAttemptState::DispatchPrepared,
        TransportAttemptState::Connecting,
        TransportAttemptState::ResponseStarted,
        TransportAttemptState::ResponseCompleted,
        TransportAttemptState::Completed,
    ] {
        attempt.state = state;
        if state == TransportAttemptState::Completed {
            attempt.raw_stream_artifact_id = Some(raw_stream.artifact_id);
            attempt.finished_at = Some(UtcTimestamp::now());
        }
        persistence
            .transition_transport_attempt(CorrelationId::new(), &attempt)
            .expect("advance compaction transport attempt");
    }
    let output = NormalizedModelOutput {
        model_step_id: fixture.execution_snapshot.model_step_id,
        assistant_blocks: vec![AssistantContentBlock {
            block_id: ModelContentBlockId::new(),
            text: output_text.to_owned(),
            provider_reference: None,
        }],
        reasoning_blocks: Vec::new(),
        tool_calls: Vec::new(),
        refusal: None,
        stop_reason: NormalizedStopReason::EndTurn,
        usage: ModelUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            reasoning_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
            source: UsageSource::LocalEstimated,
        },
        provider_continuation: None,
        provider_request_id: None,
        raw_response_ref: Some(raw_stream.artifact_id),
    };
    persistence
        .store_model_output(&fixture.execution_snapshot, attempt.id, &output)
        .expect("persist compaction model output");
    attempt
}

fn transport_attempt_at_state(
    persistence: &PersistenceCoordinator,
    fixture: &P06PreparedFixture,
    target: TransportAttemptState,
) -> TransportAttemptRecord {
    let mut attempt = TransportAttemptRecord {
        id: TransportAttemptId::new(),
        attempt_number: 1,
        attempt_kind: TransportAttemptKind::Initial,
        client_request_id: NetworkRequestId::new().to_string(),
        provider_request_id: None,
        endpoint_id: fixture.execution_snapshot.endpoint_id,
        endpoint_fingerprint: fixture.execution_snapshot.endpoint_fingerprint.clone(),
        request_evidence: request_evidence(fixture),
        state: TransportAttemptState::QueuedForEndpoint,
        started_at: UtcTimestamp::now(),
        first_byte_at: None,
        first_model_event_at: None,
        finished_at: None,
        raw_stream_artifact_id: None,
        failure_id: None,
    };
    persistence
        .create_transport_attempt(CorrelationId::new(), &attempt)
        .expect("create active compaction transport attempt");
    if target == TransportAttemptState::QueuedForEndpoint {
        return attempt;
    }
    for state in [
        TransportAttemptState::AcquiringPermit,
        TransportAttemptState::PreparingRequest,
        TransportAttemptState::AuthorizingNetwork,
        TransportAttemptState::Authorized,
        TransportAttemptState::DispatchPrepared,
        TransportAttemptState::Connecting,
        TransportAttemptState::ResponseStarted,
        TransportAttemptState::ResponseCompleted,
        TransportAttemptState::Completed,
    ] {
        if matches!(
            state,
            TransportAttemptState::ResponseStarted | TransportAttemptState::ResponseCompleted
        ) {
            attempt.first_byte_at.get_or_insert_with(UtcTimestamp::now);
        }
        attempt.state = state;
        if matches!(
            state,
            TransportAttemptState::Completed
                | TransportAttemptState::Cancelled
                | TransportAttemptState::Failed
        ) {
            attempt.finished_at = Some(UtcTimestamp::now());
        }
        persistence
            .transition_transport_attempt(CorrelationId::new(), &attempt)
            .expect("advance active compaction transport attempt");
        if state == target {
            return attempt;
        }
    }
    panic!("unsupported target TransportAttempt state: {target:?}");
}

fn valid_compaction_output() -> String {
    "# Historical-Snapshot-Summary\nHistory a000001\n\n# Active-Turn-Progress-Summary\nActive\n\n# Structured-Compaction\n## Summary\nSummary\n## Full-Turn-Session-Line\nLine\n## Active-Turn-Segment-Line\nSegment\n## Truth-Index\n- a000001\n".to_owned()
}

fn create_requested_compaction(
    persistence: &PersistenceCoordinator,
    source: &P06PreparedFixture,
    sequence: u32,
) -> aura_context_domain::CompactionAnchorManifest {
    let source_ref = ContextSourceRef::Event { event_id: source.current_user_event_id };
    let source_hash = ContentHash::sha256(source.current_user_content.as_bytes());
    let anchor_manifest = build_anchor_manifest(vec![CompactionSourceDraft {
        kind: CompactionAnchorKind::Event,
        source_ref: source_ref.clone(),
        source_hash: source_hash.clone(),
        preview: "root user message".to_owned(),
    }])
    .expect("anchor manifest");
    let source_span = aura_context_domain::CompactionSourceSpan {
        start_sequence: aura_domain_common::SessionSequence::new(1),
        end_sequence: aura_domain_common::SessionSequence::new(1),
        source_refs: vec![source_ref.clone()],
    };
    persistence
        .create_context_compaction(ContextCompactionCreateRequest {
            source_context_snapshot_id: source.prepared.context.snapshot.id,
            sequence,
            source_span,
            sources: vec![ContextCompactionSourceDraft { source_ref, source_hash }],
            focus_artifact: None,
            requested_at: UtcTimestamp::now(),
            prompt_inspector: compaction_projection(
                source,
                ContextCompactionStateView::Required,
                Some(source.execution_snapshot.model_step_id),
            ),
            correlation_id: CorrelationId::new(),
        })
        .expect("create context compaction");
    anchor_manifest
}

fn create_and_start_compaction(
    persistence: &PersistenceCoordinator,
    source: &P06PreparedFixture,
    attempt: &P06PreparedFixture,
    sequence: u32,
) -> aura_context_domain::CompactionAnchorManifest {
    let anchor_manifest = create_requested_compaction(persistence, source, sequence);
    persistence
        .mark_context_compaction_running(ContextCompactionRunningRequest {
            source_context_snapshot_id: source.prepared.context.snapshot.id,
            sequence,
            attempt_ordinal: 1,
            attempt_context_snapshot_id: attempt.prepared.context.snapshot.id,
            model_step_id: attempt.execution_snapshot.model_step_id,
            model_execution_snapshot_id: attempt.execution_snapshot.id,
            transport_attempt_id: None,
            started_at: UtcTimestamp::now(),
            prompt_inspector: compaction_projection(
                source,
                ContextCompactionStateView::Running,
                Some(attempt.execution_snapshot.model_step_id),
            ),
            correlation_id: CorrelationId::new(),
        })
        .expect("mark context compaction running");
    anchor_manifest
}

fn completed_compaction_request(
    source: &P06PreparedFixture,
    attempt: &P06PreparedFixture,
    sequence: u32,
    transport_attempt_id: TransportAttemptId,
    anchor_manifest: aura_context_domain::CompactionAnchorManifest,
) -> ContextCompactionCompletedRequest {
    let parsed = parse_and_validate_compaction_output(
        &valid_compaction_output(),
        10_000,
        &anchor_manifest,
        true,
    )
    .expect("valid compaction output");
    let historical_summary = parsed.historical_snapshot.into_bytes();
    let active_turn_summary = parsed.active_turn_summary.into_bytes();
    let structured_compaction = parsed.structured_compaction.into_bytes();
    let anchor_manifest_bytes = canonical_json_v1(&anchor_manifest).expect("canonical manifest");
    ContextCompactionCompletedRequest {
        source_context_snapshot_id: source.prepared.context.snapshot.id,
        sequence,
        attempt_ordinal: 1,
        transport_attempt_id,
        historical_summary: ContextArtifactDraft {
            artifact_id: ArtifactId::new(),
            media_type: "text/markdown; charset=utf-8".to_owned(),
            bytes: historical_summary.clone(),
        },
        active_turn_summary: ContextArtifactDraft {
            artifact_id: ArtifactId::new(),
            media_type: "text/markdown; charset=utf-8".to_owned(),
            bytes: active_turn_summary.clone(),
        },
        structured_compaction: ContextArtifactDraft {
            artifact_id: ArtifactId::new(),
            media_type: "text/markdown; charset=utf-8".to_owned(),
            bytes: structured_compaction.clone(),
        },
        anchor_manifest: ContextArtifactDraft {
            artifact_id: ArtifactId::new(),
            media_type: "application/json".to_owned(),
            bytes: anchor_manifest_bytes,
        },
        anchor_manifest_value: anchor_manifest.clone(),
        record: StructuredCompactionRecord {
            source_context_snapshot_id: source.prepared.context.snapshot.id,
            sequence,
            source_span: aura_context_domain::CompactionSourceSpan {
                start_sequence: aura_domain_common::SessionSequence::new(1),
                end_sequence: aura_domain_common::SessionSequence::new(1),
                source_refs: anchor_manifest
                    .anchors
                    .iter()
                    .map(|anchor| anchor.source_ref.clone())
                    .collect(),
            },
            historical_summary_hash: ContentHash::sha256(&historical_summary),
            active_turn_summary_hash: ContentHash::sha256(&active_turn_summary),
            structured_markdown_hash: ContentHash::sha256(&structured_compaction),
            anchor_manifest_hash: anchor_manifest.manifest_hash,
            retained_recent_tokens: 0,
            retention_exceptions: Vec::new(),
        },
        completed_at: UtcTimestamp::now(),
        prompt_inspector: compaction_projection(
            source,
            ContextCompactionStateView::Completed,
            Some(attempt.execution_snapshot.model_step_id),
        ),
        correlation_id: CorrelationId::new(),
    }
}

#[test]
fn p06_snapshot_mex_event_projection_and_outbox_commit_atomically() {
    let directory = tempfile::tempdir().expect("temporary root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let fixture = p06_prepared_context_fixture().expect("P06 fixture");
    seed_scope(&persistence, &database_path, &fixture);

    persistence
        .commit_prompt_bundle_snapshot(PromptBundleCommitRequest {
            session_id: fixture.session_id,
            snapshot: fixture.prompt_bundle_snapshot.clone(),
            artifacts: fixture
                .prompt_bundle_artifacts
                .clone()
                .into_iter()
                .map(context_artifact)
                .collect(),
            correlation_id: CorrelationId::new(),
        })
        .expect("commit prompt bundle");

    persistence
        .commit_context_source_event(ContextSourceEventCommitRequest {
            event: PersistentEventDraft {
                event_id: fixture.current_user_event_id,
                workspace_id: Some(fixture.workspace_id),
                session_id: Some(fixture.session_id),
                turn_task_id: Some(fixture.turn_task_id),
                agent_run_id: Some(fixture.agent_run_id),
                event_type: "RootUserMessageAccepted".to_owned(),
                event_version: 1,
                actor: EventActor::User,
                correlation_id: CorrelationId::new(),
                causation_id: None,
                atomic_group_id: None,
                payload_json: None,
                payload_artifact_ref: None,
                searchable_text: Some(fixture.current_user_content.clone()),
                created_at: UtcTimestamp::now(),
            },
            canonical_content: fixture.current_user_content.clone(),
            model_visible: true,
            explicitly_user_authored: true,
        })
        .expect("commit source event and authorization fact");

    let request = ContextProjectionCommitRequest {
        snapshot: fixture.prepared.context.snapshot.clone(),
        execution_snapshot: fixture.execution_snapshot.clone(),
        instruction_envelope: fixture.prepared.context.instruction_envelope.clone(),
        model_egress_manifest: fixture.prepared.context.model_egress_manifest.clone(),
        artifacts: fixture
            .prepared
            .artifacts
            .clone()
            .into_iter()
            .map(|artifact| ContextArtifactDraft {
                artifact_id: artifact.artifact_id,
                media_type: artifact.media_type,
                bytes: artifact.bytes,
            })
            .collect(),
        prompt_inspector: fixture.prepared.prompt_inspector.clone(),
        search_chunks: vec![ContextSearchChunkDraft {
            agent_run_id: Some(fixture.agent_run_id),
            turn_task_id: Some(fixture.turn_task_id),
            source_type: "user_message".to_owned(),
            source_id: fixture.current_user_event_id.to_string(),
            session_sequence: aura_domain_common::SessionSequence::new(1),
            privacy_class: ContextPrivacyClass::ModelVisible,
            source_hash: aura_domain_common::ContentHash::sha256(
                fixture.current_user_content.as_bytes(),
            ),
            searchable_text: fixture.current_user_content.clone(),
        }],
        correlation_id: CorrelationId::new(),
    };
    let event_count_before = persistence.event_count().expect("event count before fault");
    let artifact_count_before = persistence.artifact_count().expect("artifact count before fault");
    assert!(
        persistence
            .commit_context_projection_snapshot_with_fault(
                request.clone(),
                Some(PersistenceFaultPoint::BeforeCommit),
            )
            .is_err()
    );
    assert_eq!(persistence.event_count().expect("event count after rollback"), event_count_before);
    assert_eq!(
        persistence.artifact_count().expect("artifact count after rollback"),
        artifact_count_before
    );
    assert!(
        persistence
            .context_projection_snapshot(request.snapshot.id)
            .expect("read rolled back cps")
            .is_none()
    );
    assert!(
        persistence
            .model_execution_snapshot(request.execution_snapshot.id)
            .expect("read rolled back mex")
            .is_none()
    );

    let committed = persistence
        .commit_context_projection_snapshot(request.clone())
        .expect("commit P06 snapshot graph");
    persistence
        .publish_model_projection_changes(std::slice::from_ref(&committed.projection))
        .expect("publish prompt inspector projection");
    assert_eq!(
        persistence.context_projection_snapshot(request.snapshot.id).expect("read cps"),
        Some(request.snapshot.clone())
    );
    assert_eq!(
        persistence
            .stored_context_projection_snapshot(request.snapshot.id)
            .expect("read versioned cps"),
        Some(StoredContextProjectionSnapshot::CurrentV2(Box::new(request.snapshot.clone())))
    );
    assert_eq!(
        persistence.model_execution_snapshot(request.execution_snapshot.id).expect("read mex"),
        Some(request.execution_snapshot)
    );
    assert_eq!(
        persistence
            .latest_prompt_inspector_projection(fixture.session_id)
            .expect("read inspector")
            .and_then(|projection| projection.context_snapshot_id),
        Some(request.snapshot.id)
    );

    let search = persistence
        .search_context_rows(ContextSearchStoredQuery {
            session_id: fixture.session_id,
            agent_run_id: fixture.agent_run_id,
            fts_query: "frozen".to_owned(),
            filters: ContextSearchFilters::default(),
            cutoff: None,
            after: None,
            fetch_limit: 16,
        })
        .expect("search committed context");
    assert_eq!(search.rows.len(), 1);
    assert_eq!(search.rows[0].source_id, fixture.current_user_event_id.to_string());
}

#[test]
fn manual_compaction_focus_gate_is_idempotent_and_bounded_by_characters() {
    let directory = tempfile::tempdir().expect("temporary root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let fixture = p06_prepared_context_fixture().expect("P06 fixture");
    seed_scope(&persistence, &database_path, &fixture);

    let no_focus_id = CommandInvocationId::new();
    let no_focus = manual_compaction_request(&fixture, no_focus_id, None);
    let created = persistence
        .create_manual_context_compaction_request(no_focus.clone())
        .expect("create no-focus request");
    assert_eq!(created.state, ManualCompactionRequestState::Requested);
    assert_eq!(created.focus_hash, None);
    assert_eq!(
        persistence
            .create_manual_context_compaction_request(no_focus)
            .expect("idempotent no-focus request"),
        created
    );

    let boundary_focus = "界".repeat(4_096);
    let boundary = persistence
        .create_manual_context_compaction_request(manual_compaction_request(
            &fixture,
            CommandInvocationId::new(),
            Some(&boundary_focus),
        ))
        .expect("create 4096-character request");
    assert!(boundary.focus_hash.is_some());

    let over_limit = "x".repeat(4_097);
    assert!(
        persistence
            .create_manual_context_compaction_request(manual_compaction_request(
                &fixture,
                CommandInvocationId::new(),
                Some(&over_limit),
            ))
            .is_err()
    );
    let pending = persistence
        .pending_manual_context_compaction_requests(fixture.session_id, fixture.agent_run_id)
        .expect("pending manual requests");
    assert_eq!(pending.len(), 2);
}

#[test]
fn context_restart_terminalizes_requested_compaction_and_bound_manual_request_once() {
    let directory = tempfile::tempdir().expect("temporary root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let source = p06_prepared_context_fixture().expect("source fixture");
    seed_scope(&persistence, &database_path, &source);
    commit_prompt_bundle(&persistence, &source);
    commit_context_fixture(&persistence, &source);

    let command_invocation_id = CommandInvocationId::new();
    persistence
        .create_manual_context_compaction_request(manual_compaction_request(
            &source,
            command_invocation_id,
            Some("retain decisions"),
        ))
        .expect("create manual compaction request");
    create_requested_compaction(&persistence, &source, 1);
    persistence
        .mark_manual_context_compaction_running(ContextManualCompactionRunningRequest {
            session_id: source.session_id,
            agent_run_id: source.agent_run_id,
            command_invocation_id,
            source_context_snapshot_id: source.prepared.context.snapshot.id,
            compaction_sequence: 1,
            correlation_id: CorrelationId::new(),
        })
        .expect("bind manual request to compaction");

    let event_count = persistence.event_count().expect("event count before recovery");
    let recovered_at = UtcTimestamp::now();
    let report = persistence
        .reconcile_context_after_core_restart(CorrelationId::new(), recovered_at)
        .expect("recover requested compaction");
    assert_eq!(report.failed_before_model_attempt, 1);
    assert_eq!(report.manual_requests_terminalized, 1);
    assert_eq!(report.prompt_inspector_projections_published, 1);
    assert_eq!(
        persistence
            .context_compaction(
                source.session_id,
                source.agent_run_id,
                source.prepared.context.snapshot.id,
                1,
            )
            .expect("read recovered compaction")
            .expect("recovered compaction")
            .state,
        ContextCompactionStateView::Failed
    );
    assert!(
        persistence
            .pending_manual_context_compaction_requests(source.session_id, source.agent_run_id)
            .expect("read pending manual requests")
            .is_empty()
    );
    assert_eq!(
        persistence
            .latest_prompt_inspector_projection(source.session_id)
            .expect("read recovered inspector")
            .expect("recovered inspector")
            .compaction
            .state,
        ContextCompactionStateView::Failed
    );
    let after_first = persistence.event_count().expect("event count after recovery");
    assert!(after_first > event_count);
    assert_eq!(
        persistence
            .reconcile_context_after_core_restart(CorrelationId::new(), UtcTimestamp::now())
            .expect("repeat recovery"),
        Default::default()
    );
    assert_eq!(
        persistence.event_count().expect("event count after repeated recovery"),
        after_first,
        "restart recovery must not append duplicate terminal events"
    );
}

#[test]
fn context_restart_cancels_pre_dispatch_attempt_but_preserves_dispatch_prepared_evidence() {
    for (target, expected_parent, expected_logical, expected_raw, side_effect_unknown) in [
        (
            TransportAttemptState::Authorized,
            ContextCompactionStateView::Cancelled,
            "cancelled",
            TransportAttemptState::Cancelled,
            0,
        ),
        (
            TransportAttemptState::DispatchPrepared,
            ContextCompactionStateView::Failed,
            "unknown_side_effect",
            TransportAttemptState::DispatchPrepared,
            1,
        ),
    ] {
        let directory = tempfile::tempdir().expect("temporary root");
        let database_path = directory.path().join("data/aura.db");
        let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
        let source = p06_prepared_context_fixture().expect("source fixture");
        let attempt = p06_followup_prepared_context_fixture(&source).expect("attempt fixture");
        seed_scope(&persistence, &database_path, &source);
        commit_prompt_bundle(&persistence, &source);
        commit_context_fixture(&persistence, &source);
        commit_context_fixture(&persistence, &attempt);
        create_and_start_compaction(&persistence, &source, &attempt, 1);
        let raw = transport_attempt_at_state(&persistence, &attempt, target);

        let report = persistence
            .reconcile_context_after_core_restart(CorrelationId::new(), UtcTimestamp::now())
            .expect("reconcile active compaction");
        assert_eq!(report.unknown_side_effects, side_effect_unknown);
        assert_eq!(
            report.pre_dispatch_attempts_cancelled,
            u64::from(target == TransportAttemptState::Authorized)
        );
        assert_eq!(
            persistence
                .transport_attempt(raw.id)
                .expect("read raw transport")
                .expect("raw transport")
                .state,
            expected_raw
        );
        assert_eq!(
            persistence
                .context_compaction(
                    source.session_id,
                    source.agent_run_id,
                    source.prepared.context.snapshot.id,
                    1,
                )
                .expect("read compaction")
                .expect("compaction")
                .state,
            expected_parent
        );
        let connection = Connection::open(&database_path).expect("open recovery database");
        let logical_state = connection
            .query_row(
                "SELECT state FROM context_compaction_model_attempts
                 WHERE source_context_snapshot_id=?1 AND compaction_sequence=1",
                [source.prepared.context.snapshot.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("read logical attempt state");
        assert_eq!(logical_state, expected_logical);
    }
}

#[test]
fn context_restart_does_not_accept_completed_transport_output_or_replace_lkg() {
    let directory = tempfile::tempdir().expect("temporary root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let source = p06_prepared_context_fixture().expect("source fixture");
    let accepted = p06_followup_prepared_context_fixture(&source).expect("accepted fixture");
    let interrupted = p06_followup_prepared_context_fixture(&source).expect("interrupted fixture");
    seed_scope(&persistence, &database_path, &source);
    commit_prompt_bundle(&persistence, &source);
    for fixture in [&source, &accepted, &interrupted] {
        commit_context_fixture(&persistence, fixture);
    }

    let accepted_manifest = create_and_start_compaction(&persistence, &source, &accepted, 1);
    let accepted_transport =
        completed_transport_attempt(&persistence, &accepted, &valid_compaction_output());
    persistence
        .complete_context_compaction(completed_compaction_request(
            &source,
            &accepted,
            1,
            accepted_transport.id,
            accepted_manifest,
        ))
        .expect("commit accepted LKG");

    create_and_start_compaction(&persistence, &source, &interrupted, 2);
    let interrupted_transport =
        completed_transport_attempt(&persistence, &interrupted, &valid_compaction_output());
    let report = persistence
        .reconcile_context_after_core_restart(CorrelationId::new(), UtcTimestamp::now())
        .expect("reconcile completed transport without validated compaction commit");
    assert_eq!(report.completed_outputs_left_unaccepted, 1);
    assert_eq!(
        persistence
            .transport_attempt(interrupted_transport.id)
            .expect("read completed transport")
            .expect("completed transport")
            .state,
        TransportAttemptState::Completed
    );
    assert_eq!(
        persistence
            .context_compaction(
                source.session_id,
                source.agent_run_id,
                source.prepared.context.snapshot.id,
                2,
            )
            .expect("read interrupted compaction")
            .expect("interrupted compaction")
            .state,
        ContextCompactionStateView::Failed
    );
    assert_eq!(
        persistence
            .latest_context_compaction(source.session_id, source.agent_run_id)
            .expect("read LKG")
            .expect("LKG")
            .sequence,
        1,
        "restart recovery must not replace a validated LKG"
    );
}

#[test]
fn context_restart_identity_contradiction_fails_closed_without_mutation() {
    let directory = tempfile::tempdir().expect("temporary root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let source = p06_prepared_context_fixture().expect("source fixture");
    let attempt = p06_followup_prepared_context_fixture(&source).expect("attempt fixture");
    seed_scope(&persistence, &database_path, &source);
    commit_prompt_bundle(&persistence, &source);
    commit_context_fixture(&persistence, &source);
    commit_context_fixture(&persistence, &attempt);
    create_and_start_compaction(&persistence, &source, &attempt, 1);
    let connection = Connection::open(&database_path).expect("open recovery database");
    connection
        .execute(
            "UPDATE context_compactions SET model_step_id=?3
             WHERE source_context_snapshot_id=?1 AND sequence=?2",
            params![
                source.prepared.context.snapshot.id.to_string(),
                1_i64,
                aura_domain_common::ModelStepId::new().to_string(),
            ],
        )
        .expect("inject owner contradiction");
    drop(connection);
    let events_before = persistence.event_count().expect("events before failed recovery");
    assert!(matches!(
        persistence.reconcile_context_after_core_restart(
            CorrelationId::new(),
            UtcTimestamp::now()
        ),
        Err(PersistenceError::InvalidRequest(message))
            if message == "active context compaction points at the wrong ModelStep"
    ));
    assert_eq!(
        persistence.event_count().expect("events after failed recovery"),
        events_before,
        "identity contradiction must roll back the entire recovery transaction"
    );
    assert_eq!(
        persistence
            .context_compaction(
                source.session_id,
                source.agent_run_id,
                source.prepared.context.snapshot.id,
                1,
            )
            .expect("read active compaction")
            .expect("active compaction")
            .state,
        ContextCompactionStateView::Running
    );
}

#[test]
fn compaction_completion_is_atomic_rechecks_sources_and_failed_work_keeps_lkg() {
    let directory = tempfile::tempdir().expect("temporary root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let source = p06_prepared_context_fixture().expect("source fixture");
    let first_attempt =
        p06_followup_prepared_context_fixture(&source).expect("first attempt fixture");
    let failed_attempt =
        p06_followup_prepared_context_fixture(&source).expect("failed attempt fixture");
    let changed_source_attempt =
        p06_followup_prepared_context_fixture(&source).expect("changed source attempt fixture");
    seed_scope(&persistence, &database_path, &source);
    commit_prompt_bundle(&persistence, &source);
    for fixture in [&source, &first_attempt, &failed_attempt, &changed_source_attempt] {
        commit_context_fixture(&persistence, fixture);
    }

    let first_manifest = create_and_start_compaction(&persistence, &source, &first_attempt, 1);
    let first_transport =
        completed_transport_attempt(&persistence, &first_attempt, &valid_compaction_output());
    persistence
        .complete_context_compaction(completed_compaction_request(
            &source,
            &first_attempt,
            1,
            first_transport.id,
            first_manifest,
        ))
        .expect("complete first compaction");
    let lkg = persistence
        .latest_context_compaction(source.session_id, source.agent_run_id)
        .expect("read first LKG")
        .expect("first LKG");
    assert_eq!(lkg.sequence, 1);
    assert_eq!(lkg.state, ContextCompactionStateView::Completed);

    create_and_start_compaction(&persistence, &source, &failed_attempt, 2);
    persistence
        .finish_context_compaction(ContextCompactionTerminalRequest {
            source_context_snapshot_id: source.prepared.context.snapshot.id,
            sequence: 2,
            attempt_ordinal: Some(1),
            attempt_state: Some(CompactionModelAttemptState::Failed),
            transport_attempt_id: None,
            state: ContextCompactionStateView::Failed,
            error_code: "context.compaction.provider_failed_before_dispatch".to_owned(),
            completed_at: UtcTimestamp::now(),
            prompt_inspector: compaction_projection(
                &source,
                ContextCompactionStateView::Failed,
                Some(failed_attempt.execution_snapshot.model_step_id),
            ),
            correlation_id: CorrelationId::new(),
        })
        .expect("fail second compaction");
    assert_eq!(
        persistence
            .latest_context_compaction(source.session_id, source.agent_run_id)
            .expect("read preserved LKG")
            .expect("preserved LKG")
            .sequence,
        1,
        "a failed operation must not replace the last known good compaction"
    );

    let mut changed_manifest =
        create_and_start_compaction(&persistence, &source, &changed_source_attempt, 3);
    let changed_transport = completed_transport_attempt(
        &persistence,
        &changed_source_attempt,
        &valid_compaction_output(),
    );
    changed_manifest.anchors[0].source_hash = ContentHash::sha256(b"changed evidence");
    changed_manifest.manifest_hash =
        aura_domain_common::canonical_json_v1_sha256(&changed_manifest.anchors)
            .expect("changed manifest hash");
    let artifacts_before = persistence.artifact_count().expect("artifact count before mismatch");
    assert!(matches!(
        persistence.complete_context_compaction(completed_compaction_request(
            &source,
            &changed_source_attempt,
            3,
            changed_transport.id,
            changed_manifest,
        )),
        Err(PersistenceError::InvalidRequest(message))
            if message == "completed context compaction source evidence changed"
    ));
    assert_eq!(
        persistence.artifact_count().expect("artifact count after mismatch"),
        artifacts_before,
        "source mismatch must fail before compaction artifacts are committed"
    );
    persistence
        .finish_context_compaction(ContextCompactionTerminalRequest {
            source_context_snapshot_id: source.prepared.context.snapshot.id,
            sequence: 3,
            attempt_ordinal: Some(1),
            attempt_state: Some(CompactionModelAttemptState::Failed),
            transport_attempt_id: Some(changed_transport.id),
            state: ContextCompactionStateView::Failed,
            error_code: "context.compaction.source_evidence_changed".to_owned(),
            completed_at: UtcTimestamp::now(),
            prompt_inspector: compaction_projection(
                &source,
                ContextCompactionStateView::Failed,
                Some(changed_source_attempt.execution_snapshot.model_step_id),
            ),
            correlation_id: CorrelationId::new(),
        })
        .expect("terminalize source mismatch");
    assert_eq!(
        persistence
            .latest_context_compaction(source.session_id, source.agent_run_id)
            .expect("read final LKG")
            .expect("final LKG")
            .sequence,
        1
    );
}

#[test]
fn remediation_required_and_cleared_are_idempotent_atomic_and_content_free_events() {
    let directory = tempfile::tempdir().expect("temporary root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let fixture = p06_prepared_context_fixture().expect("P06 fixture");
    seed_scope(&persistence, &database_path, &fixture);
    commit_prompt_bundle(&persistence, &fixture);
    commit_context_fixture(&persistence, &fixture);

    let actions =
        vec!["移除当前附件中的大文件".to_owned(), "切换到更大上下文窗口的模型".to_owned()];
    let plan = ContextRemediationPlan {
        overflow: ContextOverflowDiagnosticView {
            request_input_budget_tokens: 50_000,
            estimated_input_tokens: 54_000,
            non_compressible_tokens: 51_000,
            compressible_history_tokens: 0,
            user_reducible_tokens: 3_000,
            overflow_tokens: 4_000,
            error_code: "context.non_compressible_overflow".to_owned(),
            remediation: actions.clone(),
        },
        actions,
    };
    let mut required_projection = fixture.prepared.prompt_inspector.clone();
    required_projection.active_agent_run_id = Some(fixture.agent_run_id);
    required_projection.context_snapshot_id = Some(fixture.prepared.context.snapshot.id);
    required_projection.overflow = Some(plan.overflow.clone());
    let required_at = UtcTimestamp::now();
    let required = ContextRemediationRequiredRequest {
        session_id: fixture.session_id,
        agent_run_id: fixture.agent_run_id,
        source_context_snapshot_id: fixture.prepared.context.snapshot.id,
        plan: plan.clone(),
        required_at,
        prompt_inspector: required_projection,
        correlation_id: CorrelationId::new(),
    };
    let events_before = persistence.event_count().expect("events before remediation");
    let created =
        persistence.require_context_remediation(required.clone()).expect("require remediation");
    assert!(created.changed);
    assert!(created.projection.is_some());
    assert_eq!(created.record.state, ContextRemediationState::Awaiting);
    let replayed =
        persistence.require_context_remediation(required).expect("replay required remediation");
    assert!(!replayed.changed);
    assert!(replayed.projection.is_none());
    assert_eq!(
        persistence
            .active_context_remediation(fixture.session_id, fixture.agent_run_id)
            .expect("read active remediation")
            .expect("active remediation")
            .plan,
        plan
    );

    let mut cleared_projection = fixture.prepared.prompt_inspector.clone();
    cleared_projection.active_agent_run_id = Some(fixture.agent_run_id);
    cleared_projection.context_snapshot_id = Some(fixture.prepared.context.snapshot.id);
    cleared_projection.overflow = None;
    let cleared = ContextRemediationClearedRequest {
        session_id: fixture.session_id,
        agent_run_id: fixture.agent_run_id,
        source_context_snapshot_id: fixture.prepared.context.snapshot.id,
        reason: ContextRemediationClearReason::RecheckSucceeded,
        cleared_at: UtcTimestamp::now(),
        prompt_inspector: cleared_projection,
        correlation_id: CorrelationId::new(),
    };
    let terminal =
        persistence.clear_context_remediation(cleared.clone()).expect("clear remediation");
    assert!(terminal.changed);
    assert_eq!(terminal.record.state, ContextRemediationState::Cleared);
    let replayed =
        persistence.clear_context_remediation(cleared).expect("replay clear remediation");
    assert!(!replayed.changed);
    assert!(
        persistence
            .active_context_remediation(fixture.session_id, fixture.agent_run_id)
            .expect("read cleared remediation")
            .is_none()
    );
    assert_eq!(
        persistence.event_count().expect("events after remediation"),
        events_before + 2,
        "idempotent replays must not append duplicate lifecycle events"
    );

    let connection = Connection::open(&database_path).expect("open remediation database");
    let event_payloads = connection
        .prepare(
            "SELECT payload_json FROM events
             WHERE event_type IN ('ContextRemediationRequired', 'ContextRemediationCleared')
             ORDER BY rowid",
        )
        .expect("prepare remediation events")
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .expect("read remediation events")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect remediation events");
    assert_eq!(event_payloads.len(), 2);
    for payload in event_payloads.into_iter().flatten() {
        assert!(!payload.contains("移除当前附件"));
        assert!(!payload.contains("切换到更大上下文"));
    }
}
