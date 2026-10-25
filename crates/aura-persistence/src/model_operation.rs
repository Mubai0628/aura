use aura_domain_common::{CorrelationId, Revision};
use aura_model_domain::{
    CancellationSource, ModelAvailabilityTestCancelledPayload,
    ModelAvailabilityTestCompletedPayload, ModelAvailabilityTestProjection,
    ModelAvailabilityTestRecord, ModelAvailabilityTestStage, ModelAvailabilityTestState,
    ModelAvailabilityTestView, ModelDiscoveryCompletedPayload, ModelDiscoveryFailedPayload,
    ModelDiscoveryOutcome, ModelDiscoveryProjection, ModelDiscoveryRunRecord,
    ModelDiscoveryRunView, ModelDiscoveryStage, ModelDiscoveryStartedPayload, ModelDiscoveryState,
    ModelDiscoveryUnsupportedPayload, ModelDiscoveryUnsupportedReason,
    ModelOperationCancellationReason, TransportAttemptRecord, TransportAttemptState,
    TransportOperationContext,
};
use aura_projection_contract::ProjectionChange;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    ModelProjectionDraft, PersistenceCoordinator, PersistenceError, PersistentEventDraft,
    coordinator::{insert_event, to_sql_u64},
    model_projection::insert_model_projections,
    model_runtime::{
        insert_transport_attempt_started, transition_transport_attempt_in_transaction,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelDiscoveryTerminalEventContext {
    Completed { models_revision: Revision },
    Failed,
    Cancelled { cancelled_by: ModelOperationCancellationReason },
    Unsupported { reason_code: ModelDiscoveryUnsupportedReason },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelOperationCommit {
    pub event_row_ids: Vec<i64>,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

pub struct ModelDiscoveryTransportTransition<'a> {
    pub correlation_id: CorrelationId,
    pub expected_revision: Revision,
    pub next: &'a ModelDiscoveryRunRecord,
    pub terminal_event: Option<ModelDiscoveryTerminalEventContext>,
    pub attempt: &'a TransportAttemptRecord,
    pub cancelled_by: Option<CancellationSource>,
    pub projection_drafts: Vec<ModelProjectionDraft>,
}

pub struct ModelAvailabilityTransportTransition<'a> {
    pub correlation_id: CorrelationId,
    pub expected_revision: Revision,
    pub next: &'a ModelAvailabilityTestRecord,
    pub cancelled_by: Option<ModelOperationCancellationReason>,
    pub attempt: &'a TransportAttemptRecord,
    pub attempt_cancelled_by: Option<CancellationSource>,
    pub projection_drafts: Vec<ModelProjectionDraft>,
}

impl PersistenceCoordinator {
    pub fn create_model_discovery_run(
        &self,
        record: &ModelDiscoveryRunRecord,
        mut projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        record.validate().map_err(operation_contract_error)?;
        if record.revision != Revision::INITIAL || record.state != ModelDiscoveryState::Queued {
            return Err(PersistenceError::InvalidRequest(
                "a discovery run must be created as queued at revision one".to_owned(),
            ));
        }
        let record = record.clone();
        let record_json = serde_json::to_string(&record)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO model_discovery_runs(
                    discovery_run_id, endpoint_id, correlation_id, source_models_revision,
                    endpoint_fingerprint_json, revision, state, record_json,
                    raw_response_artifact_id, started_at, finished_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    record.id.to_string(),
                    record.endpoint_id.to_string(),
                    record.correlation_id.to_string(),
                    to_sql_u64(record.source_models_revision.value())?,
                    serde_json::to_string(&record.endpoint_fingerprint)?,
                    to_sql_u64(record.revision.value())?,
                    discovery_state_name(record.state),
                    record_json,
                    record.raw_response_artifact_id.map(|id| id.to_string()),
                    record.started_at.to_string(),
                    record.finished_at.map(|value| value.to_string()),
                ],
            )?;
            let event = PersistentEventDraft::system(
                "ModelDiscoveryStarted",
                record.correlation_id,
                serde_json::to_value(ModelDiscoveryStartedPayload {
                    discovery_run_id: record.id,
                    endpoint_id: record.endpoint_id,
                    run_revision: record.revision,
                })?,
            );
            let event_row_ids = vec![insert_event(&transaction, &event)?];
            projection_drafts.push(discovery_projection(&record));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(ModelOperationCommit { event_row_ids, projection_changes, projection_outbox_ids })
        })
    }

    pub fn transition_model_discovery_run(
        &self,
        expected_revision: Revision,
        next: &ModelDiscoveryRunRecord,
        terminal_event: Option<ModelDiscoveryTerminalEventContext>,
        mut projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        next.validate().map_err(operation_contract_error)?;
        validate_discovery_terminal_event(next, terminal_event)?;
        let next = next.clone();
        let record_json = serde_json::to_string(&next)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_discovery_run(&transaction, next.id)?
                .ok_or(PersistenceError::ModelDiscoveryRunNotFound)?;
            validate_discovery_transition(&transaction, &current, expected_revision, &next)?;
            let changed = transaction.execute(
                "UPDATE model_discovery_runs SET
                    revision=?1, state=?2, record_json=?3,
                    raw_response_artifact_id=?4, finished_at=?5
                 WHERE discovery_run_id=?6 AND revision=?7",
                params![
                    to_sql_u64(next.revision.value())?,
                    discovery_state_name(next.state),
                    record_json,
                    next.raw_response_artifact_id.map(|id| id.to_string()),
                    next.finished_at.map(|value| value.to_string()),
                    next.id.to_string(),
                    to_sql_u64(expected_revision.value())?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ModelDiscoveryRunStateConflict);
            }
            let mut event_row_ids = Vec::new();
            if let Some(context) = terminal_event {
                let event = discovery_terminal_event(&next, context)?;
                event_row_ids.push(insert_event(&transaction, &event)?);
            }
            projection_drafts.push(discovery_projection(&next));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(ModelOperationCommit { event_row_ids, projection_changes, projection_outbox_ids })
        })
    }

    /// Commits a terminal configuration transport Attempt together with its
    /// owning Discovery transition, terminal domain Event (when applicable),
    /// and Projection outbox rows. The Attempt Event is appended first so a
    /// cancelled/failed/completed Operation can never become durable while it
    /// still owns a non-terminal Attempt.
    pub fn transition_model_discovery_run_with_transport_attempt(
        &self,
        request: ModelDiscoveryTransportTransition<'_>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        let ModelDiscoveryTransportTransition {
            correlation_id,
            expected_revision,
            next,
            terminal_event,
            attempt,
            cancelled_by,
            mut projection_drafts,
        } = request;
        next.validate().map_err(operation_contract_error)?;
        validate_discovery_terminal_event(next, terminal_event)?;
        validate_terminal_discovery_attempt(next, correlation_id, attempt)?;
        let next = next.clone();
        let attempt = attempt.clone();
        let record_json = serde_json::to_string(&next)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_discovery_run(&transaction, next.id)?
                .ok_or(PersistenceError::ModelDiscoveryRunNotFound)?;
            let attempt_event_row_id = transition_transport_attempt_in_transaction(
                &transaction,
                correlation_id,
                &attempt,
                cancelled_by,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "terminal discovery transport attempt did not emit a terminal event".to_owned(),
                )
            })?;
            validate_discovery_transition(&transaction, &current, expected_revision, &next)?;
            let changed =
                update_discovery_run(&transaction, expected_revision, &next, &record_json)?;
            if changed != 1 {
                return Err(PersistenceError::ModelDiscoveryRunStateConflict);
            }
            let mut event_row_ids = vec![attempt_event_row_id];
            if let Some(context) = terminal_event {
                event_row_ids
                    .push(insert_event(&transaction, &discovery_terminal_event(&next, context)?)?);
            }
            projection_drafts.push(discovery_projection(&next));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(ModelOperationCommit { event_row_ids, projection_changes, projection_outbox_ids })
        })
    }

    /// Creates a configuration transport Attempt and advances its Discovery
    /// owner to Running in one transaction. This closes the queued-cancel race:
    /// either cancellation wins before an Attempt exists, or the Operation
    /// already owns the Attempt and its Worker must terminalize it first.
    pub fn start_model_discovery_transport_attempt(
        &self,
        correlation_id: CorrelationId,
        expected_revision: Revision,
        next: &ModelDiscoveryRunRecord,
        attempt: &TransportAttemptRecord,
        mut projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        next.validate().map_err(operation_contract_error)?;
        validate_discovery_terminal_event(next, None)?;
        let next = next.clone();
        let attempt = attempt.clone();
        let record_json = serde_json::to_string(&next)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_discovery_run(&transaction, next.id)?
                .ok_or(PersistenceError::ModelDiscoveryRunNotFound)?;
            let started_event_row_id =
                insert_transport_attempt_started(&transaction, correlation_id, &attempt)?;
            validate_discovery_transition(&transaction, &current, expected_revision, &next)?;
            let changed = transaction.execute(
                "UPDATE model_discovery_runs SET
                    revision=?1, state=?2, record_json=?3,
                    raw_response_artifact_id=?4, finished_at=?5
                 WHERE discovery_run_id=?6 AND revision=?7",
                params![
                    to_sql_u64(next.revision.value())?,
                    discovery_state_name(next.state),
                    record_json,
                    next.raw_response_artifact_id.map(|id| id.to_string()),
                    next.finished_at.map(|value| value.to_string()),
                    next.id.to_string(),
                    to_sql_u64(expected_revision.value())?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ModelDiscoveryRunStateConflict);
            }
            projection_drafts.push(discovery_projection(&next));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            let event_row_ids = vec![started_event_row_id];
            transaction.commit()?;
            Ok(ModelOperationCommit { event_row_ids, projection_changes, projection_outbox_ids })
        })
    }

    pub fn model_discovery_run(
        &self,
        discovery_run_id: aura_domain_common::ModelDiscoveryRunId,
    ) -> Result<Option<ModelDiscoveryRunRecord>, PersistenceError> {
        self.with_reader(|connection| load_discovery_run(connection, discovery_run_id))
    }

    pub fn latest_model_discovery_run_for_endpoint(
        &self,
        endpoint_id: aura_domain_common::EndpointId,
    ) -> Result<Option<ModelDiscoveryRunRecord>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT record_json FROM model_discovery_runs
                     WHERE endpoint_id=?1
                     ORDER BY started_at DESC, discovery_run_id DESC LIMIT 1",
                    [endpoint_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .transpose()
        })
    }

    pub fn active_model_discovery_runs(
        &self,
    ) -> Result<Vec<ModelDiscoveryRunRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT record_json FROM model_discovery_runs
                 WHERE state IN ('queued', 'running')
                 ORDER BY started_at, discovery_run_id",
            )?;
            let records = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            records
                .into_iter()
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .collect()
        })
    }

    pub fn create_model_availability_test(
        &self,
        record: &ModelAvailabilityTestRecord,
        mut projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        record.validate().map_err(operation_contract_error)?;
        if record.revision != Revision::INITIAL
            || record.state != ModelAvailabilityTestState::Queued
            || record.stage.is_some()
        {
            return Err(PersistenceError::InvalidRequest(
                "a model availability test must be created as queued with no stage at revision one"
                    .to_owned(),
            ));
        }
        let record = record.clone();
        let record_json = serde_json::to_string(&record)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO model_availability_tests(
                    model_availability_test_id, endpoint_id, model_profile_id, correlation_id,
                    models_revision_at_start, endpoint_revision_at_start,
                    model_profile_revision_at_start, endpoint_security_fingerprint_json,
                    revision, state, stage, record_json, raw_response_artifact_id,
                    started_at, completed_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
                 )",
                params![
                    record.id.to_string(),
                    record.endpoint_id.to_string(),
                    record.model_profile_id.to_string(),
                    record.correlation_id.to_string(),
                    to_sql_u64(record.models_revision_at_start.value())?,
                    to_sql_u64(record.endpoint_revision_at_start.value())?,
                    to_sql_u64(record.model_profile_revision_at_start.value())?,
                    serde_json::to_string(&record.endpoint_security_fingerprint)?,
                    to_sql_u64(record.revision.value())?,
                    availability_test_state_name(record.state),
                    record.stage.map(availability_test_stage_name),
                    record_json,
                    record.raw_response_artifact_id.map(|id| id.to_string()),
                    record.started_at.to_string(),
                    record.completed_at.map(|value| value.to_string()),
                ],
            )?;
            projection_drafts.push(availability_test_projection(&record));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(ModelOperationCommit {
                event_row_ids: Vec::new(),
                projection_changes,
                projection_outbox_ids,
            })
        })
    }

    pub fn transition_model_availability_test(
        &self,
        expected_revision: Revision,
        next: &ModelAvailabilityTestRecord,
        cancelled_by: Option<ModelOperationCancellationReason>,
        mut projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        next.validate().map_err(operation_contract_error)?;
        validate_availability_terminal_event(next, cancelled_by)?;
        let next = next.clone();
        let record_json = serde_json::to_string(&next)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_availability_test(&transaction, next.id)?
                .ok_or(PersistenceError::ModelAvailabilityTestNotFound)?;
            validate_availability_test_transition(
                &transaction,
                &current,
                expected_revision,
                &next,
            )?;
            let changed = transaction.execute(
                "UPDATE model_availability_tests SET
                    revision=?1, state=?2, stage=?3, record_json=?4,
                    raw_response_artifact_id=?5, completed_at=?6
                 WHERE model_availability_test_id=?7 AND revision=?8",
                params![
                    to_sql_u64(next.revision.value())?,
                    availability_test_state_name(next.state),
                    next.stage.map(availability_test_stage_name),
                    record_json,
                    next.raw_response_artifact_id.map(|id| id.to_string()),
                    next.completed_at.map(|value| value.to_string()),
                    next.id.to_string(),
                    to_sql_u64(expected_revision.value())?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ModelAvailabilityTestStateConflict);
            }
            let mut event_row_ids = Vec::new();
            if let Some(event) = availability_terminal_event(&next, cancelled_by)? {
                event_row_ids.push(insert_event(&transaction, &event)?);
            }
            projection_drafts.push(availability_test_projection(&next));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(ModelOperationCommit { event_row_ids, projection_changes, projection_outbox_ids })
        })
    }

    /// Commits a terminal configuration transport Attempt and its Availability
    /// owner in the same SQLite transaction. This is the sole normal worker
    /// path for completing or cancelling an Attempt owned by a test.
    pub fn transition_model_availability_test_with_transport_attempt(
        &self,
        request: ModelAvailabilityTransportTransition<'_>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        let ModelAvailabilityTransportTransition {
            correlation_id,
            expected_revision,
            next,
            cancelled_by,
            attempt,
            attempt_cancelled_by,
            mut projection_drafts,
        } = request;
        next.validate().map_err(operation_contract_error)?;
        validate_availability_terminal_event(next, cancelled_by)?;
        validate_terminal_availability_attempt(next, correlation_id, attempt)?;
        let next = next.clone();
        let attempt = attempt.clone();
        let record_json = serde_json::to_string(&next)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_availability_test(&transaction, next.id)?
                .ok_or(PersistenceError::ModelAvailabilityTestNotFound)?;
            let attempt_event_row_id = transition_transport_attempt_in_transaction(
                &transaction,
                correlation_id,
                &attempt,
                attempt_cancelled_by,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "terminal availability transport attempt did not emit a terminal event"
                        .to_owned(),
                )
            })?;
            validate_availability_test_transition(
                &transaction,
                &current,
                expected_revision,
                &next,
            )?;
            let changed =
                update_availability_test(&transaction, expected_revision, &next, &record_json)?;
            if changed != 1 {
                return Err(PersistenceError::ModelAvailabilityTestStateConflict);
            }
            let mut event_row_ids = vec![attempt_event_row_id];
            if let Some(event) = availability_terminal_event(&next, cancelled_by)? {
                event_row_ids.push(insert_event(&transaction, &event)?);
            }
            projection_drafts.push(availability_test_projection(&next));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            transaction.commit()?;
            Ok(ModelOperationCommit { event_row_ids, projection_changes, projection_outbox_ids })
        })
    }

    /// Creates a configuration transport Attempt and advances its Availability
    /// owner to Running in one transaction. The Started event and owner binding
    /// therefore become visible at the same commit point.
    pub fn start_model_availability_transport_attempt(
        &self,
        correlation_id: CorrelationId,
        expected_revision: Revision,
        next: &ModelAvailabilityTestRecord,
        attempt: &TransportAttemptRecord,
        mut projection_drafts: Vec<ModelProjectionDraft>,
    ) -> Result<ModelOperationCommit, PersistenceError> {
        next.validate().map_err(operation_contract_error)?;
        validate_availability_terminal_event(next, None)?;
        let next = next.clone();
        let attempt = attempt.clone();
        let record_json = serde_json::to_string(&next)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let current = load_availability_test(&transaction, next.id)?
                .ok_or(PersistenceError::ModelAvailabilityTestNotFound)?;
            let started_event_row_id =
                insert_transport_attempt_started(&transaction, correlation_id, &attempt)?;
            validate_availability_test_transition(
                &transaction,
                &current,
                expected_revision,
                &next,
            )?;
            let changed = transaction.execute(
                "UPDATE model_availability_tests SET
                    revision=?1, state=?2, stage=?3, record_json=?4,
                    raw_response_artifact_id=?5, completed_at=?6
                 WHERE model_availability_test_id=?7 AND revision=?8",
                params![
                    to_sql_u64(next.revision.value())?,
                    availability_test_state_name(next.state),
                    next.stage.map(availability_test_stage_name),
                    record_json,
                    next.raw_response_artifact_id.map(|id| id.to_string()),
                    next.completed_at.map(|value| value.to_string()),
                    next.id.to_string(),
                    to_sql_u64(expected_revision.value())?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ModelAvailabilityTestStateConflict);
            }
            projection_drafts.push(availability_test_projection(&next));
            let (projection_changes, projection_outbox_ids) =
                insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
            let event_row_ids = vec![started_event_row_id];
            transaction.commit()?;
            Ok(ModelOperationCommit { event_row_ids, projection_changes, projection_outbox_ids })
        })
    }

    pub fn model_availability_test(
        &self,
        model_availability_test_id: aura_domain_common::ModelAvailabilityTestId,
    ) -> Result<Option<ModelAvailabilityTestRecord>, PersistenceError> {
        self.with_reader(|connection| {
            load_availability_test(connection, model_availability_test_id)
        })
    }

    pub fn latest_model_availability_test_for_endpoint(
        &self,
        endpoint_id: aura_domain_common::EndpointId,
    ) -> Result<Option<ModelAvailabilityTestRecord>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT record_json FROM model_availability_tests
                     WHERE endpoint_id=?1
                     ORDER BY started_at DESC, model_availability_test_id DESC LIMIT 1",
                    [endpoint_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .transpose()
        })
    }

    pub fn latest_model_availability_test_for_profile(
        &self,
        model_profile_id: aura_domain_common::ModelProfileId,
    ) -> Result<Option<ModelAvailabilityTestRecord>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT record_json FROM model_availability_tests
                     WHERE model_profile_id=?1
                     ORDER BY started_at DESC, model_availability_test_id DESC LIMIT 1",
                    [model_profile_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .transpose()
        })
    }

    pub fn active_model_availability_tests(
        &self,
    ) -> Result<Vec<ModelAvailabilityTestRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT record_json FROM model_availability_tests
                 WHERE state IN ('queued', 'running')
                 ORDER BY started_at, model_availability_test_id",
            )?;
            let records = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            records
                .into_iter()
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .collect()
        })
    }
}

