use std::collections::BTreeSet;

use aura_model_domain::{
    ModelDiscoveryFailedPayload, ModelDiscoveryOutcome, ModelDiscoveryRunRecord,
    ModelDiscoveryStage, ModelDiscoveryStartedPayload, ModelDiscoveryState, ModelFailureCategory,
    ModelRetryClass, NormalizedModelFailure, ProviderTransportAttemptStartedPayload,
    TransportAttemptKind, TransportAttemptRecord, TransportAttemptState, TransportOperationContext,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::{PersistenceCoordinator, PersistenceError};

pub const LEGACY_CONFIGURATION_TRANSPORT_MIGRATION_KEY: &str =
    "p05.legacy_configuration_transport_indeterminate.v1";
const LEGACY_RESOLUTION: &str = "legacy_indeterminate";
const LEGACY_FAILURE_CODE: &str = "transport.failed";
const LEGACY_FAILURE_MESSAGE: &str =
    "Model discovery failed before a complete valid model catalog was committed.";

pub(crate) const LEGACY_CONFIGURATION_TRANSPORT_LEDGER_SCHEMA: &str = r#"
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

CREATE UNIQUE INDEX provider_transport_attempt_one_terminal_event
    ON events(json_extract(payload_json, '$.transport_attempt_id'))
    WHERE event_type IN (
        'ProviderTransportAttemptCompleted',
        'ProviderTransportAttemptFailed',
        'ProviderTransportAttemptCancelled'
    );
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveTransportAttemptStatus {
    Runtime(TransportAttemptState),
    LegacyIndeterminate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalIntegrityDegradation {
    pub migration_key: String,
    pub transport_attempt_id: String,
    pub discovery_run_id: String,
    pub failure_id: String,
    pub discovery_failed_event_id: String,
    pub original_state: String,
    pub source_schema_version: i64,
    pub canonical_evidence_hash: String,
    pub reconciled_at: String,
    pub effective_status: EffectiveTransportAttemptStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigurationTransportRecoverySnapshot {
    pub runtime_safety_blockers: Vec<TransportAttemptRecord>,
    pub historical_integrity_degradations: Vec<HistoricalIntegrityDegradation>,
}

impl PersistenceCoordinator {
    pub fn configuration_transport_recovery_snapshot(
        &self,
    ) -> Result<ConfigurationTransportRecoverySnapshot, PersistenceError> {
        self.with_reader(load_configuration_transport_recovery_snapshot)
    }

    /// Enforces the P05 clean-acceptance profile without mutating data. This
    /// check is intentionally stricter than the normal Ready Gate: acceptance
    /// data must contain no legacy reconciliation, pending model journals,
    /// active configuration operations, or transport terminality mismatch.
    pub fn validate_clean_p05_acceptance_profile(&self) -> Result<(), PersistenceError> {
        self.with_reader(validate_clean_p05_acceptance_profile)
    }
}

pub(crate) fn validate_clean_p05_acceptance_profile(
    connection: &Connection,
) -> Result<(), PersistenceError> {
    let checks = [
        (
            "p05.acceptance.legacy_ledger_present",
            "SELECT COUNT(*) FROM p05_legacy_configuration_transport_reconciliation",
        ),
        (
            "p05.acceptance.pending_journal_present",
            "SELECT COUNT(*) FROM mutation_journals
             WHERE state NOT IN ('completed', 'aborted', 'reconciliation_required')",
        ),
        (
            "p05.acceptance.active_discovery_present",
            "SELECT COUNT(*) FROM model_discovery_runs WHERE state IN ('queued', 'running')",
        ),
        (
            "p05.acceptance.active_availability_present",
            "SELECT COUNT(*) FROM model_availability_tests WHERE state IN ('queued', 'running')",
        ),
        (
            "p05.acceptance.nonterminal_attempt_present",
            "SELECT COUNT(*) FROM transport_attempts
             WHERE state NOT IN ('completed', 'cancelled', 'failed')",
        ),
    ];
    for (error_code, sql) in checks {
        let count = connection.query_row(sql, [], |row| row.get::<_, i64>(0))?;
        if count != 0 {
            return Err(PersistenceError::InvalidRequest(format!(
                "{error_code}: clean P05 acceptance profile contains {count} disallowed record(s)"
            )));
        }
    }

    let inconsistent_started = connection.query_row(
        "SELECT COUNT(*)
           FROM transport_attempts attempt
          WHERE (
              SELECT COUNT(*) FROM events event
               WHERE event.event_type='ProviderTransportAttemptStarted'
                 AND json_extract(event.payload_json, '$.transport_attempt_id')
                     = attempt.transport_attempt_id
          ) != 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if inconsistent_started != 0 {
        return Err(PersistenceError::InvalidRequest(format!(
            "p05.acceptance.attempt_started_event_mismatch: clean P05 acceptance profile contains {inconsistent_started} inconsistent Attempt(s)"
        )));
    }

    let inconsistent_terminal = connection.query_row(
        "SELECT COUNT(*)
           FROM transport_attempts attempt
          WHERE attempt.state IN ('completed', 'cancelled', 'failed')
            AND (
              SELECT COUNT(*) FROM events event
               WHERE json_extract(event.payload_json, '$.transport_attempt_id')
                     = attempt.transport_attempt_id
                 AND event.event_type = CASE attempt.state
                     WHEN 'completed' THEN 'ProviderTransportAttemptCompleted'
                     WHEN 'cancelled' THEN 'ProviderTransportAttemptCancelled'
                     WHEN 'failed' THEN 'ProviderTransportAttemptFailed'
                 END
            ) != 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if inconsistent_terminal != 0 {
        return Err(PersistenceError::InvalidRequest(format!(
            "p05.acceptance.attempt_terminal_event_mismatch: clean P05 acceptance profile contains {inconsistent_terminal} inconsistent Attempt(s)"
        )));
    }

    let relation_mismatch = connection.query_row(
        "SELECT COUNT(*)
           FROM transport_attempts attempt
          WHERE json_extract(attempt.operation_context_json, '$.type') IN (
                    'model_discovery', 'model_availability_test'
                )
            AND (
              SELECT COUNT(*)
                FROM configuration_transport_operation_attempts relation
               WHERE relation.transport_attempt_id=attempt.transport_attempt_id
            ) != 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if relation_mismatch != 0 {
        return Err(PersistenceError::InvalidRequest(format!(
            "p05.acceptance.operation_attempt_relation_mismatch: clean P05 acceptance profile contains {relation_mismatch} inconsistent relation(s)"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct LedgerRow {
    migration_key: String,
    resolution: String,
    transport_attempt_id: String,
    discovery_run_id: String,
    failure_id: String,
    discovery_failed_event_id: String,
    original_state: String,
    source_schema_version: i64,
    canonical_evidence_hash: String,
    reconciled_at: String,
}

#[derive(Clone, Debug)]
struct LegacyEvidenceMatch {
    transport_attempt_id: String,
    discovery_run_id: String,
    failure_id: String,
    discovery_failed_event_id: String,
    original_state: String,
    canonical_evidence_hash: String,
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalLegacyEvidenceV1 {
    source_schema_version: i64,
    transport_attempt_id: String,
    discovery_run_id: String,
    failure_id: String,
    discovery_failed_event_id: String,
    attempt_row: CanonicalAttemptRow,
    discovery_row: CanonicalDiscoveryRow,
    failure_row: CanonicalFailureRow,
    discovery_started_event: CanonicalEventRow,
    attempt_started_event: CanonicalEventRow,
    discovery_failed_event: CanonicalEventRow,
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalAttemptRow {
    endpoint_id: String,
    operation_context_json: String,
    attempt_number: i64,
    attempt_kind: String,
    state: String,
    record_json: String,
    canonical_body_artifact_id: Option<String>,
    raw_stream_artifact_id: Option<String>,
    model_failure_id: Option<String>,
    started_at: String,
    finished_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalDiscoveryRow {
    endpoint_id: String,
    correlation_id: String,
    revision: i64,
    state: String,
    record_json: String,
    raw_response_artifact_id: Option<String>,
    started_at: String,
    finished_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalFailureRow {
    failure_fingerprint_json: String,
    failure_json: String,
    created_at: String,
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalEventRow {
    global_sequence: i64,
    event_id: String,
    event_type: String,
    event_version: i64,
    actor: String,
    correlation_id: String,
    causation_id: Option<String>,
    atomic_group_id: Option<String>,
    payload_json: String,
    payload_artifact_ref: Option<String>,
    created_at: String,
}

pub(crate) fn migrate_legacy_configuration_transport_indeterminate_v1(
    transaction: &Transaction<'_>,
    source_schema_version: i64,
    reconciled_at: &str,
) -> Result<(), PersistenceError> {
    transaction.execute_batch(LEGACY_CONFIGURATION_TRANSPORT_LEDGER_SCHEMA)?;
    if !(9..15).contains(&source_schema_version) {
        return Ok(());
    }

    let mut statement = transaction.prepare(
        "SELECT transport_attempt_id FROM transport_attempts
         WHERE state='authorizing_network'
           AND json_extract(operation_context_json, '$.type')='model_discovery'
         ORDER BY started_at, transport_attempt_id",
    )?;
    let candidate_ids =
        statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    for attempt_id in candidate_ids {
        let Some(evidence) =
            legacy_evidence_match(transaction, &attempt_id, source_schema_version)?
        else {
            continue;
        };
        transaction.execute(
            "INSERT INTO p05_legacy_configuration_transport_reconciliation(
                migration_key, resolution, transport_attempt_id, discovery_run_id,
                failure_id, discovery_failed_event_id, original_state,
                source_schema_version, canonical_evidence_hash, reconciled_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                LEGACY_CONFIGURATION_TRANSPORT_MIGRATION_KEY,
                LEGACY_RESOLUTION,
                evidence.transport_attempt_id,
                evidence.discovery_run_id,
                evidence.failure_id,
                evidence.discovery_failed_event_id,
                evidence.original_state,
                source_schema_version,
                evidence.canonical_evidence_hash,
                reconciled_at,
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn load_configuration_transport_recovery_snapshot(
    connection: &Connection,
) -> Result<ConfigurationTransportRecoverySnapshot, PersistenceError> {
    let mut ledger_statement = connection.prepare(
        "SELECT migration_key, resolution, transport_attempt_id, discovery_run_id,
                failure_id, discovery_failed_event_id, original_state,
                source_schema_version, canonical_evidence_hash, reconciled_at
         FROM p05_legacy_configuration_transport_reconciliation
         ORDER BY transport_attempt_id",
    )?;
    let ledger_rows = ledger_statement
        .query_map([], |row| {
            Ok(LedgerRow {
                migration_key: row.get(0)?,
                resolution: row.get(1)?,
                transport_attempt_id: row.get(2)?,
                discovery_run_id: row.get(3)?,
                failure_id: row.get(4)?,
                discovery_failed_event_id: row.get(5)?,
                original_state: row.get(6)?,
                source_schema_version: row.get(7)?,
                canonical_evidence_hash: row.get(8)?,
                reconciled_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(ledger_statement);

    let mut ledger_attempt_ids = BTreeSet::new();
    let mut historical_integrity_degradations = Vec::with_capacity(ledger_rows.len());
    for ledger in ledger_rows {
        if ledger.migration_key != LEGACY_CONFIGURATION_TRANSPORT_MIGRATION_KEY
            || ledger.resolution != LEGACY_RESOLUTION
            || !ledger_attempt_ids.insert(ledger.transport_attempt_id.clone())
        {
            return Err(PersistenceError::InvalidRequest(
                "legacy configuration transport ledger identity is invalid".to_owned(),
            ));
        }
        let evidence = legacy_evidence_match(
            connection,
            &ledger.transport_attempt_id,
            ledger.source_schema_version,
        )?
        .ok_or_else(|| {
            PersistenceError::InvalidRequest(format!(
                "legacy configuration transport evidence no longer matches attempt {}",
                ledger.transport_attempt_id
            ))
        })?;
        if evidence.discovery_run_id != ledger.discovery_run_id
            || evidence.failure_id != ledger.failure_id
            || evidence.discovery_failed_event_id != ledger.discovery_failed_event_id
            || evidence.original_state != ledger.original_state
            || evidence.canonical_evidence_hash != ledger.canonical_evidence_hash
        {
            return Err(PersistenceError::InvalidRequest(format!(
                "legacy configuration transport evidence hash or identity contradicts attempt {}",
                ledger.transport_attempt_id
            )));
        }
        historical_integrity_degradations.push(HistoricalIntegrityDegradation {
            migration_key: ledger.migration_key,
            transport_attempt_id: ledger.transport_attempt_id,
            discovery_run_id: ledger.discovery_run_id,
            failure_id: ledger.failure_id,
            discovery_failed_event_id: ledger.discovery_failed_event_id,
            original_state: ledger.original_state,
            source_schema_version: ledger.source_schema_version,
            canonical_evidence_hash: ledger.canonical_evidence_hash,
            reconciled_at: ledger.reconciled_at,
            effective_status: EffectiveTransportAttemptStatus::LegacyIndeterminate,
        });
    }

    let mut statement = connection.prepare(
        "SELECT record_json FROM transport_attempts
         WHERE state NOT IN ('completed', 'cancelled', 'failed')
         ORDER BY started_at, transport_attempt_id",
    )?;
    let records =
        statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let mut runtime_safety_blockers = Vec::new();
    for record_json in records {
        let record: TransportAttemptRecord = serde_json::from_str(&record_json)?;
        if !matches!(
            record.request_evidence.operation_context,
            TransportOperationContext::ModelDiscovery { .. }
                | TransportOperationContext::ModelAvailabilityTest { .. }
        ) {
            continue;
        }
        if ledger_attempt_ids.contains(&record.id.to_string()) {
            continue;
        }
        runtime_safety_blockers.push(record);
    }
    Ok(ConfigurationTransportRecoverySnapshot {
        runtime_safety_blockers,
        historical_integrity_degradations,
    })
}

fn legacy_evidence_match(
    connection: &Connection,
    attempt_id: &str,
    source_schema_version: i64,
) -> Result<Option<LegacyEvidenceMatch>, PersistenceError> {
    if !(9..15).contains(&source_schema_version) {
        return Ok(None);
    }
    let attempt_row = connection
        .query_row(
            "SELECT endpoint_id, operation_context_json, attempt_number, attempt_kind,
                    state, record_json, canonical_body_artifact_id,
                    raw_stream_artifact_id, model_failure_id, started_at, finished_at
             FROM transport_attempts WHERE transport_attempt_id=?1",
            [attempt_id],
            |row| {
                Ok(CanonicalAttemptRow {
                    endpoint_id: row.get(0)?,
                    operation_context_json: row.get(1)?,
                    attempt_number: row.get(2)?,
                    attempt_kind: row.get(3)?,
                    state: row.get(4)?,
                    record_json: row.get(5)?,
                    canonical_body_artifact_id: row.get(6)?,
                    raw_stream_artifact_id: row.get(7)?,
                    model_failure_id: row.get(8)?,
                    started_at: row.get(9)?,
                    finished_at: row.get(10)?,
                })
            },
        )
        .optional()?;
    let Some(attempt_row) = attempt_row else {
        return Ok(None);
    };
    let attempt: TransportAttemptRecord = match serde_json::from_str(&attempt_row.record_json) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let TransportOperationContext::ModelDiscovery { discovery_run_id } =
        &attempt.request_evidence.operation_context
    else {
        return Ok(None);
    };
    if attempt.id.to_string() != attempt_id
        || attempt.endpoint_id.to_string() != attempt_row.endpoint_id
        || attempt.attempt_number != 1
        || attempt.attempt_kind != TransportAttemptKind::Initial
        || attempt.state != TransportAttemptState::AuthorizingNetwork
        || attempt.provider_request_id.is_some()
        || attempt.first_byte_at.is_some()
        || attempt.first_model_event_at.is_some()
        || attempt.finished_at.is_some()
        || attempt.raw_stream_artifact_id.is_some()
        || attempt.failure_id.is_some()
        || attempt.request_evidence.canonical_body_artifact_id.is_some()
        || attempt.request_evidence.body_byte_length != 0
        || attempt.request_evidence.target.method != "GET"
        || attempt.request_evidence.target.path != "/models"
        || attempt.request_evidence.validate().is_err()
        || attempt_row.attempt_number != 1
        || attempt_row.attempt_kind != "initial"
        || attempt_row.state != "authorizing_network"
        || attempt_row.canonical_body_artifact_id.is_some()
        || attempt_row.raw_stream_artifact_id.is_some()
        || attempt_row.model_failure_id.is_some()
        || attempt_row.finished_at.is_some()
        || attempt_row.started_at != attempt.started_at.to_string()
        || attempt_row.operation_context_json
            != serde_json::to_string(&attempt.request_evidence.operation_context)?
    {
        return Ok(None);
    }

    let discovery_id = discovery_run_id.to_string();
    let discovery_row = connection
        .query_row(
            "SELECT endpoint_id, correlation_id, revision, state, record_json,
                    raw_response_artifact_id, started_at, finished_at
             FROM model_discovery_runs WHERE discovery_run_id=?1",
            [&discovery_id],
            |row| {
                Ok(CanonicalDiscoveryRow {
                    endpoint_id: row.get(0)?,
                    correlation_id: row.get(1)?,
                    revision: row.get(2)?,
                    state: row.get(3)?,
                    record_json: row.get(4)?,
                    raw_response_artifact_id: row.get(5)?,
                    started_at: row.get(6)?,
                    finished_at: row.get(7)?,
                })
            },
        )
        .optional()?;
    let Some(discovery_row) = discovery_row else {
        return Ok(None);
    };
    let discovery: ModelDiscoveryRunRecord = match serde_json::from_str(&discovery_row.record_json)
    {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if discovery.validate().is_err()
        || discovery.id.to_string() != discovery_id
        || discovery.endpoint_id != attempt.endpoint_id
        || discovery.endpoint_id.to_string() != discovery_row.endpoint_id
        || discovery.correlation_id.to_string() != discovery_row.correlation_id
        || discovery.revision.value() != 3
        || discovery_row.revision != 3
        || discovery.state != ModelDiscoveryState::Completed
        || discovery_row.state != "completed"
        || discovery.stage != Some(ModelDiscoveryStage::Completed)
        || discovery.outcome != Some(ModelDiscoveryOutcome::Failed)
        || discovery.cancellation_reason.is_some()
        || !discovery.transport_attempt_ids.is_empty()
        || discovery.result.is_some()
        || discovery.provider_error.is_some()
        || discovery.raw_response_artifact_id.is_some()
        || discovery_row.raw_response_artifact_id.is_some()
        || discovery.finished_at.is_none()
        || discovery_row.finished_at.as_deref()
            != discovery.finished_at.map(|value| value.to_string()).as_deref()
        || discovery_row.started_at != discovery.started_at.to_string()
    {
        return Ok(None);
    }
    let Some(failure_id) = discovery.failure_id.map(|value| value.to_string()) else {
        return Ok(None);
    };

    let failure_row = connection
        .query_row(
            "SELECT failure_fingerprint_json, failure_json, created_at
             FROM model_failures WHERE model_failure_id=?1",
            [&failure_id],
            |row| {
                Ok(CanonicalFailureRow {
                    failure_fingerprint_json: row.get(0)?,
                    failure_json: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )
        .optional()?;
    let Some(failure_row) = failure_row else {
        return Ok(None);
    };
    let failure: NormalizedModelFailure = match serde_json::from_str(&failure_row.failure_json) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if failure.category != ModelFailureCategory::Transport
        || failure.code != LEGACY_FAILURE_CODE
        || failure.message != LEGACY_FAILURE_MESSAGE
        || failure.retry_class != ModelRetryClass::NotRetryable
        || failure.provider_status.is_some()
        || failure.provider_request_id.is_some()
        || failure.provider_error_ref.is_some()
        || failure.partial_output_ref.is_some()
        || failure.transport_attempt_id.is_some()
    {
        return Ok(None);
    }

    let discovery_started_event =
        exactly_one_event(connection, &discovery_row.correlation_id, "ModelDiscoveryStarted")?;
    let Some(discovery_started_event) = discovery_started_event else {
        return Ok(None);
    };
    let attempt_started_event = exactly_one_event(
        connection,
        &discovery_row.correlation_id,
        "ProviderTransportAttemptStarted",
    )?;
    let Some(attempt_started_event) = attempt_started_event else {
        return Ok(None);
    };
    let discovery_failed_event =
        exactly_one_event(connection, &discovery_row.correlation_id, "ModelDiscoveryFailed")?;
    let Some(discovery_failed_event) = discovery_failed_event else {
        return Ok(None);
    };
    let discovery_started_payload: ModelDiscoveryStartedPayload =
        match serde_json::from_str(&discovery_started_event.payload_json) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
    let started_payload: ProviderTransportAttemptStartedPayload =
        match serde_json::from_str(&attempt_started_event.payload_json) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
    let failed_payload: ModelDiscoveryFailedPayload =
        match serde_json::from_str(&discovery_failed_event.payload_json) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
    if !event_is_frozen_system_fact(&discovery_started_event, &discovery_row.correlation_id)
        || !event_is_frozen_system_fact(&attempt_started_event, &discovery_row.correlation_id)
        || !event_is_frozen_system_fact(&discovery_failed_event, &discovery_row.correlation_id)
        || discovery_started_payload.discovery_run_id != discovery.id
        || discovery_started_payload.endpoint_id != discovery.endpoint_id
        || discovery_started_payload.run_revision != aura_domain_common::Revision::INITIAL
        || started_payload.transport_attempt_id != attempt.id
        || started_payload.endpoint_id != attempt.endpoint_id
        || started_payload.attempt_number != 1
        || started_payload.attempt_kind != TransportAttemptKind::Initial
        || started_payload.operation_context != attempt.request_evidence.operation_context
        || failed_payload.discovery_run_id != discovery.id
        || failed_payload.endpoint_id != discovery.endpoint_id
        || failed_payload.failure_id.to_string() != failure_id
        || failed_payload.run_revision != discovery.revision
        || terminal_attempt_event_count(connection, attempt_id)? != 0
        || operation_attempt_count(connection, &attempt_row.operation_context_json)? != 1
    {
        return Ok(None);
    }

    let discovery_failed_event_id = discovery_failed_event.event_id.clone();
    let canonical = CanonicalLegacyEvidenceV1 {
        source_schema_version,
        transport_attempt_id: attempt_id.to_owned(),
        discovery_run_id: discovery_id.clone(),
        failure_id: failure_id.clone(),
        discovery_failed_event_id: discovery_failed_event_id.clone(),
        attempt_row,
        discovery_row,
        failure_row,
        discovery_started_event,
        attempt_started_event,
        discovery_failed_event,
    };
    let canonical_bytes = serde_json::to_vec(&canonical)?;
    Ok(Some(LegacyEvidenceMatch {
        transport_attempt_id: attempt_id.to_owned(),
        discovery_run_id: discovery_id,
        failure_id,
        discovery_failed_event_id,
        original_state: "authorizing_network".to_owned(),
        canonical_evidence_hash: blake3::hash(&canonical_bytes).to_hex().to_string(),
    }))
}

fn exactly_one_event(
    connection: &Connection,
    correlation_id: &str,
    event_type: &str,
) -> Result<Option<CanonicalEventRow>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT global_sequence, event_id, event_type, event_version, actor,
                correlation_id, causation_id, atomic_group_id, payload_json,
                payload_artifact_ref, created_at
         FROM events WHERE correlation_id=?1 AND event_type=?2
         ORDER BY global_sequence",
    )?;
    let rows = statement
        .query_map(params![correlation_id, event_type], |row| {
            Ok(CanonicalEventRow {
                global_sequence: row.get(0)?,
                event_id: row.get(1)?,
                event_type: row.get(2)?,
                event_version: row.get(3)?,
                actor: row.get(4)?,
                correlation_id: row.get(5)?,
                causation_id: row.get(6)?,
                atomic_group_id: row.get(7)?,
                payload_json: row.get(8)?,
                payload_artifact_ref: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if rows.len() == 1 { rows.into_iter().next() } else { None })
}

fn event_is_frozen_system_fact(event: &CanonicalEventRow, correlation_id: &str) -> bool {
    event.event_version == 1
        && event.actor == "system_runtime"
        && event.correlation_id == correlation_id
        && event.causation_id.is_none()
        && event.atomic_group_id.is_none()
        && event.payload_artifact_ref.is_none()
}

fn terminal_attempt_event_count(
    connection: &Connection,
    attempt_id: &str,
) -> Result<i64, PersistenceError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM events
             WHERE event_type IN (
                'ProviderTransportAttemptCompleted',
                'ProviderTransportAttemptFailed',
                'ProviderTransportAttemptCancelled'
             )
             AND json_extract(payload_json, '$.transport_attempt_id')=?1",
            [attempt_id],
            |row| row.get(0),
        )
        .map_err(PersistenceError::from)
}

fn operation_attempt_count(
    connection: &Connection,
    operation_context_json: &str,
) -> Result<i64, PersistenceError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM transport_attempts WHERE operation_context_json=?1",
            [operation_context_json],
            |row| row.get(0),
        )
        .map_err(PersistenceError::from)
}
