use std::collections::BTreeSet;

use aura_artifact_contract::{ArtifactAccess, ArtifactOwnerRef, ArtifactScope};
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, CorrelationId, EndpointId, ModelAvailabilityTestId,
    ModelDiscoveryRunId, ModelExecutionSnapshotId, ModelProfileId, ModelStepId, NetworkRequestId,
    PromptBundleSnapshotId, ProviderId, Revision, RuntimeInstanceId, SessionId, TransportAttemptId,
    UtcTimestamp, WorkspaceId,
};
use aura_domain_common::{ToolCallId, ToolId, ToolRegistrySnapshotId};
use aura_model_domain::{
    EndpointAuthScheme, ModelAdapterIdentity, ModelApiFormat, ModelExecutionSnapshot, ModelUsage,
    NormalizedModelOutput, NormalizedStopReason, NormalizedToolCall, ProviderRequestEvidence,
    ProviderRequestHeaderClass, ProviderRequestHeaderMetadata, ProviderRequestTargetMetadata,
    ProviderToolCallBindingError, ProviderToolCallId, ProviderToolNameBinding,
    ResolvedModelRequestOptions, TransportAttemptKind, TransportAttemptRecord,
    TransportAttemptState, TransportOperationContext, UsageSource,
    derive_p07_normalized_tool_calls_v1, provider_wire_tool_name,
};
use aura_permission_domain::NormalizedOrigin;
use aura_persistence::{
    ArtifactWriteRequest, PersistenceCoordinator, PersistenceError, PersistenceOptions,
    PersistenceStartup,
};
use aura_tool_contract::{ToolDefinitionSnapshotRef, ToolRegistrySnapshotRef, ToolVersion};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};

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

fn seed_session(persistence: &PersistenceCoordinator) -> SessionId {
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: "P05 Runtime Workspace".to_owned(),
                selected_root: r"C:\Aura Tests\P05 Runtime".to_owned(),
                canonical_root: r"C:\Aura Tests\P05 Runtime".to_owned(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id,
                workspace_id,
                title: "P05 Runtime".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("seed session");
    session_id
}

fn snapshot() -> ModelExecutionSnapshot {
    ModelExecutionSnapshot {
        id: ModelExecutionSnapshotId::new(),
        model_step_id: ModelStepId::new(),
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: ModelProfileId::new(),
        provider_display_name: "Fixture Provider".to_owned(),
        endpoint_display_name: "Fixture Endpoint".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Fixture Provider / Fixture Endpoint / fixture-model".to_owned(),
        api_format: ModelApiFormat::OpenAiResponses,
        chat_output_token_field: None,
        chat_instruction_role: None,
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: "https://api.example.test/v1/responses".to_owned(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        endpoint_config_revision: 4,
        model_config_revision: 9,
        runtime_effective_window_tokens: 256_000,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions {
            effective_max_output_tokens: 512,
            temperature: None,
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: false,
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id: ContextProjectionSnapshotId::new(),
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::blake3(b"fixture-endpoint"),
        created_at: UtcTimestamp::now(),
    }
}

fn request_evidence(operation_context: TransportOperationContext) -> ProviderRequestEvidence {
    let body = br#"{"stream":true}"#;
    let tool_registry_snapshot_ref =
        matches!(operation_context, TransportOperationContext::AgentModelAttempt { .. }).then(
            || ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"p05-persistence-test-registry"),
            },
        );
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
        operation_context,
        tool_registry_snapshot_ref,
        tool_name_bindings: Vec::new(),
        historical_tool_call_wire_bindings: Vec::new(),
        canonical_body_artifact_id: None,
    }
}

fn queued_attempt(
    endpoint_id: EndpointId,
    endpoint_fingerprint: ContentHash,
    operation_context: TransportOperationContext,
) -> TransportAttemptRecord {
    TransportAttemptRecord {
        id: TransportAttemptId::new(),
        attempt_number: 1,
        attempt_kind: TransportAttemptKind::Initial,
        client_request_id: NetworkRequestId::new().to_string(),
        provider_request_id: None,
        endpoint_id,
        endpoint_fingerprint,
        request_evidence: request_evidence(operation_context),
        state: TransportAttemptState::QueuedForEndpoint,
        started_at: UtcTimestamp::now(),
        first_byte_at: None,
        first_model_event_at: None,
        finished_at: None,
        raw_stream_artifact_id: None,
        failure_id: None,
    }
}