fn load_discovery_run(
    connection: &Connection,
    discovery_run_id: aura_domain_common::ModelDiscoveryRunId,
) -> Result<Option<ModelDiscoveryRunRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT record_json FROM model_discovery_runs WHERE discovery_run_id=?1",
            [discovery_run_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
        .transpose()
}

fn load_availability_test(
    connection: &Connection,
    model_availability_test_id: aura_domain_common::ModelAvailabilityTestId,
) -> Result<Option<ModelAvailabilityTestRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT record_json FROM model_availability_tests
             WHERE model_availability_test_id=?1",
            [model_availability_test_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
        .transpose()
}

fn update_discovery_run(
    transaction: &Transaction<'_>,
    expected_revision: Revision,
    next: &ModelDiscoveryRunRecord,
    record_json: &str,
) -> Result<usize, PersistenceError> {
    Ok(transaction.execute(
        "UPDATE model_discovery_runs SET
            revision=?1, state=?2, record_json=?3,
            raw_response_artifact_id=?4, finished_at=?5
         WHERE discovery_run_id=?6 AND revision=?7",
        params![
            to_sql_u64(next.revision.value())?,
            discovery_state_name(next.state),
            record_json,
            next.raw_response_artifact_id.map(|id| id.to_string()),
            next.finished_at.map(|value| value.to_string()),
            next.id.to_string(),
            to_sql_u64(expected_revision.value())?,
        ],
    )?)
}

