use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aura_domain_common::ToolPackageId;
use aura_domain_common::jcs::{
    CanonicalSha256HashV1, canonical_json_jcs, canonical_sha256, parse_json_strict,
};
use aura_domain_common::{MutationJournalId, UtcTimestamp};
use aura_tool_domain::{
    ConformanceDraftRevisionRefV1, ToolLongOperationRefV1, ToolLongOperationStateV1,
};
use rusqlite::{Connection, OptionalExtension, Transaction, backup::Backup, params};

use crate::{
    MigrationRecoveryRequired, PersistenceError,
    legacy_configuration_transport::{
        LEGACY_CONFIGURATION_TRANSPORT_LEDGER_SCHEMA,
        migrate_legacy_configuration_transport_indeterminate_v1,
    },
};

pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const LATEST_SCHEMA_VERSION: i64 = 50;
mod final_migrations;

/// Rebuilds `workspace_mutation_ownership` so one **execution** owns one row.
///
/// The old shape had `workspace_id` as the primary key, which made the table a workspace-wide
/// exclusive lock held by whichever session wrote first: a second session writing an *unrelated*
/// file in the same workspace was refused with `MutationOwnedByOtherSession`, and one session's two
/// independent executions were one row that a single release ended. Neither is file coordination —
/// that is the resource owner, keyed by real identity — and neither is what the table is for.
///
/// The rebuild keeps the table, the workspace lifecycle check that reads it and the rows themselves
/// (so a database mid-transition is still represented), and changes only the key:
/// `(workspace_id, session_id, execution_id)`. Existing rows become the `legacy` execution, which
/// is exactly what a release through the pre-change API still addresses, so an in-flight release
/// after an upgrade still finds its row.
///
/// The table is recreated rather than altered because SQLite cannot change a primary key in place.
const WORKSPACE_MUTATION_OWNERSHIP_EXECUTION_SCHEMA: &str = r"
CREATE TABLE workspace_mutation_ownership_execution (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(runtime_instance_id),
    execution_id TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, session_id, execution_id)
) STRICT;

INSERT INTO workspace_mutation_ownership_execution(
    workspace_id, session_id, runtime_instance_id, execution_id, acquired_at
)
SELECT workspace_id, session_id, runtime_instance_id, 'legacy', acquired_at
FROM workspace_mutation_ownership;

DROP TABLE workspace_mutation_ownership;

ALTER TABLE workspace_mutation_ownership_execution RENAME TO workspace_mutation_ownership;
";

/// Which catalog instance currently owns the durable index of one workspace.
///
/// The workspace index is derived state, and more than one instance of it can exist for
/// one workspace: a closed and reopened workspace builds a second `PathCatalog` over the
/// same durable rows, and the first one's in-flight task can still be alive. The
/// revision alone cannot separate the two — an open that writes nothing leaves the
/// revision exactly where the dead instance planned against — so a commit needs an
/// identity as well as a revision to check inside its own transaction.
///
/// `instance_seq` is a monotonic generation, claimed once per open. A commit presents
/// the pair `(catalog_revision, instance_seq)` it planned against and is refused unless
/// both still match, which is what makes "the instance that was replaced cannot write"
/// a fact the database enforces rather than a convention the process hopes for.
const WORKSPACE_CATALOG_INSTANCE_SCHEMA: &str = r#"
CREATE TABLE workspace_catalog_instances (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    instance_seq INTEGER NOT NULL,
    runtime_instance_id TEXT NOT NULL,
    claimed_at TEXT NOT NULL
) STRICT;
"#;

/// The volume's change generation for one indexed file, read through a handle
/// that excluded writers while the text was read, together with the object
/// identity it was read from.
///
/// It is what lets a later open reuse the text it already has instead of reading
/// every file again: the entry is believed only while the object identity, the
/// size *and* this value are unchanged. Rows written before these columns existed
/// are NULL — no proof — so their files are read again once and get a value then.
/// Nothing here is derived from the file's modification time alone: the volume's
/// own counter changes even when a rewrite keeps both the size and the time.
const WORKSPACE_CATALOG_CHANGE_GENERATION_SCHEMA: &str = r#"
ALTER TABLE workspace_catalog_entries ADD COLUMN change_usn INTEGER;
ALTER TABLE workspace_catalog_entries ADD COLUMN volume_serial TEXT;
ALTER TABLE workspace_catalog_entries ADD COLUMN file_id TEXT;
"#;

/// Indexable `(workspace, path) → FTS rowid` map for the workspace text index.
///
/// `workspace_content_fts` declares `workspace_id` and `relative_path` as FTS5
/// `UNINDEXED` columns, so `DELETE ... WHERE workspace_id=?1 AND relative_path=?2`
/// has no index to use: the real plan is `SCAN workspace_content_fts VIRTUAL TABLE
/// INDEX 0:`, and one incremental batch that touches N paths scans the whole index N
/// times. This table gives every indexed path exactly one rowid, so the same delete
/// becomes a primary-key lookup. It is derived state: the maps can always be rebuilt
/// from `workspace_catalog_entries` plus the text each entry stores in FTS.
///
/// `needs_reindex` marks a path whose text could not be adopted during the migration —
/// because several rows existed for it and the text index carries no digest that could
/// say which was current. The text rows are dropped for such a path and `PathCatalog`
/// re-reads it from the file, so a repair is a re-derivation rather than a guess.
const WORKSPACE_CONTENT_INDEX_SCHEMA: &str = r#"
CREATE TABLE workspace_content_index (
    workspace_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    fts_rowid INTEGER UNIQUE,
    content_hash TEXT,
    needs_reindex INTEGER NOT NULL DEFAULT 0 CHECK (needs_reindex IN (0, 1)),
    PRIMARY KEY (workspace_id, relative_path)
) STRICT;
"#;

/// Term, optional clock, and turn binding for a reusable directory grant.
/// Existing rows stay session-scoped with no clock and no turn, which is the
/// scope they already had.
const DIRECTORY_GRANT_TERM_SCHEMA: &str = r#"
ALTER TABLE external_grants ADD COLUMN expires_at TEXT;
ALTER TABLE external_grants ADD COLUMN turn_task_id TEXT;
ALTER TABLE external_grants ADD COLUMN scope_lifetime TEXT NOT NULL DEFAULT 'session';
"#;

/// One user approval decision bound to the directory grant it minted or reused.
/// Replaying that decision returns the same row, including after a later revocation.
const DIRECTORY_GRANT_DECISION_SCHEMA: &str = r#"
CREATE TABLE external_grant_decisions (
    approval_id TEXT NOT NULL,
    root_identity_json TEXT NOT NULL,
    external_grant_id TEXT NOT NULL,
    operation_fingerprint TEXT NOT NULL,
    session_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (approval_id, root_identity_json)
) STRICT;
CREATE INDEX external_grant_decisions_operation
    ON external_grant_decisions(operation_fingerprint);
"#;

const INITIAL_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    checksum TEXT NOT NULL,
    applied_at TEXT NOT NULL
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

CREATE INDEX runtime_leases_runtime_state
    ON runtime_leases(runtime_instance_id, state);

