-- Frozen, empty Schema 42 fixture for the one-time V2 importer (ADR V2-0002).

-- Exported by running the real apply_migrations_through(..., 0, 42, None).

-- Includes every table/index/trigger and the original migration names/checksums.

-- No live coordinator, user data, WAL, credentials, or migration beyond 42.

-- migration::tests::schema_42_import_fixture_matches_the_historical_migrations

-- verifies this definition against those migrations without exposing a public API.



CREATE TABLE agent_runs (
    agent_run_id TEXT PRIMARY KEY,
    turn_task_id TEXT NOT NULL REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    parent_agent_run_id TEXT REFERENCES agent_runs(agent_run_id),
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE approval_decisions (
    approval_decision_id TEXT PRIMARY KEY,
    approval_request_id TEXT NOT NULL UNIQUE REFERENCES approval_requests(approval_request_id) ON DELETE CASCADE,
    decision_kind TEXT NOT NULL,
    decided_at TEXT NOT NULL,
    permission_grant_ids_json TEXT NOT NULL
) STRICT;

CREATE TABLE approval_request_operations (
    approval_request_id TEXT NOT NULL REFERENCES approval_requests(approval_request_id) ON DELETE CASCADE,
    operation_index INTEGER NOT NULL,
    security_operation_id TEXT NOT NULL,
    operation_fingerprint_json TEXT NOT NULL,
    PRIMARY KEY (approval_request_id, operation_index)
) STRICT;

CREATE TABLE approval_requests (
    approval_request_id TEXT PRIMARY KEY,
    request_json TEXT NOT NULL,
    identity_json TEXT NOT NULL,
    title TEXT NOT NULL,
    model_purpose TEXT,
    actual_operation_summary TEXT NOT NULL,
    source_summary TEXT NOT NULL,
    source_json TEXT NOT NULL,
    source_fingerprint_json TEXT,
    capabilities_json TEXT NOT NULL,
    scopes_json TEXT NOT NULL,
    risk_flags_json TEXT NOT NULL,
    allowed_decisions_json TEXT NOT NULL,
    permission_snapshot_id TEXT NOT NULL REFERENCES permission_snapshots(permission_snapshot_id),
    request_revision INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('pending', 'approved', 'denied', 'expired', 'cancelled', 'invalidated')
    ),
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    invalidation_reason TEXT
) STRICT;

CREATE TABLE "artifact_access_links" (
    artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    access_flags INTEGER NOT NULL,
    created_at TEXT NOT NULL, evidence_hash_json TEXT,
    PRIMARY KEY (artifact_id, session_id)
) STRICT;

CREATE TABLE artifact_chunks (
    artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    raw_offset INTEGER NOT NULL,
    raw_length INTEGER NOT NULL,
    stored_length INTEGER NOT NULL,
    compression TEXT NOT NULL CHECK (compression IN ('none', 'zstd')),
    raw_hash TEXT NOT NULL,
    object_hash TEXT NOT NULL REFERENCES artifact_object_refs(object_hash),
    PRIMARY KEY (artifact_id, chunk_index)
) STRICT;

CREATE TABLE artifact_gc_queue (
    artifact_id TEXT PRIMARY KEY REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    enqueued_at TEXT NOT NULL,
    reason TEXT NOT NULL
) STRICT;

CREATE TABLE artifact_object_refs (
    object_hash TEXT PRIMARY KEY,
    relative_path TEXT NOT NULL UNIQUE,
    stored_length INTEGER NOT NULL,
    ref_count INTEGER NOT NULL CHECK (ref_count >= 0)
) STRICT;

CREATE TABLE artifact_overwrite_journal (
    security_operation_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    external_grant_id TEXT NOT NULL REFERENCES external_grants(external_grant_id),
    binding_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'prepared', 'applying', 'published', 'completed', 'aborted',
        'interrupted', 'needs_reconcile'
    )),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error_code TEXT
) STRICT;

CREATE TABLE artifact_owners (
    artifact_id TEXT PRIMARY KEY
        REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    owner_kind TEXT NOT NULL CHECK (
        owner_kind IN ('session', 'operation', 'system', 'tool')
    ),
    owner_type TEXT NOT NULL CHECK (
        owner_type IN (
            'session',
            'model_discovery',
            'model_availability_test',
            'provider_catalog_cache',
            'package_version',
            'draft',
            'conformance_run',
            'batch',
            'tool_call',
            'execution_attempt',
            'registry_snapshot'
        )
    ),
    owner_id TEXT,
    created_at TEXT NOT NULL,
    CHECK (
        (owner_kind = 'session' AND owner_type = 'session' AND owner_id IS NOT NULL)
        OR (
            owner_kind = 'operation'
            AND owner_type IN ('model_discovery', 'model_availability_test')
            AND owner_id IS NOT NULL
        )
        OR (
            owner_kind = 'system'
            AND owner_type = 'provider_catalog_cache'
            AND owner_id IS NULL
        )
        OR (
            owner_kind = 'tool'
            AND owner_type IN (
                'package_version', 'draft', 'conformance_run', 'batch',
                'tool_call', 'execution_attempt', 'registry_snapshot'
            )
            AND owner_id IS NOT NULL
            AND json_valid(owner_id)
            AND owner_type = CASE
                WHEN json_valid(owner_id) THEN json_extract(owner_id, '$.type')
                ELSE NULL
            END
        )
    )
) STRICT;

CREATE TABLE artifacts (
    artifact_id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,
    media_type TEXT NOT NULL,
    access_flags INTEGER NOT NULL,
    byte_length INTEGER NOT NULL,
    content_hash_algorithm TEXT NOT NULL,
    content_hash_hex TEXT NOT NULL,
    inline_bytes BLOB,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE authorization_snapshots (
    authorization_snapshot_id TEXT PRIMARY KEY,
    authorization_kind TEXT NOT NULL CHECK (
        authorization_kind IN ('session_permission', 'tool_conformance_allow_once')
    ),
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE cleanup_notifications (
    notification_id TEXT PRIMARY KEY,
    notification_kind TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT
) STRICT;

CREATE TABLE configuration_transport_operation_attempts (
    transport_attempt_id TEXT PRIMARY KEY
        REFERENCES transport_attempts(transport_attempt_id) ON DELETE CASCADE,
    operation_kind TEXT NOT NULL CHECK (
        operation_kind IN ('model_discovery', 'model_availability_test')
    ),
    operation_id TEXT NOT NULL,
    model_profile_id TEXT,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    created_at TEXT NOT NULL,
    CHECK (
        (operation_kind = 'model_discovery' AND model_profile_id IS NULL)
        OR
        (operation_kind = 'model_availability_test' AND model_profile_id IS NOT NULL)
    ),
    UNIQUE(operation_kind, operation_id, attempt_number)
) STRICT;

CREATE TABLE context_compaction_lkg (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    agent_run_id TEXT NOT NULL REFERENCES agent_runs(agent_run_id) ON DELETE CASCADE,
    source_context_snapshot_id TEXT NOT NULL,
    compaction_sequence INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, agent_run_id),
    FOREIGN KEY (source_context_snapshot_id, compaction_sequence)
        REFERENCES context_compactions(source_context_snapshot_id, sequence)
) STRICT;

CREATE TABLE context_compaction_model_attempts (
    source_context_snapshot_id TEXT NOT NULL,
    compaction_sequence INTEGER NOT NULL,
    attempt_ordinal INTEGER NOT NULL CHECK (attempt_ordinal BETWEEN 1 AND 2),
    attempt_context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id),
    model_step_id TEXT NOT NULL,
    model_execution_snapshot_id TEXT NOT NULL
        REFERENCES model_execution_snapshots(model_execution_snapshot_id),
    transport_attempt_id TEXT,
    state TEXT NOT NULL CHECK (
        state IN ('running', 'completed', 'failed', 'cancelled', 'unknown_side_effect')
    ),
    error_code TEXT,
    started_at TEXT NOT NULL,
    terminal_at TEXT,
    PRIMARY KEY (source_context_snapshot_id, compaction_sequence, attempt_ordinal),
    UNIQUE (model_step_id),
    UNIQUE (model_execution_snapshot_id),
    FOREIGN KEY (source_context_snapshot_id, compaction_sequence)
        REFERENCES context_compactions(source_context_snapshot_id, sequence) ON DELETE CASCADE
) STRICT;

CREATE TABLE context_compaction_sources (
    source_context_snapshot_id TEXT NOT NULL,
    compaction_sequence INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    source_ref_json TEXT NOT NULL,
    source_hash_json TEXT NOT NULL,
    PRIMARY KEY (source_context_snapshot_id, compaction_sequence, ordinal),
    FOREIGN KEY (source_context_snapshot_id, compaction_sequence)
        REFERENCES context_compactions(source_context_snapshot_id, sequence) ON DELETE CASCADE
) STRICT;

CREATE TABLE context_compactions (
    source_context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    state TEXT NOT NULL CHECK (state IN ('requested', 'running', 'completed', 'failed', 'cancelled')),
    model_step_id TEXT,
    transport_attempt_id TEXT,
    source_span_json TEXT NOT NULL,
    historical_summary_artifact_id TEXT REFERENCES artifacts(artifact_id),
    active_turn_summary_artifact_id TEXT REFERENCES artifacts(artifact_id),
    structured_compaction_artifact_id TEXT REFERENCES artifacts(artifact_id),
    anchor_manifest_artifact_id TEXT REFERENCES artifacts(artifact_id),
    focus_artifact_id TEXT REFERENCES artifacts(artifact_id),
    record_json TEXT,
    error_code TEXT,
    requested_at TEXT NOT NULL,
    completed_at TEXT,
    PRIMARY KEY (source_context_snapshot_id, sequence)
) STRICT;

CREATE TABLE context_legacy_history_atomic_group_mappings (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    legacy_atomic_group_id TEXT NOT NULL,
    verification TEXT NOT NULL CHECK (
        verification IN ('verified_side_mapping', 'legacy_unverified')
    ),
    assistant_response_event_id TEXT REFERENCES events(event_id),
    group_hash_json TEXT,
    mapped_at TEXT NOT NULL,
    PRIMARY KEY (session_id, legacy_atomic_group_id),
    CHECK (
        (verification = 'verified_side_mapping'
            AND assistant_response_event_id IS NOT NULL
            AND group_hash_json IS NOT NULL)
        OR
        (verification = 'legacy_unverified'
            AND assistant_response_event_id IS NULL
            AND group_hash_json IS NULL)
    )
) STRICT;

CREATE TABLE context_manual_compaction_requests (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    agent_run_id TEXT NOT NULL REFERENCES agent_runs(agent_run_id) ON DELETE CASCADE,
    request_key TEXT NOT NULL,
    command_event_id TEXT NOT NULL UNIQUE REFERENCES events(event_id) ON DELETE CASCADE,
    focus_artifact_id TEXT REFERENCES artifacts(artifact_id),
    focus_hash_json TEXT,
    state TEXT NOT NULL CHECK (state IN ('requested', 'running', 'completed', 'failed', 'cancelled')),
    source_context_snapshot_id TEXT,
    compaction_sequence INTEGER CHECK (compaction_sequence IS NULL OR compaction_sequence > 0),
    error_code TEXT,
    created_at TEXT NOT NULL,
    terminal_at TEXT,
    PRIMARY KEY (session_id, request_key),
    CHECK (
        (source_context_snapshot_id IS NULL AND compaction_sequence IS NULL)
        OR (source_context_snapshot_id IS NOT NULL AND compaction_sequence IS NOT NULL)
    ),
    FOREIGN KEY (source_context_snapshot_id, compaction_sequence)
        REFERENCES context_compactions(source_context_snapshot_id, sequence)
) STRICT;

CREATE TABLE context_projection_attachments (
    context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    source_event_id TEXT NOT NULL,
    artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    content_hash_json TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    media_type TEXT NOT NULL,
    attachment_json TEXT NOT NULL,
    PRIMARY KEY (context_snapshot_id, ordinal),
    UNIQUE (context_snapshot_id, artifact_id)
) STRICT;

CREATE TABLE context_projection_conversation_items (
    context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    item_kind TEXT NOT NULL,
    source_event_id TEXT NOT NULL,
    tool_call_id TEXT,
    item_json TEXT NOT NULL,
    PRIMARY KEY (context_snapshot_id, ordinal)
) STRICT;

CREATE TABLE context_projection_sections (
    context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    physical_ordinal INTEGER NOT NULL CHECK (physical_ordinal BETWEEN 1 AND 12),
    section_kind TEXT NOT NULL,
    content_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    content_hash_json TEXT NOT NULL,
    character_count INTEGER NOT NULL CHECK (character_count >= 0),
    byte_count INTEGER NOT NULL CHECK (byte_count >= 0),
    estimated_tokens INTEGER NOT NULL CHECK (estimated_tokens >= 0),
    source_refs_json TEXT NOT NULL,
    compressibility TEXT NOT NULL,
    privacy_class TEXT NOT NULL,
    is_empty_marker INTEGER NOT NULL CHECK (is_empty_marker IN (0, 1)),
    PRIMARY KEY (context_snapshot_id, physical_ordinal),
    UNIQUE (context_snapshot_id, section_kind)
) STRICT;