fn update_availability_test(
    transaction: &Transaction<'_>,
    expected_revision: Revision,
    next: &ModelAvailabilityTestRecord,
    record_json: &str,
) -> Result<usize, PersistenceError> {
    Ok(transaction.execute(
        "UPDATE model_availability_tests SET
            revision=?1, state=?2, stage=?3, record_json=?4,
            raw_response_artifact_id=?5, completed_at=?6
         WHERE model_availability_test_id=?7 AND revision=?8",
        params![
            to_sql_u64(next.revision.value())?,
            availability_test_state_name(next.state),
            next.stage.map(availability_test_stage_name),
            record_json,
            next.raw_response_artifact_id.map(|id| id.to_string()),
            next.completed_at.map(|value| value.to_string()),
            next.id.to_string(),
            to_sql_u64(expected_revision.value())?,
        ],
    )?)
}

fn validate_discovery_transition(
    transaction: &Transaction<'_>,
    current: &ModelDiscoveryRunRecord,
    expected_revision: Revision,
    next: &ModelDiscoveryRunRecord,
) -> Result<(), PersistenceError> {
    if current.revision != expected_revision
        || current.state.is_terminal()
        || next.revision.value() != next_revision(expected_revision)?
        || current.id != next.id
        || current.correlation_id != next.correlation_id
        || current.endpoint_id != next.endpoint_id
        || current.source_models_revision != next.source_models_revision
        || current.endpoint_revision_at_start != next.endpoint_revision_at_start
        || current.endpoint_fingerprint != next.endpoint_fingerprint
        || current.started_at != next.started_at
        || discovery_state_rank(next.state) < discovery_state_rank(current.state)
        || !discovery_stage_is_monotonic(current.stage, next.stage)
        || !next.transport_attempt_ids.starts_with(&current.transport_attempt_ids)
        || !optional_is_monotonic(&current.raw_response_artifact_id, &next.raw_response_artifact_id)
        || !optional_is_monotonic(&current.result, &next.result)
        || !optional_is_monotonic(&current.failure_id, &next.failure_id)
        || !optional_is_monotonic(&current.outcome, &next.outcome)
        || !optional_is_monotonic(&current.cancellation_reason, &next.cancellation_reason)
        || !optional_is_monotonic(&current.provider_error, &next.provider_error)
        || !optional_is_monotonic(&current.finished_at, &next.finished_at)
        || records_equal_except_revision(current, next)
    {
        return Err(PersistenceError::ModelDiscoveryRunStateConflict);
    }
    validate_discovery_attempt_ownership(transaction, current, next)?;
    if next.state.is_terminal() {
        ensure_discovery_attempts_terminal(transaction, next)?;
    }
    if let Some(artifact_id) = next.raw_response_artifact_id {
        ensure_artifact_exists(transaction, artifact_id)?;
    }
    Ok(())
}