#[test]
fn model_runtime_records_are_monotonic_atomic_and_survive_restart() {
    let directory = tempfile::tempdir().expect("temp root");
    let database_path = directory.path().join("data/aura.db");
    let artifact_root = directory.path().join("data/artifacts");
    let persistence = start(database_path.clone(), artifact_root.clone());
    let session_id = seed_session(&persistence);
    let snapshot = snapshot();

    persistence.store_model_execution_snapshot(&snapshot).expect("store mex");
    persistence.store_model_execution_snapshot(&snapshot).expect("idempotent mex write");
    let mut conflicting_snapshot = snapshot.clone();
    conflicting_snapshot.model_name = "different-model".to_owned();
    assert!(matches!(
        persistence.store_model_execution_snapshot(&conflicting_snapshot),
        Err(PersistenceError::InvalidRequest(_))
    ));

    let raw_stream = persistence
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: "text/event-stream".to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Session { session_id },
            bytes: b"data: fixture\n\n".to_vec(),
        })
        .expect("raw stream artifact");
    let mut attempt = queued_attempt(
        snapshot.endpoint_id,
        snapshot.endpoint_fingerprint.clone(),
        TransportOperationContext::AgentModelAttempt {
            model_step_id: snapshot.model_step_id,
            model_execution_snapshot_id: snapshot.id,
            context_snapshot_id: snapshot.context_snapshot_id,
            prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
        },
    );
    let tool_id = "tol_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse::<ToolId>().expect("fixed fixture ToolId");
    let provider_wire_tool_name = provider_wire_tool_name("fixture.tool", tool_id);
    let tool_version = ToolVersion::new(None, Revision::INITIAL).expect("fixture ToolVersion");
    let registry_snapshot_ref = attempt
        .request_evidence
        .tool_registry_snapshot_ref
        .clone()
        .expect("agent attempt registry snapshot");
    attempt.request_evidence.tool_name_bindings = vec![ProviderToolNameBinding {
        tool_definition: ToolDefinitionSnapshotRef { tool_id, tool_version, registry_snapshot_ref },
        canonical_name: "fixture.tool".to_owned(),
        provider_wire_name: provider_wire_tool_name.clone(),
        package_schema_hash: CanonicalHashV1::sha256(br#"{"type":"object"}"#),
        provider_wrapped_schema_hash: CanonicalHashV1::sha256(
            br#"{"type":"object","required":["_aura","arguments"]}"#,
        ),
        provider_arguments_envelope_version: 1,
    }];
    persistence.upsert_transport_attempt(&attempt).expect("queued attempt");
    for state in [
        TransportAttemptState::AcquiringPermit,
        TransportAttemptState::PreparingRequest,
        TransportAttemptState::AuthorizingNetwork,
        TransportAttemptState::Connecting,
    ] {
        attempt.state = state;
        persistence.upsert_transport_attempt(&attempt).expect("attempt transition");
    }
    attempt.provider_request_id = Some("Provider_Request_ID-ExactCase".to_owned());
    attempt.first_byte_at = Some(UtcTimestamp::now());
    attempt.state = TransportAttemptState::AwaitingFirstEvent;
    persistence.upsert_transport_attempt(&attempt).expect("awaiting first event");
    attempt.first_model_event_at = Some(UtcTimestamp::now());
    attempt.state = TransportAttemptState::Streaming;
    persistence.upsert_transport_attempt(&attempt).expect("streaming");
    attempt.raw_stream_artifact_id = Some(raw_stream.artifact_id);
    attempt.finished_at = Some(UtcTimestamp::now());
    attempt.state = TransportAttemptState::Completed;
    persistence.upsert_transport_attempt(&attempt).expect("completed");

    let tool_call_id = ToolCallId::new();
    let output = NormalizedModelOutput {
        model_step_id: snapshot.model_step_id,
        assistant_blocks: Vec::new(),
        reasoning_blocks: Vec::new(),
        tool_calls: vec![NormalizedToolCall {
            tool_call_id,
            provider_call_id: ProviderToolCallId::new("Provider_Tool_ID-ExactCase")
                .expect("provider tool ID"),
            tool_name: provider_wire_tool_name.clone(),
            arguments_json:
                r#"{"_aura":{"purpose":"inspect fixture"},"arguments":{"path":"README.md"}}"#
                    .to_owned(),
            position: 0,
        }],
        refusal: None,
        stop_reason: NormalizedStopReason::ToolCalls,
        usage: ModelUsage {
            input_tokens: 10,
            output_tokens: 3,
            total_tokens: 13,
            reasoning_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
            source: UsageSource::ProviderReported,
        },
        provider_continuation: None,
        provider_request_id: attempt.provider_request_id.clone(),
        raw_response_ref: Some(raw_stream.artifact_id),
    };
    let p07_calls =
        derive_p07_normalized_tool_calls_v1(&snapshot, &attempt.request_evidence, &output)
            .expect("derive exact P07 calls from the completed Provider response");
    assert_eq!(p07_calls.len(), 1);
    assert_eq!(p07_calls[0].tool_call_id, tool_call_id);
    assert_eq!(p07_calls[0].ordinal, 0);
    assert_eq!(p07_calls[0].definition_ref.as_ref().expect("known call").tool_id, tool_id);
    assert_eq!(
        p07_calls[0].definition_ref.as_ref().expect("known call").registry_snapshot_ref,
        attempt.request_evidence.tool_registry_snapshot_ref.clone().expect("registry snapshot")
    );
    assert!(matches!(
        &p07_calls[0].input,
        aura_tool_contract::NormalizedToolCallInputStateV1::Ready {
            purpose,
            arguments,
            ..
        } if purpose.as_str() == "inspect fixture"
            && arguments == &serde_json::json!({"path": "README.md"})
    ));

    let mut wrong_output = output.clone();
    wrong_output.model_step_id = ModelStepId::new();
    assert_eq!(
        derive_p07_normalized_tool_calls_v1(&snapshot, &attempt.request_evidence, &wrong_output),
        Err(ProviderToolCallBindingError::OutputIdentityMismatch)
    );

    let mut duplicate_ordinal_output = output.clone();
    duplicate_ordinal_output.tool_calls.push(NormalizedToolCall {
        tool_call_id: ToolCallId::new(),
        provider_call_id: ProviderToolCallId::new("Provider_Tool_ID-Second")
            .expect("second provider tool ID"),
        tool_name: provider_wire_tool_name.clone(),
        arguments_json: r#"{"_aura":{"purpose":"inspect another fixture"},"arguments":{}}"#
            .to_owned(),
        position: 0,
    });
    assert_eq!(
        derive_p07_normalized_tool_calls_v1(
            &snapshot,
            &attempt.request_evidence,
            &duplicate_ordinal_output
        ),
        Err(ProviderToolCallBindingError::DuplicateOutputOrdinal)
    );
    persistence
        .store_model_output(&snapshot, attempt.id, &output)
        .expect("store output atomically");
    persistence
        .store_model_output(&snapshot, attempt.id, &output)
        .expect("idempotent output write");

    let mut regressed = attempt.clone();
    regressed.state = TransportAttemptState::Streaming;
    regressed.finished_at = None;
    regressed.raw_stream_artifact_id = None;
    assert!(matches!(
        persistence.upsert_transport_attempt(&regressed),
        Err(PersistenceError::InvalidRequest(_))
    ));
    drop(persistence);

    let reopened = start(database_path.clone(), artifact_root);
    assert_eq!(
        reopened.model_execution_snapshot(snapshot.id).expect("load mex"),
        Some(snapshot.clone())
    );
    assert_eq!(reopened.transport_attempt(attempt.id).expect("load tat"), Some(attempt.clone()));
    assert_eq!(
        reopened.model_output(snapshot.model_step_id).expect("load output"),
        Some(output.clone())
    );
    drop(reopened);

    let connection = rusqlite::Connection::open(database_path).expect("open verification DB");
    let mapping = connection
        .query_row(
            "SELECT tool_id, provider_wire_tool_name, provider_tool_call_id
             FROM provider_tool_call_protocol_bindings WHERE tool_call_id=?1",
            [tool_call_id.to_string()],
            |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            },
        )
        .expect("tool mapping");
    assert_eq!(mapping.0, tool_id.to_string());
    assert_eq!(mapping.1, provider_wire_tool_name);
    assert_eq!(mapping.2, "Provider_Tool_ID-ExactCase");
    let usage_rows = connection
        .query_row("SELECT COUNT(*) FROM model_usage", [], |row| row.get::<_, i64>(0))
        .expect("usage count");
    assert_eq!(usage_rows, 1, "idempotent output writes must not duplicate usage");
}