CREATE TABLE context_projection_snapshots (
    context_snapshot_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    turn_task_id TEXT NOT NULL REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    agent_run_id TEXT NOT NULL REFERENCES agent_runs(agent_run_id) ON DELETE CASCADE,
    model_step_id TEXT NOT NULL UNIQUE,
    prompt_bundle_snapshot_id TEXT NOT NULL
        REFERENCES prompt_bundle_snapshots(prompt_bundle_snapshot_id),
    model_profile_id TEXT NOT NULL,
    runtime_effective_window_tokens INTEGER NOT NULL CHECK (runtime_effective_window_tokens > 0),
    output_reserve_tokens INTEGER NOT NULL CHECK (output_reserve_tokens > 0),
    safety_reserve_tokens INTEGER NOT NULL CHECK (safety_reserve_tokens > 0),
    request_input_budget_tokens INTEGER NOT NULL CHECK (request_input_budget_tokens >= 0),
    estimated_input_tokens INTEGER NOT NULL CHECK (estimated_input_tokens >= 0),
    history_cutoff INTEGER NOT NULL CHECK (history_cutoff >= 0),
    instruction_envelope_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    instruction_envelope_hash_json TEXT NOT NULL,
    model_egress_policy_version INTEGER NOT NULL CHECK (model_egress_policy_version > 0),
    model_egress_manifest_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    model_egress_manifest_hash_json TEXT NOT NULL,
    budget_json TEXT NOT NULL,
    output_request_json TEXT NOT NULL,
    context_usage_json TEXT NOT NULL,
    active_task_lineage_id TEXT,
    snapshot_hash_json TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL
, snapshot_version INTEGER NOT NULL DEFAULT 1
        CHECK (snapshot_version IN (1, 2))) STRICT;

CREATE TABLE context_projection_source_authorizations (
    context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    source_kind TEXT NOT NULL,
    data_class TEXT NOT NULL,
    source_ref_json TEXT NOT NULL,
    authorization_basis_json TEXT NOT NULL,
    content_hash_json TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    PRIMARY KEY (context_snapshot_id, ordinal)
) STRICT;

CREATE TABLE context_projection_tool_registry_v2 (
    context_snapshot_id TEXT PRIMARY KEY
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    tool_registry_snapshot_id TEXT NOT NULL,
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    registry_content_hash_json TEXT NOT NULL,
    tool_set_hash_json TEXT NOT NULL,
    schema_set_json TEXT NOT NULL,
    effective_bindings_json TEXT NOT NULL
) STRICT;

CREATE TABLE context_projection_tool_schemas (
    context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    schema_hash_json TEXT NOT NULL,
    tool_schema_json TEXT NOT NULL,
    PRIMARY KEY (context_snapshot_id, ordinal),
    UNIQUE (context_snapshot_id, tool_id)
) STRICT;

CREATE TABLE context_remediations (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    agent_run_id TEXT NOT NULL REFERENCES agent_runs(agent_run_id) ON DELETE CASCADE,
    source_context_snapshot_id TEXT NOT NULL
        REFERENCES context_projection_snapshots(context_snapshot_id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('awaiting', 'cleared')),
    plan_json TEXT NOT NULL,
    required_at TEXT NOT NULL,
    cleared_at TEXT,
    clear_reason TEXT CHECK (
        clear_reason IS NULL OR clear_reason IN ('recheck_succeeded', 'cancelled')
    ),
    PRIMARY KEY (session_id, agent_run_id, source_context_snapshot_id),
    CHECK (
        (state = 'awaiting' AND cleared_at IS NULL AND clear_reason IS NULL)
        OR (state = 'cleared' AND cleared_at IS NOT NULL AND clear_reason IS NOT NULL)
    )
) STRICT;

CREATE TABLE context_search_chunks (
    chunk_id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    agent_run_id TEXT REFERENCES agent_runs(agent_run_id) ON DELETE CASCADE,
    turn_task_id TEXT REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    source_type TEXT NOT NULL,
    source_id TEXT NOT NULL,
    session_sequence INTEGER NOT NULL CHECK (session_sequence >= 0),
    privacy_class TEXT NOT NULL,
    source_hash_json TEXT NOT NULL,
    searchable_text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (session_id, source_type, source_id, source_hash_json)
) STRICT;

CREATE VIRTUAL TABLE context_search_fts USING fts5(
    chunk_id UNINDEXED,
    session_id UNINDEXED,
    agent_run_id UNINDEXED,
    source_type UNINDEXED,
    source_id UNINDEXED,
    session_sequence UNINDEXED,
    searchable_text,
    tokenize = 'unicode61'
);