fn validate_availability_test_transition(
    transaction: &Transaction<'_>,
    current: &ModelAvailabilityTestRecord,
    expected_revision: Revision,
    next: &ModelAvailabilityTestRecord,
) -> Result<(), PersistenceError> {
    if current.revision != expected_revision
        || current.state.is_terminal()
        || next.revision.value() != next_revision(expected_revision)?
        || current.id != next.id
        || current.correlation_id != next.correlation_id
        || current.endpoint_id != next.endpoint_id
        || current.model_profile_id != next.model_profile_id
        || current.models_revision_at_start != next.models_revision_at_start
        || current.endpoint_revision_at_start != next.endpoint_revision_at_start
        || current.model_profile_revision_at_start != next.model_profile_revision_at_start
        || current.endpoint_security_fingerprint != next.endpoint_security_fingerprint
        || current.endpoint_snapshot != next.endpoint_snapshot
        || current.model_profile_snapshot != next.model_profile_snapshot
        || current.credential_status_at_start != next.credential_status_at_start
        || current.credential_binding_fingerprint != next.credential_binding_fingerprint
        || current.started_at != next.started_at
        || availability_test_state_rank(next.state) < availability_test_state_rank(current.state)
        || !availability_stage_is_monotonic(current.stage, next.stage)
        || !next.direct_transport_attempt_ids.starts_with(&current.direct_transport_attempt_ids)
        || !optional_is_monotonic(
            &current.applied_output_token_field_alias,
            &next.applied_output_token_field_alias,
        )
        || !optional_is_monotonic(&current.result, &next.result)
        || !optional_is_monotonic(&current.cancellation_reason, &next.cancellation_reason)
        || !optional_is_monotonic(&current.raw_response_artifact_id, &next.raw_response_artifact_id)
        || !optional_is_monotonic(&current.completed_at, &next.completed_at)
        || availability_test_records_equal_except_revision(current, next)
    {
        return Err(PersistenceError::ModelAvailabilityTestStateConflict);
    }
    validate_availability_test_attempt_ownership(transaction, current, next)?;
    if next.state.is_terminal() {
        ensure_availability_attempts_terminal(transaction, next)?;
    }
    if let Some(artifact_id) = next.raw_response_artifact_id {
        ensure_artifact_exists(transaction, artifact_id)?;
    }
    Ok(())
}