#[test]
fn discovery_and_availability_test_attempts_require_no_synthetic_model_snapshots() {
    let directory = tempfile::tempdir().expect("temp root");
    let database_path = directory.path().join("data/aura.db");
    let persistence = start(database_path.clone(), directory.path().join("data/artifacts"));
    let endpoint_id = EndpointId::new();
    let endpoint_fingerprint = ContentHash::blake3(b"non-model-endpoint");

    let discovery_run_id = ModelDiscoveryRunId::new();
    let discovery = queued_attempt(
        endpoint_id,
        endpoint_fingerprint.clone(),
        TransportOperationContext::ModelDiscovery { discovery_run_id },
    );
    persistence.upsert_transport_attempt(&discovery).expect("store discovery tat");

    let availability_test = queued_attempt(
        endpoint_id,
        endpoint_fingerprint,
        TransportOperationContext::ModelAvailabilityTest {
            model_availability_test_id: ModelAvailabilityTestId::new(),
            model_profile_id: ModelProfileId::new(),
        },
    );
    persistence.upsert_transport_attempt(&availability_test).expect("store availability-test tat");
    assert_eq!(
        persistence.transport_attempt(discovery.id).expect("load discovery tat"),
        Some(discovery.clone())
    );
    assert_eq!(
        persistence.transport_attempt(availability_test.id).expect("load availability-test tat"),
        Some(availability_test)
    );

    let duplicate_attempt_number = queued_attempt(
        endpoint_id,
        ContentHash::blake3(b"non-model-endpoint"),
        TransportOperationContext::ModelDiscovery { discovery_run_id },
    );
    assert!(
        persistence.upsert_transport_attempt(&duplicate_attempt_number).is_err(),
        "one operation context cannot reuse an attempt number"
    );
    drop(persistence);

    let connection = rusqlite::Connection::open(database_path).expect("open verification DB");
    let columns = connection
        .prepare("PRAGMA table_info(transport_attempts)")
        .expect("prepare table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query table info")
        .collect::<Result<Vec<_>, _>>()
        .expect("transport columns");
    assert!(columns.contains(&"operation_context_json".to_owned()));
    assert!(!columns.contains(&"model_execution_snapshot_id".to_owned()));
    assert!(!columns.contains(&"model_step_id".to_owned()));
}