CREATE TABLE context_search_watermarks (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_id) ON DELETE CASCADE,
    indexed_through_sequence INTEGER NOT NULL CHECK (indexed_through_sequence >= 0),
    complete INTEGER NOT NULL CHECK (complete IN (0, 1)),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE context_source_event_facts (
    event_id TEXT PRIMARY KEY REFERENCES events(event_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    content_hash_json TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    model_visible INTEGER NOT NULL CHECK (model_visible IN (0, 1)),
    explicitly_user_authored INTEGER NOT NULL CHECK (explicitly_user_authored IN (0, 1)),
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE credential_records (
    credential_id TEXT PRIMARY KEY,
    endpoint_id TEXT NOT NULL,
    credential_slot TEXT NOT NULL CHECK (credential_slot = 'api-key'),
    state TEXT NOT NULL CHECK (
        state IN (
            'unconfigured',
            'configured',
            'cleared',
            'unavailable',
            'reconciliation_required',
            'deletion_pending'
        )
    ),
    row_revision INTEGER NOT NULL CHECK (row_revision > 0),
    last_committed_credential_revision INTEGER
        CHECK (
            last_committed_credential_revision IS NULL
            OR last_committed_credential_revision > 0
        ),
    credential_ref TEXT NOT NULL,
    credential_binding_fingerprint TEXT NOT NULL,
    bundle_namespace_key TEXT NOT NULL,
    backend_kind TEXT NOT NULL CHECK (backend_kind = 'wincred'),
    wincred_type TEXT NOT NULL CHECK (wincred_type = 'generic'),
    final_target TEXT NOT NULL,
    normalized_origin_json TEXT NOT NULL,
    auth_scheme TEXT NOT NULL CHECK (
        auth_scheme IN ('open_ai_bearer_api_key', 'anthropic_api_key')
    ),
    error_code TEXT,
    configured_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(endpoint_id, credential_slot),
    CHECK (
        (state = 'unconfigured' AND last_committed_credential_revision IS NULL)
        OR (
            state IN ('configured', 'cleared')
            AND last_committed_credential_revision IS NOT NULL
        )
        OR state IN ('unavailable', 'reconciliation_required', 'deletion_pending')
    )
) STRICT;

CREATE TABLE credential_tombstones (
    credential_id TEXT PRIMARY KEY,
    last_committed_credential_revision INTEGER,
    former_owner_json TEXT NOT NULL CHECK (json_valid(former_owner_json)),
    deletion_reason TEXT NOT NULL CHECK (
        deletion_reason IN ('endpoint_deleted', 'provider_deleted')
    ),
    parent_journal_id TEXT NOT NULL
        REFERENCES mutation_journals(journal_id),
    child_journal_id TEXT NOT NULL UNIQUE
        REFERENCES mutation_journals(journal_id),
    deleted_at TEXT NOT NULL
) STRICT;

CREATE TABLE endpoint_gate_metrics (
    endpoint_id TEXT PRIMARY KEY,
    active_requests INTEGER NOT NULL CHECK (active_requests >= 0),
    queued_requests INTEGER NOT NULL CHECK (queued_requests >= 0),
    metrics_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE endpoint_runtime_states (
    endpoint_id TEXT PRIMARY KEY,
    state_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE VIRTUAL TABLE event_search_fts USING fts5(
    event_id UNINDEXED,
    session_id UNINDEXED,
    searchable_text,
    tokenize = 'unicode61'
);

CREATE TABLE events (
    global_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    session_sequence INTEGER,
    workspace_id TEXT REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT REFERENCES sessions(session_id) ON DELETE CASCADE,
    turn_task_id TEXT REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    agent_run_id TEXT REFERENCES agent_runs(agent_run_id) ON DELETE SET NULL,
    event_type TEXT NOT NULL,
    event_version INTEGER NOT NULL,
    actor TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    causation_id TEXT,
    atomic_group_id TEXT,
    payload_json TEXT,
    payload_artifact_ref TEXT,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE external_grants (
    external_grant_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    permission_grant_id TEXT REFERENCES permission_grants(permission_grant_id) ON DELETE SET NULL,
    selected_root TEXT NOT NULL,
    canonical_root TEXT NOT NULL,
    root_identity_json TEXT NOT NULL,
    root_is_directory INTEGER NOT NULL CHECK (root_is_directory IN (0, 1)),
    capabilities_json TEXT NOT NULL,
    recursive_scope INTEGER NOT NULL CHECK (recursive_scope IN (0, 1)),
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('active', 'suspended', 'revoked')),
    revalidation_revision INTEGER NOT NULL CHECK (revalidation_revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    suspended_at TEXT,
    revoked_at TEXT
, workspace_authorization_epoch INTEGER) STRICT;

CREATE TABLE external_trust_records (
    trust_record_id TEXT PRIMARY KEY,
    record_json TEXT NOT NULL,
    source_key TEXT NOT NULL UNIQUE,
    source_json TEXT NOT NULL,
    trust_state_json TEXT NOT NULL,
    effective_fingerprint_json TEXT,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE file_references (
    file_reference_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE host_preparation_journal (
    host_preparation_operation_id TEXT PRIMARY KEY,
    action TEXT NOT NULL CHECK (action IN ('prepare', 'repair', 'remove')),
    state TEXT NOT NULL CHECK (
        state IN (
            'prepared', 'awaiting_elevation', 'applying', 'completed',
            'rollback_pending', 'rolled_back', 'denied', 'failed'
        )
    ),
    operation_json TEXT NOT NULL,
    broker_plan_json TEXT NOT NULL,
    plan_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error_code TEXT
, approval_json TEXT) STRICT;

CREATE TABLE host_preparation_states (
    state_key TEXT PRIMARY KEY,
    summary_json TEXT NOT NULL,
    ownership_json TEXT NOT NULL,
    revision INTEGER NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE model_availability_tests (
    model_availability_test_id TEXT PRIMARY KEY,
    endpoint_id TEXT NOT NULL,
    model_profile_id TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    models_revision_at_start INTEGER NOT NULL CHECK (models_revision_at_start > 0),
    endpoint_revision_at_start INTEGER NOT NULL CHECK (endpoint_revision_at_start > 0),
    model_profile_revision_at_start INTEGER NOT NULL CHECK (model_profile_revision_at_start > 0),
    endpoint_security_fingerprint_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    state TEXT NOT NULL,
    stage TEXT,
    record_json TEXT NOT NULL,
    raw_response_artifact_id TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT
) STRICT;

CREATE TABLE model_discovery_runs (
    discovery_run_id TEXT PRIMARY KEY,
    endpoint_id TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    source_models_revision INTEGER NOT NULL CHECK (source_models_revision > 0),
    endpoint_fingerprint_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    state TEXT NOT NULL,
    record_json TEXT NOT NULL,
    raw_response_artifact_id TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT
) STRICT;

CREATE TABLE model_execution_snapshots (
    model_execution_snapshot_id TEXT PRIMARY KEY,
    model_step_id TEXT NOT NULL,
    model_profile_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE model_failures (
    model_failure_id TEXT PRIMARY KEY,
    failure_fingerprint_json TEXT NOT NULL,
    failure_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE model_outputs (
    model_step_id TEXT PRIMARY KEY,
    model_execution_snapshot_id TEXT NOT NULL
        REFERENCES model_execution_snapshots(model_execution_snapshot_id),
    output_json TEXT NOT NULL,
    raw_response_artifact_id TEXT REFERENCES artifacts(artifact_id),
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE model_runtime_states (
    model_profile_id TEXT PRIMARY KEY,
    state_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE model_usage (
    usage_id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_step_id TEXT NOT NULL,
    model_execution_snapshot_id TEXT NOT NULL
        REFERENCES model_execution_snapshots(model_execution_snapshot_id),
    transport_attempt_id TEXT REFERENCES transport_attempts(transport_attempt_id),
    usage_json TEXT NOT NULL,
    recorded_at TEXT NOT NULL
) STRICT;

CREATE TABLE mutation_journals (
    journal_id TEXT PRIMARY KEY,
    journal_kind TEXT NOT NULL CHECK (length(trim(journal_kind)) > 0),
    idempotency_key TEXT CHECK (
        idempotency_key IS NULL OR length(trim(idempotency_key)) > 0
    ),
    state TEXT NOT NULL CHECK (length(trim(state)) > 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    owner_runtime_instance_id TEXT NOT NULL
        REFERENCES runtime_instances(runtime_instance_id),
    lease_epoch INTEGER NOT NULL CHECK (lease_epoch > 0),
    error_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE mutation_leases (
    resource_kind TEXT NOT NULL CHECK (length(trim(resource_kind)) > 0),
    resource_id TEXT NOT NULL CHECK (length(trim(resource_id)) > 0),
    journal_id TEXT NOT NULL UNIQUE
        REFERENCES mutation_journals(journal_id) ON DELETE CASCADE,
    runtime_instance_id TEXT NOT NULL
        REFERENCES runtime_instances(runtime_instance_id),
    lease_epoch INTEGER NOT NULL CHECK (lease_epoch > 0),
    acquired_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    PRIMARY KEY(resource_kind, resource_id)
) STRICT;

CREATE TABLE network_grants (
    network_grant_id TEXT PRIMARY KEY,
    grant_json TEXT NOT NULL,
    identity_scope TEXT NOT NULL,
    identity_scope_id TEXT NOT NULL,
    lifetime TEXT NOT NULL,
    origins_json TEXT NOT NULL,
    destination_classes_json TEXT NOT NULL,
    methods_json TEXT NOT NULL,
    allowed_data_classes_json TEXT NOT NULL,
    max_upload_bytes INTEGER,
    max_download_bytes INTEGER,
    source_fingerprint_json TEXT,
    expires_at TEXT,
    revoked_at TEXT,
    approval_decision_id TEXT NOT NULL REFERENCES approval_decisions(approval_decision_id)
) STRICT;

CREATE TABLE network_rate_limit_state (
    origin TEXT PRIMARY KEY,
    active_connections INTEGER NOT NULL,
    window_started_at TEXT NOT NULL,
    request_count INTEGER NOT NULL,
    upload_bytes INTEGER NOT NULL,
    download_bytes INTEGER NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE network_request_audit (
    network_request_id TEXT PRIMARY KEY,
    identity_json TEXT NOT NULL,
    caller TEXT NOT NULL,
    inspection_level TEXT NOT NULL,
    method TEXT NOT NULL,
    normalized_origin TEXT,
    destination_classes_json TEXT,
    data_classes_json TEXT NOT NULL,
    upload_bytes INTEGER NOT NULL DEFAULT 0,
    download_bytes INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    error_code TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT
) STRICT;

CREATE TABLE p05_legacy_configuration_transport_reconciliation (
    migration_key TEXT NOT NULL CHECK (
        migration_key = 'p05.legacy_configuration_transport_indeterminate.v1'
    ),
    resolution TEXT NOT NULL CHECK (resolution = 'legacy_indeterminate'),
    transport_attempt_id TEXT PRIMARY KEY
        REFERENCES transport_attempts(transport_attempt_id),
    discovery_run_id TEXT NOT NULL UNIQUE
        REFERENCES model_discovery_runs(discovery_run_id),
    failure_id TEXT NOT NULL
        REFERENCES model_failures(model_failure_id),
    discovery_failed_event_id TEXT NOT NULL UNIQUE
        REFERENCES events(event_id),
    original_state TEXT NOT NULL,
    source_schema_version INTEGER NOT NULL CHECK (
        source_schema_version >= 9 AND source_schema_version < 15
    ),
    canonical_evidence_hash TEXT NOT NULL CHECK (
        length(canonical_evidence_hash) = 64
        AND canonical_evidence_hash NOT GLOB '*[^0-9a-f]*'
    ),
    reconciled_at TEXT NOT NULL
) STRICT;

CREATE TABLE p07_security_source_v1_migration_ledger (
    table_name TEXT NOT NULL,
    record_key TEXT NOT NULL,
    original_source_hash_json TEXT NOT NULL,
    disposition TEXT NOT NULL CHECK (
        disposition IN (
            'grant_revoked', 'approval_invalidated', 'trust_revoked',
            'deny_isolated', 'sandbox_isolated'
        )
    ),
    migrated_at TEXT NOT NULL,
    PRIMARY KEY (table_name, record_key)
) STRICT;

CREATE TABLE permission_grants (
    permission_grant_id TEXT PRIMARY KEY,
    grant_json TEXT NOT NULL,
    lifetime TEXT NOT NULL CHECK (lifetime IN ('once', 'turn_task', 'session', 'workspace')),
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT REFERENCES sessions(session_id) ON DELETE CASCADE,
    turn_task_id TEXT REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    source_json TEXT NOT NULL,
    source_fingerprint_json TEXT,
    capabilities_json TEXT NOT NULL,
    scopes_json TEXT NOT NULL,
    approved_risk_flags_json TEXT NOT NULL,
    operation_fingerprint_json TEXT,
    created_at TEXT NOT NULL,
    expires_at TEXT,
    revoked_at TEXT,
    consumed_at TEXT,
    approval_decision_id TEXT
) STRICT;

CREATE TABLE permission_snapshots (
    permission_snapshot_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    mode TEXT NOT NULL,
    session_permission_revision INTEGER NOT NULL,
    active_grant_ids_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE projection_outbox (
    outbox_id INTEGER PRIMARY KEY AUTOINCREMENT,
    projection_kind TEXT NOT NULL,
    projection_key TEXT NOT NULL,
    revision INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    published_at TEXT
) STRICT;

CREATE TABLE projection_revisions (
    projection_kind TEXT NOT NULL,
    projection_key TEXT NOT NULL,
    revision INTEGER NOT NULL,
    PRIMARY KEY (projection_kind, projection_key)
) STRICT;

CREATE TABLE prompt_bundle_snapshots (
    prompt_bundle_snapshot_id TEXT PRIMARY KEY,
    turn_task_id TEXT NOT NULL UNIQUE REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    bundle_version TEXT NOT NULL,
    serializer_version INTEGER NOT NULL CHECK (serializer_version > 0),
    settings_revision INTEGER NOT NULL CHECK (settings_revision >= 0),
    bundle_manifest_hash_json TEXT NOT NULL,
    release_identity TEXT NOT NULL,
    custom_addendum_artifact_id TEXT REFERENCES artifacts(artifact_id),
    custom_addendum_hash_json TEXT,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE prompt_module_snapshots (
    prompt_bundle_snapshot_id TEXT NOT NULL
        REFERENCES prompt_bundle_snapshots(prompt_bundle_snapshot_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    module_id TEXT NOT NULL,
    module_version TEXT NOT NULL,
    section TEXT NOT NULL,
    activation_json TEXT NOT NULL,
    module_order INTEGER NOT NULL CHECK (module_order >= 0),
    content_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    content_hash_json TEXT NOT NULL,
    PRIMARY KEY (prompt_bundle_snapshot_id, ordinal),
    UNIQUE (prompt_bundle_snapshot_id, module_id)
) STRICT;

CREATE TABLE provider_catalog_entries (
    catalog_revision TEXT NOT NULL
        REFERENCES provider_catalog_snapshots(catalog_revision) ON DELETE CASCADE,
    catalog_provider_id TEXT NOT NULL,
    entry_json TEXT NOT NULL,
    PRIMARY KEY (catalog_revision, catalog_provider_id)
) STRICT;

CREATE TABLE provider_catalog_icons (
    catalog_provider_id TEXT PRIMARY KEY CHECK (length(trim(catalog_provider_id)) > 0),
    artifact_id TEXT NOT NULL UNIQUE,
    content_hash_json TEXT NOT NULL CHECK (json_valid(content_hash_json)),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    pixel_size INTEGER NOT NULL CHECK (pixel_size = 128),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE provider_catalog_runtime_state (
    catalog_key TEXT PRIMARY KEY CHECK (catalog_key = 'models.dev'),
    current_revision TEXT REFERENCES provider_catalog_snapshots(catalog_revision),
    last_error_code TEXT,
    checked_at TEXT,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE provider_catalog_snapshots (
    catalog_revision TEXT PRIMARY KEY,
    catalog_content_hash TEXT NOT NULL UNIQUE,
    checked_at TEXT NOT NULL,
    last_successful_refresh_at TEXT NOT NULL,
    etag TEXT,
    last_modified TEXT,
    raw_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE provider_continuation_refs (
    model_step_id TEXT PRIMARY KEY,
    model_execution_snapshot_id TEXT NOT NULL
        REFERENCES model_execution_snapshots(model_execution_snapshot_id),
    endpoint_id TEXT NOT NULL,
    model_profile_id TEXT NOT NULL,
    continuation_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE provider_tool_call_protocol_bindings (
    tool_call_id TEXT PRIMARY KEY,
    model_execution_snapshot_id TEXT NOT NULL
        REFERENCES model_execution_snapshots(model_execution_snapshot_id),
    tool_id TEXT NOT NULL,
    provider_wire_tool_name TEXT NOT NULL,
    provider_tool_call_id TEXT NOT NULL,
    created_at TEXT NOT NULL, tool_definition_json TEXT,
    UNIQUE(model_execution_snapshot_id, provider_tool_call_id)
) STRICT;

CREATE TABLE quarantine_records (
    quarantine_record_id TEXT PRIMARY KEY,
    record_json TEXT NOT NULL,
    original_name TEXT NOT NULL,
    source_url TEXT NOT NULL,
    media_type TEXT,
    content_disposition TEXT,
    detected_kinds_json TEXT NOT NULL,
    content_hash_json TEXT NOT NULL,
    byte_length INTEGER NOT NULL,
    relative_object_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (state IN ('quarantined', 'released', 'discarded')),
    created_at TEXT NOT NULL,
    resolved_at TEXT
, artifact_id TEXT REFERENCES artifacts(artifact_id) ON DELETE SET NULL) STRICT;

CREATE TABLE quarantine_release_journal (
    quarantine_record_id TEXT PRIMARY KEY
        REFERENCES quarantine_records(quarantine_record_id) ON DELETE CASCADE,
    approval_request_id TEXT NOT NULL REFERENCES approval_requests(approval_request_id),
    binding_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'authorized', 'applying', 'published', 'completed', 'failed', 'interrupted'
    )),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error_code TEXT
) STRICT;

CREATE TABLE runtime_instances (
    runtime_instance_id TEXT PRIMARY KEY,
    process_kind TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT
) STRICT;

CREATE TABLE runtime_leases (
    lease_id TEXT PRIMARY KEY,
    runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(runtime_instance_id),
    owner_kind TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('active', 'expired', 'released'))
) STRICT;

CREATE TABLE sandbox_acl_journal (
    sandbox_acl_journal_id TEXT PRIMARY KEY,
    sandbox_execution_id TEXT NOT NULL
        REFERENCES sandbox_executions(sandbox_execution_id) ON DELETE CASCADE,
    target_identity_json TEXT NOT NULL,
    target_path_json TEXT NOT NULL,
    target_path_display TEXT NOT NULL,
    original_dacl_sddl TEXT NOT NULL,
    original_dacl_hash TEXT NOT NULL,
    appcontainer_profile_name TEXT NOT NULL,
    appcontainer_sid TEXT NOT NULL,
    access_mask INTEGER NOT NULL,
    recursive INTEGER NOT NULL CHECK (recursive IN (0, 1)),
    state TEXT NOT NULL CHECK (
        state IN ('prepared', 'applied', 'cleanup_pending', 'cleaned', 'cleanup_failed')
    ),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error_code TEXT
) STRICT;

CREATE TABLE sandbox_executions (
    sandbox_execution_id TEXT PRIMARY KEY,
    identity_json TEXT NOT NULL,
    permission_snapshot_id TEXT NOT NULL
        REFERENCES authorization_snapshots(authorization_snapshot_id),
    mode TEXT NOT NULL,
    plan_hash_json TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    state TEXT NOT NULL,
    process_id INTEGER,
    exit_code INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    terminal_at TEXT
) STRICT;

CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    checksum TEXT NOT NULL,
    applied_at TEXT NOT NULL
) STRICT;

CREATE TABLE session_model_bindings (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_id) ON DELETE CASCADE,
    model_profile_id TEXT,
    binding_revision INTEGER NOT NULL CHECK (binding_revision > 0),
    bound_at TEXT,
    readable_ref_at_binding TEXT,
    CHECK (
        (model_profile_id IS NULL AND bound_at IS NULL AND readable_ref_at_binding IS NULL)
        OR
        (model_profile_id IS NOT NULL AND bound_at IS NOT NULL AND readable_ref_at_binding IS NOT NULL)
    )
) STRICT;

CREATE TABLE session_permission_states (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    mode TEXT NOT NULL CHECK (mode IN ('read_only', 'workspace_read_write', 'full_access')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE session_tool_pins (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, tool_id)
) STRICT;

CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE signed_builtin_release_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    release_revision INTEGER NOT NULL CHECK (release_revision > 0),
    signed_release_hash_json TEXT NOT NULL,
    source_hash TEXT NOT NULL CHECK (
        length(source_hash) = 64
        AND source_hash NOT GLOB '*[^0-9a-f]*'
    ),
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    materialized_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_activity_cursor_bindings (
    cursor_token_hash TEXT PRIMARY KEY CHECK (
        length(cursor_token_hash) = 64
        AND cursor_token_hash NOT GLOB '*[^0-9a-f]*'
    ),
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    binding_json TEXT NOT NULL,
    binding_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_attempt_operational_state (
    tool_execution_attempt_id TEXT PRIMARY KEY
        REFERENCES tool_execution_attempts(tool_execution_attempt_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    percentage INTEGER CHECK (percentage BETWEEN 0 AND 100),
    progress_json TEXT,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_batch_payload_leases (
    tool_batch_id TEXT NOT NULL REFERENCES tool_batches(tool_batch_id) ON DELETE CASCADE,
    tool_package_id TEXT NOT NULL,
    package_version_json TEXT NOT NULL,
    strong_reference_id TEXT NOT NULL UNIQUE,
    acquired_at TEXT NOT NULL,
    released_at TEXT,
    PRIMARY KEY (tool_batch_id, tool_package_id, package_version_json),
    FOREIGN KEY (tool_package_id, package_version_json)
        REFERENCES tool_package_versions(tool_package_id, package_version_json)
) STRICT;

CREATE TABLE tool_batches (
    tool_batch_id TEXT PRIMARY KEY,
    batch_content_hash_json TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(session_id),
    turn_task_id TEXT NOT NULL REFERENCES turn_tasks(turn_task_id),
    agent_run_id TEXT NOT NULL REFERENCES agent_runs(agent_run_id),
    model_step_id TEXT NOT NULL,
    registry_snapshot_id TEXT NOT NULL
        REFERENCES tool_registry_snapshots(tool_registry_snapshot_id),
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    registry_content_hash_json TEXT NOT NULL,
    p08_state_revision INTEGER NOT NULL CHECK (p08_state_revision > 0),
    p08_state_hash_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'pending_admission', 'admitted', 'running', 'cancelling',
            'completed', 'recovery_required'
        )
    ),
    request_json TEXT NOT NULL,
    request_hash_json TEXT NOT NULL,
    execution_plan_json TEXT,
    execution_plan_hash_json TEXT,
    result_json TEXT,
    result_hash_json TEXT,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    internal_diagnostic_artifact_id TEXT REFERENCES artifacts(artifact_id),
    created_at TEXT NOT NULL,
    admitted_at TEXT,
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT NOT NULL,
    CHECK (
        (execution_plan_json IS NULL AND execution_plan_hash_json IS NULL AND admitted_at IS NULL)
        OR
        (execution_plan_json IS NOT NULL
            AND execution_plan_hash_json IS NOT NULL
            AND admitted_at IS NOT NULL)
    ),
    CHECK (
        (result_json IS NULL AND result_hash_json IS NULL AND completed_at IS NULL)
        OR
        (result_json IS NOT NULL AND result_hash_json IS NOT NULL AND completed_at IS NOT NULL)
    ),
    CHECK (
        (status = 'completed' AND result_json IS NOT NULL)
        OR
        (status <> 'completed' AND result_json IS NULL)
    )
) STRICT;

CREATE TABLE tool_builtin_core_handler_dispatch_bindings (
    tool_execution_attempt_id TEXT PRIMARY KEY
        REFERENCES tool_execution_attempts(tool_execution_attempt_id) ON DELETE RESTRICT,
    security_operation_id TEXT NOT NULL UNIQUE,
    binding_json TEXT NOT NULL,
    binding_hash_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('dispatch_prepared', 'consumed')),
    prepared_at TEXT NOT NULL,
    consumed_at TEXT,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_calls (
    tool_call_id TEXT PRIMARY KEY,
    tool_batch_id TEXT NOT NULL REFERENCES tool_batches(tool_batch_id),
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'pending_admission', 'waiting_dependency', 'waiting_permission',
            'waiting_approval', 'waiting_resource', 'queued', 'running',
            'cancelling', 'finalized', 'recovery_required'
        )
    ),
    normalized_call_json TEXT NOT NULL,
    normalized_call_hash_json TEXT NOT NULL,
    execution_plan_call_json TEXT,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT NOT NULL,
    UNIQUE (tool_batch_id, ordinal),
    CHECK (
        (status = 'finalized' AND completed_at IS NOT NULL)
        OR
        (status <> 'finalized' AND completed_at IS NULL)
    )
) STRICT;

CREATE TABLE tool_conformance_runs (
    tool_conformance_run_id TEXT PRIMARY KEY,
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL CHECK (draft_revision > 0),
    draft_snapshot_hash_json TEXT NOT NULL,
    tool_key TEXT NOT NULL,
    conformance_case TEXT NOT NULL,
    canonical_manifest_hash_json TEXT NOT NULL,
    runtime_closure_hash_json TEXT NOT NULL,
    package_fingerprint_json TEXT NOT NULL,
    binding_json TEXT NOT NULL,
    binding_hash_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('queued', 'running', 'passed', 'failed', 'cancelled', 'recovery_required')
    ),
    evidence_json TEXT,
    evidence_hash_json TEXT,
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    started_event_id TEXT NOT NULL UNIQUE REFERENCES events(event_id),
    completed_event_id TEXT UNIQUE REFERENCES events(event_id),
    started_at TEXT NOT NULL,
    completed_at TEXT,
    updated_at TEXT NOT NULL,
    CHECK (
        (status IN ('queued', 'running')
            AND evidence_json IS NULL
            AND evidence_hash_json IS NULL
            AND completed_event_id IS NULL
            AND completed_at IS NULL)
        OR
        (status IN ('passed', 'failed', 'cancelled', 'recovery_required')
            AND evidence_json IS NOT NULL
            AND evidence_hash_json IS NOT NULL
            AND completed_event_id IS NOT NULL
            AND completed_at IS NOT NULL)
    )
) STRICT;

CREATE TABLE tool_conformance_sandbox_dispatch_bindings (
    tool_conformance_run_id TEXT PRIMARY KEY
        REFERENCES tool_conformance_runs(tool_conformance_run_id) ON DELETE RESTRICT,
    sandbox_execution_id TEXT NOT NULL UNIQUE,
    security_operation_id TEXT NOT NULL UNIQUE,
    tool_call_id TEXT NOT NULL UNIQUE,
    tool_execution_attempt_id TEXT NOT NULL UNIQUE,
    allow_once_evidence_json TEXT NOT NULL CHECK (json_valid(allow_once_evidence_json)),
    allow_once_evidence_hash_json TEXT NOT NULL
        CHECK (json_valid(allow_once_evidence_hash_json)),
    runtime_binding_hash_json TEXT NOT NULL CHECK (json_valid(runtime_binding_hash_json)),
    sandbox_plan_hash_json TEXT NOT NULL CHECK (json_valid(sandbox_plan_hash_json)),
    binding_json TEXT NOT NULL CHECK (json_valid(binding_json)),
    binding_hash_json TEXT NOT NULL CHECK (json_valid(binding_hash_json)),
    state TEXT NOT NULL CHECK (state IN ('dispatch_prepared', 'consumed', 'revoked')),
    prepared_at TEXT NOT NULL,
    consumed_at TEXT,
    revoked_at TEXT,
    updated_at TEXT NOT NULL,
    CHECK (
        (state = 'dispatch_prepared' AND consumed_at IS NULL AND revoked_at IS NULL)
        OR (state = 'consumed' AND consumed_at IS NOT NULL AND revoked_at IS NULL)
        OR (state = 'revoked' AND revoked_at IS NOT NULL)
    )
) STRICT;

CREATE TABLE tool_credential_authority_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    candidate_set_revision INTEGER NOT NULL CHECK (candidate_set_revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_credential_secret_bindings (
    credential_id TEXT PRIMARY KEY REFERENCES tool_credentials(credential_id) ON DELETE CASCADE,
    credential_revision INTEGER NOT NULL CHECK (credential_revision > 0),
    credential_ref TEXT NOT NULL UNIQUE,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_credentials (
    credential_id TEXT PRIMARY KEY,
    credential_revision INTEGER NOT NULL CHECK (credential_revision > 0),
    authenticated_owner TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('configured', 'suspended', 'revoked', 'recovery_required')
    ),
    row_revision INTEGER NOT NULL CHECK (row_revision > 0),
    record_json TEXT NOT NULL CHECK (json_valid(record_json)),
    record_hash_json TEXT NOT NULL CHECK (json_valid(record_hash_json)),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_definitions (
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    tool_package_id TEXT REFERENCES tool_packages(tool_package_id),
    tool_key TEXT,
    canonical_name TEXT NOT NULL,
    definition_revision INTEGER NOT NULL CHECK (definition_revision > 0),
    definition_fingerprint_json TEXT NOT NULL,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    retired_at TEXT,
    PRIMARY KEY (tool_id, tool_version_json),
    CHECK (
        (tool_package_id IS NULL AND tool_key IS NULL)
        OR (tool_package_id IS NOT NULL AND tool_key IS NOT NULL)
    )
) STRICT;

CREATE TABLE tool_draft_approvals (
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    snapshot_hash_json TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('install', 'install_and_trust', 'reject')),
    evidence_json TEXT NOT NULL,
    evidence_hash_json TEXT NOT NULL,
    decided_at TEXT NOT NULL, consumed_by_journal_id TEXT REFERENCES mutation_journals(journal_id),
    PRIMARY KEY (tool_draft_id, draft_revision),
    FOREIGN KEY (tool_draft_id, draft_revision)
        REFERENCES tool_draft_revisions(tool_draft_id, draft_revision) ON DELETE CASCADE
) STRICT;

CREATE TABLE tool_draft_previews (
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    snapshot_hash_json TEXT NOT NULL,
    preview_json TEXT NOT NULL,
    preview_hash_json TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    PRIMARY KEY (tool_draft_id, draft_revision),
    FOREIGN KEY (tool_draft_id, draft_revision)
        REFERENCES tool_draft_revisions(tool_draft_id, draft_revision) ON DELETE CASCADE
) STRICT;

CREATE TABLE tool_draft_revision_files (
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    path TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    raw_file_sha256_json TEXT NOT NULL,
    content_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    content_hash_json TEXT NOT NULL,
    PRIMARY KEY (tool_draft_id, draft_revision, ordinal),
    UNIQUE (tool_draft_id, draft_revision, path),
    FOREIGN KEY (tool_draft_id, draft_revision)
        REFERENCES tool_draft_revisions(tool_draft_id, draft_revision) ON DELETE CASCADE
) STRICT;

CREATE TABLE tool_draft_revisions (
    tool_draft_id TEXT NOT NULL REFERENCES tool_drafts(tool_draft_id),
    draft_revision INTEGER NOT NULL CHECK (draft_revision > 0),
    parent_revision INTEGER CHECK (parent_revision IS NULL OR parent_revision > 0),
    snapshot_hash_json TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    snapshot_record_hash_json TEXT NOT NULL,
    raw_manifest_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
    actor TEXT NOT NULL CHECK (actor IN ('user', 'parent_agent', 'child_agent', 'system_runtime')),
    created_at TEXT NOT NULL,
    PRIMARY KEY (tool_draft_id, draft_revision),
    UNIQUE (tool_draft_id, snapshot_hash_json, draft_revision),
    CHECK (
        (draft_revision = 1 AND parent_revision IS NULL)
        OR (draft_revision > 1 AND parent_revision = draft_revision - 1)
    )
) STRICT;

CREATE TABLE tool_draft_snapshot_staging (
    snapshot_artifact_id TEXT PRIMARY KEY REFERENCES artifacts(artifact_id),
    tool_draft_id TEXT NOT NULL REFERENCES tool_drafts(tool_draft_id),
    expected_draft_revision INTEGER NOT NULL CHECK (expected_draft_revision > 0),
    snapshot_hash_json TEXT NOT NULL CHECK (json_valid(snapshot_hash_json)),
    command_invocation_id TEXT NOT NULL UNIQUE,
    correlation_id TEXT NOT NULL,
    authenticated_owner TEXT NOT NULL,
    core_runtime_instance_id TEXT NOT NULL,
    authenticated_main_runtime_instance_id TEXT NOT NULL,
    authenticated_channel_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT,
    CHECK (expires_at > created_at),
    CHECK (consumed_at IS NULL OR consumed_at >= created_at)
) STRICT;

CREATE TABLE tool_draft_snapshot_staging_artifacts (
    snapshot_artifact_id TEXT NOT NULL
        REFERENCES tool_draft_snapshot_staging(snapshot_artifact_id) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL UNIQUE REFERENCES artifacts(artifact_id),
    PRIMARY KEY (snapshot_artifact_id, artifact_id)
) STRICT;

CREATE TABLE tool_draft_validation_failures (
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    snapshot_hash_json TEXT NOT NULL,
    safe_error_json TEXT NOT NULL,
    safe_error_hash_json TEXT NOT NULL,
    failed_at TEXT NOT NULL,
    PRIMARY KEY (tool_draft_id, draft_revision),
    FOREIGN KEY (tool_draft_id, draft_revision)
        REFERENCES tool_draft_revisions(tool_draft_id, draft_revision) ON DELETE CASCADE
) STRICT;

CREATE TABLE tool_draft_validations (
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    snapshot_hash_json TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    evidence_hash_json TEXT NOT NULL,
    validated_at TEXT NOT NULL,
    PRIMARY KEY (tool_draft_id, draft_revision),
    FOREIGN KEY (tool_draft_id, draft_revision)
        REFERENCES tool_draft_revisions(tool_draft_id, draft_revision) ON DELETE CASCADE
) STRICT;

CREATE TABLE tool_drafts (
    tool_draft_id TEXT PRIMARY KEY,
    status TEXT NOT NULL CHECK (
        status IN (
            'editing', 'validating', 'validation_failed', 'ready_for_test',
            'testing', 'test_failed', 'ready_to_apply', 'applying', 'applied',
            'discarded', 'recovery_required'
        )
    ),
    current_draft_revision INTEGER NOT NULL CHECK (current_draft_revision > 0),
    current_snapshot_hash_json TEXT NOT NULL,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
, draft_kind TEXT NOT NULL DEFAULT 'content_mutation'
    CHECK (draft_kind IN ('content_mutation', 'package_deletion'))) STRICT;

CREATE TABLE tool_ephemeral_loads (
    turn_task_id TEXT NOT NULL REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    loaded_at TEXT NOT NULL,
    PRIMARY KEY (turn_task_id, tool_id)
) STRICT;

CREATE TABLE tool_execution_attempts (
    tool_execution_attempt_id TEXT PRIMARY KEY,
    tool_call_id TEXT NOT NULL REFERENCES tool_calls(tool_call_id),
    tool_batch_id TEXT NOT NULL REFERENCES tool_batches(tool_batch_id),
    attempt_number INTEGER NOT NULL CHECK (attempt_number BETWEEN 1 AND 4),
    execution_plan_hash_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'started', 'dispatch_prepared', 'running', 'waiting_external',
            'cancelling', 'succeeded', 'failed', 'cancelled', 'indeterminate'
        )
    ),
    side_effect_state TEXT NOT NULL CHECK (
        side_effect_state IN (
            'not_started', 'no_effect', 'fully_applied', 'partially_applied',
            'compensated', 'unknown'
        )
    ),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    evidence_artifact_id TEXT REFERENCES artifacts(artifact_id),
    started_event_id TEXT NOT NULL UNIQUE REFERENCES events(event_id),
    dispatch_prepared_event_id TEXT UNIQUE REFERENCES events(event_id),
    cancellation_requested_event_id TEXT UNIQUE REFERENCES events(event_id),
    terminal_event_id TEXT UNIQUE REFERENCES events(event_id),
    started_at TEXT NOT NULL,
    dispatch_prepared_at TEXT,
    cancellation_requested_at TEXT,
    completed_at TEXT,
    updated_at TEXT NOT NULL,
    UNIQUE (tool_call_id, attempt_number),
    CHECK (
        (status = 'started'
            AND dispatch_prepared_event_id IS NULL
            AND dispatch_prepared_at IS NULL)
        OR
        (status <> 'started'
            AND dispatch_prepared_event_id IS NOT NULL
            AND dispatch_prepared_at IS NOT NULL)
    ),
    CHECK (
        (cancellation_requested_event_id IS NULL AND cancellation_requested_at IS NULL)
        OR
        (cancellation_requested_event_id IS NOT NULL AND cancellation_requested_at IS NOT NULL)
    ),
    CHECK (
        (status IN ('succeeded', 'failed', 'cancelled', 'indeterminate')
            AND terminal_event_id IS NOT NULL
            AND evidence_artifact_id IS NOT NULL
            AND completed_at IS NOT NULL
            AND side_effect_state <> 'not_started')
        OR
        (status NOT IN ('succeeded', 'failed', 'cancelled', 'indeterminate')
            AND terminal_event_id IS NULL
            AND evidence_artifact_id IS NULL
            AND completed_at IS NULL)
    ),
    CHECK (
        status <> 'succeeded'
        OR side_effect_state IN ('no_effect', 'fully_applied', 'unknown')
    )
) STRICT;

CREATE TABLE tool_execution_recovery_diagnostics (
    tool_execution_attempt_id TEXT PRIMARY KEY
        REFERENCES tool_execution_attempts(tool_execution_attempt_id),
    recovery_code TEXT NOT NULL CHECK (
        recovery_code IN ('historical_started_only', 'dispatch_outcome_unknown')
    ),
    evidence_hash_json TEXT NOT NULL,
    detected_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_history_atomic_group_associations (
    tool_batch_id TEXT PRIMARY KEY REFERENCES tool_batches(tool_batch_id) ON DELETE CASCADE,
    assistant_response_event_id TEXT NOT NULL UNIQUE REFERENCES events(event_id),
    assistant_response_content_hash_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'finalized')),
    preimage_json TEXT,
    group_hash_json TEXT,
    created_at TEXT NOT NULL,
    finalized_at TEXT,
    CHECK (
        (state = 'pending'
            AND preimage_json IS NULL
            AND group_hash_json IS NULL
            AND finalized_at IS NULL)
        OR
        (state = 'finalized'
            AND preimage_json IS NOT NULL
            AND group_hash_json IS NOT NULL
            AND finalized_at IS NOT NULL)
    )
) STRICT;

CREATE TABLE tool_instance_configurations (
    tool_instance_id TEXT PRIMARY KEY REFERENCES tool_instances(tool_instance_id),
    configuration_revision INTEGER NOT NULL CHECK (configuration_revision > 0),
    instance_config_schema_hash_json TEXT NOT NULL,
    configuration_json TEXT NOT NULL,
    configuration_hash_json TEXT NOT NULL,
    secret_scan_evidence_hash_json TEXT NOT NULL,
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_instance_credential_bindings (
    tool_instance_id TEXT PRIMARY KEY REFERENCES tool_instances(tool_instance_id),
    credential_slot_key TEXT NOT NULL,
    credential_id TEXT NOT NULL REFERENCES tool_credentials(credential_id),
    credential_revision INTEGER NOT NULL CHECK (credential_revision > 0),
    placement_json TEXT NOT NULL CHECK (json_valid(placement_json)),
    binding_revision INTEGER NOT NULL CHECK (binding_revision > 0),
    binding_hash_json TEXT NOT NULL CHECK (json_valid(binding_hash_json)),
    record_json TEXT NOT NULL CHECK (json_valid(record_json)),
    record_hash_json TEXT NOT NULL CHECK (json_valid(record_hash_json)),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_instance_execution_bindings (
    tool_instance_id TEXT PRIMARY KEY
        REFERENCES tool_instances(tool_instance_id) ON DELETE RESTRICT,
    binding_kind TEXT NOT NULL CHECK (binding_kind = 'builtin_core_handler'),
    binding_revision INTEGER NOT NULL CHECK (binding_revision > 0),
    signed_release_hash_json TEXT NOT NULL,
    binding_json TEXT NOT NULL,
    binding_hash_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_instances (
    tool_instance_id TEXT PRIMARY KEY,
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    runtime_health TEXT NOT NULL CHECK (
        runtime_health IN ('healthy', 'unavailable', 'corrupted')
    ),
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (tool_id, tool_version_json)
        REFERENCES tool_definitions(tool_id, tool_version_json)
) STRICT;

CREATE TABLE tool_long_operations (
    journal_id TEXT PRIMARY KEY REFERENCES mutation_journals(journal_id),
    operation_json TEXT NOT NULL CHECK (json_valid(operation_json)),
    operation_hash_json TEXT NOT NULL CHECK (json_valid(operation_hash_json)),
    approval_draft_id TEXT NOT NULL REFERENCES tool_drafts(tool_draft_id),
    approval_draft_revision INTEGER NOT NULL CHECK (approval_draft_revision > 0),
    approval_evidence_hash_json TEXT NOT NULL CHECK (json_valid(approval_evidence_hash_json)),
    approval_reservation_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN (
            'queued', 'running', 'cancelling', 'cancelled',
            'succeeded', 'failed', 'recovery_required'
        )
    ),
    operational_revision INTEGER NOT NULL CHECK (operational_revision > 0),
    boundary_crossed INTEGER NOT NULL CHECK (boundary_crossed IN (0, 1)),
    safe_error_json TEXT CHECK (safe_error_json IS NULL OR json_valid(safe_error_json)),
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT NOT NULL,
    CHECK (
        (state IN ('queued', 'running', 'cancelling') AND completed_at IS NULL)
        OR (state IN ('cancelled', 'succeeded', 'failed', 'recovery_required')
            AND completed_at IS NOT NULL)
    ),
    CHECK (state <> 'cancelled' OR boundary_crossed = 0)
) STRICT;

CREATE TABLE tool_mutation_command_journals (
    command_invocation_id TEXT PRIMARY KEY,
    method_name TEXT NOT NULL CHECK (length(trim(method_name)) > 0),
    request_json TEXT NOT NULL CHECK (json_valid(request_json)),
    request_hash_json TEXT NOT NULL CHECK (json_valid(request_hash_json)),
    authenticated_identity_hash_json TEXT NOT NULL
        CHECK (json_valid(authenticated_identity_hash_json)),
    seed_json TEXT NOT NULL CHECK (json_valid(seed_json)),
    seed_hash_json TEXT NOT NULL CHECK (json_valid(seed_hash_json)),
    state TEXT NOT NULL CHECK (state IN ('prepared', 'succeeded', 'failed')),
    outcome_json TEXT CHECK (outcome_json IS NULL OR json_valid(outcome_json)),
    outcome_hash_json TEXT CHECK (
        outcome_hash_json IS NULL OR json_valid(outcome_hash_json)
    ),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL, original_correlation_id TEXT,
    CHECK (
        (state = 'prepared' AND outcome_json IS NULL AND outcome_hash_json IS NULL)
        OR
        (state IN ('succeeded', 'failed')
            AND outcome_json IS NOT NULL
            AND outcome_hash_json IS NOT NULL)
    )
) STRICT;

CREATE TABLE tool_package_delete_approvals (
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    target_snapshot_hash_json TEXT NOT NULL,
    preview_hash_json TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('approve_delete', 'reject')),
    user_configuration_identity_binding_hash_json TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    evidence_hash_json TEXT NOT NULL UNIQUE,
    decided_at TEXT NOT NULL,
    consumed_by_journal_id TEXT UNIQUE REFERENCES mutation_journals(journal_id),
    PRIMARY KEY (tool_draft_id, draft_revision),
    FOREIGN KEY (tool_draft_id, draft_revision)
        REFERENCES tool_package_delete_draft_revisions(tool_draft_id, draft_revision)
        ON DELETE CASCADE
) STRICT;

CREATE TABLE tool_package_delete_draft_revisions (
    tool_draft_id TEXT NOT NULL REFERENCES tool_drafts(tool_draft_id),
    draft_revision INTEGER NOT NULL CHECK (draft_revision > 0),
    parent_revision INTEGER CHECK (parent_revision IS NULL OR parent_revision > 0),
    target_snapshot_hash_json TEXT NOT NULL,
    preview_hash_json TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    snapshot_record_hash_json TEXT NOT NULL,
    initiator TEXT NOT NULL CHECK (
        initiator IN ('user', 'parent_agent', 'child_agent', 'system_runtime')
    ),
    created_at TEXT NOT NULL,
    PRIMARY KEY (tool_draft_id, draft_revision),
    CHECK (
        (draft_revision = 1 AND parent_revision IS NULL)
        OR (draft_revision > 1 AND parent_revision = draft_revision - 1)
    )
) STRICT;

CREATE TABLE tool_package_orphaned_payloads (
    package_store_relative_path TEXT PRIMARY KEY,
    detected_at TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (reason = 'missing_authority_record')
) STRICT;

CREATE TABLE tool_package_payload_strong_references (
    reference_kind TEXT NOT NULL,
    reference_id TEXT NOT NULL,
    tool_package_id TEXT NOT NULL,
    package_version_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    released_at TEXT,
    PRIMARY KEY (reference_kind, reference_id),
    FOREIGN KEY (tool_package_id, package_version_json)
        REFERENCES tool_package_versions(tool_package_id, package_version_json)
) STRICT;

CREATE TABLE tool_package_payloads (
    tool_package_id TEXT NOT NULL,
    package_version_json TEXT NOT NULL,
    package_fingerprint_json TEXT NOT NULL,
    package_security_fingerprint_json TEXT NOT NULL,
    package_store_relative_path TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL CHECK (
        state IN ('published', 'quarantined', 'gc_eligible', 'deleted', 'orphaned')
    ),
    mutation_journal_id TEXT UNIQUE REFERENCES mutation_journals(journal_id),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    published_at TEXT NOT NULL,
    quarantined_at TEXT,
    deleted_at TEXT,
    PRIMARY KEY (tool_package_id, package_version_json)
) STRICT;

CREATE TABLE tool_package_tombstones (
    tool_package_id TEXT PRIMARY KEY,
    package_key TEXT NOT NULL,
    tombstone_json TEXT NOT NULL,
    tombstone_hash_json TEXT NOT NULL,
    mutation_journal_id TEXT NOT NULL UNIQUE REFERENCES mutation_journals(journal_id),
    deleted_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_package_trust (
    tool_package_id TEXT PRIMARY KEY REFERENCES tool_packages(tool_package_id),
    package_security_fingerprint_json TEXT NOT NULL,
    trusted INTEGER NOT NULL CHECK (trusted IN (0, 1)),
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    reviewed_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_package_trust_intents (
    tool_package_id TEXT NOT NULL,
    package_security_fingerprint_json TEXT NOT NULL,
    approval_evidence_hash_json TEXT NOT NULL,
    mutation_journal_id TEXT NOT NULL REFERENCES mutation_journals(journal_id),
    state TEXT NOT NULL CHECK (state IN ('pending', 'applied', 'failed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tool_package_id, package_security_fingerprint_json)
) STRICT;

CREATE TABLE tool_package_version_evidence (
    tool_package_id TEXT NOT NULL,
    package_version_json TEXT NOT NULL,
    tool_draft_id TEXT NOT NULL REFERENCES tool_drafts(tool_draft_id),
    draft_revision INTEGER NOT NULL CHECK (draft_revision > 0),
    draft_snapshot_hash_json TEXT NOT NULL,
    validation_evidence_hash_json TEXT NOT NULL,
    approval_evidence_hash_json TEXT NOT NULL,
    approval_decision TEXT NOT NULL CHECK (approval_decision IN ('install', 'install_and_trust')),
    canonical_manifest_json TEXT NOT NULL,
    canonical_manifest_hash_json TEXT NOT NULL,
    runtime_closure_json TEXT NOT NULL,
    runtime_closure_hash_json TEXT NOT NULL,
    tool_contract_fingerprints_json TEXT NOT NULL,
    conformance_run_ids_json TEXT NOT NULL,
    mutation_journal_id TEXT NOT NULL UNIQUE REFERENCES mutation_journals(journal_id),
    evidence_json TEXT NOT NULL,
    evidence_hash_json TEXT NOT NULL,
    PRIMARY KEY (tool_package_id, package_version_json),
    FOREIGN KEY (tool_package_id, package_version_json)
        REFERENCES tool_package_versions(tool_package_id, package_version_json)
) STRICT;

CREATE TABLE tool_package_versions (
    tool_package_id TEXT NOT NULL REFERENCES tool_packages(tool_package_id),
    package_version_json TEXT NOT NULL,
    package_fingerprint_json TEXT NOT NULL,
    package_security_fingerprint_json TEXT NOT NULL,
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    installed_at TEXT NOT NULL,
    PRIMARY KEY (tool_package_id, package_version_json)
) STRICT;

CREATE TABLE tool_packages (
    tool_package_id TEXT PRIMARY KEY,
    package_key TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('installed', 'corrupted', 'deleted')),
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
, deletion_pending INTEGER NOT NULL DEFAULT 0
    CHECK (deletion_pending IN (0, 1))) STRICT;

CREATE TABLE tool_profile_principals (
    profile_key TEXT PRIMARY KEY CHECK (profile_key = 'current'),
    principal_id TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_registry_authority_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_registry_definition_records (
    tool_registry_snapshot_id TEXT NOT NULL
        REFERENCES tool_registry_snapshots(tool_registry_snapshot_id) ON DELETE CASCADE,
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    definition_json TEXT NOT NULL,
    definition_hash_json TEXT NOT NULL,
    instance_json TEXT NOT NULL,
    instance_hash_json TEXT NOT NULL,
    PRIMARY KEY (tool_registry_snapshot_id, tool_id)
) STRICT;

CREATE TABLE tool_registry_execution_authority_records (
    tool_registry_snapshot_id TEXT NOT NULL
        REFERENCES tool_registry_snapshots(tool_registry_snapshot_id) ON DELETE CASCADE,
    tool_id TEXT NOT NULL,
    authority_json TEXT NOT NULL,
    authority_hash_json TEXT NOT NULL,
    PRIMARY KEY (tool_registry_snapshot_id, tool_id),
    FOREIGN KEY (tool_registry_snapshot_id, tool_id)
        REFERENCES tool_registry_definition_records(tool_registry_snapshot_id, tool_id)
        ON DELETE CASCADE
) STRICT;

CREATE TABLE tool_registry_package_payload_bindings (
    tool_registry_snapshot_id TEXT NOT NULL
        REFERENCES tool_registry_snapshots(tool_registry_snapshot_id) ON DELETE CASCADE,
    tool_id TEXT NOT NULL,
    tool_package_id TEXT NOT NULL,
    package_version_json TEXT NOT NULL,
    binding_hash_json TEXT NOT NULL,
    PRIMARY KEY (tool_registry_snapshot_id, tool_id),
    FOREIGN KEY (tool_registry_snapshot_id, tool_id)
        REFERENCES tool_registry_definition_records(tool_registry_snapshot_id, tool_id)
        ON DELETE CASCADE,
    FOREIGN KEY (tool_package_id, package_version_json)
        REFERENCES tool_package_versions(tool_package_id, package_version_json)
) STRICT;

CREATE TABLE tool_registry_scope_heads (
    scope_hash_json TEXT PRIMARY KEY,
    scope_json TEXT NOT NULL,
    tool_registry_snapshot_id TEXT NOT NULL
        REFERENCES tool_registry_snapshots(tool_registry_snapshot_id),
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    content_hash_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_registry_snapshots (
    tool_registry_snapshot_id TEXT PRIMARY KEY,
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    content_hash_json TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    search_documents_json TEXT,
    search_documents_hash_json TEXT,
    search_corpus_retired_at TEXT,
    created_at TEXT NOT NULL, snapshot_contract_version INTEGER NOT NULL DEFAULT 1
        CHECK (snapshot_contract_version IN (1, 2)),
    CHECK (
        (search_documents_json IS NOT NULL
            AND search_documents_hash_json IS NOT NULL
            AND search_corpus_retired_at IS NULL)
        OR
        (search_documents_json IS NULL
            AND search_documents_hash_json IS NULL
            AND search_corpus_retired_at IS NOT NULL)
    )
) STRICT;

CREATE TABLE tool_results (
    tool_call_id TEXT PRIMARY KEY REFERENCES tool_calls(tool_call_id),
    tool_batch_id TEXT NOT NULL REFERENCES tool_batches(tool_batch_id),
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    outcome_kind TEXT NOT NULL CHECK (
        outcome_kind IN ('succeeded', 'failed', 'cancelled', 'not_executed', 'indeterminate')
    ),
    side_effect_state TEXT NOT NULL CHECK (
        side_effect_state IN (
            'not_started', 'no_effect', 'fully_applied', 'partially_applied',
            'compensated', 'unknown'
        )
    ),
    result_json TEXT NOT NULL,
    result_hash_json TEXT NOT NULL,
    finalized_event_id TEXT NOT NULL UNIQUE REFERENCES events(event_id),
    model_visible_result_artifact_id TEXT REFERENCES artifacts(artifact_id),
    user_audit_result_artifact_id TEXT REFERENCES artifacts(artifact_id),
    internal_diagnostic_artifact_id TEXT REFERENCES artifacts(artifact_id),
    aggregate_side_effect_evidence_artifact_id TEXT REFERENCES artifacts(artifact_id),
    completed_at TEXT NOT NULL,
    UNIQUE (tool_batch_id, ordinal),
    CHECK (
        outcome_kind <> 'succeeded'
        OR side_effect_state IN ('no_effect', 'fully_applied', 'unknown')
    ),
    CHECK (outcome_kind <> 'not_executed' OR side_effect_state = 'not_started'),
    CHECK (
        model_visible_result_artifact_id IS NULL
        OR model_visible_result_artifact_id <> internal_diagnostic_artifact_id
    )
) STRICT;

CREATE TABLE tool_sandbox_dispatch_bindings (
    sandbox_execution_id TEXT PRIMARY KEY,
    security_operation_id TEXT NOT NULL UNIQUE,
    tool_execution_attempt_id TEXT NOT NULL UNIQUE
        REFERENCES tool_execution_attempts(tool_execution_attempt_id),
    tool_batch_id TEXT NOT NULL REFERENCES tool_batches(tool_batch_id),
    tool_call_id TEXT NOT NULL REFERENCES tool_calls(tool_call_id),
    permission_snapshot_id TEXT NOT NULL REFERENCES permission_snapshots(permission_snapshot_id),
    execution_plan_hash_json TEXT NOT NULL,
    runtime_binding_hash_json TEXT NOT NULL,
    binding_json TEXT NOT NULL,
    binding_hash_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('dispatch_prepared', 'consumed', 'revoked', 'cancelled', 'recovery_required')
    ),
    prepared_at TEXT NOT NULL,
    consumed_at TEXT,
    updated_at TEXT NOT NULL,
    CHECK (
        (state = 'dispatch_prepared' AND consumed_at IS NULL)
        OR (state = 'consumed' AND consumed_at IS NOT NULL)
        OR state IN ('revoked', 'cancelled', 'recovery_required')
    )
) STRICT;

CREATE TABLE tool_search_cursor_bindings (
    cursor_token_hash TEXT PRIMARY KEY CHECK (
        length(cursor_token_hash) = 64
        AND cursor_token_hash NOT GLOB '*[^0-9a-f]*'
    ),
    tool_registry_snapshot_id TEXT NOT NULL,
    binding_json TEXT NOT NULL,
    binding_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_tombstones (
    tool_id TEXT PRIMARY KEY,
    tool_package_id TEXT NOT NULL REFERENCES tool_package_tombstones(tool_package_id),
    tombstone_json TEXT NOT NULL,
    tombstone_hash_json TEXT NOT NULL,
    deleted_at TEXT NOT NULL
) STRICT;

CREATE TABLE transport_attempts (
    transport_attempt_id TEXT PRIMARY KEY,
    endpoint_id TEXT NOT NULL,
    operation_context_json TEXT NOT NULL,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    attempt_kind TEXT NOT NULL,
    state TEXT NOT NULL,
    record_json TEXT NOT NULL,
    canonical_body_artifact_id TEXT REFERENCES artifacts(artifact_id),
    raw_stream_artifact_id TEXT REFERENCES artifacts(artifact_id),
    model_failure_id TEXT REFERENCES model_failures(model_failure_id),
    started_at TEXT NOT NULL,
    finished_at TEXT,
    UNIQUE(operation_context_json, attempt_number)
) STRICT;

CREATE TABLE turn_deny_records (
    turn_task_id TEXT NOT NULL REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    source_key TEXT NOT NULL,
    source_json TEXT NOT NULL,
    operation_fingerprint_json TEXT NOT NULL,
    denied_at TEXT NOT NULL,
    PRIMARY KEY (turn_task_id, source_key, operation_fingerprint_json)
) STRICT;

CREATE TABLE turn_tasks (
    turn_task_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE workspace_catalog_entries (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    parent_path TEXT NOT NULL,
    file_name TEXT NOT NULL,
    entry_kind TEXT NOT NULL CHECK (entry_kind IN ('file', 'directory', 'reparse_point')),
    partition_path TEXT NOT NULL,
    byte_length INTEGER,
    modified_at TEXT,
    content_hash TEXT,
    text_encoding TEXT,
    ignored INTEGER NOT NULL CHECK (ignored IN (0, 1)),
    generated INTEGER NOT NULL CHECK (generated IN (0, 1)),
    hidden_from_search INTEGER NOT NULL CHECK (hidden_from_search IN (0, 1)),
    reparse_tag INTEGER,
    PRIMARY KEY (workspace_id, relative_path)
) STRICT;

CREATE TABLE workspace_catalog_partitions (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    partition_path TEXT NOT NULL,
    file_count INTEGER NOT NULL,
    indexable_text_bytes INTEGER NOT NULL,
    index_mode TEXT NOT NULL CHECK (index_mode IN ('full_text', 'metadata_only')),
    PRIMARY KEY (workspace_id, partition_path)
) STRICT;

CREATE VIRTUAL TABLE workspace_content_fts USING fts5(
    workspace_id UNINDEXED,
    relative_path UNINDEXED,
    content,
    tokenize = 'unicode61'
);

CREATE TABLE workspace_deferred_mutations (
    deferred_mutation_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    expected_hash TEXT,
    reason TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending_reread', 'superseded', 'cancelled')),
    created_at TEXT NOT NULL,
    resolved_at TEXT
) STRICT;

CREATE TABLE workspace_lifecycle_operations (
    operation_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    renderer_instance_id TEXT NOT NULL,
    operation_kind TEXT NOT NULL CHECK (
        operation_kind IN ('archive', 'relocate', 'permanent_delete')
    ),
    operation_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN (
            'preparing', 'blocked', 'saving', 'discarding', 'committing',
            'completed', 'cancelled', 'failed'
        )
    ),
    lifecycle_revision INTEGER NOT NULL,
    ack_deadline_at TEXT,
    preflight_json TEXT,
    error_code TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    terminal_at TEXT
) STRICT;

CREATE TABLE workspace_mutation_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    operation_kind TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    ignored INTEGER NOT NULL CHECK (ignored IN (0, 1)),
    generated INTEGER NOT NULL CHECK (generated IN (0, 1)),
    before_hash TEXT,
    after_hash TEXT,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE workspace_mutation_ownership (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(runtime_instance_id),
    acquired_at TEXT NOT NULL
) STRICT;

CREATE TABLE workspace_operation_journals (
    journal_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(runtime_instance_id),
    operation_kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('preparing', 'applying', 'compensating', 'completed', 'compensated', 'reconciliation_required')
    ),
    trace_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE workspace_path_recent_use (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    used_at TEXT NOT NULL,
    use_count INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, relative_path)
) STRICT;

CREATE TABLE workspace_runtime_objects (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    object_kind TEXT NOT NULL,
    object_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('active', 'completed', 'cancelled')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, object_kind, object_id)
) STRICT;

CREATE TABLE workspace_tool_pins (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    tool_id TEXT NOT NULL,
    tool_version_json TEXT NOT NULL,
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, tool_id)
) STRICT;

CREATE TABLE workspaces (
    workspace_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    selected_root TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
, canonical_root TEXT NOT NULL DEFAULT '', root_volume_serial TEXT, root_file_id TEXT, revision INTEGER NOT NULL DEFAULT 1, catalog_freshness TEXT NOT NULL DEFAULT 'unavailable', catalog_revision INTEGER NOT NULL DEFAULT 0, authorization_epoch INTEGER NOT NULL DEFAULT 1, archived_at TEXT) STRICT;

CREATE INDEX approval_requests_pending
    ON approval_requests(created_at, approval_request_id)
    WHERE state='pending';

CREATE INDEX artifact_overwrite_journal_active
    ON artifact_overwrite_journal(workspace_id, updated_at, security_operation_id)
    WHERE state IN ('prepared', 'applying', 'published', 'needs_reconcile');

CREATE INDEX artifact_owners_identity
    ON artifact_owners(owner_kind, owner_type, owner_id, artifact_id);

CREATE INDEX artifacts_content_hash
    ON artifacts(content_hash_algorithm, content_hash_hex);

CREATE INDEX context_manual_compaction_pending
    ON context_manual_compaction_requests(session_id, agent_run_id, state, created_at);

CREATE INDEX context_projection_session_created
    ON context_projection_snapshots(session_id, created_at DESC, context_snapshot_id);

CREATE INDEX context_projection_turn
    ON context_projection_snapshots(turn_task_id, created_at, context_snapshot_id);

CREATE UNIQUE INDEX context_remediations_one_active_per_agent
    ON context_remediations(session_id, agent_run_id)
    WHERE state = 'awaiting';

CREATE INDEX context_search_chunks_cursor
    ON context_search_chunks(session_id, session_sequence, chunk_id);

CREATE INDEX credential_tombstones_parent
    ON credential_tombstones(parent_journal_id, credential_id);

CREATE UNIQUE INDEX events_session_sequence
    ON events(session_id, session_sequence)
    WHERE session_id IS NOT NULL;

CREATE INDEX events_type_sequence ON events(event_type, global_sequence);

CREATE INDEX events_workspace_sequence ON events(workspace_id, global_sequence);

CREATE INDEX external_grants_authorization_epoch
    ON external_grants(workspace_id, workspace_authorization_epoch, lifecycle)
    WHERE workspace_authorization_epoch IS NOT NULL;

CREATE INDEX external_grants_session_lifecycle
    ON external_grants(session_id, lifecycle, created_at);

CREATE INDEX external_grants_workspace_permission
    ON external_grants(workspace_id, permission_grant_id, lifecycle)
    WHERE permission_grant_id IS NOT NULL;

CREATE INDEX host_preparation_journal_active
    ON host_preparation_journal(updated_at, host_preparation_operation_id)
    WHERE state NOT IN ('completed', 'rolled_back', 'denied', 'failed');

CREATE UNIQUE INDEX model_availability_tests_one_active_profile
    ON model_availability_tests(model_profile_id)
    WHERE state IN ('queued', 'running');

CREATE INDEX model_availability_tests_profile
    ON model_availability_tests(model_profile_id, started_at DESC);

CREATE INDEX model_discovery_runs_endpoint
    ON model_discovery_runs(endpoint_id, started_at DESC);

CREATE INDEX model_execution_snapshots_step
    ON model_execution_snapshots(model_step_id, created_at);

CREATE INDEX model_failures_fingerprint
    ON model_failures(failure_fingerprint_json, created_at);

CREATE INDEX model_usage_step ON model_usage(model_step_id, recorded_at);

CREATE UNIQUE INDEX mutation_journals_idempotency
    ON mutation_journals(journal_kind, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX mutation_journals_kind_state
    ON mutation_journals(journal_kind, state, created_at);

CREATE INDEX mutation_journals_recovery
    ON mutation_journals(state, updated_at);

CREATE INDEX network_grants_active
    ON network_grants(identity_scope, identity_scope_id)
    WHERE revoked_at IS NULL;

CREATE INDEX permission_grants_active
    ON permission_grants(workspace_id, session_id, turn_task_id)
    WHERE revoked_at IS NULL AND consumed_at IS NULL;

CREATE INDEX permission_snapshots_session
    ON permission_snapshots(session_id, created_at DESC);

CREATE INDEX projection_outbox_pending
    ON projection_outbox(outbox_id)
    WHERE published_at IS NULL;

CREATE INDEX provider_catalog_icons_artifact
    ON provider_catalog_icons(artifact_id, content_hash_json);

CREATE INDEX provider_tool_call_protocol_bindings_snapshot
    ON provider_tool_call_protocol_bindings(model_execution_snapshot_id, tool_call_id);

CREATE UNIQUE INDEX provider_transport_attempt_one_terminal_event
    ON events(json_extract(payload_json, '$.transport_attempt_id'))
    WHERE event_type IN (
        'ProviderTransportAttemptCompleted',
        'ProviderTransportAttemptFailed',
        'ProviderTransportAttemptCancelled'
    );

CREATE INDEX quarantine_artifact_reference
    ON quarantine_records(artifact_id, state)
    WHERE artifact_id IS NOT NULL;

CREATE INDEX quarantine_release_journal_active
    ON quarantine_release_journal(updated_at, quarantine_record_id)
    WHERE state IN ('authorized', 'applying', 'published');

CREATE INDEX runtime_leases_runtime_state
    ON runtime_leases(runtime_instance_id, state);

CREATE INDEX session_permission_workspace
    ON session_permission_states(workspace_id, session_id);

CREATE INDEX sessions_workspace ON sessions(workspace_id, created_at);

CREATE INDEX tool_activity_cursor_session
    ON tool_activity_cursor_bindings(session_id, created_at);

CREATE INDEX tool_batch_payload_leases_live
    ON tool_batch_payload_leases(tool_batch_id, released_at)
    WHERE released_at IS NULL;

CREATE INDEX tool_batches_runtime_state
    ON tool_batches(status, updated_at, tool_batch_id)
    WHERE status NOT IN ('completed', 'recovery_required');

CREATE INDEX tool_batches_session_activity
    ON tool_batches(session_id, updated_at DESC, tool_batch_id);

CREATE INDEX tool_builtin_core_handler_dispatch_state
    ON tool_builtin_core_handler_dispatch_bindings(state, updated_at);

CREATE INDEX tool_calls_batch_status
    ON tool_calls(tool_batch_id, status, ordinal);

CREATE INDEX tool_calls_definition
    ON tool_calls(tool_id, tool_version_json, status);

CREATE INDEX tool_conformance_runs_draft
    ON tool_conformance_runs(tool_draft_id, draft_revision, status, tool_conformance_run_id);

CREATE INDEX tool_conformance_runs_recovery
    ON tool_conformance_runs(status, updated_at, tool_conformance_run_id)
    WHERE status IN ('queued', 'running');

CREATE INDEX tool_conformance_sandbox_dispatch_state
    ON tool_conformance_sandbox_dispatch_bindings(state, updated_at);

CREATE INDEX tool_definitions_canonical_name
    ON tool_definitions(canonical_name, retired_at);

CREATE UNIQUE INDEX tool_definitions_package_tool_version
    ON tool_definitions(tool_package_id, tool_key, tool_version_json)
    WHERE tool_package_id IS NOT NULL;

CREATE UNIQUE INDEX tool_draft_approval_consumption
    ON tool_draft_approvals(consumed_by_journal_id)
    WHERE consumed_by_journal_id IS NOT NULL;

CREATE INDEX tool_draft_snapshot_staging_expiry
    ON tool_draft_snapshot_staging(consumed_at, expires_at);

CREATE INDEX tool_draft_status
    ON tool_drafts(status, updated_at, tool_draft_id);

CREATE INDEX tool_ephemeral_loads_session
    ON tool_ephemeral_loads(session_id, turn_task_id, tool_id);

CREATE INDEX tool_execution_attempts_call
    ON tool_execution_attempts(tool_call_id, attempt_number);

CREATE INDEX tool_execution_attempts_runtime_state
    ON tool_execution_attempts(status, updated_at, tool_execution_attempt_id)
    WHERE status NOT IN ('succeeded', 'failed', 'cancelled', 'indeterminate');

CREATE INDEX tool_history_atomic_group_associations_event
    ON tool_history_atomic_group_associations(assistant_response_event_id, state);

CREATE INDEX tool_instances_exact_version
    ON tool_instances(tool_id, tool_version_json);

CREATE UNIQUE INDEX tool_long_operation_active_approval
    ON tool_long_operations(approval_reservation_key)
    WHERE state IN ('queued', 'running', 'cancelling');

CREATE INDEX tool_long_operation_state
    ON tool_long_operations(state, updated_at, journal_id);

CREATE UNIQUE INDEX tool_mutation_command_journals_correlation
    ON tool_mutation_command_journals(original_correlation_id)
    WHERE original_correlation_id IS NOT NULL;

CREATE INDEX tool_mutation_command_journals_state
    ON tool_mutation_command_journals(state, updated_at);

CREATE INDEX tool_package_payload_gc
    ON tool_package_payloads(state, deleted_at, tool_package_id, package_version_json);

CREATE INDEX tool_package_payload_references_live
    ON tool_package_payload_strong_references(tool_package_id, package_version_json)
    WHERE released_at IS NULL;

CREATE UNIQUE INDEX tool_packages_live_key
    ON tool_packages(package_key)
    WHERE lifecycle <> 'deleted';

CREATE UNIQUE INDEX tool_registry_snapshot_revision_hash
    ON tool_registry_snapshots(registry_revision, content_hash_json);

CREATE INDEX tool_results_batch ON tool_results(tool_batch_id, ordinal);

CREATE INDEX tool_sandbox_dispatch_bindings_attempt
    ON tool_sandbox_dispatch_bindings(tool_execution_attempt_id, state);

CREATE INDEX tool_sandbox_dispatch_bindings_pending
    ON tool_sandbox_dispatch_bindings(state, prepared_at)
    WHERE state = 'dispatch_prepared';

CREATE INDEX tool_search_cursor_snapshot
    ON tool_search_cursor_bindings(tool_registry_snapshot_id);

CREATE INDEX transport_attempts_endpoint_state
    ON transport_attempts(endpoint_id, state, started_at);

CREATE INDEX transport_attempts_operation
    ON transport_attempts(operation_context_json, started_at);

CREATE INDEX workspace_catalog_name
    ON workspace_catalog_entries(workspace_id, file_name);

CREATE INDEX workspace_catalog_parent
    ON workspace_catalog_entries(workspace_id, parent_path, file_name);

CREATE INDEX workspace_deferred_pending
    ON workspace_deferred_mutations(workspace_id, state, created_at);

CREATE INDEX workspace_journals_incomplete
    ON workspace_operation_journals(workspace_id, state)
    WHERE state NOT IN ('completed', 'compensated');

CREATE UNIQUE INDEX workspace_lifecycle_one_active
    ON workspace_lifecycle_operations(workspace_id)
    WHERE state NOT IN ('completed', 'cancelled', 'failed');

CREATE INDEX workspace_lifecycle_recent
    ON workspace_lifecycle_operations(workspace_id, updated_at DESC);

CREATE UNIQUE INDEX workspaces_canonical_root_unique
    ON workspaces(canonical_root COLLATE NOCASE);

CREATE UNIQUE INDEX workspaces_root_identity_unique
    ON workspaces(root_volume_serial, root_file_id)
    WHERE root_volume_serial IS NOT NULL AND root_file_id IS NOT NULL;

CREATE TRIGGER artifact_owners_immutable
BEFORE UPDATE ON artifact_owners
BEGIN
    SELECT RAISE(ABORT, 'artifact direct owner is immutable');
END;

CREATE TRIGGER artifact_owners_scope_guard
BEFORE INSERT ON artifact_owners
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1
        FROM artifacts AS artifact
        WHERE artifact.artifact_id = NEW.artifact_id
          AND (
              (
                  NEW.owner_kind = 'session'
                  AND artifact.scope IN (
                      'session', 'runtime_ephemeral', 'workspace_cache', 'quarantine'
                  )
              )
              OR (
                  NEW.owner_kind = 'operation'
                  AND artifact.scope IN ('system_cache', 'runtime_ephemeral')
              )
              OR (
                  NEW.owner_kind = 'system'
                  AND artifact.scope = 'system_cache'
              )
              OR (
                  NEW.owner_kind = 'tool'
                  AND (
                      (
                          NEW.owner_type IN ('package_version', 'registry_snapshot')
                          AND artifact.scope = 'package_store'
                      )
                      OR (
                          NEW.owner_type = 'draft'
                          AND artifact.scope IN (
                              'quarantine', 'tool_execution', 'runtime_ephemeral'
                          )
                      )
                      OR (
                          NEW.owner_type = 'conformance_run'
                          AND artifact.scope IN ('quarantine', 'tool_execution')
                      )
                      OR (
                          NEW.owner_type IN ('batch', 'tool_call', 'execution_attempt')
                          AND artifact.scope IN ('tool_execution', 'runtime_ephemeral')
                      )
                  )
              )
          )
    ) THEN RAISE(ABORT, 'artifact owner and scope are incompatible') END;

    SELECT CASE WHEN
        (NEW.owner_kind = 'session' AND NOT EXISTS (
            SELECT 1 FROM sessions WHERE session_id = NEW.owner_id
        ))
        OR (NEW.owner_type = 'model_discovery' AND NOT EXISTS (
            SELECT 1 FROM model_discovery_runs WHERE discovery_run_id = NEW.owner_id
        ))
        OR (NEW.owner_type = 'model_availability_test' AND NOT EXISTS (
            SELECT 1 FROM model_availability_tests
            WHERE model_availability_test_id = NEW.owner_id
        ))
        OR (NEW.owner_type = 'batch' AND NOT EXISTS (
            SELECT 1 FROM tool_batches
            WHERE tool_batch_id = json_extract(NEW.owner_id, '$.tool_batch_id')
        ))
        OR (NEW.owner_type = 'tool_call' AND NOT EXISTS (
            SELECT 1 FROM tool_calls
            WHERE tool_call_id = json_extract(NEW.owner_id, '$.tool_call_id')
        ))
        OR (NEW.owner_type = 'execution_attempt' AND NOT EXISTS (
            SELECT 1 FROM tool_execution_attempts
            WHERE tool_execution_attempt_id =
                json_extract(NEW.owner_id, '$.tool_execution_attempt_id')
        ))
        OR (NEW.owner_type = 'registry_snapshot' AND NOT EXISTS (
            SELECT 1 FROM tool_registry_snapshots
            WHERE tool_registry_snapshot_id =
                json_extract(NEW.owner_id, '$.tool_registry_snapshot_id')
        ))
        OR (NEW.owner_type = 'package_version' AND NOT EXISTS (
            SELECT 1
            FROM tool_package_versions AS version
            WHERE version.tool_package_id =
                    json_extract(NEW.owner_id, '$.tool_package_id')
              AND json_extract(version.package_version_json, '$') =
                    json_extract(NEW.owner_id, '$.package_version')
              AND json_extract(version.package_fingerprint_json, '$.algorithm') =
                    json_extract(NEW.owner_id, '$.package_fingerprint.algorithm')
              AND json_extract(
                    version.package_fingerprint_json, '$.canonicalization_version'
                  ) = json_extract(
                    NEW.owner_id, '$.package_fingerprint.canonicalization_version'
                  )
              AND json_extract(version.package_fingerprint_json, '$.hex') =
                    json_extract(NEW.owner_id, '$.package_fingerprint.hex')
        ))
    THEN RAISE(ABORT, 'artifact direct owner does not exist') END;
END;

CREATE TRIGGER artifact_tool_workflow_owner_guard
BEFORE INSERT ON artifact_owners
WHEN NEW.owner_kind = 'tool' AND NEW.owner_type IN ('draft', 'conformance_run')
BEGIN
    SELECT CASE WHEN
        (NEW.owner_type = 'draft' AND NOT EXISTS (
            SELECT 1 FROM tool_drafts
            WHERE tool_draft_id = json_extract(NEW.owner_id, '$.tool_draft_id')
        ))
        OR (NEW.owner_type = 'conformance_run' AND NOT EXISTS (
            SELECT 1 FROM tool_conformance_runs
            WHERE tool_conformance_run_id =
                json_extract(NEW.owner_id, '$.tool_conformance_run_id')
        ))
    THEN RAISE(ABORT, 'tool workflow artifact owner does not exist') END;
END;

CREATE TRIGGER payload_reference_deletion_pending_guard
BEFORE INSERT ON tool_package_payload_strong_references
WHEN EXISTS (
    SELECT 1 FROM tool_packages p
    WHERE p.tool_package_id = NEW.tool_package_id AND p.deletion_pending = 1
)
BEGIN
    SELECT RAISE(ABORT, 'tool package deletion is pending');
END;

CREATE TRIGGER permission_snapshots_authorization_root_insert
AFTER INSERT ON permission_snapshots
BEGIN
    INSERT INTO authorization_snapshots(
        authorization_snapshot_id, authorization_kind, created_at
    ) VALUES (NEW.permission_snapshot_id, 'session_permission', NEW.created_at);
END;

CREATE TRIGGER session_pin_deletion_pending_guard
BEFORE INSERT ON session_tool_pins
WHEN EXISTS (
    SELECT 1
    FROM tool_definitions d
    JOIN tool_packages p ON p.tool_package_id = d.tool_package_id
    WHERE d.tool_id = NEW.tool_id AND p.deletion_pending = 1
)
BEGIN
    SELECT RAISE(ABORT, 'tool package deletion is pending');
END;

CREATE TRIGGER tool_draft_kind_immutable
BEFORE UPDATE OF draft_kind ON tool_drafts
WHEN NEW.draft_kind <> OLD.draft_kind
BEGIN
    SELECT RAISE(ABORT, 'tool draft kind is immutable');
END;

CREATE TRIGGER tool_instance_deletion_pending_guard
BEFORE INSERT ON tool_instances
WHEN EXISTS (
    SELECT 1
    FROM tool_definitions d
    JOIN tool_packages p ON p.tool_package_id = d.tool_package_id
    WHERE d.tool_id = NEW.tool_id
      AND d.tool_version_json = NEW.tool_version_json
      AND p.deletion_pending = 1
)
BEGIN
    SELECT RAISE(ABORT, 'tool package deletion is pending');
END;

CREATE TRIGGER workspace_pin_deletion_pending_guard
BEFORE INSERT ON workspace_tool_pins
WHEN EXISTS (
    SELECT 1
    FROM tool_definitions d
    JOIN tool_packages p ON p.tool_package_id = d.tool_package_id
    WHERE d.tool_id = NEW.tool_id AND p.deletion_pending = 1
)
BEGIN
    SELECT RAISE(ABORT, 'tool package deletion is pending');
END;

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(1,'p02_initial','1806e1cb859ab72f18ba8a858c4eaf1965f3c005cdce6eeeb14fbda9790c2b0f','2026-10-06T10:16:48.2905033Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(2,'p03_workspace_files','7b2804b38bcc27dbf92f6a6be316b6f2276847f20ea6c12b7d6bdba23cc50d7a','2026-10-06T10:16:48.2952062Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(3,'p04_security_control_plane','ff705db1910e2f3bf7008d145896205c61bcb0e4db9a529fce56495f9946ed77','2026-10-06T10:16:48.2964372Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(4,'p04_security_revision','fa49d802d68c91b1c5be74659d201d05a9cb8387587988ca9382f277c8e8f422','2026-10-06T10:16:48.3013642Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(5,'p04_host_preparation_approval','e5303c7cc7929b4636919aa774032ab96000d6dc6731bac0ca7b703f4fb956b1','2026-10-06T10:16:48.3020567Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(6,'p04_quarantine_release_journal','95da3eb5073fd168a9874513779c8d86e3052ec4360d2fc7d6a963bd3e2b3cf5','2026-10-06T10:16:48.3021949Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(7,'p04_artifact_overwrite_journal','baf42511a3d083561bb8b760d1b80b624055469ead00e9f46140f5d6ceac984a','2026-10-06T10:16:48.3030673Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(8,'p04_release_descriptor_namespace','3fb2912ae278a9a1efb2e5a8d34c957ddfd741f2ae861fe6ab65eb4c69b2423f','2026-10-06T10:16:48.3031459Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(9,'p05_model_runtime','69b21cf8805483dd98ac3c0c894d26d4c03e2dfd4c34de87abe8b190553d9be5','2026-10-06T10:16:48.304444Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(10,'p02_mutation_journal_p05_credential','b08c95bd1751d83dbdeb1d1f80454626b9618d80b98724b4a18d6665fb0b29c0','2026-10-06T10:16:48.3048481Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(11,'p02_artifact_direct_owner','9368af758814367e02a03767825c2617edf3ac258b9986f87906e83bb6013a37','2026-10-06T10:16:48.308228Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(12,'p05_credential_delete','5b6fb5cb124b82f51cb02f91fd5386bc8b3dc5a477293a3d42df9b244b59934c','2026-10-06T10:16:48.311627Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(13,'p05_provider_catalog_icon','3b187138ccb21129e255c426370ecb0b97f33d42e9ea91766d33130a08dab066','2026-10-06T10:16:48.3117565Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(14,'p05_provider_catalog_cache_v2','1227af69e037c1c3be324ecc0162c2ea08b2e2ade4d1cdeb64e09869fc1708dc','2026-10-06T10:16:48.3119793Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(15,'p05.legacy_configuration_transport_indeterminate.v1','dc549db20723822d6f89872642d9b274d865c76d5cd65ef8ec966474be42f551','2026-10-06T10:16:48.3119876Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(16,'p06_prompt_context_projection_compaction','8f5c45ad5700b306e39300ebbde8d4d3b4be3118efb059b045d8bd26f6d7c1e2','2026-10-06T10:16:48.3141666Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(17,'p06_context_remediation','64ed5a97789bf032b21c68e393aecc07ddf6d8de1ac84c5c398efe87629e73c1','2026-10-06T10:16:48.3143373Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(18,'p07_tool_registry_foundation','5f9fb763740aac751ef3cf604b3bdd6ea967f73719a75eb855f0601b398b68a7','2026-10-06T10:16:48.3145804Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(19,'p07_context_tool_registry_snapshot_v2','c1d5639dd5ee503c1779f8fee11c655ad618a5bef6839922e4d2a6978cb3d14c','2026-10-06T10:16:48.3176339Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(20,'p07_tool_authority_entities','61b48ddae4e21f31cec9d94c143af9f404ffd3805d27f82bc8c0a4767fcfa1f6','2026-10-06T10:16:48.3185588Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(21,'p07_tool_execution_authority','7e64416a433a7ed3ebe730e9f0d3c03e48c78b8353159cbfabcbdf080807db10','2026-10-06T10:16:48.3196086Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(22,'p07_tool_call_definition_reference_v1','6591ad92c8dd94b3432148e5526d79e0cf3c74ee5ce6715b178fa205375c75c3','2026-10-06T10:16:48.3196569Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(23,'p07_tool_artifact_direct_owner','6ee7a8e876bd9907500b8bb4e14f176ccee7e102dd6efedcc543bb1e515d507a','2026-10-06T10:16:48.3254985Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(24,'p07_tool_conformance_runs','339e321849f03e9922b7575cc7e66c7c7013d1c6128c8eedb174d14218647e11','2026-10-06T10:16:48.3257951Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(25,'p07_tool_draft_workflow','7a46c81cdadd4f39bd9c12e2b850717c798be8bdf7ce6e3ddde5d0faefcaafa1','2026-10-06T10:16:48.3265214Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(26,'p07_tool_package_publish','3465f45cb99428792823a2777226ffade68772956d195ae15dcc92f7845d6d45','2026-10-06T10:16:48.3318835Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(27,'p07_tool_trust_eligible_registry','864f7bbcca1db42eedae354b37319b1296d4be6900f500fbb88c04ec7a90bd3a','2026-10-06T10:16:48.3344499Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(28,'p07_tool_runtime_dispatch','9a009c7d7ab3c6cee7982cfc88da5deea4f1d9a59181891e95289da0f0057ea5','2026-10-06T10:16:48.3349672Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(29,'p07_tool_sandbox_authorized_dispatch','ef21d849d09f4220fccd973115b48ab5f8b710d9e126fa1582211ce1cc8b2b8d','2026-10-06T10:16:48.3352228Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(30,'p07_tool_result_artifact_access','af2bdb3de030a2eed7d4d27b4ffeec122e4a5c77428b11c4281391a8e51a4ebb','2026-10-06T10:16:48.3376194Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(31,'p07_tool_result_unknown_and_history_group','d766f0818529492273c10fa8aa3c5f027242d66ef6a93db8eb83cbb2e8dd5c8c','2026-10-06T10:16:48.3757592Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(32,'p07_tool_activity_cursor','2b9e94b37caa46bd684653915ad5a7fc9a4f2f5f7468937c1a7799812eaaea5c','2026-10-06T10:16:48.3759336Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(33,'p07_signed_builtin_release','63d1298468aaa474d58a4e347c33f9a0b6d635c9619bbdb155637bf239764a17','2026-10-06T10:16:48.3761161Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(34,'p07_builtin_core_handler_dispatch','4579fae6fb382d1c4de2b36772653187cf10bb8b454976d6ab539b72a4dfbe67','2026-10-06T10:16:48.3763429Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(35,'p07_tool_mutation_command_journal','5db59bc2a8c6d57b0111e007641e74e48d424e2352d40388e7b50bc13258a75d','2026-10-06T10:16:48.376576Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(36,'p07_tool_mutation_command_correlation','e35ade279dd92f71013165de61087f4a271fd5591d3ca8f5f82a5b34c3db5259','2026-10-06T10:16:48.3792444Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(37,'p07_tool_conformance_sandbox_dispatch','ed903c3e7ba7fca6437d1abd0e51e45abce725bfbd1e19f9d96e7dac266ac180','2026-10-06T10:16:48.3795931Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(38,'p07_sandbox_authorization_root','9df9d9df31af4a38e279ce772437534fa1851da0409141c550295b6ca9b975a8','2026-10-06T10:16:48.3961935Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(39,'p07_tool_long_operation','e1b8eeabdde202a2e299fb061736c2b82cc91ba7d536689c7f51538bd402b17f','2026-10-06T10:16:48.3993664Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(40,'p07_tool_credential_authority','a96953ab4175faaed90437c48993c3dfb404ecb45c63bb000be1c4ca676cc566','2026-10-06T10:16:48.4000332Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(41,'p07_tool_draft_snapshot_staging','f78e375593756cfc266e0a9f091afd356e3f7146cea0e47d8248f8ada8d9a2f0','2026-10-06T10:16:48.4003568Z');

INSERT INTO schema_migrations(version,name,checksum,applied_at) VALUES(42,'p07_tool_draft_runtime_ephemeral_artifact','96e5676cfe86d66f491fb00b30ee549b9e8aacc318c0721b63a3f5a38812b23f','2026-10-06T10:16:48.4007095Z');