CREATE TABLE workspaces (
    workspace_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    root_path TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX sessions_workspace ON sessions(workspace_id, created_at);

CREATE TABLE turn_tasks (
    turn_task_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE agent_runs (
    agent_run_id TEXT PRIMARY KEY,
    turn_task_id TEXT NOT NULL REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    parent_agent_run_id TEXT REFERENCES agent_runs(agent_run_id),
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

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

CREATE UNIQUE INDEX events_session_sequence
    ON events(session_id, session_sequence)
    WHERE session_id IS NOT NULL;
CREATE INDEX events_workspace_sequence ON events(workspace_id, global_sequence);
CREATE INDEX events_type_sequence ON events(event_type, global_sequence);

CREATE VIRTUAL TABLE event_search_fts USING fts5(
    event_id UNINDEXED,
    session_id UNINDEXED,
    searchable_text,
    tokenize = 'unicode61'
);

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

CREATE INDEX artifacts_content_hash
    ON artifacts(content_hash_algorithm, content_hash_hex);

CREATE TABLE artifact_object_refs (
    object_hash TEXT PRIMARY KEY,
    relative_path TEXT NOT NULL UNIQUE,
    stored_length INTEGER NOT NULL,
    ref_count INTEGER NOT NULL CHECK (ref_count >= 0)
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

CREATE TABLE artifact_links (
    artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    access_flags INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (artifact_id, session_id)
) STRICT;

CREATE TABLE artifact_gc_queue (
    artifact_id TEXT PRIMARY KEY REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    enqueued_at TEXT NOT NULL,
    reason TEXT NOT NULL
) STRICT;

CREATE TABLE projection_revisions (
    projection_kind TEXT NOT NULL,
    projection_key TEXT NOT NULL,
    revision INTEGER NOT NULL,
    PRIMARY KEY (projection_kind, projection_key)
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

CREATE INDEX projection_outbox_pending
    ON projection_outbox(outbox_id)
    WHERE published_at IS NULL;
"#;

const P03_WORKSPACE_SCHEMA: &str = r#"
ALTER TABLE workspaces RENAME COLUMN root_path TO selected_root;
ALTER TABLE workspaces ADD COLUMN canonical_root TEXT NOT NULL DEFAULT '';
ALTER TABLE workspaces ADD COLUMN root_volume_serial TEXT;
ALTER TABLE workspaces ADD COLUMN root_file_id TEXT;
ALTER TABLE workspaces ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE workspaces ADD COLUMN catalog_freshness TEXT NOT NULL DEFAULT 'unavailable';
ALTER TABLE workspaces ADD COLUMN catalog_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE workspaces ADD COLUMN authorization_epoch INTEGER NOT NULL DEFAULT 1;
ALTER TABLE workspaces ADD COLUMN archived_at TEXT;

UPDATE workspaces SET canonical_root=selected_root WHERE canonical_root='';
UPDATE workspaces SET state='online' WHERE state='active';

CREATE UNIQUE INDEX workspaces_canonical_root_unique
    ON workspaces(canonical_root COLLATE NOCASE);
CREATE UNIQUE INDEX workspaces_root_identity_unique
    ON workspaces(root_volume_serial, root_file_id)
    WHERE root_volume_serial IS NOT NULL AND root_file_id IS NOT NULL;

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

CREATE UNIQUE INDEX workspace_lifecycle_one_active
    ON workspace_lifecycle_operations(workspace_id)
    WHERE state NOT IN ('completed', 'cancelled', 'failed');

CREATE INDEX workspace_lifecycle_recent
    ON workspace_lifecycle_operations(workspace_id, updated_at DESC);

CREATE TABLE workspace_runtime_objects (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    object_kind TEXT NOT NULL,
    object_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('active', 'completed', 'cancelled')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (workspace_id, object_kind, object_id)
) STRICT;

CREATE TABLE workspace_mutation_ownership (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(runtime_instance_id),
    acquired_at TEXT NOT NULL
) STRICT;

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

CREATE INDEX workspace_deferred_pending
    ON workspace_deferred_mutations(workspace_id, state, created_at);

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

CREATE INDEX workspace_journals_incomplete
    ON workspace_operation_journals(workspace_id, state)
    WHERE state NOT IN ('completed', 'compensated');

CREATE TABLE file_references (
    file_reference_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE external_grants (
    external_grant_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    canonical_root TEXT NOT NULL,
    created_at TEXT NOT NULL,
    revoked_at TEXT
) STRICT;

CREATE TABLE workspace_catalog_partitions (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    partition_path TEXT NOT NULL,
    file_count INTEGER NOT NULL,
    indexable_text_bytes INTEGER NOT NULL,
    index_mode TEXT NOT NULL CHECK (index_mode IN ('full_text', 'metadata_only')),
    PRIMARY KEY (workspace_id, partition_path)
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

CREATE INDEX workspace_catalog_parent
    ON workspace_catalog_entries(workspace_id, parent_path, file_name);
CREATE INDEX workspace_catalog_name
    ON workspace_catalog_entries(workspace_id, file_name);

CREATE VIRTUAL TABLE workspace_content_fts USING fts5(
    workspace_id UNINDEXED,
    relative_path UNINDEXED,
    content,
    tokenize = 'unicode61'
);

CREATE TABLE workspace_path_recent_use (
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    used_at TEXT NOT NULL,
    use_count INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, relative_path)
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

CREATE TABLE cleanup_notifications (
    notification_id TEXT PRIMARY KEY,
    notification_kind TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT
) STRICT;
"#;

const P04_SECURITY_SCHEMA: &str = r#"
CREATE TABLE session_permission_states (
    session_id TEXT PRIMARY KEY REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    mode TEXT NOT NULL CHECK (mode IN ('read_only', 'workspace_read_write', 'full_access')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX session_permission_workspace
    ON session_permission_states(workspace_id, session_id);

CREATE TABLE permission_snapshots (
    permission_snapshot_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    mode TEXT NOT NULL,
    session_permission_revision INTEGER NOT NULL,
    active_grant_ids_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX permission_snapshots_session
    ON permission_snapshots(session_id, created_at DESC);

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

CREATE INDEX permission_grants_active
    ON permission_grants(workspace_id, session_id, turn_task_id)
    WHERE revoked_at IS NULL AND consumed_at IS NULL;

CREATE TABLE turn_deny_records (
    turn_task_id TEXT NOT NULL REFERENCES turn_tasks(turn_task_id) ON DELETE CASCADE,
    source_key TEXT NOT NULL,
    source_json TEXT NOT NULL,
    operation_fingerprint_json TEXT NOT NULL,
    denied_at TEXT NOT NULL,
    PRIMARY KEY (turn_task_id, source_key, operation_fingerprint_json)
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

CREATE INDEX approval_requests_pending
    ON approval_requests(created_at, approval_request_id)
    WHERE state='pending';

CREATE TABLE approval_request_operations (
    approval_request_id TEXT NOT NULL REFERENCES approval_requests(approval_request_id) ON DELETE CASCADE,
    operation_index INTEGER NOT NULL,
    security_operation_id TEXT NOT NULL,
    operation_fingerprint_json TEXT NOT NULL,
    PRIMARY KEY (approval_request_id, operation_index)
) STRICT;

CREATE TABLE approval_decisions (
    approval_decision_id TEXT PRIMARY KEY,
    approval_request_id TEXT NOT NULL UNIQUE REFERENCES approval_requests(approval_request_id) ON DELETE CASCADE,
    decision_kind TEXT NOT NULL,
    decided_at TEXT NOT NULL,
    permission_grant_ids_json TEXT NOT NULL
) STRICT;

CREATE TABLE external_trust_records (
    trust_record_id TEXT PRIMARY KEY,
    record_json TEXT NOT NULL,
    source_key TEXT NOT NULL UNIQUE,
    source_json TEXT NOT NULL,
    trust_state_json TEXT NOT NULL,
    effective_fingerprint_json TEXT,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE sandbox_executions (
    sandbox_execution_id TEXT PRIMARY KEY,
    identity_json TEXT NOT NULL,
    permission_snapshot_id TEXT NOT NULL REFERENCES permission_snapshots(permission_snapshot_id),
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

CREATE TABLE sandbox_acl_journal (
    sandbox_acl_journal_id TEXT PRIMARY KEY,
    sandbox_execution_id TEXT NOT NULL REFERENCES sandbox_executions(sandbox_execution_id) ON DELETE CASCADE,
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

CREATE INDEX sandbox_acl_cleanup
    ON sandbox_acl_journal(state, created_at)
    WHERE state NOT IN ('cleaned');

CREATE TABLE host_preparation_states (
    state_key TEXT PRIMARY KEY,
    summary_json TEXT NOT NULL,
    ownership_json TEXT NOT NULL,
    revision INTEGER NOT NULL,
    updated_at TEXT NOT NULL
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
) STRICT;

CREATE INDEX host_preparation_journal_active
    ON host_preparation_journal(updated_at, host_preparation_operation_id)
    WHERE state NOT IN ('completed', 'rolled_back', 'denied', 'failed');

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

CREATE INDEX network_grants_active
    ON network_grants(identity_scope, identity_scope_id)
    WHERE revoked_at IS NULL;

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

CREATE TABLE network_rate_limit_state (
    origin TEXT PRIMARY KEY,
    active_connections INTEGER NOT NULL,
    window_started_at TEXT NOT NULL,
    request_count INTEGER NOT NULL,
    upload_bytes INTEGER NOT NULL,
    download_bytes INTEGER NOT NULL,
    updated_at TEXT NOT NULL
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
) STRICT;
"#;

const P04_SECURITY_REVISION_SCHEMA: &str = r#"
DROP INDEX host_preparation_journal_active;

ALTER TABLE host_preparation_journal
    RENAME TO host_preparation_journal_p04_legacy;

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
) STRICT;

INSERT INTO host_preparation_journal(
    host_preparation_operation_id, action, state, operation_json,
    broker_plan_json, plan_hash_json, created_at, updated_at, error_code
)
SELECT
    host_preparation_operation_id, action, state, operation_json,
    broker_plan_json, plan_hash_json, created_at, updated_at, error_code
FROM host_preparation_journal_p04_legacy;

DROP TABLE host_preparation_journal_p04_legacy;

CREATE INDEX host_preparation_journal_active
    ON host_preparation_journal(updated_at, host_preparation_operation_id)
    WHERE state NOT IN ('completed', 'rolled_back', 'denied', 'failed');

ALTER TABLE quarantine_records
    ADD COLUMN artifact_id TEXT REFERENCES artifacts(artifact_id) ON DELETE SET NULL;

CREATE INDEX quarantine_artifact_reference
    ON quarantine_records(artifact_id, state)
    WHERE artifact_id IS NOT NULL;

ALTER TABLE external_grants RENAME TO external_grants_p03_legacy;

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
) STRICT;

INSERT INTO external_grants(
    external_grant_id, session_id, workspace_id, permission_grant_id,
    selected_root, canonical_root, root_identity_json, root_is_directory,
    capabilities_json, recursive_scope, lifecycle, revalidation_revision,
    created_at, updated_at, suspended_at, revoked_at
)
SELECT
    legacy.external_grant_id,
    legacy.session_id,
    sessions.workspace_id,
    NULL,
    legacy.canonical_root,
    legacy.canonical_root,
    '{"volume_serial_number":"","file_id":""}',
    0,
    '[]',
    0,
    'revoked',
    1,
    legacy.created_at,
    COALESCE(legacy.revoked_at, legacy.created_at),
    NULL,
    COALESCE(legacy.revoked_at, legacy.created_at)
FROM external_grants_p03_legacy AS legacy
JOIN sessions ON sessions.session_id=legacy.session_id;

DROP TABLE external_grants_p03_legacy;

CREATE INDEX external_grants_session_lifecycle
    ON external_grants(session_id, lifecycle, created_at);

CREATE INDEX external_grants_workspace_permission
    ON external_grants(workspace_id, permission_grant_id, lifecycle)
    WHERE permission_grant_id IS NOT NULL;
"#;

const P04_HOST_PREPARATION_APPROVAL_SCHEMA: &str = r#"
ALTER TABLE host_preparation_journal
    ADD COLUMN approval_json TEXT;
"#;

const P04_QUARANTINE_RELEASE_JOURNAL_SCHEMA: &str = r#"
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

CREATE INDEX quarantine_release_journal_active
    ON quarantine_release_journal(updated_at, quarantine_record_id)
    WHERE state IN ('authorized', 'applying', 'published');
"#;

const P04_ARTIFACT_OVERWRITE_JOURNAL_SCHEMA: &str = r#"
ALTER TABLE external_grants
    ADD COLUMN workspace_authorization_epoch INTEGER;

UPDATE external_grants
SET workspace_authorization_epoch = (
    SELECT authorization_epoch FROM workspaces
    WHERE workspaces.workspace_id = external_grants.workspace_id
)
WHERE permission_grant_id IS NOT NULL;

CREATE INDEX external_grants_authorization_epoch
    ON external_grants(workspace_id, workspace_authorization_epoch, lifecycle)
    WHERE workspace_authorization_epoch IS NOT NULL;

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

CREATE INDEX artifact_overwrite_journal_active
    ON artifact_overwrite_journal(workspace_id, updated_at, security_operation_id)
    WHERE state IN ('prepared', 'applying', 'published', 'needs_reconcile');
"#;

const P04_RELEASE_DESCRIPTOR_SCHEMA: &str = r#"
UPDATE approval_requests
SET state='invalidated',
    resolved_at=COALESCE(resolved_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    invalidation_reason='release_descriptor_schema_migration'
WHERE state='pending';

UPDATE host_preparation_journal
SET state='failed',
    updated_at=strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    error_code='sandbox.host_preparation_release_descriptor_migrated'
WHERE state NOT IN ('completed', 'rolled_back', 'denied', 'failed');

UPDATE network_grants
SET revoked_at=COALESCE(revoked_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
WHERE revoked_at IS NULL;
"#;

const P05_MODEL_RUNTIME_SCHEMA: &str = r#"
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

CREATE TABLE model_execution_snapshots (
    model_execution_snapshot_id TEXT PRIMARY KEY,
    model_step_id TEXT NOT NULL,
    model_profile_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX model_execution_snapshots_step
    ON model_execution_snapshots(model_step_id, created_at);

CREATE TABLE model_failures (
    model_failure_id TEXT PRIMARY KEY,
    failure_fingerprint_json TEXT NOT NULL,
    failure_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX model_failures_fingerprint
    ON model_failures(failure_fingerprint_json, created_at);

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

CREATE INDEX transport_attempts_operation
    ON transport_attempts(operation_context_json, started_at);
CREATE INDEX transport_attempts_endpoint_state
    ON transport_attempts(endpoint_id, state, started_at);

CREATE TABLE model_outputs (
    model_step_id TEXT PRIMARY KEY,
    model_execution_snapshot_id TEXT NOT NULL
        REFERENCES model_execution_snapshots(model_execution_snapshot_id),
    output_json TEXT NOT NULL,
    raw_response_artifact_id TEXT REFERENCES artifacts(artifact_id),
    created_at TEXT NOT NULL
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

CREATE INDEX model_usage_step ON model_usage(model_step_id, recorded_at);

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
    created_at TEXT NOT NULL,
    UNIQUE(model_execution_snapshot_id, provider_tool_call_id)
) STRICT;

CREATE INDEX provider_tool_call_protocol_bindings_snapshot
    ON provider_tool_call_protocol_bindings(model_execution_snapshot_id, tool_call_id);

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

CREATE INDEX model_discovery_runs_endpoint
    ON model_discovery_runs(endpoint_id, started_at DESC);

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

CREATE INDEX model_availability_tests_profile
    ON model_availability_tests(model_profile_id, started_at DESC);

CREATE UNIQUE INDEX model_availability_tests_one_active_profile
    ON model_availability_tests(model_profile_id)
    WHERE state IN ('queued', 'running');

CREATE TABLE model_runtime_states (
    model_profile_id TEXT PRIMARY KEY,
    state_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE endpoint_runtime_states (
    endpoint_id TEXT PRIMARY KEY,
    state_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE endpoint_gate_metrics (
    endpoint_id TEXT PRIMARY KEY,
    active_requests INTEGER NOT NULL CHECK (active_requests >= 0),
    queued_requests INTEGER NOT NULL CHECK (queued_requests >= 0),
    metrics_json TEXT NOT NULL,
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

CREATE TABLE provider_catalog_entries (
    catalog_revision TEXT NOT NULL
        REFERENCES provider_catalog_snapshots(catalog_revision) ON DELETE CASCADE,
    catalog_provider_id TEXT NOT NULL,
    entry_json TEXT NOT NULL,
    PRIMARY KEY (catalog_revision, catalog_provider_id)
) STRICT;

CREATE TABLE provider_catalog_runtime_state (
    catalog_key TEXT PRIMARY KEY CHECK (catalog_key = 'models.dev'),
    current_revision TEXT REFERENCES provider_catalog_snapshots(catalog_revision),
    last_error_code TEXT,
    checked_at TEXT,
    updated_at TEXT NOT NULL
) STRICT;
"#;

const P02_MUTATION_JOURNAL_P05_CREDENTIAL_SCHEMA: &str = r#"
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
            'reconciliation_required'
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
        OR state IN ('unavailable', 'reconciliation_required')
    )
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

CREATE INDEX mutation_journals_kind_state
    ON mutation_journals(journal_kind, state, created_at);

CREATE UNIQUE INDEX mutation_journals_idempotency
    ON mutation_journals(journal_kind, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX mutation_journals_recovery
    ON mutation_journals(state, updated_at);

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
"#;

const P02_ARTIFACT_OWNER_SCHEMA: &str = r#"
ALTER TABLE artifact_links RENAME TO artifact_access_links;

CREATE TABLE artifact_owners (
    artifact_id TEXT PRIMARY KEY
        REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('session', 'operation', 'system')),
    owner_type TEXT NOT NULL CHECK (
        owner_type IN (
            'session',
            'model_discovery',
            'model_availability_test',
            'provider_catalog_cache'
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
    )
) STRICT;

CREATE INDEX artifact_owners_identity
    ON artifact_owners(owner_kind, owner_type, owner_id, artifact_id);

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
    THEN RAISE(ABORT, 'artifact direct owner does not exist') END;
END;

INSERT INTO artifact_owners(
    artifact_id, owner_kind, owner_type, owner_id, created_at
)
SELECT DISTINCT
    snapshot.raw_artifact_id,
    'system',
    'provider_catalog_cache',
    NULL,
    artifact.created_at
FROM provider_catalog_snapshots AS snapshot
JOIN artifacts AS artifact ON artifact.artifact_id = snapshot.raw_artifact_id;

INSERT INTO artifact_owners(
    artifact_id, owner_kind, owner_type, owner_id, created_at
)
SELECT
    artifact.artifact_id,
    'session',
    'session',
    MIN(link.session_id),
    artifact.created_at
FROM artifacts AS artifact
JOIN artifact_access_links AS link ON link.artifact_id = artifact.artifact_id
WHERE NOT EXISTS (
    SELECT 1 FROM artifact_owners AS owner
    WHERE owner.artifact_id = artifact.artifact_id
)
GROUP BY artifact.artifact_id, artifact.created_at
HAVING COUNT(DISTINCT link.session_id) = 1;

DELETE FROM artifact_access_links
WHERE EXISTS (
    SELECT 1
    FROM artifact_owners AS owner
    WHERE owner.artifact_id = artifact_access_links.artifact_id
      AND owner.owner_kind = 'session'
      AND owner.owner_id = artifact_access_links.session_id
);

CREATE TEMP TABLE artifact_owner_migration_guard (
    unowned_artifact_count INTEGER NOT NULL CHECK (unowned_artifact_count = 0)
) STRICT;

INSERT INTO artifact_owner_migration_guard(unowned_artifact_count)
SELECT COUNT(*)
FROM artifacts AS artifact
WHERE NOT EXISTS (
    SELECT 1 FROM artifact_owners AS owner
    WHERE owner.artifact_id = artifact.artifact_id
);

DROP TABLE artifact_owner_migration_guard;
"#;

const P05_CREDENTIAL_DELETE_SCHEMA: &str = r#"
ALTER TABLE credential_records RENAME TO credential_records_p05_pre_delete;

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

INSERT INTO credential_records(
    credential_id, endpoint_id, credential_slot, state, row_revision,
    last_committed_credential_revision, credential_ref,
    credential_binding_fingerprint, bundle_namespace_key, backend_kind,
    wincred_type, final_target, normalized_origin_json, auth_scheme,
    error_code, configured_at, created_at, updated_at
)
SELECT
    credential_id, endpoint_id, credential_slot, state, row_revision,
    last_committed_credential_revision, credential_ref,
    credential_binding_fingerprint, bundle_namespace_key, backend_kind,
    wincred_type, final_target, normalized_origin_json, auth_scheme,
    error_code, configured_at, created_at, updated_at
FROM credential_records_p05_pre_delete;

DROP TABLE credential_records_p05_pre_delete;

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

CREATE INDEX credential_tombstones_parent
    ON credential_tombstones(parent_journal_id, credential_id);
"#;

const P05_PROVIDER_CATALOG_ICON_SCHEMA: &str = r#"
CREATE TABLE provider_catalog_icons (
    catalog_provider_id TEXT PRIMARY KEY CHECK (length(trim(catalog_provider_id)) > 0),
    artifact_id TEXT NOT NULL UNIQUE,
    content_hash_json TEXT NOT NULL CHECK (json_valid(content_hash_json)),
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    pixel_size INTEGER NOT NULL CHECK (pixel_size = 128),
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX provider_catalog_icons_artifact
    ON provider_catalog_icons(artifact_id, content_hash_json);
"#;

const P05_PROVIDER_CATALOG_CACHE_V2_RESET_SCHEMA: &str = r#"
DROP TABLE provider_catalog_entries;
DROP TABLE provider_catalog_runtime_state;
DROP TABLE provider_catalog_snapshots;

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

CREATE TABLE provider_catalog_entries (
    catalog_revision TEXT NOT NULL
        REFERENCES provider_catalog_snapshots(catalog_revision) ON DELETE CASCADE,
    catalog_provider_id TEXT NOT NULL,
    entry_json TEXT NOT NULL,
    PRIMARY KEY (catalog_revision, catalog_provider_id)
) STRICT;

CREATE TABLE provider_catalog_runtime_state (
    catalog_key TEXT PRIMARY KEY CHECK (catalog_key = 'models.dev'),
    current_revision TEXT REFERENCES provider_catalog_snapshots(catalog_revision),
    last_error_code TEXT,
    checked_at TEXT,
    updated_at TEXT NOT NULL
) STRICT;
"#;

const P06_CONTEXT_SCHEMA: &str = r#"
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
) STRICT;

CREATE INDEX context_projection_session_created
    ON context_projection_snapshots(session_id, created_at DESC, context_snapshot_id);
CREATE INDEX context_projection_turn
    ON context_projection_snapshots(turn_task_id, created_at, context_snapshot_id);

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

CREATE TABLE context_source_event_facts (
    event_id TEXT PRIMARY KEY REFERENCES events(event_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    content_hash_json TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    model_visible INTEGER NOT NULL CHECK (model_visible IN (0, 1)),
    explicitly_user_authored INTEGER NOT NULL CHECK (explicitly_user_authored IN (0, 1)),
    created_at TEXT NOT NULL
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

CREATE INDEX context_manual_compaction_pending
    ON context_manual_compaction_requests(session_id, agent_run_id, state, created_at);

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

CREATE INDEX context_search_chunks_cursor
    ON context_search_chunks(session_id, session_sequence, chunk_id);

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
"#;

const P06_CONTEXT_REMEDIATION_SCHEMA: &str = r#"
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

CREATE UNIQUE INDEX context_remediations_one_active_per_agent
    ON context_remediations(session_id, agent_run_id)
    WHERE state = 'awaiting';
"#;

const P07_TOOL_REGISTRY_FOUNDATION_SCHEMA: &str = r#"
CREATE TABLE tool_registry_snapshots (
    tool_registry_snapshot_id TEXT PRIMARY KEY,
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    content_hash_json TEXT NOT NULL,
    snapshot_json TEXT NOT NULL,
    search_documents_json TEXT,
    search_documents_hash_json TEXT,
    search_corpus_retired_at TEXT,
    created_at TEXT NOT NULL,
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

CREATE UNIQUE INDEX tool_registry_snapshot_revision_hash
    ON tool_registry_snapshots(registry_revision, content_hash_json);

-- Cursor rows deliberately retain their immutable Snapshot binding after the
-- searchable corpus is retired, so callers receive cursor_expired instead of
-- silently moving to the latest Registry Snapshot.
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

CREATE INDEX tool_search_cursor_snapshot
    ON tool_search_cursor_bindings(tool_registry_snapshot_id);
"#;

const P07_CONTEXT_TOOL_REGISTRY_V2_SCHEMA: &str = r#"
ALTER TABLE context_projection_snapshots
    ADD COLUMN snapshot_version INTEGER NOT NULL DEFAULT 1
        CHECK (snapshot_version IN (1, 2));

ALTER TABLE provider_tool_call_protocol_bindings
    ADD COLUMN tool_definition_json TEXT;

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
"#;

const P07_TOOL_AUTHORITY_ENTITIES_SCHEMA: &str = r#"
CREATE TABLE tool_registry_authority_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

INSERT INTO tool_registry_authority_state(singleton, registry_revision, updated_at)
VALUES (1, 1, '1970-01-01T00:00:00Z');

CREATE TABLE tool_packages (
    tool_package_id TEXT PRIMARY KEY,
    package_key TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('installed', 'corrupted', 'deleted')),
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX tool_packages_live_key
    ON tool_packages(package_key)
    WHERE lifecycle <> 'deleted';

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

CREATE UNIQUE INDEX tool_definitions_package_tool_version
    ON tool_definitions(tool_package_id, tool_key, tool_version_json)
    WHERE tool_package_id IS NOT NULL;

CREATE INDEX tool_definitions_canonical_name
    ON tool_definitions(canonical_name, retired_at);

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

CREATE INDEX tool_instances_exact_version
    ON tool_instances(tool_id, tool_version_json);

CREATE TABLE tool_package_trust (
    tool_package_id TEXT PRIMARY KEY REFERENCES tool_packages(tool_package_id),
    package_security_fingerprint_json TEXT NOT NULL,
    trusted INTEGER NOT NULL CHECK (trusted IN (0, 1)),
    entity_revision INTEGER NOT NULL CHECK (entity_revision > 0),
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    reviewed_at TEXT NOT NULL
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

CREATE INDEX tool_ephemeral_loads_session
    ON tool_ephemeral_loads(session_id, turn_task_id, tool_id);
"#;

const P07_TOOL_EXECUTION_AUTHORITY_SCHEMA: &str = r#"
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

CREATE INDEX tool_batches_session_activity
    ON tool_batches(session_id, updated_at DESC, tool_batch_id);
CREATE INDEX tool_batches_runtime_state
    ON tool_batches(status, updated_at, tool_batch_id)
    WHERE status NOT IN ('completed', 'recovery_required');

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

CREATE INDEX tool_calls_batch_status
    ON tool_calls(tool_batch_id, status, ordinal);
CREATE INDEX tool_calls_definition
    ON tool_calls(tool_id, tool_version_json, status);

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
        OR side_effect_state IN ('no_effect', 'fully_applied')
    )
) STRICT;

CREATE INDEX tool_execution_attempts_call
    ON tool_execution_attempts(tool_call_id, attempt_number);
CREATE INDEX tool_execution_attempts_runtime_state
    ON tool_execution_attempts(status, updated_at, tool_execution_attempt_id)
    WHERE status NOT IN ('succeeded', 'failed', 'cancelled', 'indeterminate');

CREATE TABLE tool_attempt_operational_state (
    tool_execution_attempt_id TEXT PRIMARY KEY
        REFERENCES tool_execution_attempts(tool_execution_attempt_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    percentage INTEGER CHECK (percentage BETWEEN 0 AND 100),
    progress_json TEXT,
    updated_at TEXT NOT NULL
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
    CHECK (outcome_kind <> 'succeeded' OR side_effect_state IN ('no_effect', 'fully_applied')),
    CHECK (outcome_kind <> 'not_executed' OR side_effect_state = 'not_started'),
    CHECK (
        model_visible_result_artifact_id IS NULL
        OR model_visible_result_artifact_id <> internal_diagnostic_artifact_id
    )
) STRICT;

CREATE INDEX tool_results_batch
    ON tool_results(tool_batch_id, ordinal);
"#;

// Development builds that opened Schema v21 before the full Admission path
// existed received a definition FK on tool_calls. That FK is incompatible
// with the frozen requirement to persist tool_not_found/version_mismatch as a
// paired NotExecuted result. New databases use the corrected v21 DDL above;
// v22 repairs only databases carrying the earlier FK and preserves every row.
const P07_TOOL_CALL_REFERENCE_REPAIR_SCHEMA: &str = r#"
DROP INDEX tool_calls_batch_status;
DROP INDEX tool_calls_definition;
DROP INDEX tool_execution_attempts_call;
DROP INDEX tool_execution_attempts_runtime_state;
DROP INDEX tool_results_batch;

ALTER TABLE tool_attempt_operational_state
    RENAME TO tool_attempt_operational_state_v21_definition_fk;
ALTER TABLE tool_results
    RENAME TO tool_results_v21_definition_fk;
ALTER TABLE tool_execution_attempts
    RENAME TO tool_execution_attempts_v21_definition_fk;
ALTER TABLE tool_calls
    RENAME TO tool_calls_v21_definition_fk;

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

CREATE INDEX tool_calls_batch_status
    ON tool_calls(tool_batch_id, status, ordinal);
CREATE INDEX tool_calls_definition
    ON tool_calls(tool_id, tool_version_json, status);

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
        OR side_effect_state IN ('no_effect', 'fully_applied')
    )
) STRICT;

CREATE INDEX tool_execution_attempts_call
    ON tool_execution_attempts(tool_call_id, attempt_number);
CREATE INDEX tool_execution_attempts_runtime_state
    ON tool_execution_attempts(status, updated_at, tool_execution_attempt_id)
    WHERE status NOT IN ('succeeded', 'failed', 'cancelled', 'indeterminate');

CREATE TABLE tool_attempt_operational_state (
    tool_execution_attempt_id TEXT PRIMARY KEY
        REFERENCES tool_execution_attempts(tool_execution_attempt_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    percentage INTEGER CHECK (percentage BETWEEN 0 AND 100),
    progress_json TEXT,
    updated_at TEXT NOT NULL
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
    CHECK (outcome_kind <> 'succeeded' OR side_effect_state IN ('no_effect', 'fully_applied')),
    CHECK (outcome_kind <> 'not_executed' OR side_effect_state = 'not_started'),
    CHECK (
        model_visible_result_artifact_id IS NULL
        OR model_visible_result_artifact_id <> internal_diagnostic_artifact_id
    )
) STRICT;

CREATE INDEX tool_results_batch
    ON tool_results(tool_batch_id, ordinal);

INSERT INTO tool_calls(
    tool_call_id, tool_batch_id, ordinal, tool_id, tool_version_json, status,
    normalized_call_json, normalized_call_hash_json, execution_plan_call_json,
    entity_revision, created_at, started_at, completed_at, updated_at
)
SELECT
    tool_call_id, tool_batch_id, ordinal, tool_id, tool_version_json, status,
    normalized_call_json, normalized_call_hash_json, execution_plan_call_json,
    entity_revision, created_at, started_at, completed_at, updated_at
FROM tool_calls_v21_definition_fk;

INSERT INTO tool_execution_attempts(
    tool_execution_attempt_id, tool_call_id, tool_batch_id, attempt_number,
    execution_plan_hash_json, status, side_effect_state, record_json,
    record_hash_json, evidence_artifact_id, started_event_id,
    dispatch_prepared_event_id, cancellation_requested_event_id, terminal_event_id,
    started_at, dispatch_prepared_at, cancellation_requested_at, completed_at, updated_at
)
SELECT
    tool_execution_attempt_id, tool_call_id, tool_batch_id, attempt_number,
    execution_plan_hash_json, status, side_effect_state, record_json,
    record_hash_json, evidence_artifact_id, started_event_id,
    dispatch_prepared_event_id, cancellation_requested_event_id, terminal_event_id,
    started_at, dispatch_prepared_at, cancellation_requested_at, completed_at, updated_at
FROM tool_execution_attempts_v21_definition_fk;

INSERT INTO tool_attempt_operational_state(
    tool_execution_attempt_id, sequence, percentage, progress_json, updated_at
)
SELECT tool_execution_attempt_id, sequence, percentage, progress_json, updated_at
FROM tool_attempt_operational_state_v21_definition_fk;

INSERT INTO tool_results(
    tool_call_id, tool_batch_id, ordinal, outcome_kind, side_effect_state,
    result_json, result_hash_json, finalized_event_id,
    model_visible_result_artifact_id, user_audit_result_artifact_id,
    internal_diagnostic_artifact_id, aggregate_side_effect_evidence_artifact_id,
    completed_at
)
SELECT
    tool_call_id, tool_batch_id, ordinal, outcome_kind, side_effect_state,
    result_json, result_hash_json, finalized_event_id,
    model_visible_result_artifact_id, user_audit_result_artifact_id,
    internal_diagnostic_artifact_id, aggregate_side_effect_evidence_artifact_id,
    completed_at
FROM tool_results_v21_definition_fk;

DROP TABLE tool_attempt_operational_state_v21_definition_fk;
DROP TABLE tool_results_v21_definition_fk;
DROP TABLE tool_execution_attempts_v21_definition_fk;
DROP TABLE tool_calls_v21_definition_fk;
"#;

const P07_TOOL_ARTIFACT_OWNER_SCHEMA: &str = r#"
DROP TRIGGER artifact_owners_immutable;
DROP TRIGGER artifact_owners_scope_guard;
DROP INDEX artifact_owners_identity;

ALTER TABLE artifact_owners RENAME TO artifact_owners_pre_p07_tool;

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

INSERT INTO artifact_owners(
    artifact_id, owner_kind, owner_type, owner_id, created_at
)
SELECT artifact_id, owner_kind, owner_type, owner_id, created_at
FROM artifact_owners_pre_p07_tool;

DROP TABLE artifact_owners_pre_p07_tool;

CREATE INDEX artifact_owners_identity
    ON artifact_owners(owner_kind, owner_type, owner_id, artifact_id);

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
                          NEW.owner_type IN ('draft', 'conformance_run')
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
"#;

const P07_TOOL_CONFORMANCE_RUN_SCHEMA: &str = r#"
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

CREATE INDEX tool_conformance_runs_draft
    ON tool_conformance_runs(tool_draft_id, draft_revision, status, tool_conformance_run_id);

CREATE INDEX tool_conformance_runs_recovery
    ON tool_conformance_runs(status, updated_at, tool_conformance_run_id)
    WHERE status IN ('queued', 'running');
"#;

const P07_TOOL_DRAFT_WORKFLOW_SCHEMA: &str = r#"
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

CREATE TABLE tool_draft_approvals (
    tool_draft_id TEXT NOT NULL,
    draft_revision INTEGER NOT NULL,
    snapshot_hash_json TEXT NOT NULL,
    decision TEXT NOT NULL CHECK (decision IN ('install', 'install_and_trust', 'reject')),
    evidence_json TEXT NOT NULL,
    evidence_hash_json TEXT NOT NULL,
    decided_at TEXT NOT NULL,
    PRIMARY KEY (tool_draft_id, draft_revision),
    FOREIGN KEY (tool_draft_id, draft_revision)
        REFERENCES tool_draft_revisions(tool_draft_id, draft_revision) ON DELETE CASCADE
) STRICT;

CREATE INDEX tool_draft_status
    ON tool_drafts(status, updated_at, tool_draft_id);

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
"#;

const P07_TOOL_PACKAGE_PUBLISH_SCHEMA: &str = r#"
ALTER TABLE tool_drafts ADD COLUMN draft_kind TEXT NOT NULL DEFAULT 'content_mutation'
    CHECK (draft_kind IN ('content_mutation', 'package_deletion'));

ALTER TABLE tool_packages ADD COLUMN deletion_pending INTEGER NOT NULL DEFAULT 0
    CHECK (deletion_pending IN (0, 1));

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

CREATE TABLE tool_package_tombstones (
    tool_package_id TEXT PRIMARY KEY,
    package_key TEXT NOT NULL,
    tombstone_json TEXT NOT NULL,
    tombstone_hash_json TEXT NOT NULL,
    mutation_journal_id TEXT NOT NULL UNIQUE REFERENCES mutation_journals(journal_id),
    deleted_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_tombstones (
    tool_id TEXT PRIMARY KEY,
    tool_package_id TEXT NOT NULL REFERENCES tool_package_tombstones(tool_package_id),
    tombstone_json TEXT NOT NULL,
    tombstone_hash_json TEXT NOT NULL,
    deleted_at TEXT NOT NULL
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

CREATE INDEX tool_package_payload_gc
    ON tool_package_payloads(state, deleted_at, tool_package_id, package_version_json);

CREATE INDEX tool_package_payload_references_live
    ON tool_package_payload_strong_references(tool_package_id, package_version_json)
    WHERE released_at IS NULL;

CREATE TABLE tool_package_orphaned_payloads (
    package_store_relative_path TEXT PRIMARY KEY,
    detected_at TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (reason = 'missing_authority_record')
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

CREATE TRIGGER payload_reference_deletion_pending_guard
BEFORE INSERT ON tool_package_payload_strong_references
WHEN EXISTS (
    SELECT 1 FROM tool_packages p
    WHERE p.tool_package_id = NEW.tool_package_id AND p.deletion_pending = 1
)
BEGIN
    SELECT RAISE(ABORT, 'tool package deletion is pending');
END;
"#;

const P07_TOOL_TRUST_ELIGIBLE_REGISTRY_SCHEMA: &str = r#"
ALTER TABLE tool_registry_snapshots
    ADD COLUMN snapshot_contract_version INTEGER NOT NULL DEFAULT 1
        CHECK (snapshot_contract_version IN (1, 2));

CREATE TABLE tool_registry_scope_heads (
    scope_hash_json TEXT PRIMARY KEY,
    scope_json TEXT NOT NULL,
    tool_registry_snapshot_id TEXT NOT NULL
        REFERENCES tool_registry_snapshots(tool_registry_snapshot_id),
    registry_revision INTEGER NOT NULL CHECK (registry_revision > 0),
    content_hash_json TEXT NOT NULL,
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
    credential_id TEXT NOT NULL,
    credential_revision INTEGER NOT NULL CHECK (credential_revision > 0),
    binding_revision INTEGER NOT NULL CHECK (binding_revision > 0),
    binding_hash_json TEXT NOT NULL,
    record_json TEXT NOT NULL,
    record_hash_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
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
"#;

const P07_TOOL_RUNTIME_DISPATCH_SCHEMA: &str = r#"
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

CREATE INDEX tool_batch_payload_leases_live
    ON tool_batch_payload_leases(tool_batch_id, released_at)
    WHERE released_at IS NULL;

CREATE TABLE tool_execution_recovery_diagnostics (
    tool_execution_attempt_id TEXT PRIMARY KEY
        REFERENCES tool_execution_attempts(tool_execution_attempt_id),
    recovery_code TEXT NOT NULL CHECK (
        recovery_code IN ('historical_started_only', 'dispatch_outcome_unknown')
    ),
    evidence_hash_json TEXT NOT NULL,
    detected_at TEXT NOT NULL
) STRICT;
"#;

const P07_TOOL_SANDBOX_AUTHORIZED_DISPATCH_SCHEMA: &str = r#"
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

CREATE INDEX tool_sandbox_dispatch_bindings_attempt
    ON tool_sandbox_dispatch_bindings(tool_execution_attempt_id, state);
CREATE INDEX tool_sandbox_dispatch_bindings_pending
    ON tool_sandbox_dispatch_bindings(state, prepared_at)
    WHERE state = 'dispatch_prepared';
"#;

const P07_TOOL_RESULT_ARTIFACT_ACCESS_SCHEMA: &str = r#"
ALTER TABLE artifact_access_links ADD COLUMN evidence_hash_json TEXT;
"#;

const P07_TOOL_RESULT_UNKNOWN_AND_HISTORY_GROUP_SCHEMA: &str = r#"
DROP TRIGGER artifact_owners_scope_guard;
DROP INDEX tool_execution_attempts_call;
DROP INDEX tool_execution_attempts_runtime_state;
DROP INDEX tool_sandbox_dispatch_bindings_attempt;
DROP INDEX tool_sandbox_dispatch_bindings_pending;
DROP INDEX tool_results_batch;

ALTER TABLE tool_attempt_operational_state
    RENAME TO tool_attempt_operational_state_pre_v31;
ALTER TABLE tool_execution_recovery_diagnostics
    RENAME TO tool_execution_recovery_diagnostics_pre_v31;
ALTER TABLE tool_sandbox_dispatch_bindings
    RENAME TO tool_sandbox_dispatch_bindings_pre_v31;
ALTER TABLE tool_results RENAME TO tool_results_pre_v31;
ALTER TABLE tool_execution_attempts RENAME TO tool_execution_attempts_pre_v31;

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

CREATE INDEX tool_execution_attempts_call
    ON tool_execution_attempts(tool_call_id, attempt_number);
CREATE INDEX tool_execution_attempts_runtime_state
    ON tool_execution_attempts(status, updated_at, tool_execution_attempt_id)
    WHERE status NOT IN ('succeeded', 'failed', 'cancelled', 'indeterminate');

CREATE TABLE tool_attempt_operational_state (
    tool_execution_attempt_id TEXT PRIMARY KEY
        REFERENCES tool_execution_attempts(tool_execution_attempt_id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    percentage INTEGER CHECK (percentage BETWEEN 0 AND 100),
    progress_json TEXT,
    updated_at TEXT NOT NULL
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

CREATE INDEX tool_sandbox_dispatch_bindings_attempt
    ON tool_sandbox_dispatch_bindings(tool_execution_attempt_id, state);
CREATE INDEX tool_sandbox_dispatch_bindings_pending
    ON tool_sandbox_dispatch_bindings(state, prepared_at)
    WHERE state = 'dispatch_prepared';

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

CREATE INDEX tool_results_batch ON tool_results(tool_batch_id, ordinal);

INSERT INTO tool_execution_attempts(
    tool_execution_attempt_id, tool_call_id, tool_batch_id, attempt_number,
    execution_plan_hash_json, status, side_effect_state, record_json,
    record_hash_json, evidence_artifact_id, started_event_id,
    dispatch_prepared_event_id, cancellation_requested_event_id, terminal_event_id,
    started_at, dispatch_prepared_at, cancellation_requested_at, completed_at, updated_at
)
SELECT
    tool_execution_attempt_id, tool_call_id, tool_batch_id, attempt_number,
    execution_plan_hash_json, status, side_effect_state, record_json,
    record_hash_json, evidence_artifact_id, started_event_id,
    dispatch_prepared_event_id, cancellation_requested_event_id, terminal_event_id,
    started_at, dispatch_prepared_at, cancellation_requested_at, completed_at, updated_at
FROM tool_execution_attempts_pre_v31;

INSERT INTO tool_attempt_operational_state(
    tool_execution_attempt_id, sequence, percentage, progress_json, updated_at
)
SELECT tool_execution_attempt_id, sequence, percentage, progress_json, updated_at
FROM tool_attempt_operational_state_pre_v31;

INSERT INTO tool_execution_recovery_diagnostics(
    tool_execution_attempt_id, recovery_code, evidence_hash_json, detected_at
)
SELECT tool_execution_attempt_id, recovery_code, evidence_hash_json, detected_at
FROM tool_execution_recovery_diagnostics_pre_v31;

INSERT INTO tool_sandbox_dispatch_bindings(
    sandbox_execution_id, security_operation_id, tool_execution_attempt_id,
    tool_batch_id, tool_call_id, permission_snapshot_id, execution_plan_hash_json,
    runtime_binding_hash_json, binding_json, binding_hash_json, state,
    prepared_at, consumed_at, updated_at
)
SELECT
    sandbox_execution_id, security_operation_id, tool_execution_attempt_id,
    tool_batch_id, tool_call_id, permission_snapshot_id, execution_plan_hash_json,
    runtime_binding_hash_json, binding_json, binding_hash_json, state,
    prepared_at, consumed_at, updated_at
FROM tool_sandbox_dispatch_bindings_pre_v31;

INSERT INTO tool_results(
    tool_call_id, tool_batch_id, ordinal, outcome_kind, side_effect_state,
    result_json, result_hash_json, finalized_event_id,
    model_visible_result_artifact_id, user_audit_result_artifact_id,
    internal_diagnostic_artifact_id, aggregate_side_effect_evidence_artifact_id,
    completed_at
)
SELECT
    tool_call_id, tool_batch_id, ordinal, outcome_kind, side_effect_state,
    result_json, result_hash_json, finalized_event_id,
    model_visible_result_artifact_id, user_audit_result_artifact_id,
    internal_diagnostic_artifact_id, aggregate_side_effect_evidence_artifact_id,
    completed_at
FROM tool_results_pre_v31;

DROP TABLE tool_attempt_operational_state_pre_v31;
DROP TABLE tool_execution_recovery_diagnostics_pre_v31;
DROP TABLE tool_sandbox_dispatch_bindings_pre_v31;
DROP TABLE tool_results_pre_v31;
DROP TABLE tool_execution_attempts_pre_v31;

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
                          NEW.owner_type IN ('draft', 'conformance_run')
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

CREATE INDEX tool_history_atomic_group_associations_event
    ON tool_history_atomic_group_associations(assistant_response_event_id, state);

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
"#;

const P07_TOOL_ACTIVITY_CURSOR_SCHEMA: &str = r#"
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

CREATE INDEX tool_activity_cursor_session
    ON tool_activity_cursor_bindings(session_id, created_at);
"#;

const P07_SIGNED_BUILTIN_RELEASE_SCHEMA: &str = r#"
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
"#;

const P07_BUILTIN_CORE_HANDLER_DISPATCH_SCHEMA: &str = r#"
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

CREATE INDEX tool_builtin_core_handler_dispatch_state
    ON tool_builtin_core_handler_dispatch_bindings(state, updated_at);
"#;

const P07_TOOL_MUTATION_COMMAND_JOURNAL_SCHEMA: &str = r#"
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
    updated_at TEXT NOT NULL,
    CHECK (
        (state = 'prepared' AND outcome_json IS NULL AND outcome_hash_json IS NULL)
        OR
        (state IN ('succeeded', 'failed')
            AND outcome_json IS NOT NULL
            AND outcome_hash_json IS NOT NULL)
    )
) STRICT;

CREATE INDEX tool_mutation_command_journals_state
    ON tool_mutation_command_journals(state, updated_at);
"#;

const P07_TOOL_MUTATION_COMMAND_CORRELATION_SCHEMA: &str = r#"
ALTER TABLE tool_mutation_command_journals
    ADD COLUMN original_correlation_id TEXT;

UPDATE tool_mutation_command_journals
SET original_correlation_id=json_extract(seed_json, '$.original_correlation_id')
WHERE original_correlation_id IS NULL;

CREATE UNIQUE INDEX tool_mutation_command_journals_correlation
    ON tool_mutation_command_journals(original_correlation_id)
    WHERE original_correlation_id IS NOT NULL;
"#;

const P07_TOOL_CONFORMANCE_SANDBOX_DISPATCH_SCHEMA: &str = r#"
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

CREATE INDEX tool_conformance_sandbox_dispatch_state
    ON tool_conformance_sandbox_dispatch_bindings(state, updated_at);
"#;

const P07_SANDBOX_AUTHORIZATION_ROOT_SCHEMA: &str = r#"
CREATE TABLE authorization_snapshots (
    authorization_snapshot_id TEXT PRIMARY KEY,
    authorization_kind TEXT NOT NULL CHECK (
        authorization_kind IN ('session_permission', 'tool_conformance_allow_once')
    ),
    created_at TEXT NOT NULL
) STRICT;

INSERT INTO authorization_snapshots(
    authorization_snapshot_id, authorization_kind, created_at
)
SELECT permission_snapshot_id, 'session_permission', created_at
FROM permission_snapshots;

INSERT INTO authorization_snapshots(
    authorization_snapshot_id, authorization_kind, created_at
)
SELECT
    json_extract(allow_once_evidence_json, '$.authorization_snapshot_id'),
    'tool_conformance_allow_once',
    json_extract(allow_once_evidence_json, '$.approved_at')
FROM tool_conformance_sandbox_dispatch_bindings;

CREATE TRIGGER permission_snapshots_authorization_root_insert
AFTER INSERT ON permission_snapshots
BEGIN
    INSERT INTO authorization_snapshots(
        authorization_snapshot_id, authorization_kind, created_at
    ) VALUES (NEW.permission_snapshot_id, 'session_permission', NEW.created_at);
END;

ALTER TABLE sandbox_acl_journal RENAME TO sandbox_acl_journal_pre_v38;
ALTER TABLE sandbox_executions RENAME TO sandbox_executions_pre_v38;

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

INSERT INTO sandbox_executions(
    sandbox_execution_id, identity_json, permission_snapshot_id,
    mode, plan_hash_json, plan_json, state, process_id, exit_code,
    created_at, updated_at, terminal_at
)
SELECT
    sandbox_execution_id, identity_json, permission_snapshot_id,
    mode, plan_hash_json, plan_json, state, process_id, exit_code,
    created_at, updated_at, terminal_at
FROM sandbox_executions_pre_v38;

INSERT INTO sandbox_acl_journal(
    sandbox_acl_journal_id, sandbox_execution_id, target_identity_json,
    target_path_json, target_path_display, original_dacl_sddl,
    original_dacl_hash, appcontainer_profile_name, appcontainer_sid,
    access_mask, recursive, state, created_at, updated_at, error_code
)
SELECT
    sandbox_acl_journal_id, sandbox_execution_id, target_identity_json,
    target_path_json, target_path_display, original_dacl_sddl,
    original_dacl_hash, appcontainer_profile_name, appcontainer_sid,
    access_mask, recursive, state, created_at, updated_at, error_code
FROM sandbox_acl_journal_pre_v38;

DROP TABLE sandbox_acl_journal_pre_v38;
DROP TABLE sandbox_executions_pre_v38;
"#;

const P07_TOOL_LONG_OPERATION_SCHEMA: &str = r#"
ALTER TABLE tool_draft_approvals
    ADD COLUMN consumed_by_journal_id TEXT REFERENCES mutation_journals(journal_id);

CREATE UNIQUE INDEX tool_draft_approval_consumption
    ON tool_draft_approvals(consumed_by_journal_id)
    WHERE consumed_by_journal_id IS NOT NULL;

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

CREATE UNIQUE INDEX tool_long_operation_active_approval
    ON tool_long_operations(approval_reservation_key)
    WHERE state IN ('queued', 'running', 'cancelling');

CREATE INDEX tool_long_operation_state
    ON tool_long_operations(state, updated_at, journal_id);
"#;

const P07_TOOL_CREDENTIAL_AUTHORITY_SCHEMA: &str = r#"
CREATE TABLE tool_profile_principals (
    profile_key TEXT PRIMARY KEY CHECK (profile_key = 'current'),
    principal_id TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE tool_credential_authority_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    candidate_set_revision INTEGER NOT NULL CHECK (candidate_set_revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

INSERT INTO tool_credential_authority_state(singleton, candidate_set_revision, updated_at)
VALUES (1, 1, '1970-01-01T00:00:00Z');

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

CREATE TABLE tool_credential_secret_bindings (
    credential_id TEXT PRIMARY KEY REFERENCES tool_credentials(credential_id) ON DELETE CASCADE,
    credential_revision INTEGER NOT NULL CHECK (credential_revision > 0),
    credential_ref TEXT NOT NULL UNIQUE,
    updated_at TEXT NOT NULL
) STRICT;

-- The pre-authority binding shape lacked Slot and exact Placement and was
-- backed by the P05 Endpoint credential table. It cannot be upgraded without
-- manufacturing security facts, so it is invalidated fail-closed.
DROP TABLE tool_instance_credential_bindings;

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
"#;

const P07_TOOL_DRAFT_SNAPSHOT_STAGING_SCHEMA: &str = r#"
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

CREATE INDEX tool_draft_snapshot_staging_expiry
    ON tool_draft_snapshot_staging(consumed_at, expires_at);
"#;

const P07_TOOL_DRAFT_RUNTIME_EPHEMERAL_ARTIFACT_SCHEMA: &str = r#"
DROP TRIGGER artifact_owners_scope_guard;

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
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationFaultPoint {
    AfterBackup,
    DuringMigration,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DatabaseOpenOptions {
    pub migration_fault: Option<MigrationFaultPoint>,
}

pub(crate) enum MigrationResult {
    Ready(Connection),
    RecoveryRequired(MigrationRecoveryRequired),
}

pub(crate) fn open_and_migrate(
    database_path: &Path,
    options: DatabaseOpenOptions,
) -> Result<MigrationResult, PersistenceError> {
    let parent = database_path.parent().ok_or_else(|| {
        PersistenceError::InvalidRequest("database path has no parent".to_owned())
    })?;
    fs::create_dir_all(parent).map_err(|source| PersistenceError::Io {
        action: "create database directory",
        path: parent.to_path_buf(),
        source,
    })?;
    let existed = database_path.is_file()
        && fs::metadata(database_path).map(|metadata| metadata.len() > 0).unwrap_or(false);
    let connection = Connection::open(database_path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let current = current_schema_version(&connection)?;
    if current > LATEST_SCHEMA_VERSION {
        drop(connection);
        return Ok(MigrationResult::RecoveryRequired(MigrationRecoveryRequired {
            database_path: database_path.to_path_buf(),
            backup_path: None,
            reason: format!(
                "database schema version {current} is newer than supported version {LATEST_SCHEMA_VERSION}"
            ),
        }));
    }
    configure_writer(&connection)?;
    if current == LATEST_SCHEMA_VERSION {
        return Ok(MigrationResult::Ready(connection));
    }

    let backup_path =
        if existed { Some(create_backup(&connection, database_path, current)?) } else { None };
    if options.migration_fault == Some(MigrationFaultPoint::AfterBackup) {
        drop(connection);
        return recovery_after_failure(
            database_path,
            backup_path,
            "injected failure after migration backup".to_owned(),
        );
    }

    let migration = apply_migrations(&connection, current, options.migration_fault);
    if let Err(error) = migration {
        drop(connection);
        return recovery_after_failure(database_path, backup_path, error.to_string());
    }
    Ok(MigrationResult::Ready(connection))
}

pub(crate) fn configure_writer(connection: &Connection) -> Result<(), PersistenceError> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "wal_autocheckpoint", 1000_i64)?;
    Ok(())
}

pub(crate) fn configure_reader(connection: &Connection) -> Result<(), PersistenceError> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "query_only", "ON")?;
    Ok(())
}

pub(crate) fn current_schema_version(connection: &Connection) -> Result<i64, PersistenceError> {
    let exists = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master
            WHERE type = 'table' AND name = 'schema_migrations'
        )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Ok(0);
    }
    Ok(connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0))
}

fn apply_migrations(
    connection: &Connection,
    current: i64,
    fault: Option<MigrationFaultPoint>,
) -> Result<(), PersistenceError> {
    apply_migrations_through(connection, current, LATEST_SCHEMA_VERSION, fault)
}

fn apply_migrations_through(
    connection: &Connection,
    current: i64,
    target: i64,
    fault: Option<MigrationFaultPoint>,
) -> Result<(), PersistenceError> {
    let transaction = connection.unchecked_transaction()?;
    if current < 1 && target >= 1 {
        transaction.execute_batch(INITIAL_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) {
            transaction.execute_batch("THIS IS AN INJECTED INVALID MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                1_i64,
                "p02_initial",
                blake3::hash(INITIAL_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 2 && target >= 2 {
        transaction.execute_batch(P03_WORKSPACE_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 1 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P03 MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                2_i64,
                "p03_workspace_files",
                blake3::hash(P03_WORKSPACE_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 3 && target >= 3 {
        transaction.execute_batch(P04_SECURITY_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 2 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P04 MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                3_i64,
                "p04_security_control_plane",
                blake3::hash(P04_SECURITY_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 4 && target >= 4 {
        transaction.execute_batch(P04_SECURITY_REVISION_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 3 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P04 REVISION MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                4_i64,
                "p04_security_revision",
                blake3::hash(P04_SECURITY_REVISION_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 5 && target >= 5 {
        transaction.execute_batch(P04_HOST_PREPARATION_APPROVAL_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 4 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID HOST PREPARATION APPROVAL MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                5_i64,
                "p04_host_preparation_approval",
                blake3::hash(P04_HOST_PREPARATION_APPROVAL_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 6 && target >= 6 {
        transaction.execute_batch(P04_QUARANTINE_RELEASE_JOURNAL_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 5 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID QUARANTINE JOURNAL MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                6_i64,
                "p04_quarantine_release_journal",
                blake3::hash(P04_QUARANTINE_RELEASE_JOURNAL_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 7 && target >= 7 {
        transaction.execute_batch(P04_ARTIFACT_OVERWRITE_JOURNAL_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 6 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID ARTIFACT JOURNAL MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                7_i64,
                "p04_artifact_overwrite_journal",
                blake3::hash(P04_ARTIFACT_OVERWRITE_JOURNAL_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 8 && target >= 8 {
        transaction.execute_batch(P04_RELEASE_DESCRIPTOR_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 7 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID RELEASE DESCRIPTOR MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                8_i64,
                "p04_release_descriptor_namespace",
                blake3::hash(P04_RELEASE_DESCRIPTOR_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 9 && target >= 9 {
        transaction.execute_batch(P05_MODEL_RUNTIME_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 8 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P05 MODEL MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                9_i64,
                "p05_model_runtime",
                blake3::hash(P05_MODEL_RUNTIME_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 10 && target >= 10 {
        transaction.execute_batch(P02_MUTATION_JOURNAL_P05_CREDENTIAL_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 9 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P05 CREDENTIAL JOURNAL MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                10_i64,
                "p02_mutation_journal_p05_credential",
                blake3::hash(P02_MUTATION_JOURNAL_P05_CREDENTIAL_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 11 && target >= 11 {
        transaction.execute_batch(P02_ARTIFACT_OWNER_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 10 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID ARTIFACT OWNER MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                11_i64,
                "p02_artifact_direct_owner",
                blake3::hash(P02_ARTIFACT_OWNER_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 12 && target >= 12 {
        transaction.execute_batch(P05_CREDENTIAL_DELETE_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 11 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P05 CREDENTIAL DELETE MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                12_i64,
                "p05_credential_delete",
                blake3::hash(P05_CREDENTIAL_DELETE_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 13 && target >= 13 {
        transaction.execute_batch(P05_PROVIDER_CATALOG_ICON_SCHEMA)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                13_i64,
                "p05_provider_catalog_icon",
                blake3::hash(P05_PROVIDER_CATALOG_ICON_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 14 && target >= 14 {
        migrate_provider_catalog_cache_v2(&transaction)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 13 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID CATALOG CACHE MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                14_i64,
                "p05_provider_catalog_cache_v2",
                blake3::hash(P05_PROVIDER_CATALOG_CACHE_V2_RESET_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 15 && target >= 15 {
        let applied_at = UtcTimestamp::now().to_string();
        migrate_legacy_configuration_transport_indeterminate_v1(
            &transaction,
            current,
            &applied_at,
        )?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 14 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID LEGACY TRANSPORT RECONCILIATION MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                15_i64,
                "p05.legacy_configuration_transport_indeterminate.v1",
                blake3::hash(LEGACY_CONFIGURATION_TRANSPORT_LEDGER_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                applied_at,
            ),
        )?;
    }
    if current < 16 && target >= 16 {
        transaction.execute_batch(P06_CONTEXT_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 15 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P06 CONTEXT MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                16_i64,
                "p06_prompt_context_projection_compaction",
                blake3::hash(P06_CONTEXT_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 17 && target >= 17 {
        transaction.execute_batch(P06_CONTEXT_REMEDIATION_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 16 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P06 REMEDIATION MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                17_i64,
                "p06_context_remediation",
                blake3::hash(P06_CONTEXT_REMEDIATION_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 18 && target >= 18 {
        transaction.execute_batch(P07_TOOL_REGISTRY_FOUNDATION_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 17 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 REGISTRY MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                18_i64,
                "p07_tool_registry_foundation",
                blake3::hash(P07_TOOL_REGISTRY_FOUNDATION_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 19 && target >= 19 {
        transaction.execute_batch(P07_CONTEXT_TOOL_REGISTRY_V2_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 18 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 CONTEXT V2 MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                19_i64,
                "p07_context_tool_registry_snapshot_v2",
                blake3::hash(P07_CONTEXT_TOOL_REGISTRY_V2_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 20 && target >= 20 {
        transaction.execute_batch(P07_TOOL_AUTHORITY_ENTITIES_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 19 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 ENTITY MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                20_i64,
                "p07_tool_authority_entities",
                blake3::hash(P07_TOOL_AUTHORITY_ENTITIES_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 21 && target >= 21 {
        transaction.execute_batch(P07_TOOL_EXECUTION_AUTHORITY_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 20 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 EXECUTION MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                21_i64,
                "p07_tool_execution_authority",
                blake3::hash(P07_TOOL_EXECUTION_AUTHORITY_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 22 && target >= 22 {
        repair_tool_call_definition_reference_v1(&transaction)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 21 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 CALL REPAIR MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                22_i64,
                "p07_tool_call_definition_reference_v1",
                blake3::hash(P07_TOOL_CALL_REFERENCE_REPAIR_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 23 && target >= 23 {
        transaction.execute_batch(P07_TOOL_ARTIFACT_OWNER_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 22 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P07 ARTIFACT OWNER MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                23_i64,
                "p07_tool_artifact_direct_owner",
                blake3::hash(P07_TOOL_ARTIFACT_OWNER_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 24 && target >= 24 {
        transaction.execute_batch(P07_TOOL_CONFORMANCE_RUN_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 23 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P07 CONFORMANCE RUN MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                24_i64,
                "p07_tool_conformance_runs",
                blake3::hash(P07_TOOL_CONFORMANCE_RUN_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 25 && target >= 25 {
        transaction.execute_batch(P07_TOOL_DRAFT_WORKFLOW_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 24 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 DRAFT MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                25_i64,
                "p07_tool_draft_workflow",
                blake3::hash(P07_TOOL_DRAFT_WORKFLOW_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 26 && target >= 26 {
        transaction.execute_batch(P07_TOOL_PACKAGE_PUBLISH_SCHEMA)?;
        migrate_p07_w5_record_kinds(&transaction)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 25 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P07 PACKAGE PUBLISH MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                26_i64,
                "p07_tool_package_publish",
                blake3::hash(P07_TOOL_PACKAGE_PUBLISH_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 27 && target >= 27 {
        transaction.execute_batch(P07_TOOL_TRUST_ELIGIBLE_REGISTRY_SCHEMA)?;
        let applied_at = UtcTimestamp::now().to_string();
        migrate_p07_tool_instance_source_fingerprints_v2(&transaction)?;
        migrate_p04_security_sources_v2(&transaction, &applied_at)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 26 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 W6 MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                27_i64,
                "p07_tool_trust_eligible_registry",
                blake3::hash(P07_TOOL_TRUST_ELIGIBLE_REGISTRY_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                applied_at,
            ),
        )?;
    }
    if current < 28 && target >= 28 {
        transaction.execute_batch(P07_TOOL_RUNTIME_DISPATCH_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 27 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 W7 MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                28_i64,
                "p07_tool_runtime_dispatch",
                blake3::hash(P07_TOOL_RUNTIME_DISPATCH_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 29 && target >= 29 {
        transaction.execute_batch(P07_TOOL_SANDBOX_AUTHORIZED_DISPATCH_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 28 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID P07 W7 ADAPTER MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                29_i64,
                "p07_tool_sandbox_authorized_dispatch",
                blake3::hash(P07_TOOL_SANDBOX_AUTHORIZED_DISPATCH_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 30 && target >= 30 {
        transaction.execute_batch(P07_TOOL_RESULT_ARTIFACT_ACCESS_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 29 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 RESULT ARTIFACT ACCESS MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                30_i64,
                "p07_tool_result_artifact_access",
                blake3::hash(P07_TOOL_RESULT_ARTIFACT_ACCESS_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 31 && target >= 31 {
        transaction.execute_batch(P07_TOOL_RESULT_UNKNOWN_AND_HISTORY_GROUP_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 30 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P07 HISTORY ATOMIC GROUP MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                31_i64,
                "p07_tool_result_unknown_and_history_group",
                blake3::hash(P07_TOOL_RESULT_UNKNOWN_AND_HISTORY_GROUP_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 32 && target >= 32 {
        transaction.execute_batch(P07_TOOL_ACTIVITY_CURSOR_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 31 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P07 ACTIVITY CURSOR MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                32_i64,
                "p07_tool_activity_cursor",
                blake3::hash(P07_TOOL_ACTIVITY_CURSOR_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 33 && target >= 33 {
        transaction.execute_batch(P07_SIGNED_BUILTIN_RELEASE_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 32 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 SIGNED BUILTIN RELEASE MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                33_i64,
                "p07_signed_builtin_release",
                blake3::hash(P07_SIGNED_BUILTIN_RELEASE_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 34 && target >= 34 {
        transaction.execute_batch(P07_BUILTIN_CORE_HANDLER_DISPATCH_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 33 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 BUILTIN CORE HANDLER DISPATCH MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                34_i64,
                "p07_builtin_core_handler_dispatch",
                blake3::hash(P07_BUILTIN_CORE_HANDLER_DISPATCH_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 35 && target >= 35 {
        transaction.execute_batch(P07_TOOL_MUTATION_COMMAND_JOURNAL_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 34 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 TOOL MUTATION COMMAND JOURNAL MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                35_i64,
                "p07_tool_mutation_command_journal",
                blake3::hash(P07_TOOL_MUTATION_COMMAND_JOURNAL_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 36 && target >= 36 {
        transaction.execute_batch(P07_TOOL_MUTATION_COMMAND_CORRELATION_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 35 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 TOOL MUTATION COMMAND CORRELATION MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                36_i64,
                "p07_tool_mutation_command_correlation",
                blake3::hash(P07_TOOL_MUTATION_COMMAND_CORRELATION_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 37 && target >= 37 {
        transaction.execute_batch(P07_TOOL_CONFORMANCE_SANDBOX_DISPATCH_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 36 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 TOOL CONFORMANCE SANDBOX DISPATCH MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                37_i64,
                "p07_tool_conformance_sandbox_dispatch",
                blake3::hash(P07_TOOL_CONFORMANCE_SANDBOX_DISPATCH_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 38 && target >= 38 {
        transaction.execute_batch(P07_SANDBOX_AUTHORIZATION_ROOT_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 37 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 SANDBOX AUTHORIZATION ROOT MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                38_i64,
                "p07_sandbox_authorization_root",
                blake3::hash(P07_SANDBOX_AUTHORIZATION_ROOT_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 39 && target >= 39 {
        transaction.execute_batch(P07_TOOL_LONG_OPERATION_SCHEMA)?;
        migrate_p07_tool_long_operations_v1(&transaction)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 38 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID P07 TOOL LONG OPERATION MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                39_i64,
                "p07_tool_long_operation",
                blake3::hash(P07_TOOL_LONG_OPERATION_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 40 && target >= 40 {
        transaction.execute_batch(P07_TOOL_CREDENTIAL_AUTHORITY_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 39 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 TOOL CREDENTIAL AUTHORITY MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                40_i64,
                "p07_tool_credential_authority",
                blake3::hash(P07_TOOL_CREDENTIAL_AUTHORITY_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 41 && target >= 41 {
        transaction.execute_batch(P07_TOOL_DRAFT_SNAPSHOT_STAGING_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 40 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 TOOL DRAFT SNAPSHOT STAGING MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                41_i64,
                "p07_tool_draft_snapshot_staging",
                blake3::hash(P07_TOOL_DRAFT_SNAPSHOT_STAGING_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 42 && target >= 42 {
        transaction.execute_batch(P07_TOOL_DRAFT_RUNTIME_EPHEMERAL_ARTIFACT_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 41 {
            transaction.execute_batch(
                "THIS IS AN INJECTED INVALID P07 TOOL DRAFT RUNTIME EPHEMERAL ARTIFACT MIGRATION",
            )?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                42_i64,
                "p07_tool_draft_runtime_ephemeral_artifact",
                blake3::hash(P07_TOOL_DRAFT_RUNTIME_EPHEMERAL_ARTIFACT_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 43 && target >= 43 {
        transaction.execute_batch(DIRECTORY_GRANT_TERM_SCHEMA)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                43_i64,
                "directory_grant_term",
                blake3::hash(DIRECTORY_GRANT_TERM_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 44 && target >= 44 {
        transaction.execute_batch(DIRECTORY_GRANT_DECISION_SCHEMA)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                44_i64,
                "directory_grant_decision",
                blake3::hash(DIRECTORY_GRANT_DECISION_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 45 && target >= 45 {
        transaction.execute_batch(WORKSPACE_CONTENT_INDEX_SCHEMA)?;
        backfill_workspace_content_index(&transaction)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 44 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID WORKSPACE CONTENT INDEX MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                45_i64,
                "workspace_content_index",
                blake3::hash(WORKSPACE_CONTENT_INDEX_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 46 && target >= 46 {
        transaction.execute_batch(WORKSPACE_CATALOG_CHANGE_GENERATION_SCHEMA)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                46_i64,
                "workspace_catalog_change_generation",
                blake3::hash(WORKSPACE_CATALOG_CHANGE_GENERATION_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 47 && target >= 47 {
        transaction.execute_batch(WORKSPACE_CATALOG_INSTANCE_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 46 {
            transaction.execute_batch("THIS IS AN INJECTED INVALID CATALOG INSTANCE MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                47_i64,
                "workspace_catalog_instance",
                blake3::hash(WORKSPACE_CATALOG_INSTANCE_SCHEMA.as_bytes()).to_hex().to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    if current < 48 && target >= 48 {
        transaction.execute_batch(WORKSPACE_MUTATION_OWNERSHIP_EXECUTION_SCHEMA)?;
        if fault == Some(MigrationFaultPoint::DuringMigration) && current >= 47 {
            transaction
                .execute_batch("THIS IS AN INJECTED INVALID MUTATION OWNERSHIP MIGRATION")?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations(version, name, checksum, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            (
                48_i64,
                "workspace_mutation_ownership_execution",
                blake3::hash(WORKSPACE_MUTATION_OWNERSHIP_EXECUTION_SCHEMA.as_bytes())
                    .to_hex()
                    .to_string(),
                UtcTimestamp::now().to_string(),
            ),
        )?;
    }
    final_migrations::apply(&transaction, current, target, fault)?;
    transaction.commit()?;
    Ok(())
}

/// Fills the `(workspace, path) → FTS rowid` map from the text index an older database
/// already holds.
///
/// The map turns a per-path text delete from a full index scan into one primary-key
/// lookup, so every text row that is still correct has to end up in it. The hard part is
/// deciding which row *is* correct when a path has more than one: an older database can
/// hold both an old and a current generation of the same file, because the delete path
/// that was supposed to remove the old one scanned the whole index and could miss it.
/// Picking the lowest rowid — or the highest — would delete real text on a coin flip,
/// and the text index carries no digest of the file's bytes that could settle it.
///
/// So no row is chosen by position:
///
/// * **one row for the path**: the old delete-then-insert path kept at most one row per
///   path at a time, so a lone row is the current one and is adopted as-is;
/// * **more than one row**: the current one cannot be identified from the rowid or from
///   the text alone. Every row is dropped and the path is recorded as needing a rebuild,
///   so the text is re-derived from the file rather than guessed. A wrong guess here
///   would be invisible — the map would carry the catalog's digest over someone else's
///   text and report the path as indexed;
/// * **a path the entry table no longer holds**: the text is unreachable, so it goes.
///
/// The rebuild is not this migration's job: it re-reads files, and a migration may not
/// touch the filesystem. [`crate::workspace::PersistenceCoordinator::workspace_content_rebuild_paths`]
/// reports the marked paths, and `PathCatalog::open` re-reads exactly those.
fn backfill_workspace_content_index(transaction: &Transaction<'_>) -> Result<(), PersistenceError> {
    let rows = {
        let mut statement = transaction
            .prepare("SELECT rowid, workspace_id, relative_path FROM workspace_content_fts")?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut by_path = std::collections::BTreeMap::<(String, String), Vec<i64>>::new();
    let mut unaddressable = Vec::new();
    for (rowid, workspace_id, relative_path) in rows {
        if relative_path.is_empty() || workspace_id.is_empty() {
            unaddressable.push(rowid);
            continue;
        }
        by_path.entry((workspace_id, relative_path)).or_default().push(rowid);
    }
    for rowid in unaddressable {
        transaction.execute("DELETE FROM workspace_content_fts WHERE rowid=?1", [rowid])?;
    }

    let mut catalog = transaction.prepare(
        "SELECT 1 FROM workspace_catalog_entries WHERE workspace_id=?1 AND relative_path=?2",
    )?;
    let mut store = transaction.prepare(
        "INSERT INTO workspace_content_index(
            workspace_id, relative_path, fts_rowid, content_hash, needs_reindex
         ) VALUES (?1, ?2, ?3, NULL, ?4)",
    )?;
    for ((workspace_id, relative_path), rowids) in by_path {
        let known = catalog
            .query_row(params![workspace_id, relative_path], |_| Ok(()))
            .optional()?
            .is_some();
        if !known {
            for rowid in rowids {
                transaction.execute("DELETE FROM workspace_content_fts WHERE rowid=?1", [rowid])?;
            }
            continue;
        }
        if let [rowid] = rowids.as_slice() {
            store.execute(params![workspace_id, relative_path, rowid, 0])?;
            continue;
        }
        for rowid in rowids {
            transaction.execute("DELETE FROM workspace_content_fts WHERE rowid=?1", [rowid])?;
        }
        store.execute(params![workspace_id, relative_path, None::<i64>, 1])?;
    }
    drop(store);
    drop(catalog);
    Ok(())
}

fn migrate_p07_tool_long_operations_v1(
    transaction: &Transaction<'_>,
) -> Result<(), PersistenceError> {
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT journal_id, journal_kind, state, payload_json, created_at, updated_at
             FROM mutation_journals
             WHERE journal_kind IN ('p07.tool_package_publish.v1', 'p07.tool_package_delete.v1')
             ORDER BY created_at, journal_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (journal_id, journal_kind, journal_state, payload_json, created_at, updated_at) in rows {
        let journal_id = MutationJournalId::from_str(&journal_id)?;
        let payload = parse_json_strict(payload_json.as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let draft_pointer = if journal_kind == "p07.tool_package_publish.v1" {
            "/plan/draft"
        } else {
            "/plan/snapshot/draft"
        };
        let draft: ConformanceDraftRevisionRefV1 = serde_json::from_value(
            payload.pointer(draft_pointer).cloned().ok_or(PersistenceError::ToolEntityIntegrity)?,
        )?;
        let approval_evidence_hash: CanonicalSha256HashV1 = serde_json::from_value(
            payload
                .pointer("/plan/approval/evidence_hash")
                .cloned()
                .ok_or(PersistenceError::ToolEntityIntegrity)?,
        )?;
        let operation = if journal_kind == "p07.tool_package_publish.v1" {
            ToolLongOperationRefV1::DraftApply { tool_draft_id: draft.tool_draft_id, journal_id }
        } else {
            ToolLongOperationRefV1::PackageDelete {
                tool_package_id: serde_json::from_value::<ToolPackageId>(
                    payload
                        .pointer("/plan/snapshot/tool_package_id")
                        .cloned()
                        .ok_or(PersistenceError::ToolEntityIntegrity)?,
                )?,
                journal_id,
            }
        };
        let (state, operational_revision, boundary_crossed, started_at, completed_at) =
            match (journal_kind.as_str(), journal_state.as_str()) {
                (_, "completed") => (
                    ToolLongOperationStateV1::Succeeded,
                    4_i64,
                    true,
                    Some(created_at.as_str()),
                    Some(updated_at.as_str()),
                ),
                (_, "reconciliation_required") => (
                    ToolLongOperationStateV1::RecoveryRequired,
                    4_i64,
                    true,
                    Some(created_at.as_str()),
                    Some(updated_at.as_str()),
                ),
                (_, "aborted") => (
                    ToolLongOperationStateV1::Failed,
                    2_i64,
                    false,
                    Some(created_at.as_str()),
                    Some(updated_at.as_str()),
                ),
                ("p07.tool_package_publish.v1", "prepared") => {
                    (ToolLongOperationStateV1::Queued, 1_i64, false, None, None)
                }
                ("p07.tool_package_publish.v1", "durable_published" | "metadata_committed")
                | ("p07.tool_package_delete.v1", "deletion_pending") => (
                    ToolLongOperationStateV1::Running,
                    3_i64,
                    true,
                    Some(created_at.as_str()),
                    None,
                ),
                _ => (
                    ToolLongOperationStateV1::RecoveryRequired,
                    2_i64,
                    false,
                    Some(created_at.as_str()),
                    Some(updated_at.as_str()),
                ),
            };
        let operation_json = serde_json::to_string(&operation)?;
        let operation_hash =
            canonical_sha256(&operation).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let reservation_key = crate::migration_frozen::long_operation_reservation_key_v1(
            draft.tool_draft_id,
            draft.draft_revision,
            &approval_evidence_hash,
        );
        transaction.execute(
            "INSERT INTO tool_long_operations(
                journal_id, operation_json, operation_hash_json,
                approval_draft_id, approval_draft_revision,
                approval_evidence_hash_json, approval_reservation_key,
                state, operational_revision, boundary_crossed,
                safe_error_json, started_at, completed_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13)",
            rusqlite::params![
                journal_id.to_string(),
                operation_json,
                serde_json::to_string(&operation_hash)?,
                draft.tool_draft_id.to_string(),
                i64::try_from(draft.draft_revision.value())
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                serde_json::to_string(&approval_evidence_hash)?,
                reservation_key,
                crate::migration_frozen::long_operation_state_name(state),
                operational_revision,
                boundary_crossed,
                started_at,
                completed_at,
                updated_at,
            ],
        )?;
        if journal_kind == "p07.tool_package_publish.v1" && boundary_crossed {
            let changed = transaction.execute(
                "UPDATE tool_draft_approvals
                 SET consumed_by_journal_id=?1
                 WHERE tool_draft_id=?2 AND draft_revision=?3
                   AND consumed_by_journal_id IS NULL",
                rusqlite::params![
                    journal_id.to_string(),
                    draft.tool_draft_id.to_string(),
                    i64::try_from(draft.draft_revision.value())
                        .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
        }
    }
    Ok(())
}

fn migrate_p07_tool_instance_source_fingerprints_v2(
    transaction: &Transaction<'_>,
) -> Result<(), PersistenceError> {
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT i.tool_instance_id, i.record_json, d.record_json
             FROM tool_instances i
             JOIN tool_definitions d
               ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json
             ORDER BY i.tool_instance_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };

    for (instance_id, record_json, definition_json) in rows {
        let mut record = parse_json_strict(record_json.as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let definition = parse_json_strict(definition_json.as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let object = record.as_object_mut().ok_or(PersistenceError::ToolEntityIntegrity)?;
        let source_kind = object
            .get("source_kind")
            .and_then(serde_json::Value::as_str)
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        let existing = object
            .get("source_contract_fingerprint")
            .cloned()
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if existing.get("source_kind").is_some() {
            // A development database may already contain the V2 wire while
            // still carrying schema v26. Validate it through the current type
            // rather than wrapping it a second time.
            serde_json::from_value::<aura_tool_contract::ToolSourceContractFingerprintV1>(existing)
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            continue;
        }
        serde_json::from_value::<aura_domain_common::jcs::CanonicalHashV1>(existing.clone())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let migrated = match source_kind {
            "built_in" => serde_json::json!({
                "source_kind": "built_in",
                "signed_builtin_tool_contract_fingerprint": existing,
            }),
            "user_created" | "model_created" | "imported" => {
                let closure_hash = definition
                    .get("runtime_file_closure_hash")
                    .cloned()
                    .ok_or(PersistenceError::ToolEntityIntegrity)?;
                serde_json::from_value::<aura_domain_common::jcs::CanonicalHashV1>(
                    closure_hash.clone(),
                )
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                serde_json::json!({
                    "source_kind": source_kind,
                    "tool_contract_fingerprint": {
                        "package_runtime_closure_hash": closure_hash,
                        "fingerprint": existing,
                    }
                })
            }
            "mcp" => serde_json::json!({
                "source_kind": "mcp",
                "normalized_mcp_tool_contract_fingerprint": existing,
            }),
            "plugin" => serde_json::json!({
                "source_kind": "plugin",
                "normalized_plugin_tool_contract_fingerprint": existing,
            }),
            _ => return Err(PersistenceError::ToolEntityIntegrity),
        };
        serde_json::from_value::<aura_tool_contract::ToolSourceContractFingerprintV1>(
            migrated.clone(),
        )
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        object.insert("source_contract_fingerprint".to_owned(), migrated);
        update_migrated_canonical_record(
            transaction,
            "tool_instances",
            "tool_instance_id",
            &instance_id,
            &record,
        )?;
    }
    Ok(())
}

fn migrate_p04_security_sources_v2(
    transaction: &Transaction<'_>,
    migrated_at: &str,
) -> Result<(), PersistenceError> {
    migrate_permission_grant_sources_v2(transaction, migrated_at)?;
    migrate_approval_sources_v2(transaction, migrated_at)?;
    migrate_external_trust_sources_v2(transaction, migrated_at)?;
    migrate_turn_deny_sources_v2(transaction, migrated_at)?;
    migrate_sandbox_sources_v2(transaction, migrated_at)
}

fn migrate_permission_grant_sources_v2(
    transaction: &Transaction<'_>,
    migrated_at: &str,
) -> Result<(), PersistenceError> {
    let rows = migration_text_rows(
        transaction,
        "SELECT permission_grant_id, source_json, grant_json FROM permission_grants",
        3,
    )?;
    for row in rows {
        let record_key = &row[0];
        let source = parse_json_strict(row[1].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if !legacy_tool_security_source(&source) {
            continue;
        }
        let original_hash =
            canonical_sha256(&source).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let replacement = migration_isolation_source(&original_hash);
        let mut grant = parse_json_strict(row[2].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        replace_security_source_field(&mut grant, &replacement)?;
        if grant.get("revoked_at").is_some_and(serde_json::Value::is_null) {
            grant
                .as_object_mut()
                .ok_or(PersistenceError::ToolEntityIntegrity)?
                .insert("revoked_at".to_owned(), serde_json::Value::String(migrated_at.to_owned()));
        }
        transaction.execute(
            "UPDATE permission_grants
             SET grant_json=?1, source_json=?2, revoked_at=COALESCE(revoked_at, ?3)
             WHERE permission_grant_id=?4",
            (canonical_string(&grant)?, canonical_string(&replacement)?, migrated_at, record_key),
        )?;
        insert_security_source_migration_ledger(
            transaction,
            "permission_grants",
            record_key,
            &original_hash,
            "grant_revoked",
            migrated_at,
        )?;
    }
    Ok(())
}

fn migrate_approval_sources_v2(
    transaction: &Transaction<'_>,
    migrated_at: &str,
) -> Result<(), PersistenceError> {
    let rows = migration_text_rows(
        transaction,
        "SELECT approval_request_id, source_json, request_json, state FROM approval_requests",
        4,
    )?;
    for row in rows {
        let record_key = &row[0];
        let source = parse_json_strict(row[1].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if !legacy_tool_security_source(&source) {
            continue;
        }
        let original_hash =
            canonical_sha256(&source).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let replacement = migration_isolation_source(&original_hash);
        let mut request = parse_json_strict(row[2].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        replace_security_source_field(&mut request, &replacement)?;
        let was_pending = row[3] == "pending";
        if was_pending {
            let object = request.as_object_mut().ok_or(PersistenceError::ToolEntityIntegrity)?;
            let revision = object
                .get("request_revision")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| value.checked_add(1))
                .ok_or(PersistenceError::ToolEntityIntegrity)?;
            object.insert("request_revision".to_owned(), serde_json::json!(revision));
            object.insert("state".to_owned(), serde_json::Value::String("invalidated".to_owned()));
            object.insert(
                "resolved_at".to_owned(),
                serde_json::Value::String(migrated_at.to_owned()),
            );
            object.insert(
                "invalidation_reason".to_owned(),
                serde_json::Value::String("p07_security_source_v1_migration_unresolved".to_owned()),
            );
        }
        transaction.execute(
            "UPDATE approval_requests
             SET request_json=?1, source_json=?2,
                 state=CASE WHEN state='pending' THEN 'invalidated' ELSE state END,
                 request_revision=CASE WHEN state='pending' THEN request_revision+1 ELSE request_revision END,
                 resolved_at=CASE WHEN state='pending' THEN ?3 ELSE resolved_at END,
                 invalidation_reason=CASE WHEN state='pending'
                     THEN 'p07_security_source_v1_migration_unresolved'
                     ELSE invalidation_reason END
             WHERE approval_request_id=?4",
            (
                canonical_string(&request)?,
                canonical_string(&replacement)?,
                migrated_at,
                record_key,
            ),
        )?;
        insert_security_source_migration_ledger(
            transaction,
            "approval_requests",
            record_key,
            &original_hash,
            if was_pending { "approval_invalidated" } else { "deny_isolated" },
            migrated_at,
        )?;
    }
    Ok(())
}

fn migrate_external_trust_sources_v2(
    transaction: &Transaction<'_>,
    migrated_at: &str,
) -> Result<(), PersistenceError> {
    let rows = migration_text_rows(
        transaction,
        "SELECT trust_record_id, source_json, record_json FROM external_trust_records",
        3,
    )?;
    for row in rows {
        let record_key = &row[0];
        let source = parse_json_strict(row[1].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if !legacy_tool_security_source(&source) {
            continue;
        }
        let original_hash =
            canonical_sha256(&source).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let replacement = migration_isolation_source(&original_hash);
        let mut record = parse_json_strict(row[2].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        replace_security_source_field(&mut record, &replacement)?;
        let object = record.as_object_mut().ok_or(PersistenceError::ToolEntityIntegrity)?;
        object.insert("state".to_owned(), serde_json::json!({"trust_state": "revoked"}));
        object.insert("effective_fingerprint".to_owned(), serde_json::Value::Null);
        object.insert("updated_at".to_owned(), serde_json::Value::String(migrated_at.to_owned()));
        let source_key = migration_isolation_source_key(&original_hash);
        transaction.execute(
            "UPDATE external_trust_records
             SET record_json=?1, source_key=?2, source_json=?3,
                 trust_state_json=?4, effective_fingerprint_json=NULL, updated_at=?5
             WHERE trust_record_id=?6",
            (
                canonical_string(&record)?,
                source_key,
                canonical_string(&replacement)?,
                canonical_string(&serde_json::json!({"trust_state": "revoked"}))?,
                migrated_at,
                record_key,
            ),
        )?;
        insert_security_source_migration_ledger(
            transaction,
            "external_trust_records",
            record_key,
            &original_hash,
            "trust_revoked",
            migrated_at,
        )?;
    }
    Ok(())
}

fn migrate_turn_deny_sources_v2(
    transaction: &Transaction<'_>,
    migrated_at: &str,
) -> Result<(), PersistenceError> {
    let rows = migration_text_rows(
        transaction,
        "SELECT turn_task_id || ':' || source_key || ':' || operation_fingerprint_json,
                turn_task_id, source_key, operation_fingerprint_json, source_json
         FROM turn_deny_records",
        5,
    )?;
    for row in rows {
        let record_key = &row[0];
        let source = parse_json_strict(row[4].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        if !legacy_tool_security_source(&source) {
            continue;
        }
        let original_hash =
            canonical_sha256(&source).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let replacement = migration_isolation_source(&original_hash);
        transaction.execute(
            "UPDATE turn_deny_records SET source_key=?1, source_json=?2
             WHERE turn_task_id=?3 AND source_key=?4 AND operation_fingerprint_json=?5",
            (
                migration_isolation_source_key(&original_hash),
                canonical_string(&replacement)?,
                &row[1],
                &row[2],
                &row[3],
            ),
        )?;
        insert_security_source_migration_ledger(
            transaction,
            "turn_deny_records",
            record_key,
            &original_hash,
            "deny_isolated",
            migrated_at,
        )?;
    }
    Ok(())
}

fn migrate_sandbox_sources_v2(
    transaction: &Transaction<'_>,
    migrated_at: &str,
) -> Result<(), PersistenceError> {
    let rows = migration_text_rows(
        transaction,
        "SELECT sandbox_execution_id, plan_json, state FROM sandbox_executions",
        3,
    )?;
    for row in rows {
        let record_key = &row[0];
        let mut plan = parse_json_strict(row[1].as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let source = plan.get("source").cloned().ok_or(PersistenceError::ToolEntityIntegrity)?;
        if !legacy_tool_security_source(&source) {
            continue;
        }
        let original_hash =
            canonical_sha256(&source).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        replace_security_source_field(&mut plan, &migration_isolation_source(&original_hash))?;
        let terminal = matches!(
            row[2].as_str(),
            "exited"
                | "cancelled"
                | "timed_out"
                | "policy_violation"
                | "cleanup_failed"
                | "state_unknown"
        );
        transaction.execute(
            "UPDATE sandbox_executions SET plan_json=?1,
                 state=CASE WHEN ?2 THEN state ELSE 'state_unknown' END,
                 updated_at=CASE WHEN ?2 THEN updated_at ELSE ?3 END,
                 terminal_at=CASE WHEN ?2 THEN terminal_at ELSE ?3 END
             WHERE sandbox_execution_id=?4",
            (canonical_string(&plan)?, terminal, migrated_at, record_key),
        )?;
        insert_security_source_migration_ledger(
            transaction,
            "sandbox_executions",
            record_key,
            &original_hash,
            "sandbox_isolated",
            migrated_at,
        )?;
    }
    Ok(())
}

fn migration_text_rows(
    transaction: &Transaction<'_>,
    query: &str,
    columns: usize,
) -> Result<Vec<Vec<String>>, PersistenceError> {
    let mut statement = transaction.prepare(query)?;
    statement
        .query_map([], |row| {
            (0..columns).map(|index| row.get::<_, String>(index)).collect::<Result<Vec<_>, _>>()
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(PersistenceError::from)
}

fn legacy_tool_security_source(source: &serde_json::Value) -> bool {
    source
        .get("source_kind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| matches!(kind, "built_in_tool" | "custom_tool"))
}

fn migration_isolation_source(
    original_hash: &aura_domain_common::jcs::CanonicalHashV1,
) -> serde_json::Value {
    serde_json::json!({
        "source_kind": "system_component",
        "component_id": format!("p07.security_source_v1_unresolved.{}", original_hash.hex()),
    })
}

fn migration_isolation_source_key(
    original_hash: &aura_domain_common::jcs::CanonicalHashV1,
) -> String {
    format!("system:p07.security_source_v1_unresolved.{}", original_hash.hex())
}

fn replace_security_source_field(
    record: &mut serde_json::Value,
    replacement: &serde_json::Value,
) -> Result<(), PersistenceError> {
    let object = record.as_object_mut().ok_or(PersistenceError::ToolEntityIntegrity)?;
    if !object.contains_key("source") {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    object.insert("source".to_owned(), replacement.clone());
    Ok(())
}

fn insert_security_source_migration_ledger(
    transaction: &Transaction<'_>,
    table_name: &str,
    record_key: &str,
    original_hash: &aura_domain_common::jcs::CanonicalHashV1,
    disposition: &str,
    migrated_at: &str,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO p07_security_source_v1_migration_ledger(
            table_name, record_key, original_source_hash_json, disposition, migrated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        (table_name, record_key, canonical_string(original_hash)?, disposition, migrated_at),
    )?;
    Ok(())
}

fn canonical_string<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, PersistenceError> {
    String::from_utf8(canonical_json_jcs(value).map_err(|_| PersistenceError::ToolEntityIntegrity)?)
        .map_err(|_| PersistenceError::ToolEntityIntegrity)
}

fn update_migrated_canonical_record(
    transaction: &Transaction<'_>,
    table: &str,
    id_column: &str,
    id: &str,
    record: &serde_json::Value,
) -> Result<(), PersistenceError> {
    let record_json = canonical_string(record)?;
    let record_hash_json = canonical_string(
        &canonical_sha256(record).map_err(|_| PersistenceError::ToolEntityIntegrity)?,
    )?;
    let query =
        format!("UPDATE {table} SET record_json=?1, record_hash_json=?2 WHERE {id_column}=?3");
    let changed = transaction.execute(&query, (&record_json, &record_hash_json, id))?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn migrate_p07_w5_record_kinds(transaction: &Transaction<'_>) -> Result<(), PersistenceError> {
    rewrite_canonical_record_field(
        transaction,
        "tool_drafts",
        "tool_draft_id",
        "kind",
        serde_json::Value::String("content_mutation".to_owned()),
    )?;
    rewrite_canonical_record_field(
        transaction,
        "tool_packages",
        "tool_package_id",
        "deletion_pending",
        serde_json::Value::Bool(false),
    )
}

fn rewrite_canonical_record_field(
    transaction: &Transaction<'_>,
    table: &str,
    id_column: &str,
    field: &str,
    value: serde_json::Value,
) -> Result<(), PersistenceError> {
    // Table and column names are compile-time migration constants, never user
    // input. Values remain bound parameters.
    let query = format!("SELECT {id_column}, record_json FROM {table}");
    let rows = {
        let mut statement = transaction.prepare(&query)?;
        statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let update =
        format!("UPDATE {table} SET record_json=?1, record_hash_json=?2 WHERE {id_column}=?3");
    for (id, encoded) in rows {
        let mut document = parse_json_strict(encoded.as_bytes())
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let object = document.as_object_mut().ok_or(PersistenceError::ToolEntityIntegrity)?;
        if object.insert(field.to_owned(), value.clone()).is_some() {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let record_json = String::from_utf8(
            canonical_json_jcs(&document).map_err(|_| PersistenceError::ToolEntityIntegrity)?,
        )
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let record_hash =
            canonical_sha256(&document).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let record_hash_json = String::from_utf8(
            canonical_json_jcs(&record_hash).map_err(|_| PersistenceError::ToolEntityIntegrity)?,
        )
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        transaction.execute(&update, (&record_json, &record_hash_json, &id))?;
    }
    Ok(())
}

fn repair_tool_call_definition_reference_v1(
    transaction: &Transaction<'_>,
) -> Result<(), PersistenceError> {
    let has_definition_foreign_key = {
        let mut statement = transaction.prepare("PRAGMA foreign_key_list(tool_calls)")?;
        let foreign_tables = statement
            .query_map([], |row| row.get::<_, String>(2))?
            .collect::<Result<Vec<_>, _>>()?;
        foreign_tables.iter().any(|table| table == "tool_definitions")
    };
    if has_definition_foreign_key {
        transaction.execute_batch(P07_TOOL_CALL_REFERENCE_REPAIR_SCHEMA)?;
    }
    Ok(())
}

fn migrate_provider_catalog_cache_v2(
    transaction: &Transaction<'_>,
) -> Result<(), PersistenceError> {
    let snapshot_columns = migration_table_columns(transaction, "provider_catalog_snapshots")?;
    let runtime_columns = migration_table_columns(transaction, "provider_catalog_runtime_state")?;
    let current_shape = [
        "catalog_revision",
        "catalog_content_hash",
        "checked_at",
        "last_successful_refresh_at",
        "etag",
        "last_modified",
        "raw_artifact_id",
        "snapshot_json",
        "created_at",
    ]
    .iter()
    .all(|column| snapshot_columns.iter().any(|actual| actual == column))
        && ["catalog_key", "current_revision", "last_error_code", "checked_at", "updated_at"]
            .iter()
            .all(|column| runtime_columns.iter().any(|actual| actual == column));
    if current_shape {
        return Ok(());
    }

    let legacy_shape = [
        "catalog_revision",
        "fetched_at",
        "etag",
        "last_modified",
        "raw_artifact_id",
        "snapshot_json",
        "created_at",
    ]
    .iter()
    .all(|column| snapshot_columns.iter().any(|actual| actual == column))
        && ["catalog_key", "current_revision", "last_error", "updated_at"]
            .iter()
            .all(|column| runtime_columns.iter().any(|actual| actual == column));
    if !legacy_shape {
        return Err(PersistenceError::InvalidRequest(
            "provider catalog cache schema is neither the frozen legacy nor current shape"
                .to_owned(),
        ));
    }

    // The legacy cache used a content hash as its catalog identity, while the
    // current contract uses an operational Revision plus a separately encoded
    // ContentHash. It is a rebuildable SystemCache, so carrying legacy rows
    // forward would manufacture semantics that the old records never stored.
    transaction.execute_batch(P05_PROVIDER_CATALOG_CACHE_V2_RESET_SCHEMA)?;
    Ok(())
}

fn migration_table_columns(
    transaction: &Transaction<'_>,
    table: &'static str,
) -> Result<Vec<String>, PersistenceError> {
    let mut statement = transaction.prepare(&format!("PRAGMA table_info({table})"))?;
    statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(PersistenceError::from)
}

fn create_backup(
    source: &Connection,
    database_path: &Path,
    current_version: i64,
) -> Result<PathBuf, PersistenceError> {
    let parent = database_path.parent().expect("database parent was validated");
    let directory = parent.join("migration-backups");
    fs::create_dir_all(&directory).map_err(|source| PersistenceError::Io {
        action: "create migration backup directory",
        path: directory.clone(),
        source,
    })?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let path =
        directory.join(format!("schema-{current_version}-to-{LATEST_SCHEMA_VERSION}-{stamp}.db"));
    let mut destination = Connection::open(&path)?;
    let backup = Backup::new(source, &mut destination)?;
    backup.run_to_completion(64, Duration::from_millis(5), None)?;
    drop(backup);
    destination.close().map_err(|(_, error)| error)?;
    Ok(path)
}

fn recovery_after_failure(
    database_path: &Path,
    backup_path: Option<PathBuf>,
    reason: String,
) -> Result<MigrationResult, PersistenceError> {
    if let Some(path) = backup_path.as_ref() {
        restore_backup(path, database_path)?;
    }
    Ok(MigrationResult::RecoveryRequired(MigrationRecoveryRequired {
        database_path: database_path.to_path_buf(),
        backup_path,
        reason,
    }))
}

fn restore_backup(backup_path: &Path, database_path: &Path) -> Result<(), PersistenceError> {
    let source = Connection::open_with_flags(
        backup_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut destination = Connection::open(database_path)?;
    let backup = Backup::new(&source, &mut destination)?;
    backup.run_to_completion(64, Duration::from_millis(5), None)?;
    drop(backup);
    destination.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use aura_domain_common::{
        AgentRunId, ArtifactId, ContentHash, ContextProjectionSnapshotId, CorrelationId,
        EndpointId, EventId, ModelDiscoveryRunId, ModelFailureId, ModelProfileId, ModelStepId,
        NetworkRequestId, PromptBundleSnapshotId, Revision, SessionId, TransportAttemptId,
        TurnTaskId, UtcTimestamp, WorkspaceId,
    };
    use aura_model_domain::{
        FailureFingerprint, ModelAdapterIdentity, ModelApiFormat, ModelDiscoveryFailedPayload,
        ModelDiscoveryOutcome, ModelDiscoveryRunRecord, ModelDiscoveryStage,
        ModelDiscoveryStartedPayload, ModelDiscoveryState, ModelFailureCategory, ModelRetryClass,
        NormalizedModelFailure, ProviderRequestEvidence, ProviderRequestTargetMetadata,
        ProviderTransportAttemptStartedPayload, TransportAttemptKind, TransportAttemptRecord,
        TransportAttemptState, TransportOperationContext,
    };
    use aura_permission_domain::NormalizedOrigin;
    use rusqlite::{Connection, OptionalExtension, params};
    use tempfile::tempdir;

    use crate::legacy_configuration_transport::{
        LEGACY_CONFIGURATION_TRANSPORT_MIGRATION_KEY,
        load_configuration_transport_recovery_snapshot, validate_clean_p05_acceptance_profile,
    };

    use super::{
        DatabaseOpenOptions, LATEST_SCHEMA_VERSION, MigrationFaultPoint, MigrationResult,
        apply_migrations_through, canonical_string, open_and_migrate,
    };

    fn seed_database_at_version(path: &std::path::Path, version: i64) {
        let connection = Connection::open(path).expect("seed database");
        apply_migrations_through(&connection, 0, version, None)
            .expect("seed complete historical schema");
    }

    #[derive(Clone)]
    struct LegacyFixtureIds {
        attempt_id: TransportAttemptId,
        discovery_run_id: ModelDiscoveryRunId,
        discovery_failed_event_id: EventId,
    }

    fn seed_legacy_configuration_transport_fixture(connection: &Connection) -> LegacyFixtureIds {
        let endpoint_id = EndpointId::new();
        let discovery_run_id = ModelDiscoveryRunId::new();
        let attempt_id = TransportAttemptId::new();
        let failure_id = ModelFailureId::new();
        let correlation_id = CorrelationId::new();
        let started_at = UtcTimestamp::now();
        let completed_at = UtcTimestamp::now();
        let endpoint_fingerprint = ContentHash::sha256(b"legacy-fixture-endpoint");
        let failure = NormalizedModelFailure {
            category: ModelFailureCategory::Transport,
            code: "transport.failed".to_owned(),
            message: "Model discovery failed before a complete valid model catalog was committed."
                .to_owned(),
            retry_class: ModelRetryClass::NotRetryable,
            provider_status: None,
            provider_request_id: None,
            provider_error_ref: None,
            partial_output_ref: None,
            transport_attempt_id: None,
        };
        connection
            .execute(
                "INSERT INTO model_failures(
                    model_failure_id, failure_fingerprint_json, failure_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    failure_id.to_string(),
                    serde_json::to_string(&FailureFingerprint(ContentHash::sha256(
                        b"legacy-fixture-failure"
                    )))
                    .expect("failure fingerprint"),
                    serde_json::to_string(&failure).expect("failure json"),
                    completed_at.to_string(),
                ],
            )
            .expect("seed legacy failure");

        let discovery = ModelDiscoveryRunRecord {
            id: discovery_run_id,
            correlation_id,
            endpoint_id,
            source_models_revision: Revision::INITIAL,
            endpoint_revision_at_start: Revision::INITIAL,
            endpoint_fingerprint: endpoint_fingerprint.clone(),
            revision: Revision::new(3),
            state: ModelDiscoveryState::Completed,
            stage: Some(ModelDiscoveryStage::Completed),
            outcome: Some(ModelDiscoveryOutcome::Failed),
            cancellation_reason: None,
            transport_attempt_ids: Vec::new(),
            result: None,
            failure_id: Some(failure_id),
            provider_error: None,
            raw_response_artifact_id: None,
            started_at,
            finished_at: Some(completed_at),
        };
        connection
            .execute(
                "INSERT INTO model_discovery_runs(
                    discovery_run_id, endpoint_id, correlation_id, source_models_revision,
                    endpoint_fingerprint_json, revision, state, record_json,
                    raw_response_artifact_id, started_at, finished_at
                 ) VALUES (?1, ?2, ?3, 1, ?4, 3, 'completed', ?5, NULL, ?6, ?7)",
                params![
                    discovery_run_id.to_string(),
                    endpoint_id.to_string(),
                    correlation_id.to_string(),
                    serde_json::to_string(&endpoint_fingerprint).expect("fingerprint json"),
                    serde_json::to_string(&discovery).expect("discovery json"),
                    started_at.to_string(),
                    completed_at.to_string(),
                ],
            )
            .expect("seed legacy discovery");

        let operation_context = TransportOperationContext::ModelDiscovery { discovery_run_id };
        let attempt = TransportAttemptRecord {
            id: attempt_id,
            attempt_number: 1,
            attempt_kind: TransportAttemptKind::Initial,
            client_request_id: NetworkRequestId::new().to_string(),
            provider_request_id: None,
            endpoint_id,
            endpoint_fingerprint,
            request_evidence: ProviderRequestEvidence {
                body_sha256: ContentHash::sha256(b""),
                body_byte_length: 0,
                target: ProviderRequestTargetMetadata {
                    method: "GET".to_owned(),
                    origin: NormalizedOrigin {
                        scheme: "https".to_owned(),
                        host: "api.example.test".to_owned(),
                        port: 443,
                    },
                    path: "/models".to_owned(),
                    query_parameter_names: BTreeSet::new(),
                },
                headers: Vec::new(),
                compatibility_removals: BTreeSet::new(),
                applied_output_token_field_alias: None,
                adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses),
                operation_context: operation_context.clone(),
                tool_registry_snapshot_ref: None,
                tool_name_bindings: Vec::new(),
                historical_tool_call_wire_bindings: Vec::new(),
                canonical_body_artifact_id: None,
            },
            state: TransportAttemptState::AuthorizingNetwork,
            started_at,
            first_byte_at: None,
            first_model_event_at: None,
            finished_at: None,
            raw_stream_artifact_id: None,
            failure_id: None,
        };
        connection
            .execute(
                "INSERT INTO transport_attempts(
                    transport_attempt_id, endpoint_id, operation_context_json,
                    attempt_number, attempt_kind, state, record_json,
                    canonical_body_artifact_id, raw_stream_artifact_id, model_failure_id,
                    started_at, finished_at
                 ) VALUES (?1, ?2, ?3, 1, 'initial', 'authorizing_network', ?4,
                           NULL, NULL, NULL, ?5, NULL)",
                params![
                    attempt_id.to_string(),
                    endpoint_id.to_string(),
                    serde_json::to_string(&operation_context).expect("operation context"),
                    serde_json::to_string(&attempt).expect("attempt json"),
                    started_at.to_string(),
                ],
            )
            .expect("seed legacy attempt");

        let discovery_started_event_id = EventId::new();
        let attempt_started_event_id = EventId::new();
        let discovery_failed_event_id = EventId::new();
        for (event_id, event_type, payload) in [
            (
                discovery_started_event_id,
                "ModelDiscoveryStarted",
                serde_json::to_value(ModelDiscoveryStartedPayload {
                    discovery_run_id,
                    endpoint_id,
                    run_revision: Revision::INITIAL,
                })
                .expect("discovery started payload"),
            ),
            (
                attempt_started_event_id,
                "ProviderTransportAttemptStarted",
                serde_json::to_value(ProviderTransportAttemptStartedPayload {
                    transport_attempt_id: attempt_id,
                    operation_context,
                    endpoint_id,
                    attempt_number: 1,
                    attempt_kind: TransportAttemptKind::Initial,
                })
                .expect("attempt started payload"),
            ),
            (
                discovery_failed_event_id,
                "ModelDiscoveryFailed",
                serde_json::to_value(ModelDiscoveryFailedPayload {
                    discovery_run_id,
                    endpoint_id,
                    failure_id,
                    run_revision: Revision::new(3),
                })
                .expect("discovery failed payload"),
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO events(
                        event_id, event_type, event_version, actor, correlation_id,
                        payload_json, created_at
                     ) VALUES (?1, ?2, 1, 'system_runtime', ?3, ?4, ?5)",
                    params![
                        event_id.to_string(),
                        event_type,
                        correlation_id.to_string(),
                        serde_json::to_string(&payload).expect("event payload json"),
                        completed_at.to_string(),
                    ],
                )
                .expect("seed legacy event");
        }
        LegacyFixtureIds { attempt_id, discovery_run_id, discovery_failed_event_id }
    }

    #[test]
    fn legacy_configuration_transport_migration_isolates_without_rewriting_history() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 14);
        let connection = Connection::open(&path).expect("open version fourteen database");
        let fixture = seed_legacy_configuration_transport_fixture(&connection);
        let original_attempt_json = connection
            .query_row(
                "SELECT record_json FROM transport_attempts WHERE transport_attempt_id=?1",
                [fixture.attempt_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("original attempt json");
        let original_discovery_json = connection
            .query_row(
                "SELECT record_json FROM model_discovery_runs WHERE discovery_run_id=?1",
                [fixture.discovery_run_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("original discovery json");
        let original_event_count = connection
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get::<_, i64>(0))
            .expect("original event count");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("legacy fixture must migrate to the current schema");
        };
        let ledger = connection
            .query_row(
                "SELECT migration_key, resolution, discovery_run_id,
                        discovery_failed_event_id, original_state, source_schema_version,
                        length(canonical_evidence_hash)
                 FROM p05_legacy_configuration_transport_reconciliation
                 WHERE transport_attempt_id=?1",
                [fixture.attempt_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .expect("legacy ledger row");
        assert_eq!(ledger.0, LEGACY_CONFIGURATION_TRANSPORT_MIGRATION_KEY);
        assert_eq!(ledger.1, "legacy_indeterminate");
        assert_eq!(ledger.2, fixture.discovery_run_id.to_string());
        assert_eq!(ledger.3, fixture.discovery_failed_event_id.to_string());
        assert_eq!(ledger.4, "authorizing_network");
        assert_eq!(ledger.5, 14);
        assert_eq!(ledger.6, 64);
        assert_eq!(
            connection
                .query_row(
                    "SELECT record_json FROM transport_attempts WHERE transport_attempt_id=?1",
                    [fixture.attempt_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .expect("migrated attempt json"),
            original_attempt_json
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT record_json FROM model_discovery_runs WHERE discovery_run_id=?1",
                    [fixture.discovery_run_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .expect("migrated discovery json"),
            original_discovery_json
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM events", [], |row| row.get::<_, i64>(0))
                .expect("migrated event count"),
            original_event_count
        );
        let snapshot = load_configuration_transport_recovery_snapshot(&connection)
            .expect("validated recovery snapshot");
        assert!(snapshot.runtime_safety_blockers.is_empty());
        assert_eq!(snapshot.historical_integrity_degradations.len(), 1);
        let acceptance_error = validate_clean_p05_acceptance_profile(&connection)
            .expect_err("legacy ledger is forbidden in the clean acceptance profile");
        assert!(acceptance_error.to_string().contains("legacy_ledger_present"));

        // A record created after the migration has no ledger authority. Even
        // if its raw shape resembles the historical defect, it remains a
        // RuntimeSafetyBlocker and the one-time migration never scans it.
        let future_fixture = seed_legacy_configuration_transport_fixture(&connection);
        drop(connection);
        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("repeat open result");
        let MigrationResult::Ready(connection) = result else {
            panic!("current schema must reopen without migration");
        };
        let snapshot = load_configuration_transport_recovery_snapshot(&connection)
            .expect("post-migration recovery snapshot");
        assert_eq!(snapshot.historical_integrity_degradations.len(), 1);
        assert_eq!(snapshot.runtime_safety_blockers.len(), 1);
        assert_eq!(snapshot.runtime_safety_blockers[0].id, future_fixture.attempt_id);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM p05_legacy_configuration_transport_reconciliation",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("ledger row count"),
            1
        );
    }

    #[test]
    fn legacy_configuration_transport_ready_gate_rejects_evidence_drift() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 14);
        let connection = Connection::open(&path).expect("open version fourteen database");
        let fixture = seed_legacy_configuration_transport_fixture(&connection);
        drop(connection);
        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("legacy fixture must migrate to the current schema");
        };
        connection
            .execute(
                "UPDATE events SET actor='user' WHERE event_id=?1",
                [fixture.discovery_failed_event_id.to_string()],
            )
            .expect("inject evidence drift");
        let error = load_configuration_transport_recovery_snapshot(&connection)
            .expect_err("evidence drift must remain a runtime safety blocker");
        assert!(error.to_string().contains("evidence no longer matches"));
    }

    #[test]
    fn context_v2_migration_preserves_historical_v1_snapshot_bytes_without_backfill() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 18);
        let connection = Connection::open(&path).expect("open version eighteen database");
        connection.execute_batch("PRAGMA foreign_keys=OFF;").expect("disable fixture foreign keys");
        let context_snapshot_id = ContextProjectionSnapshotId::new();
        let snapshot_hash = ContentHash::sha256(b"historical-cps-v1");
        let legacy_snapshot_json =
            r#"{"legacy_snapshot_version":1,"tool_schemas":[],"verbatim":"preserve me"}"#;
        connection
            .execute(
                "INSERT INTO context_projection_snapshots(
                    context_snapshot_id, session_id, turn_task_id, agent_run_id, model_step_id,
                    prompt_bundle_snapshot_id, model_profile_id, runtime_effective_window_tokens,
                    output_reserve_tokens, safety_reserve_tokens, request_input_budget_tokens,
                    estimated_input_tokens, history_cutoff, instruction_envelope_artifact_id,
                    instruction_envelope_hash_json, model_egress_policy_version,
                    model_egress_manifest_artifact_id, model_egress_manifest_hash_json,
                    budget_json, output_request_json, context_usage_json, active_task_lineage_id,
                    snapshot_hash_json, snapshot_json, created_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, 32768, 4096, 4096, 24576, 128, 1,
                    ?8, ?9, 1, ?10, ?11, '{}', '{}', '{}', NULL, ?12, ?13, ?14
                 )",
                params![
                    context_snapshot_id.to_string(),
                    SessionId::new().to_string(),
                    TurnTaskId::new().to_string(),
                    AgentRunId::new().to_string(),
                    ModelStepId::new().to_string(),
                    PromptBundleSnapshotId::new().to_string(),
                    ModelProfileId::new().to_string(),
                    ArtifactId::new().to_string(),
                    serde_json::to_string(&ContentHash::sha256(b"instruction"))
                        .expect("instruction hash"),
                    ArtifactId::new().to_string(),
                    serde_json::to_string(&ContentHash::sha256(b"egress")).expect("egress hash"),
                    serde_json::to_string(&snapshot_hash).expect("snapshot hash"),
                    legacy_snapshot_json,
                    UtcTimestamp::now().to_string(),
                ],
            )
            .expect("seed historical cps v1");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version eighteen database must migrate to the current schema");
        };
        let (snapshot_version, snapshot_json) = connection
            .query_row(
                "SELECT snapshot_version, snapshot_json
                 FROM context_projection_snapshots WHERE context_snapshot_id=?1",
                [context_snapshot_id.to_string()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("migrated historical cps");
        assert_eq!(snapshot_version, 1);
        assert_eq!(snapshot_json, legacy_snapshot_json);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM context_projection_tool_registry_v2
                     WHERE context_snapshot_id=?1",
                    [context_snapshot_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .expect("v2 child row count"),
            0
        );
    }

    #[test]
    fn migration_failure_restores_the_backup_and_requires_recovery() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        let connection = Connection::open(&path).expect("seed database");
        connection
            .execute_batch("CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('ok');")
            .expect("seed sentinel");
        drop(connection);

        let outcome = open_and_migrate(
            &path,
            DatabaseOpenOptions { migration_fault: Some(MigrationFaultPoint::DuringMigration) },
        )
        .expect("migration recovery outcome");
        assert!(matches!(outcome, MigrationResult::RecoveryRequired(_)));
        let restored = Connection::open(&path).expect("restored database");
        assert_eq!(
            restored
                .query_row("SELECT value FROM sentinel", [], |row| row.get::<_, String>(0))
                .expect("sentinel value"),
            "ok"
        );
        assert!(
            restored
                .query_row("SELECT name FROM sqlite_master WHERE name='workspaces'", [], |row| {
                    row.get::<_, String>(0)
                })
                .optional()
                .expect("schema query")
                .is_none()
        );
    }

    #[test]
    fn v38_rebinds_sandbox_execution_to_the_closed_authorization_root() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 37);

        let result = open_and_migrate(&path, DatabaseOpenOptions::default())
            .expect("authorization-root migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version thirty-seven database must migrate to the authorization root");
        };
        let foreign_table = connection
            .query_row("PRAGMA foreign_key_list('sandbox_executions')", [], |row| {
                row.get::<_, String>(2)
            })
            .expect("sandbox execution foreign key");
        assert_eq!(foreign_table, "authorization_snapshots");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type='trigger'
                       AND name='permission_snapshots_authorization_root_insert'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("permission authorization-root trigger"),
            1
        );
        connection
            .execute(
                "INSERT INTO authorization_snapshots(
                    authorization_snapshot_id, authorization_kind, created_at
                 ) VALUES ('conformance-auth', 'tool_conformance_allow_once',
                           '2026-09-06T00:00:00Z')",
                [],
            )
            .expect("insert dedicated Conformance authorization root");
        connection
            .execute(
                "INSERT INTO sandbox_executions(
                    sandbox_execution_id, identity_json, permission_snapshot_id,
                    mode, plan_hash_json, plan_json, state, process_id, exit_code,
                    created_at, updated_at, terminal_at
                 ) VALUES (
                    'conformance-sandbox', '{}', 'conformance-auth', 'strict_app_container',
                    '{}', '{}', 'planned', NULL, NULL,
                    '2026-09-06T00:00:00Z', '2026-09-06T00:00:00Z', NULL
                 )",
                [],
            )
            .expect("sandbox execution accepts a non-Session Conformance authorization");
    }

    #[test]
    fn v38_authorization_root_migration_is_atomic_on_fault() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 37);

        let outcome = open_and_migrate(
            &path,
            DatabaseOpenOptions { migration_fault: Some(MigrationFaultPoint::DuringMigration) },
        )
        .expect("authorization-root fault outcome");
        assert!(matches!(outcome, MigrationResult::RecoveryRequired(_)));
        let restored = Connection::open(&path).expect("restored version thirty-seven database");
        assert_eq!(
            restored
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("restored schema version"),
            37
        );
        assert_eq!(
            restored
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type='table' AND name='authorization_snapshots'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("authorization root absence"),
            0
        );
    }

    #[test]
    fn v39_adds_long_operation_authority_and_apply_approval_consumption_atomically() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 38);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("v39 migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version thirty-eight database must migrate to long-operation authority");
        };
        assert_eq!(
            connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("schema version"),
            LATEST_SCHEMA_VERSION
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master
                     WHERE type='table' AND name='tool_long_operations'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("long operation table"),
            1
        );
        let approval_columns = connection
            .prepare("PRAGMA table_info('tool_draft_approvals')")
            .expect("approval columns")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query approval columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect approval columns");
        assert!(approval_columns.iter().any(|name| name == "consumed_by_journal_id"));
        drop(connection);

        let fault_path = directory.path().join("fault.db");
        seed_database_at_version(&fault_path, 38);
        let outcome = open_and_migrate(
            &fault_path,
            DatabaseOpenOptions { migration_fault: Some(MigrationFaultPoint::DuringMigration) },
        )
        .expect("v39 fault outcome");
        assert!(matches!(outcome, MigrationResult::RecoveryRequired(_)));
        let restored = Connection::open(fault_path).expect("restored version thirty-eight db");
        assert_eq!(
            restored
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("restored schema version"),
            38
        );
    }

    #[test]
    fn w6_migration_revokes_only_active_legacy_tool_authority_and_is_atomic_on_fault() {
        fn seed_legacy_grant(path: &std::path::Path) -> String {
            seed_database_at_version(path, 26);
            let connection = Connection::open(path).expect("open version twenty-six database");
            connection
                .execute_batch("PRAGMA foreign_keys=OFF;")
                .expect("disable fixture foreign keys");
            let source = serde_json::json!({
                "source_kind": "custom_tool",
                "package_id": "legacy-package-name"
            });
            let grant = serde_json::json!({
                "source": source,
                "revoked_at": null
            });
            connection
                .execute(
                    "INSERT INTO permission_grants(
                        permission_grant_id, grant_json, lifetime, workspace_id,
                        session_id, turn_task_id, source_json, source_fingerprint_json,
                        capabilities_json, scopes_json, approved_risk_flags_json,
                        operation_fingerprint_json, created_at, expires_at, revoked_at,
                        consumed_at, approval_decision_id
                     ) VALUES (
                        'legacy-grant', ?1, 'workspace', 'legacy-workspace',
                        NULL, NULL, ?2, NULL, '[]', '[]', '[]', NULL,
                        '2026-09-02T00:00:00Z', NULL, NULL, NULL, NULL
                     )",
                    params![
                        canonical_string(&grant).expect("grant"),
                        canonical_string(&source).expect("source"),
                    ],
                )
                .expect("seed legacy grant");
            canonical_string(&source).expect("source wire")
        }

        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        let original_source = seed_legacy_grant(&path);
        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("W6 migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("isolatable legacy authority must migrate safely");
        };
        let (source_json, revoked_at) = connection
            .query_row(
                "SELECT source_json, revoked_at FROM permission_grants
                 WHERE permission_grant_id='legacy-grant'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .expect("migrated grant");
        assert_ne!(source_json, original_source);
        assert!(source_json.contains("p07.security_source_v1_unresolved"));
        assert!(revoked_at.is_some());
        assert_eq!(
            connection
                .query_row(
                    "SELECT disposition FROM p07_security_source_v1_migration_ledger
                     WHERE table_name='permission_grants' AND record_key='legacy-grant'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("migration ledger"),
            "grant_revoked"
        );
        drop(connection);

        let fault_path = directory.path().join("fault.db");
        let fault_source = seed_legacy_grant(&fault_path);
        let outcome = open_and_migrate(
            &fault_path,
            DatabaseOpenOptions { migration_fault: Some(MigrationFaultPoint::DuringMigration) },
        )
        .expect("fault outcome");
        assert!(matches!(outcome, MigrationResult::RecoveryRequired(_)));
        let restored = Connection::open(&fault_path).expect("restored W6 database");
        assert_eq!(
            restored
                .query_row(
                    "SELECT source_json FROM permission_grants
                     WHERE permission_grant_id='legacy-grant'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .expect("restored source"),
            fault_source
        );
        assert!(
            restored
                .query_row(
                    "SELECT name FROM sqlite_master
                     WHERE type='table' AND name='p07_security_source_v1_migration_ledger'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .expect("migration table lookup")
                .is_none()
        );
    }

    #[test]
    fn newer_database_schema_requires_recovery_without_mutation() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        let connection = Connection::open(&path).expect("seed database");
        let future_version = LATEST_SCHEMA_VERSION + 1;
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations(
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    checksum TEXT NOT NULL,
                    applied_at TEXT NOT NULL
                 ) STRICT;",
            )
            .expect("future migration table");
        connection
            .execute(
                "INSERT INTO schema_migrations(version, name, checksum, applied_at)
                 VALUES (?1, 'future', 'future', '2026-01-01T00:00:00Z')",
                params![future_version],
            )
            .expect("future migration row");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("recovery result");
        let MigrationResult::RecoveryRequired(recovery) = result else {
            panic!("future schema must not be opened");
        };
        assert!(recovery.reason.contains("newer than supported"));
        let connection = Connection::open(&path).expect("verify database");
        let version =
            connection.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            });
        assert_eq!(version.expect("schema version"), future_version);
        let journal_mode =
            connection.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0));
        assert_eq!(journal_mode.expect("journal mode"), "delete");
    }

    #[test]
    fn version_nine_database_adds_the_generic_journal_and_credential_tables() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 9);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version nine database must migrate to the current schema");
        };
        let version = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get::<_, i64>(0))
            .expect("schema version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        for table in ["credential_records", "mutation_journals", "mutation_leases"] {
            let actual = connection
                .query_row(
                    "SELECT name FROM sqlite_master
                     WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get::<_, String>(0),
                )
                .expect("credential journal table");
            assert_eq!(actual, table);
        }
        let legacy_table = connection
            .query_row(
                "SELECT name FROM sqlite_master
                 WHERE type='table' AND name='model_endpoint_credential_bindings'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .expect("legacy binding lookup");
        assert_eq!(legacy_table, None);
        for forbidden in ["credential_mutation_journals", "credential_mutation_leases"] {
            let present = connection
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1
                     )",
                    [forbidden],
                    |row| row.get::<_, bool>(0),
                )
                .expect("forbidden dedicated journal lookup");
            assert!(!present, "dedicated credential journal table must not exist");
        }
    }

    /// An older database whose text index exists but has no rowid map.
    ///
    /// The map is what makes a per-path text delete a lookup instead of a scan, so an
    /// upgraded database has to have one for every text row it already holds. What it
    /// must *not* do is decide which of several rows for one path is the current text:
    /// the text index stores no digest of the file's bytes, so neither the rowid nor the
    /// text itself can settle it. This test pins the honest answer — a lone row is
    /// adopted, an ambiguous path is marked for a rebuild — and pins that no digest is
    /// ever written for text that was not verified.
    #[test]
    fn a_version_forty_four_database_gains_a_complete_text_rowid_map() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 44);
        let connection = Connection::open(&path).expect("open seeded database");
        let workspace_id = WorkspaceId::new().to_string();
        // A raw insert rather than today's helper: the point of this test is a database
        // that predates today's code, so the row is written against the columns the
        // table really has at version forty-four.
        let workspace_columns = {
            let mut statement =
                connection.prepare("PRAGMA table_info(workspaces)").expect("pragma");
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .expect("columns")
                .collect::<Result<Vec<_>, _>>()
                .expect("columns")
        };
        let now = UtcTimestamp::now().to_string();
        let workspace_value = |name: &str| -> Option<String> {
            match name {
                "workspace_id" => Some(workspace_id.clone()),
                "display_name" => Some("legacy".to_owned()),
                "state" => Some("online".to_owned()),
                "catalog_freshness" => Some("fresh".to_owned()),
                "canonical_root" | "root_path" | "selected_root" => Some("C:\\legacy".to_owned()),
                "catalog_revision" | "revision" => Some("4".to_owned()),
                "authorization_epoch" => Some("1".to_owned()),
                "created_at" | "updated_at" => Some(now.clone()),
                _ => None,
            }
        };
        let literals = workspace_columns
            .iter()
            .map(|name| match workspace_value(name) {
                Some(value) => format!("'{}'", value.replace('\'', "''")),
                None => "NULL".to_owned(),
            })
            .collect::<Vec<_>>();
        connection
            .execute_batch(&format!(
                "INSERT INTO workspaces({}) VALUES({})",
                workspace_columns.join(", "),
                literals.join(", ")
            ))
            .expect("workspace");
        for (relative_path, content_hash) in
            [("kept.txt", Some("aa")), ("duplicated.txt", Some("bb")), ("pruned.txt", None)]
        {
            if let Some(content_hash) = content_hash {
                connection
                    .execute(
                        "INSERT INTO workspace_catalog_entries(
                            workspace_id, relative_path, parent_path, file_name, entry_kind,
                            partition_path, byte_length, modified_at, content_hash, text_encoding,
                            ignored, generated, hidden_from_search, reparse_tag
                         ) VALUES (?1, ?2, '.', ?2, 'file', '.', 4, '1', ?3, 'utf8', 0, 0, 0, NULL)",
                        params![workspace_id, relative_path, content_hash],
                    )
                    .expect("catalog entry");
            }
            connection
                .execute(
                    "INSERT INTO workspace_content_fts(workspace_id, relative_path, content)
                     VALUES (?1, ?2, ?3)",
                    params![workspace_id, relative_path, format!("body of {relative_path}")],
                )
                .expect("text row");
        }
        // The shape the old per-path delete could leave behind: a second text row for a
        // path that already has one.
        connection
            .execute(
                "INSERT INTO workspace_content_fts(workspace_id, relative_path, content)
                 VALUES (?1, 'duplicated.txt', 'a stale second generation')",
                [&workspace_id],
            )
            .expect("duplicate text row");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version forty-four database must migrate to the current schema");
        };
        assert_eq!(
            connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("schema version"),
            LATEST_SCHEMA_VERSION
        );

        let rows = {
            let mut statement = connection
                .prepare(
                    "SELECT relative_path, content_hash, fts_rowid, needs_reindex
                     FROM workspace_content_index
                     WHERE workspace_id=?1 ORDER BY relative_path",
                )
                .expect("statement");
            statement
                .query_map([&workspace_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .expect("rows")
                .collect::<Result<Vec<_>, _>>()
                .expect("rows")
        };
        // A path with one text row keeps it. A path with two cannot be settled from the
        // rowid or the text alone, so its text is dropped and the path is marked for a
        // rebuild — the digest column stays empty rather than borrowing the catalog's.
        assert_eq!(
            rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
            vec!["duplicated.txt", "kept.txt"],
            "the map must hold every path whose entry still exists, and only those"
        );
        let duplicate = rows.iter().find(|row| row.0 == "duplicated.txt").expect("marked path");
        assert_eq!(
            duplicate.1, None,
            "a digest may only be recorded for text that was verified, and these rows were \
             never verified"
        );
        assert_eq!(duplicate.2, None, "the dropped rows must not be addressed");
        assert_eq!(duplicate.3, 1, "and the path must be marked for a rebuild");
        let kept = rows.iter().find(|row| row.0 == "kept.txt").expect("kept path");
        assert!(kept.2.is_some(), "a lone text row for a known path is adopted as-is");
        assert_eq!(kept.3, 0);

        let orphaned = connection
            .query_row(
                "SELECT COUNT(*) FROM workspace_content_fts AS text
                 WHERE NOT EXISTS (
                    SELECT 1 FROM workspace_content_index AS map WHERE map.fts_rowid = text.rowid
                 )",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("orphan count");
        assert_eq!(
            orphaned, 0,
            "no text row may be left unreachable: a duplicate, or a row whose entry is gone, \
             must be removed rather than orphaned"
        );
        let remaining = connection
            .query_row(
                "SELECT COUNT(*) FROM workspace_content_fts WHERE relative_path='duplicated.txt'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("duplicate count");
        assert_eq!(
            remaining, 0,
            "neither generation of an unprovable path may survive: keeping either one would \
             index bytes nobody checked"
        );
        // The marked path is what the catalog re-reads; the loader must be able to see it.
        let rebuild = connection
            .query_row(
                "SELECT relative_path FROM workspace_content_index
                 WHERE workspace_id=?1 AND needs_reindex=1",
                [&workspace_id],
                |row| row.get::<_, String>(0),
            )
            .expect("rebuild path");
        assert_eq!(rebuild, "duplicated.txt");
    }

    #[test]
    fn version_eleven_database_adds_credential_delete_state_and_tombstones() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 11);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version eleven database must migrate to the current schema");
        };
        let version = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get::<_, i64>(0))
            .expect("schema version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);

        let credential_schema = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type='table' AND name='credential_records'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("credential table schema");
        assert!(credential_schema.contains("'deletion_pending'"));

        let tombstone_schema = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type='table' AND name='credential_tombstones'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("credential tombstone schema");
        for required in [
            "credential_id",
            "last_committed_credential_revision",
            "former_owner_json",
            "deletion_reason",
            "parent_journal_id",
            "child_journal_id",
            "deleted_at",
        ] {
            assert!(tombstone_schema.contains(required), "missing {required}");
        }
        for forbidden in ["credential_ref", "target", "fingerprint", "secret"] {
            assert!(!tombstone_schema.contains(forbidden), "unexpected {forbidden}");
        }
    }

    #[test]
    fn version_thirteen_legacy_catalog_cache_is_rebuilt_to_the_current_contract() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 13);
        let connection = Connection::open(&path).expect("open version thirteen database");
        connection
            .execute_batch(
                "CREATE TABLE migration_sentinel(value TEXT NOT NULL);
                 INSERT INTO migration_sentinel VALUES ('preserved');
                 DROP TABLE provider_catalog_entries;
                 DROP TABLE provider_catalog_runtime_state;
                 DROP TABLE provider_catalog_snapshots;
                 CREATE TABLE provider_catalog_snapshots (
                    catalog_revision TEXT PRIMARY KEY,
                    fetched_at TEXT NOT NULL,
                    etag TEXT,
                    last_modified TEXT,
                    raw_artifact_id TEXT NOT NULL REFERENCES artifacts(artifact_id),
                    snapshot_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                 ) STRICT;
                 CREATE TABLE provider_catalog_entries (
                    catalog_revision TEXT NOT NULL
                        REFERENCES provider_catalog_snapshots(catalog_revision) ON DELETE CASCADE,
                    catalog_provider_id TEXT NOT NULL,
                    entry_json TEXT NOT NULL,
                    PRIMARY KEY (catalog_revision, catalog_provider_id)
                 ) STRICT;
                 CREATE TABLE provider_catalog_runtime_state (
                    catalog_key TEXT PRIMARY KEY CHECK (catalog_key = 'models.dev'),
                    current_revision TEXT REFERENCES provider_catalog_snapshots(catalog_revision),
                    last_error TEXT,
                    updated_at TEXT NOT NULL
                 ) STRICT;
                 INSERT INTO provider_catalog_runtime_state(
                    catalog_key, current_revision, last_error, updated_at
                 ) VALUES (
                    'models.dev', NULL, 'legacy failure text', '2026-01-01T00:00:00Z'
                 );",
            )
            .expect("replace catalog cache with released legacy shape");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("legacy catalog database must migrate to the current schema");
        };
        let version = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get::<_, i64>(0))
            .expect("schema version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        let snapshot_columns = connection
            .prepare("PRAGMA table_info(provider_catalog_snapshots)")
            .expect("prepare catalog columns")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query catalog columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("catalog columns");
        assert!(snapshot_columns.contains(&"catalog_content_hash".to_owned()));
        assert!(snapshot_columns.contains(&"last_successful_refresh_at".to_owned()));
        assert!(!snapshot_columns.contains(&"fetched_at".to_owned()));
        let runtime_columns = connection
            .prepare("PRAGMA table_info(provider_catalog_runtime_state)")
            .expect("prepare catalog runtime columns")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query catalog runtime columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("catalog runtime columns");
        assert!(runtime_columns.contains(&"last_error_code".to_owned()));
        assert!(runtime_columns.contains(&"checked_at".to_owned()));
        assert!(!runtime_columns.contains(&"last_error".to_owned()));
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM provider_catalog_runtime_state", [], |row| {
                    row.get::<_, i64>(0)
                })
                .expect("empty current runtime cache"),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT value FROM migration_sentinel", [], |row| row
                    .get::<_, String>(0))
                .expect("unrelated data survives"),
            "preserved"
        );
    }

    #[test]
    fn version_thirteen_current_catalog_cache_is_preserved() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 13);
        let connection = Connection::open(&path).expect("open version thirteen database");
        connection
            .execute(
                "INSERT INTO provider_catalog_runtime_state(
                    catalog_key, current_revision, last_error_code, checked_at, updated_at
                 ) VALUES ('models.dev', NULL, ?1, ?2, ?2)",
                ("provider.catalog_refresh_failed", "2026-01-01T00:00:00Z"),
            )
            .expect("seed current catalog runtime state");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("current catalog database must migrate without cache invalidation");
        };
        let state = connection
            .query_row(
                "SELECT last_error_code, checked_at
                 FROM provider_catalog_runtime_state WHERE catalog_key='models.dev'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("preserved catalog runtime state");
        assert_eq!(state.0, "provider.catalog_refresh_failed");
        assert_eq!(state.1, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn version_eight_database_receives_the_tagged_transport_attempt_schema() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 8);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version eight database must migrate to the current schema");
        };
        let columns = connection
            .prepare("PRAGMA table_info(transport_attempts)")
            .expect("prepare transport columns")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query transport columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("transport columns");
        assert!(columns.contains(&"operation_context_json".to_owned()));
        assert!(columns.contains(&"canonical_body_artifact_id".to_owned()));
        assert!(!columns.contains(&"model_execution_snapshot_id".to_owned()));
        assert!(!columns.contains(&"model_step_id".to_owned()));
        assert!(!columns.contains(&"provider_request_artifact_id".to_owned()));
    }

    #[test]
    fn version_twenty_database_adds_tool_execution_authority_without_rewriting_history() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 20);
        let connection = Connection::open(&path).expect("open version twenty database");
        connection
            .execute_batch(
                "CREATE TABLE p07_execution_migration_sentinel(value TEXT NOT NULL);
                 INSERT INTO p07_execution_migration_sentinel VALUES ('preserved');",
            )
            .expect("seed migration sentinel");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version twenty database must migrate to the current schema");
        };
        let version = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get::<_, i64>(0))
            .expect("schema version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        assert_eq!(
            connection
                .query_row("SELECT value FROM p07_execution_migration_sentinel", [], |row| {
                    row.get::<_, String>(0)
                })
                .expect("sentinel value"),
            "preserved"
        );

        for table in [
            "tool_batches",
            "tool_calls",
            "tool_execution_attempts",
            "tool_attempt_operational_state",
            "tool_results",
        ] {
            let actual = connection
                .query_row(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get::<_, String>(0),
                )
                .expect("tool execution authority table");
            assert_eq!(actual, table);
        }

        let attempt_schema = connection
            .query_row(
                "SELECT sql FROM sqlite_master
                 WHERE type='table' AND name='tool_execution_attempts'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("attempt schema");
        for required in [
            "attempt_number BETWEEN 1 AND 4",
            "dispatch_prepared_event_id",
            "cancellation_requested_event_id",
            "terminal_event_id",
            "evidence_artifact_id",
            "side_effect_state",
        ] {
            assert!(attempt_schema.contains(required), "missing {required}");
        }

        let result_schema = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='tool_results'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("tool result schema");
        for outcome in ["succeeded", "failed", "cancelled", "not_executed", "indeterminate"] {
            assert!(result_schema.contains(&format!("'{outcome}'")), "missing {outcome}");
        }
    }

    #[test]
    fn version_twenty_one_definition_fk_is_repaired_without_losing_tool_calls() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 20);
        let connection = Connection::open(&path).expect("open version twenty database");
        connection.execute_batch("PRAGMA foreign_keys=ON;").expect("enable foreign keys");
        let legacy_schema = super::P07_TOOL_EXECUTION_AUTHORITY_SCHEMA.replace(
            "    updated_at TEXT NOT NULL,\n    UNIQUE (tool_batch_id, ordinal),\n    CHECK (",
            "    updated_at TEXT NOT NULL,\n    UNIQUE (tool_batch_id, ordinal),\n    FOREIGN KEY (tool_id, tool_version_json)\n        REFERENCES tool_definitions(tool_id, tool_version_json),\n    CHECK (",
        );
        assert!(legacy_schema.contains("REFERENCES tool_definitions"));
        connection.execute_batch(&legacy_schema).expect("seed pre-repair v21 schema");
        connection
            .execute(
                "INSERT INTO schema_migrations(version, name, checksum, applied_at)
                 VALUES (21, 'p07_tool_execution_authority', 'pre-repair', ?1)",
                ["2026-09-04T00:00:00Z"],
            )
            .expect("record pre-repair migration");
        connection
            .execute_batch(
                "INSERT INTO workspaces(
                    workspace_id, display_name, selected_root, canonical_root, state,
                    revision, catalog_freshness, catalog_revision, authorization_epoch,
                    created_at, updated_at
                 ) VALUES (
                    'workspace', 'Workspace', 'C:\\Aura\\v21', 'C:\\Aura\\v21', 'online',
                    1, 'unavailable', 0, 1, '2026-09-04T00:00:00Z', '2026-09-04T00:00:00Z'
                 );
                 INSERT INTO sessions(
                    session_id, workspace_id, title, state, created_at, updated_at
                 ) VALUES (
                    'session', 'workspace', 'Session', 'running',
                    '2026-09-04T00:00:00Z', '2026-09-04T00:00:00Z'
                 );
                 INSERT INTO turn_tasks(
                    turn_task_id, session_id, state, created_at, updated_at
                 ) VALUES (
                    'turn', 'session', 'running',
                    '2026-09-04T00:00:00Z', '2026-09-04T00:00:00Z'
                 );
                 INSERT INTO agent_runs(
                    agent_run_id, turn_task_id, state, created_at, updated_at
                 ) VALUES (
                    'agent', 'turn', 'running',
                    '2026-09-04T00:00:00Z', '2026-09-04T00:00:00Z'
                 );
                 INSERT INTO tool_registry_snapshots(
                    tool_registry_snapshot_id, registry_revision, content_hash_json,
                    snapshot_json, search_documents_json, search_documents_hash_json,
                    search_corpus_retired_at, created_at
                 ) VALUES (
                    'registry', 1, '{}', '{}', '[]', '{}', NULL,
                    '2026-09-04T00:00:00Z'
                 );
                 INSERT INTO tool_definitions(
                    tool_id, tool_version_json, canonical_name, definition_revision,
                    definition_fingerprint_json, entity_revision, record_json,
                    record_hash_json, retired_at
                 ) VALUES ('known-tool', '{}', 'sample.tool', 1, '{}', 1, '{}', '{}', NULL);
                 INSERT INTO tool_batches(
                    tool_batch_id, batch_content_hash_json, session_id, turn_task_id,
                    agent_run_id, model_step_id, registry_snapshot_id, registry_revision,
                    registry_content_hash_json, p08_state_revision, p08_state_hash_json,
                    status, request_json, request_hash_json, entity_revision,
                    created_at, updated_at
                 ) VALUES (
                    'batch', '{}', 'session', 'turn', 'agent', 'step', 'registry', 1,
                    '{}', 1, '{}', 'pending_admission', '{}', '{}', 1,
                    '2026-09-04T00:00:00Z', '2026-09-04T00:00:00Z'
                 );
                 INSERT INTO tool_calls(
                    tool_call_id, tool_batch_id, ordinal, tool_id, tool_version_json,
                    status, normalized_call_json, normalized_call_hash_json,
                    entity_revision, created_at, updated_at
                 ) VALUES (
                    'known-call', 'batch', 1, 'known-tool', '{}', 'pending_admission',
                    '{}', '{}', 1, '2026-09-04T00:00:00Z', '2026-09-04T00:00:00Z'
                 );",
            )
            .expect("seed v21 tool call");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("pre-repair v21 database must migrate");
        };
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM tool_calls WHERE tool_call_id='known-call'",
                    [],
                    |row| { row.get::<_, i64>(0) }
                )
                .expect("preserved tool call"),
            1
        );
        let foreign_tables = connection
            .prepare("PRAGMA foreign_key_list(tool_calls)")
            .expect("prepare foreign keys")
            .query_map([], |row| row.get::<_, String>(2))
            .expect("query foreign keys")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect foreign keys");
        assert!(!foreign_tables.iter().any(|table| table == "tool_definitions"));
        connection
            .execute(
                "INSERT INTO tool_calls(
                    tool_call_id, tool_batch_id, ordinal, tool_id, tool_version_json,
                    status, normalized_call_json, normalized_call_hash_json,
                    entity_revision, created_at, updated_at
                 ) VALUES (
                    'missing-call', 'batch', 2, 'missing-tool', '{}', 'pending_admission',
                    '{}', '{}', 1, '2026-09-04T00:00:00Z', '2026-09-04T00:00:00Z'
                 )",
                [],
            )
            .expect("persist unresolved tool call for paired NotExecuted result");
    }

    #[test]
    fn version_twenty_two_expands_artifact_owners_without_rewriting_history() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("aura.db");
        seed_database_at_version(&path, 22);
        let artifact_id = ArtifactId::new();
        let connection = Connection::open(&path).expect("open version twenty-two database");
        connection.execute_batch("PRAGMA foreign_keys=ON;").expect("enable foreign keys");
        connection
            .execute(
                "INSERT INTO artifacts(
                    artifact_id, scope, media_type, access_flags, byte_length,
                    content_hash_algorithm, content_hash_hex, inline_bytes, created_at
                 ) VALUES (?1, 'system_cache', 'application/json', 0, 2,
                           'blake3', ?2, X'7B7D', ?3)",
                params![
                    artifact_id.to_string(),
                    blake3::hash(b"{}").to_hex().to_string(),
                    "2026-09-04T00:00:00Z",
                ],
            )
            .expect("seed artifact");
        connection
            .execute(
                "INSERT INTO artifact_owners(
                    artifact_id, owner_kind, owner_type, owner_id, created_at
                 ) VALUES (?1, 'system', 'provider_catalog_cache', NULL, ?2)",
                params![artifact_id.to_string(), "2026-09-04T00:00:00Z"],
            )
            .expect("seed historical owner");
        drop(connection);

        let result =
            open_and_migrate(&path, DatabaseOpenOptions::default()).expect("migration result");
        let MigrationResult::Ready(connection) = result else {
            panic!("version twenty-two database must migrate to the current schema");
        };
        let owner = connection
            .query_row(
                "SELECT owner_kind, owner_type, owner_id, created_at
                 FROM artifact_owners WHERE artifact_id=?1",
                [artifact_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .expect("preserved owner");
        assert_eq!(
            owner,
            (
                "system".to_owned(),
                "provider_catalog_cache".to_owned(),
                None,
                "2026-09-04T00:00:00Z".to_owned(),
            )
        );
        let schema = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='artifact_owners'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("artifact owner schema");
        assert!(schema.contains("'tool'"));
        assert!(schema.contains("'execution_attempt'"));
    }
    #[test]
    fn schema_42_import_fixture_matches_the_historical_migrations() {
        fn signature(connection: &Connection) -> Vec<(String, String, String, String)> {
            connection
                .prepare(
                    "SELECT type,name,tbl_name,sql FROM sqlite_master
                     WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
                )
                .expect("historical schema comparison")
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .expect("historical schema comparison")
                .collect::<Result<Vec<_>, _>>()
                .expect("historical schema comparison")
        }
        fn versions(connection: &Connection) -> Vec<(i64, String, String)> {
            connection
                .prepare("SELECT version,name,checksum FROM schema_migrations ORDER BY version")
                .expect("historical schema comparison")
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .expect("historical schema comparison")
                .collect::<Result<Vec<_>, _>>()
                .expect("historical schema comparison")
        }
        let historical = Connection::open_in_memory().expect("historical schema comparison");
        apply_migrations_through(&historical, 0, 42, None).expect("historical schema comparison");
        let frozen = Connection::open_in_memory().expect("historical schema comparison");
        frozen
            .execute_batch(include_str!("../../../fixtures/persistence/schema-42.sql"))
            .expect("historical schema comparison");
        assert_eq!(signature(&frozen), signature(&historical), "all Schema 42 objects");
        assert_eq!(versions(&frozen), versions(&historical), "original migration checksums");
        assert_eq!(versions(&frozen).last().expect("historical schema comparison").0, 42);
    }
}