fn validate_discovery_attempt_ownership(
    transaction: &Transaction<'_>,
    current: &ModelDiscoveryRunRecord,
    next: &ModelDiscoveryRunRecord,
) -> Result<(), PersistenceError> {
    for attempt_id in &next.transport_attempt_ids[current.transport_attempt_ids.len()..] {
        let attempt = load_transport_attempt(transaction, *attempt_id)?;
        if !matches!(
            attempt.request_evidence.operation_context,
            TransportOperationContext::ModelDiscovery { discovery_run_id, .. }
                if discovery_run_id == next.id
        ) {
            return Err(PersistenceError::InvalidRequest(
                "discovery run may only record attempts it directly owns".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_availability_test_attempt_ownership(
    transaction: &Transaction<'_>,
    current: &ModelAvailabilityTestRecord,
    next: &ModelAvailabilityTestRecord,
) -> Result<(), PersistenceError> {
    for attempt_id in
        &next.direct_transport_attempt_ids[current.direct_transport_attempt_ids.len()..]
    {
        let attempt = load_transport_attempt(transaction, *attempt_id)?;
        if !matches!(
            attempt.request_evidence.operation_context,
            TransportOperationContext::ModelAvailabilityTest {
                model_availability_test_id,
                model_profile_id,
            } if model_availability_test_id == next.id && model_profile_id == next.model_profile_id
        ) {
            return Err(PersistenceError::InvalidRequest(
                "availability test may only record attempts it directly owns".to_owned(),
            ));
        }
    }
    Ok(())
}

fn load_transport_attempt(
    connection: &Connection,
    attempt_id: aura_domain_common::TransportAttemptId,
) -> Result<TransportAttemptRecord, PersistenceError> {
    connection
        .query_row(
            "SELECT record_json FROM transport_attempts WHERE transport_attempt_id=?1",
            [attempt_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "model operation references an unknown transport attempt".to_owned(),
            )
        })
        .and_then(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
}

fn validate_terminal_discovery_attempt(
    next: &ModelDiscoveryRunRecord,
    correlation_id: CorrelationId,
    attempt: &TransportAttemptRecord,
) -> Result<(), PersistenceError> {
    let owned = matches!(
        attempt.request_evidence.operation_context,
        TransportOperationContext::ModelDiscovery { discovery_run_id }
            if discovery_run_id == next.id
    );
    if correlation_id != next.correlation_id
        || attempt.endpoint_id != next.endpoint_id
        || !owned
        || next.transport_attempt_ids.last().copied() != Some(attempt.id)
        || !transport_attempt_state_is_terminal(attempt.state)
    {
        return Err(PersistenceError::InvalidRequest(
            "terminal discovery transport attempt does not match its direct owner".to_owned(),
        ));
    }
    Ok(())
}

fn validate_terminal_availability_attempt(
    next: &ModelAvailabilityTestRecord,
    correlation_id: CorrelationId,
    attempt: &TransportAttemptRecord,
) -> Result<(), PersistenceError> {
    let owned = matches!(
        attempt.request_evidence.operation_context,
        TransportOperationContext::ModelAvailabilityTest {
            model_availability_test_id,
            model_profile_id,
        } if model_availability_test_id == next.id
            && model_profile_id == next.model_profile_id
    );
    if correlation_id != next.correlation_id
        || attempt.endpoint_id != next.endpoint_id
        || !owned
        || next.direct_transport_attempt_ids.last().copied() != Some(attempt.id)
        || !transport_attempt_state_is_terminal(attempt.state)
    {
        return Err(PersistenceError::InvalidRequest(
            "terminal availability transport attempt does not match its direct owner".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_discovery_attempts_terminal(
    transaction: &Transaction<'_>,
    record: &ModelDiscoveryRunRecord,
) -> Result<(), PersistenceError> {
    ensure_recorded_attempts_terminal(transaction, &record.transport_attempt_ids)?;
    ensure_relation_has_no_nonterminal_attempt(
        transaction,
        "model_discovery",
        &record.id.to_string(),
    )
}

fn ensure_availability_attempts_terminal(
    transaction: &Transaction<'_>,
    record: &ModelAvailabilityTestRecord,
) -> Result<(), PersistenceError> {
    ensure_recorded_attempts_terminal(transaction, &record.direct_transport_attempt_ids)?;
    ensure_relation_has_no_nonterminal_attempt(
        transaction,
        "model_availability_test",
        &record.id.to_string(),
    )
}

fn ensure_recorded_attempts_terminal(
    transaction: &Transaction<'_>,
    attempt_ids: &[aura_domain_common::TransportAttemptId],
) -> Result<(), PersistenceError> {
    for attempt_id in attempt_ids {
        let attempt = load_transport_attempt(transaction, *attempt_id)?;
        if !transport_attempt_state_is_terminal(attempt.state) {
            return Err(PersistenceError::InvalidRequest(
                "a terminal model operation cannot reference a non-terminal transport attempt"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

fn ensure_relation_has_no_nonterminal_attempt(
    transaction: &Transaction<'_>,
    operation_kind: &str,
    operation_id: &str,
) -> Result<(), PersistenceError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1
              FROM configuration_transport_operation_attempts relation
              JOIN transport_attempts attempt
                ON attempt.transport_attempt_id=relation.transport_attempt_id
             WHERE relation.operation_kind=?1
               AND relation.operation_id=?2
               AND attempt.state NOT IN ('completed', 'cancelled', 'failed')
         )",
        params![operation_kind, operation_id],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Err(PersistenceError::InvalidRequest(
            "a terminal model operation still owns an unresolved transport attempt".to_owned(),
        ))
    } else {
        Ok(())
    }
}

const fn transport_attempt_state_is_terminal(state: TransportAttemptState) -> bool {
    matches!(
        state,
        TransportAttemptState::Completed
            | TransportAttemptState::Cancelled
            | TransportAttemptState::Failed
    )
}

fn validate_discovery_terminal_event(
    record: &ModelDiscoveryRunRecord,
    terminal_event: Option<ModelDiscoveryTerminalEventContext>,
) -> Result<(), PersistenceError> {
    let matches = matches!(
        (record.state, record.outcome, terminal_event),
        (
            ModelDiscoveryState::Completed,
            Some(ModelDiscoveryOutcome::Succeeded | ModelDiscoveryOutcome::Partial),
            Some(ModelDiscoveryTerminalEventContext::Completed { .. })
        ) | (
            ModelDiscoveryState::Completed,
            Some(ModelDiscoveryOutcome::Failed),
            Some(ModelDiscoveryTerminalEventContext::Failed)
        ) | (
            ModelDiscoveryState::Cancelled,
            None,
            Some(ModelDiscoveryTerminalEventContext::Cancelled { .. })
        ) | (
            ModelDiscoveryState::Completed,
            Some(ModelDiscoveryOutcome::Unsupported),
            Some(ModelDiscoveryTerminalEventContext::Unsupported { .. })
        ) | (ModelDiscoveryState::Queued | ModelDiscoveryState::Running, None, None)
    );
    if matches {
        Ok(())
    } else {
        Err(PersistenceError::InvalidRequest(
            "discovery terminal state and event context do not match".to_owned(),
        ))
    }
}

fn discovery_terminal_event(
    record: &ModelDiscoveryRunRecord,
    context: ModelDiscoveryTerminalEventContext,
) -> Result<PersistentEventDraft, PersistenceError> {
    let (event_type, payload) = match context {
        ModelDiscoveryTerminalEventContext::Completed { models_revision } => {
            let result = record.result.as_ref().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "completed discovery is missing its merge result".to_owned(),
                )
            })?;
            let returned_count = u32::try_from(result.returned_profiles.len()).map_err(|_| {
                PersistenceError::InvalidRequest(
                    "discovery returned model count exceeds contract range".to_owned(),
                )
            })?;
            let created_count = u32::try_from(
                result.returned_profiles.iter().filter(|profile| profile.created).count(),
            )
            .map_err(|_| {
                PersistenceError::InvalidRequest(
                    "discovery created model count exceeds contract range".to_owned(),
                )
            })?;
            let not_returned_count =
                u32::try_from(result.not_returned_profile_ids.len()).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "discovery missing model count exceeds contract range".to_owned(),
                    )
                })?;
            (
                "ModelDiscoveryCompleted",
                serde_json::to_value(ModelDiscoveryCompletedPayload {
                    discovery_run_id: record.id,
                    endpoint_id: record.endpoint_id,
                    outcome: record.outcome.ok_or_else(|| {
                        PersistenceError::InvalidRequest(
                            "completed discovery is missing its outcome".to_owned(),
                        )
                    })?,
                    returned_count,
                    created_count,
                    not_returned_count,
                    run_revision: record.revision,
                    models_revision,
                })?,
            )
        }
        ModelDiscoveryTerminalEventContext::Failed => (
            "ModelDiscoveryFailed",
            serde_json::to_value(ModelDiscoveryFailedPayload {
                discovery_run_id: record.id,
                endpoint_id: record.endpoint_id,
                failure_id: record.failure_id.ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "failed discovery is missing its model failure".to_owned(),
                    )
                })?,
                run_revision: record.revision,
            })?,
        ),
        ModelDiscoveryTerminalEventContext::Cancelled { cancelled_by } => (
            "ModelDiscoveryCancelled",
            serde_json::to_value(aura_model_domain::ModelDiscoveryCancelledPayload {
                discovery_run_id: record.id,
                endpoint_id: record.endpoint_id,
                cancelled_by,
                run_revision: record.revision,
            })?,
        ),
        ModelDiscoveryTerminalEventContext::Unsupported { reason_code } => (
            "ModelDiscoveryUnsupported",
            serde_json::to_value(ModelDiscoveryUnsupportedPayload {
                discovery_run_id: record.id,
                endpoint_id: record.endpoint_id,
                reason_code,
                run_revision: record.revision,
            })?,
        ),
    };
    Ok(PersistentEventDraft::system(event_type, record.correlation_id, payload))
}

fn validate_availability_terminal_event(
    record: &ModelAvailabilityTestRecord,
    cancelled_by: Option<ModelOperationCancellationReason>,
) -> Result<(), PersistenceError> {
    let matches = match record.state {
        ModelAvailabilityTestState::Completed => cancelled_by.is_none(),
        ModelAvailabilityTestState::Cancelled => cancelled_by.is_some(),
        ModelAvailabilityTestState::Queued | ModelAvailabilityTestState::Running => {
            cancelled_by.is_none()
        }
    };
    if matches {
        Ok(())
    } else {
        Err(PersistenceError::InvalidRequest(
            "availability test terminal state and cancellation context do not match".to_owned(),
        ))
    }
}

fn availability_terminal_event(
    record: &ModelAvailabilityTestRecord,
    cancelled_by: Option<ModelOperationCancellationReason>,
) -> Result<Option<PersistentEventDraft>, PersistenceError> {
    let event = match record.state {
        ModelAvailabilityTestState::Completed => {
            let result = record.result.clone().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "completed availability test is missing its result".to_owned(),
                )
            })?;
            Some(PersistentEventDraft::system(
                "ModelAvailabilityTestCompleted",
                record.correlation_id,
                serde_json::to_value(ModelAvailabilityTestCompletedPayload {
                    model_availability_test_id: record.id,
                    endpoint_id: record.endpoint_id,
                    model_profile_id: record.model_profile_id,
                    endpoint_revision_at_start: record.endpoint_revision_at_start,
                    model_profile_revision_at_start: record.model_profile_revision_at_start,
                    models_revision_at_start: record.models_revision_at_start,
                    endpoint_security_fingerprint: record.endpoint_security_fingerprint.clone(),
                    result,
                    test_revision: record.revision,
                })?,
            ))
        }
        ModelAvailabilityTestState::Cancelled => Some(PersistentEventDraft::system(
            "ModelAvailabilityTestCancelled",
            record.correlation_id,
            serde_json::to_value(ModelAvailabilityTestCancelledPayload {
                model_availability_test_id: record.id,
                endpoint_id: record.endpoint_id,
                model_profile_id: record.model_profile_id,
                stage: record.stage,
                cancelled_by: cancelled_by.ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "cancelled availability test is missing its cancellation source".to_owned(),
                    )
                })?,
                test_revision: record.revision,
            })?,
        )),
        ModelAvailabilityTestState::Queued | ModelAvailabilityTestState::Running => None,
    };
    Ok(event)
}

fn availability_test_projection(record: &ModelAvailabilityTestRecord) -> ModelProjectionDraft {
    ModelProjectionDraft::ModelAvailabilityTest(Box::new(ModelAvailabilityTestProjection {
        model_availability_test_id: record.id,
        test: ModelAvailabilityTestView::from_record(record),
    }))
}

fn discovery_projection(record: &ModelDiscoveryRunRecord) -> ModelProjectionDraft {
    ModelProjectionDraft::ModelDiscovery(Box::new(ModelDiscoveryProjection {
        model_discovery_run_id: record.id,
        run: ModelDiscoveryRunView::from_record(record),
    }))
}

fn ensure_artifact_exists(
    transaction: &Transaction<'_>,
    artifact_id: aura_domain_common::ArtifactId,
) -> Result<(), PersistenceError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifacts WHERE artifact_id=?1)",
        [artifact_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if exists { Ok(()) } else { Err(PersistenceError::ArtifactNotFound) }
}

fn next_revision(revision: Revision) -> Result<u64, PersistenceError> {
    revision.value().checked_add(1).ok_or_else(|| {
        PersistenceError::InvalidRequest("operational revision is exhausted".to_owned())
    })
}

fn optional_is_monotonic<T: PartialEq>(current: &Option<T>, next: &Option<T>) -> bool {
    match (current, next) {
        (None, _) => true,
        (Some(current), Some(next)) => current == next,
        (Some(_), None) => false,
    }
}

fn records_equal_except_revision(
    current: &ModelDiscoveryRunRecord,
    next: &ModelDiscoveryRunRecord,
) -> bool {
    let mut next = next.clone();
    next.revision = current.revision;
    current == &next
}

fn availability_test_records_equal_except_revision(
    current: &ModelAvailabilityTestRecord,
    next: &ModelAvailabilityTestRecord,
) -> bool {
    let mut next = next.clone();
    next.revision = current.revision;
    current == &next
}

const fn discovery_state_rank(state: ModelDiscoveryState) -> u8 {
    match state {
        ModelDiscoveryState::Queued => 1,
        ModelDiscoveryState::Running => 2,
        ModelDiscoveryState::Completed | ModelDiscoveryState::Cancelled => 3,
    }
}

const fn discovery_stage_rank(stage: ModelDiscoveryStage) -> u8 {
    match stage {
        ModelDiscoveryStage::Requesting => 1,
        ModelDiscoveryStage::Parsing => 2,
        ModelDiscoveryStage::EnrichingCatalog => 3,
        ModelDiscoveryStage::Completed => 4,
    }
}

fn discovery_stage_is_monotonic(
    current: Option<ModelDiscoveryStage>,
    next: Option<ModelDiscoveryStage>,
) -> bool {
    match (current, next) {
        (None, None) => true,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (Some(current), Some(next)) => discovery_stage_rank(next) >= discovery_stage_rank(current),
    }
}

const fn availability_test_state_rank(state: ModelAvailabilityTestState) -> u8 {
    match state {
        ModelAvailabilityTestState::Queued => 1,
        ModelAvailabilityTestState::Running => 2,
        ModelAvailabilityTestState::Completed | ModelAvailabilityTestState::Cancelled => 3,
    }
}

const fn availability_test_stage_rank(stage: ModelAvailabilityTestStage) -> u8 {
    match stage {
        ModelAvailabilityTestStage::Config => 1,
        ModelAvailabilityTestStage::Network => 2,
        ModelAvailabilityTestStage::Tls => 3,
        ModelAvailabilityTestStage::Http => 4,
        ModelAvailabilityTestStage::Authentication => 5,
        ModelAvailabilityTestStage::ModelGeneration => 6,
        ModelAvailabilityTestStage::Completed => 7,
    }
}

fn availability_stage_is_monotonic(
    current: Option<ModelAvailabilityTestStage>,
    next: Option<ModelAvailabilityTestStage>,
) -> bool {
    match (current, next) {
        (None, None) => true,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (Some(current), Some(next)) => {
            availability_test_stage_rank(next) >= availability_test_stage_rank(current)
        }
    }
}

const fn discovery_state_name(state: ModelDiscoveryState) -> &'static str {
    match state {
        ModelDiscoveryState::Queued => "queued",
        ModelDiscoveryState::Running => "running",
        ModelDiscoveryState::Completed => "completed",
        ModelDiscoveryState::Cancelled => "cancelled",
    }
}

const fn availability_test_state_name(state: ModelAvailabilityTestState) -> &'static str {
    match state {
        ModelAvailabilityTestState::Queued => "queued",
        ModelAvailabilityTestState::Running => "running",
        ModelAvailabilityTestState::Completed => "completed",
        ModelAvailabilityTestState::Cancelled => "cancelled",
    }
}

const fn availability_test_stage_name(stage: ModelAvailabilityTestStage) -> &'static str {
    match stage {
        ModelAvailabilityTestStage::Config => "config",
        ModelAvailabilityTestStage::Network => "network",
        ModelAvailabilityTestStage::Tls => "tls",
        ModelAvailabilityTestStage::Http => "http",
        ModelAvailabilityTestStage::Authentication => "authentication",
        ModelAvailabilityTestStage::ModelGeneration => "model_generation",
        ModelAvailabilityTestStage::Completed => "completed",
    }
}

fn operation_contract_error(error: impl std::fmt::Display) -> PersistenceError {
    PersistenceError::InvalidRequest(format!("model operation contract is invalid: {error}"))
}
