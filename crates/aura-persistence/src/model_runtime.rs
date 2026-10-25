use std::str::FromStr;

use aura_domain_common::ToolCallId;
use aura_domain_common::{
    CorrelationId, ModelExecutionSnapshotId, ModelFailureId, ModelStepId, TransportAttemptId,
    UtcTimestamp,
};
use aura_model_domain::{
    CancellationSource, ModelExecutionSnapshot, NormalizedModelOutput, PersistedModelFailure,
    ProviderContinuationState, ProviderToolCallId, ProviderToolCallProtocolBindingRecord,
    ProviderTransportAttemptCancelledPayload, ProviderTransportAttemptCompletedPayload,
    ProviderTransportAttemptFailedPayload, ProviderTransportAttemptStartedPayload,
    TransportAttemptKind, TransportAttemptRecord, TransportAttemptState, TransportOperationContext,
    derive_provider_tool_call_protocol_bindings,
};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    PersistenceCoordinator, PersistenceError, PersistentEventDraft, coordinator::insert_event,
};

impl PersistenceCoordinator {
    pub fn store_model_execution_snapshot(
        &self,
        snapshot: &ModelExecutionSnapshot,
    ) -> Result<(), PersistenceError> {
        let snapshot = snapshot.clone();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            insert_model_execution_snapshot_in_transaction(&transaction, &snapshot, true)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn model_execution_snapshot(
        &self,
        snapshot_id: ModelExecutionSnapshotId,
    ) -> Result<Option<ModelExecutionSnapshot>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT snapshot_json FROM model_execution_snapshots
                     WHERE model_execution_snapshot_id=?1",
                    [snapshot_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .transpose()
        })
    }

    pub fn store_model_failure(
        &self,
        failure: &PersistedModelFailure,
    ) -> Result<(), PersistenceError> {
        let failure = failure.clone();
        let fingerprint_json = serde_json::to_string(&failure.fingerprint)?;
        let failure_json = serde_json::to_string(&failure.failure)?;
        self.dispatch(move |connection| {
            let existing = connection
                .query_row(
                    "SELECT failure_fingerprint_json, failure_json, created_at
                     FROM model_failures WHERE model_failure_id=?1",
                    [failure.id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing
                    == (
                        fingerprint_json.clone(),
                        failure_json.clone(),
                        failure.created_at.to_string(),
                    )
                {
                    return Ok(());
                }
                return Err(PersistenceError::InvalidRequest(
                    "model failure identity was reused for different failure data".to_owned(),
                ));
            }
            connection.execute(
                "INSERT INTO model_failures(
                    model_failure_id, failure_fingerprint_json, failure_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    failure.id.to_string(),
                    fingerprint_json,
                    failure_json,
                    failure.created_at.to_string(),
                ],
            )?;
            Ok(())
        })
    }

    pub fn model_failure(
        &self,
        failure_id: ModelFailureId,
    ) -> Result<Option<PersistedModelFailure>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT failure_fingerprint_json, failure_json, created_at
                     FROM model_failures WHERE model_failure_id=?1",
                    [failure_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
                .map(|(fingerprint, failure, created_at)| {
                    Ok(PersistedModelFailure {
                        id: failure_id,
                        fingerprint: serde_json::from_str(&fingerprint)?,
                        failure: serde_json::from_str(&failure)?,
                        created_at: created_at.parse().map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "persisted model failure timestamp is invalid".to_owned(),
                            )
                        })?,
                    })
                })
                .transpose()
        })
    }
}

pub(crate) fn insert_model_execution_snapshot_in_transaction(
    transaction: &Transaction<'_>,
    snapshot: &ModelExecutionSnapshot,
    allow_identical_existing: bool,
) -> Result<(), PersistenceError> {
    let snapshot_json = serde_json::to_string(snapshot)?;
    let existing = transaction
        .query_row(
            "SELECT snapshot_json FROM model_execution_snapshots
             WHERE model_execution_snapshot_id=?1",
            [snapshot.id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        if allow_identical_existing && existing == snapshot_json {
            return Ok(());
        }
        return Err(PersistenceError::InvalidRequest(
            "immutable model execution snapshot identity was reused".to_owned(),
        ));
    }
    transaction.execute(
        "INSERT INTO model_execution_snapshots(
            model_execution_snapshot_id, model_step_id, model_profile_id,
            endpoint_id, snapshot_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            snapshot.id.to_string(),
            snapshot.model_step_id.to_string(),
            snapshot.model_profile_id.to_string(),
            snapshot.endpoint_id.to_string(),
            snapshot_json,
            snapshot.created_at.to_string(),
        ],
    )?;
    Ok(())
}

impl PersistenceCoordinator {
    pub fn store_model_output(
        &self,
        snapshot: &ModelExecutionSnapshot,
        attempt_id: TransportAttemptId,
        output: &NormalizedModelOutput,
    ) -> Result<(), PersistenceError> {
        if output.model_step_id != snapshot.model_step_id {
            return Err(PersistenceError::InvalidRequest(
                "normalized model output does not belong to the execution snapshot".to_owned(),
            ));
        }
        let raw_response_artifact_id = output.raw_response_ref.ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "normalized model output must reference its raw provider stream".to_owned(),
            )
        })?;
        let snapshot = snapshot.clone();
        let output = output.clone();
        let output_json = serde_json::to_string(&output)?;
        let usage_json = serde_json::to_string(&output.usage)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (attempt_json, attempt_state) = transaction
                .query_row(
                    "SELECT record_json, state
                     FROM transport_attempts WHERE transport_attempt_id=?1",
                    [attempt_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "cannot store model output before its transport attempt".to_owned(),
                    )
                })?;
            let attempt: TransportAttemptRecord = serde_json::from_str(&attempt_json)?;
            let matches_snapshot = matches!(
                attempt.request_evidence.operation_context,
                TransportOperationContext::AgentModelAttempt {
                    model_step_id,
                    model_execution_snapshot_id,
                    context_snapshot_id,
                    prompt_bundle_snapshot_id,
                } if model_step_id == snapshot.model_step_id
                    && model_execution_snapshot_id == snapshot.id
                    && context_snapshot_id == snapshot.context_snapshot_id
                    && prompt_bundle_snapshot_id == snapshot.prompt_bundle_snapshot_id
            );
            if attempt_state != "completed" || !matches_snapshot {
                return Err(PersistenceError::InvalidRequest(
                    "model output attempt is not completed or belongs to another mex".to_owned(),
                ));
            }
            let protocol_bindings = derive_provider_tool_call_protocol_bindings(
                &snapshot,
                &attempt.request_evidence,
                &output,
            )
            .map_err(|error| {
                PersistenceError::InvalidRequest(format!(
                    "normalized tool-call protocol binding is invalid: {error}"
                ))
            })?;
            let existing = transaction
                .query_row(
                    "SELECT model_execution_snapshot_id, output_json, raw_response_artifact_id
                     FROM model_outputs WHERE model_step_id=?1",
                    [snapshot.model_step_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing
                    == (
                        snapshot.id.to_string(),
                        output_json.clone(),
                        Some(raw_response_artifact_id.to_string()),
                    )
                {
                    transaction.commit()?;
                    return Ok(());
                }
                return Err(PersistenceError::InvalidRequest(
                    "model step already has a different normalized output".to_owned(),
                ));
            }
            ensure_artifact_exists(&transaction, raw_response_artifact_id)?;
            if let Some(continuation) = &output.provider_continuation {
                ensure_artifact_exists(&transaction, continuation_artifact_id(continuation))?;
            }
            transaction.execute(
                "INSERT INTO model_outputs(
                    model_step_id, model_execution_snapshot_id, output_json,
                    raw_response_artifact_id, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    snapshot.model_step_id.to_string(),
                    snapshot.id.to_string(),
                    output_json,
                    raw_response_artifact_id.to_string(),
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            transaction.execute(
                "INSERT INTO model_usage(
                    model_step_id, model_execution_snapshot_id, transport_attempt_id,
                    usage_json, recorded_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    snapshot.model_step_id.to_string(),
                    snapshot.id.to_string(),
                    attempt_id.to_string(),
                    usage_json,
                    UtcTimestamp::now().to_string(),
                ],
            )?;
            if let Some(continuation) = &output.provider_continuation {
                transaction.execute(
                    "INSERT INTO provider_continuation_refs(
                        model_step_id, model_execution_snapshot_id, endpoint_id,
                        model_profile_id, continuation_json, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        snapshot.model_step_id.to_string(),
                        snapshot.id.to_string(),
                        snapshot.endpoint_id.to_string(),
                        snapshot.model_profile_id.to_string(),
                        serde_json::to_string(continuation)?,
                        UtcTimestamp::now().to_string(),
                    ],
                )?;
            }
            for binding in protocol_bindings {
                let Some(definition) = &binding.tool_definition else { continue };
                transaction.execute(
                    "INSERT INTO provider_tool_call_protocol_bindings(
                        tool_call_id, model_execution_snapshot_id, tool_id,
                        provider_wire_tool_name, provider_tool_call_id, created_at,
                        tool_definition_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        binding.tool_call_id.to_string(),
                        binding.model_execution_snapshot_id.to_string(),
                        definition.tool_id.to_string(),
                        binding.provider_wire_tool_name,
                        binding.provider_tool_call_id.as_str(),
                        UtcTimestamp::now().to_string(),
                        serde_json::to_string(&binding.tool_definition)?,
                    ],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn model_output(
        &self,
        model_step_id: ModelStepId,
    ) -> Result<Option<NormalizedModelOutput>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT output_json FROM model_outputs WHERE model_step_id=?1",
                    [model_step_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .transpose()
        })
    }

    pub fn provider_tool_call_protocol_bindings(
        &self,
        snapshot_id: ModelExecutionSnapshotId,
    ) -> Result<Vec<ProviderToolCallProtocolBindingRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT tool_call_id, tool_id, provider_wire_tool_name, provider_tool_call_id,
                        tool_definition_json
                 FROM provider_tool_call_protocol_bindings
                 WHERE model_execution_snapshot_id=?1
                 ORDER BY tool_call_id",
            )?;
            let rows = statement.query_map([snapshot_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?;
            rows.map(|row| {
                let (
                    tool_call_id,
                    tool_id,
                    provider_wire_tool_name,
                    provider_tool_call_id,
                    tool_definition_json,
                ) = row?;
                let tool_definition_json = tool_definition_json.ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "legacy provider tool-call binding has no Registry snapshot identity"
                            .to_owned(),
                    )
                })?;
                let tool_definition = serde_json::from_str::<
                    aura_tool_contract::ToolDefinitionSnapshotRef,
                >(&tool_definition_json)?;
                if tool_definition.tool_id.to_string() != tool_id {
                    return Err(PersistenceError::InvalidRequest(
                        "persisted provider tool-call binding identity is inconsistent".to_owned(),
                    ));
                }
                Ok(ProviderToolCallProtocolBindingRecord {
                    model_execution_snapshot_id: snapshot_id,
                    tool_call_id: ToolCallId::from_str(&tool_call_id).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "persisted ToolCallId is invalid".to_owned(),
                        )
                    })?,
                    // Only resolved bindings are stored; an unknown name has no row.
                    tool_definition: Some(tool_definition),
                    provider_wire_tool_name,
                    provider_tool_call_id: ProviderToolCallId::new(provider_tool_call_id).map_err(
                        |_| {
                            PersistenceError::InvalidRequest(
                                "persisted ProviderToolCallId is invalid".to_owned(),
                            )
                        },
                    )?,
                })
            })
            .collect()
        })
    }
}

fn continuation_artifact_id(state: &ProviderContinuationState) -> aura_domain_common::ArtifactId {
    match state {
        ProviderContinuationState::OpenAiResponses { state_ref }
        | ProviderContinuationState::Anthropic { state_ref }
        | ProviderContinuationState::Opaque { state_ref, .. } => *state_ref,
    }
}

fn ensure_artifact_exists(
    transaction: &rusqlite::Transaction<'_>,
    artifact_id: aura_domain_common::ArtifactId,
) -> Result<(), PersistenceError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifacts WHERE artifact_id=?1)",
        [artifact_id.to_string()],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(PersistenceError::InvalidRequest(
            "model output references a missing artifact".to_owned(),
        ));
    }
    Ok(())
}

impl PersistenceCoordinator {
    /// Creates an immutable Provider transport attempt and its Started event
    /// in one SQLite transaction. Callers must complete this commit before
    /// issuing the one-shot Provider grant or opening the network request.
    pub fn create_transport_attempt(
        &self,
        correlation_id: CorrelationId,
        attempt: &TransportAttemptRecord,
    ) -> Result<i64, PersistenceError> {
        let attempt = attempt.clone();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let event_row_id =
                insert_transport_attempt_started(&transaction, correlation_id, &attempt)?;
            transaction.commit()?;
            Ok(event_row_id)
        })
    }

    /// Commits a transport-attempt transition and emits a terminal Event only
    /// when the new state is Completed or Failed. Intermediate transitions do
    /// not create additional domain facts.
    pub fn transition_transport_attempt(
        &self,
        correlation_id: CorrelationId,
        attempt: &TransportAttemptRecord,
    ) -> Result<Option<i64>, PersistenceError> {
        let attempt = attempt.clone();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let event_row_id = transition_transport_attempt_in_transaction(
                &transaction,
                correlation_id,
                &attempt,
                None,
            )?;
            transaction.commit()?;
            Ok(event_row_id)
        })
    }

    pub fn cancel_transport_attempt(
        &self,
        correlation_id: CorrelationId,
        attempt: &TransportAttemptRecord,
        cancelled_by: CancellationSource,
    ) -> Result<i64, PersistenceError> {
        if attempt.state != TransportAttemptState::Cancelled {
            return Err(PersistenceError::InvalidRequest(
                "cancel transport transition requires a cancelled attempt".to_owned(),
            ));
        }
        let attempt = attempt.clone();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let event_row_id = transition_transport_attempt_in_transaction(
                &transaction,
                correlation_id,
                &attempt,
                Some(cancelled_by),
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "cancelled transport attempt did not emit its terminal event".to_owned(),
                )
            })?;
            transaction.commit()?;
            Ok(event_row_id)
        })
    }

    pub fn upsert_transport_attempt(
        &self,
        attempt: &TransportAttemptRecord,
    ) -> Result<(), PersistenceError> {
        validate_attempt_shape(attempt)?;
        let attempt = attempt.clone();
        let record_json = serde_json::to_string(&attempt)?;
        let operation_context_json =
            serde_json::to_string(&attempt.request_evidence.operation_context)?;
        self.dispatch(move |connection| {
            let existing = connection
                .query_row(
                    "SELECT record_json FROM transport_attempts WHERE transport_attempt_id=?1",
                    [attempt.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(existing_json) = existing {
                let existing: TransportAttemptRecord = serde_json::from_str(&existing_json)?;
                validate_attempt_update(&existing, &attempt)?;
            }
            connection.execute(
                "INSERT INTO transport_attempts(
                    transport_attempt_id, endpoint_id, operation_context_json,
                    attempt_number, attempt_kind, state, record_json,
                    canonical_body_artifact_id, raw_stream_artifact_id, model_failure_id,
                    started_at, finished_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(transport_attempt_id) DO UPDATE SET
                    state=excluded.state,
                    record_json=excluded.record_json,
                    raw_stream_artifact_id=excluded.raw_stream_artifact_id,
                    model_failure_id=excluded.model_failure_id,
                    finished_at=excluded.finished_at",
                params![
                    attempt.id.to_string(),
                    attempt.endpoint_id.to_string(),
                    operation_context_json,
                    i64::from(attempt.attempt_number),
                    attempt_kind_name(attempt.attempt_kind),
                    attempt_state_name(attempt.state),
                    record_json,
                    attempt.request_evidence.canonical_body_artifact_id.map(|id| id.to_string()),
                    attempt.raw_stream_artifact_id.map(|id| id.to_string()),
                    attempt.failure_id.map(|id| id.to_string()),
                    attempt.started_at.to_string(),
                    attempt.finished_at.map(|value| value.to_string()),
                ],
            )?;
            Ok(())
        })
    }

    pub fn transport_attempt(
        &self,
        attempt_id: TransportAttemptId,
    ) -> Result<Option<TransportAttemptRecord>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT record_json FROM transport_attempts WHERE transport_attempt_id=?1",
                    [attempt_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
                .transpose()
        })
    }

    /// Loads every non-terminal configuration transport attempt in stable
    /// start order. Agent-model attempts are deliberately excluded because
    /// their recovery owner is outside P05.
    pub fn nonterminal_configuration_transport_attempts(
        &self,
    ) -> Result<Vec<TransportAttemptRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT record_json FROM transport_attempts
                 WHERE state NOT IN ('completed', 'cancelled', 'failed')
                 ORDER BY started_at, transport_attempt_id",
            )?;
            let records = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            records
                .into_iter()
                .map(|json| {
                    serde_json::from_str::<TransportAttemptRecord>(&json)
                        .map_err(PersistenceError::from)
                })
                .filter_map(|record| match record {
                    Ok(record)
                        if matches!(
                            record.request_evidence.operation_context,
                            TransportOperationContext::ModelDiscovery { .. }
                                | TransportOperationContext::ModelAvailabilityTest { .. }
                        ) =>
                    {
                        Some(Ok(record))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect()
        })
    }
}

pub(crate) fn insert_transport_attempt_started(
    transaction: &rusqlite::Transaction<'_>,
    correlation_id: CorrelationId,
    attempt: &TransportAttemptRecord,
) -> Result<i64, PersistenceError> {
    validate_attempt_shape(attempt)?;
    if matches!(
        attempt.state,
        TransportAttemptState::Completed
            | TransportAttemptState::Cancelled
            | TransportAttemptState::Failed
    ) {
        return Err(PersistenceError::InvalidRequest(
            "a new transport attempt must be persisted before its terminal state".to_owned(),
        ));
    }
    if load_transport_attempt_record(transaction, attempt.id)?.is_some() {
        return Err(PersistenceError::InvalidRequest(
            "transport attempt identity was already used".to_owned(),
        ));
    }
    write_transport_attempt(transaction, attempt)?;
    insert_configuration_transport_attempt_relation(transaction, attempt)?;
    let event = PersistentEventDraft::system(
        "ProviderTransportAttemptStarted",
        correlation_id,
        serde_json::to_value(ProviderTransportAttemptStartedPayload {
            transport_attempt_id: attempt.id,
            operation_context: attempt.request_evidence.operation_context.clone(),
            endpoint_id: attempt.endpoint_id,
            attempt_number: attempt.attempt_number,
            attempt_kind: attempt.attempt_kind,
        })?,
    );
    insert_event(transaction, &event)
}

pub(crate) fn transition_transport_attempt_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    correlation_id: CorrelationId,
    attempt: &TransportAttemptRecord,
    cancelled_by: Option<CancellationSource>,
) -> Result<Option<i64>, PersistenceError> {
    validate_attempt_shape(attempt)?;
    let current = load_transport_attempt_record(transaction, attempt.id)?.ok_or_else(|| {
        PersistenceError::InvalidRequest(
            "transport attempt must be created before it can transition".to_owned(),
        )
    })?;
    validate_attempt_update(&current, attempt)?;
    write_transport_attempt(transaction, attempt)?;
    let event = match attempt.state {
        TransportAttemptState::Completed => {
            reject_cancellation_source(cancelled_by, "completed")?;
            Some(PersistentEventDraft::system(
                "ProviderTransportAttemptCompleted",
                correlation_id,
                serde_json::to_value(ProviderTransportAttemptCompletedPayload {
                    transport_attempt_id: attempt.id,
                    operation_context: attempt.request_evidence.operation_context.clone(),
                    provider_request_id: attempt.provider_request_id.clone(),
                    usage: None,
                })?,
            ))
        }
        TransportAttemptState::Failed => {
            reject_cancellation_source(cancelled_by, "failed")?;
            Some(PersistentEventDraft::system(
                "ProviderTransportAttemptFailed",
                correlation_id,
                serde_json::to_value(ProviderTransportAttemptFailedPayload {
                    transport_attempt_id: attempt.id,
                    operation_context: attempt.request_evidence.operation_context.clone(),
                    failure_id: attempt.failure_id.ok_or_else(|| {
                        PersistenceError::InvalidRequest(
                            "failed transport attempt is missing its failure ID".to_owned(),
                        )
                    })?,
                })?,
            ))
        }
        TransportAttemptState::Cancelled => Some(PersistentEventDraft::system(
            "ProviderTransportAttemptCancelled",
            correlation_id,
            serde_json::to_value(ProviderTransportAttemptCancelledPayload {
                transport_attempt_id: attempt.id,
                operation_context: attempt.request_evidence.operation_context.clone(),
                cancelled_by: cancelled_by.ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "cancelled transport attempt requires a cancellation source".to_owned(),
                    )
                })?,
                provider_request_id: attempt.provider_request_id.clone(),
            })?,
        )),
        TransportAttemptState::QueuedForEndpoint
        | TransportAttemptState::AcquiringPermit
        | TransportAttemptState::PreparingRequest
        | TransportAttemptState::AuthorizingNetwork
        | TransportAttemptState::Authorized
        | TransportAttemptState::DispatchPrepared
        | TransportAttemptState::Connecting
        | TransportAttemptState::ResponseStarted
        | TransportAttemptState::ResponseCompleted
        | TransportAttemptState::AwaitingFirstEvent
        | TransportAttemptState::Streaming => {
            reject_cancellation_source(cancelled_by, "non-terminal")?;
            None
        }
    };
    event.as_ref().map(|event| insert_event(transaction, event)).transpose()
}

fn reject_cancellation_source(
    cancelled_by: Option<CancellationSource>,
    state: &'static str,
) -> Result<(), PersistenceError> {
    if cancelled_by.is_some() {
        Err(PersistenceError::InvalidRequest(format!(
            "{state} transport attempt cannot carry a cancellation source"
        )))
    } else {
        Ok(())
    }
}

fn insert_configuration_transport_attempt_relation(
    transaction: &rusqlite::Transaction<'_>,
    attempt: &TransportAttemptRecord,
) -> Result<(), PersistenceError> {
    let (operation_kind, operation_id, model_profile_id) =
        match &attempt.request_evidence.operation_context {
            TransportOperationContext::ModelDiscovery { discovery_run_id } => {
                ("model_discovery", discovery_run_id.to_string(), None)
            }
            TransportOperationContext::ModelAvailabilityTest {
                model_availability_test_id,
                model_profile_id,
            } => (
                "model_availability_test",
                model_availability_test_id.to_string(),
                Some(model_profile_id.to_string()),
            ),
            TransportOperationContext::AgentModelAttempt { .. } => return Ok(()),
        };
    transaction.execute(
        "INSERT INTO configuration_transport_operation_attempts(
            transport_attempt_id, operation_kind, operation_id,
            model_profile_id, attempt_number, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            attempt.id.to_string(),
            operation_kind,
            operation_id,
            model_profile_id,
            i64::from(attempt.attempt_number),
            attempt.started_at.to_string(),
        ],
    )?;
    Ok(())
}

fn load_transport_attempt_record(
    connection: &rusqlite::Connection,
    attempt_id: TransportAttemptId,
) -> Result<Option<TransportAttemptRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT record_json FROM transport_attempts WHERE transport_attempt_id=?1",
            [attempt_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
        .transpose()
}

fn write_transport_attempt(
    connection: &rusqlite::Connection,
    attempt: &TransportAttemptRecord,
) -> Result<(), PersistenceError> {
    let record_json = serde_json::to_string(attempt)?;
    let operation_context_json =
        serde_json::to_string(&attempt.request_evidence.operation_context)?;
    connection.execute(
        "INSERT INTO transport_attempts(
            transport_attempt_id, endpoint_id, operation_context_json,
            attempt_number, attempt_kind, state, record_json,
            canonical_body_artifact_id, raw_stream_artifact_id, model_failure_id,
            started_at, finished_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(transport_attempt_id) DO UPDATE SET
            state=excluded.state,
            record_json=excluded.record_json,
            raw_stream_artifact_id=excluded.raw_stream_artifact_id,
            model_failure_id=excluded.model_failure_id,
            finished_at=excluded.finished_at",
        params![
            attempt.id.to_string(),
            attempt.endpoint_id.to_string(),
            operation_context_json,
            i64::from(attempt.attempt_number),
            attempt_kind_name(attempt.attempt_kind),
            attempt_state_name(attempt.state),
            record_json,
            attempt.request_evidence.canonical_body_artifact_id.map(|id| id.to_string()),
            attempt.raw_stream_artifact_id.map(|id| id.to_string()),
            attempt.failure_id.map(|id| id.to_string()),
            attempt.started_at.to_string(),
            attempt.finished_at.map(|value| value.to_string()),
        ],
    )?;
    Ok(())
}

fn validate_attempt_shape(attempt: &TransportAttemptRecord) -> Result<(), PersistenceError> {
    attempt.request_evidence.validate().map_err(|error| {
        PersistenceError::InvalidRequest(format!(
            "transport attempt request evidence is invalid: {error}"
        ))
    })?;
    let terminal = matches!(
        attempt.state,
        TransportAttemptState::Completed
            | TransportAttemptState::Cancelled
            | TransportAttemptState::Failed
    );
    let failure_shape_is_valid = match attempt.state {
        TransportAttemptState::Failed => attempt.failure_id.is_some(),
        _ => attempt.failure_id.is_none(),
    };
    if attempt.attempt_number == 0
        || terminal != attempt.finished_at.is_some()
        || !failure_shape_is_valid
    {
        return Err(PersistenceError::InvalidRequest(
            "transport attempt terminal fields are inconsistent".to_owned(),
        ));
    }
    Ok(())
}

fn validate_attempt_update(
    current: &TransportAttemptRecord,
    next: &TransportAttemptRecord,
) -> Result<(), PersistenceError> {
    if current.id != next.id
        || current.attempt_number != next.attempt_number
        || current.attempt_kind != next.attempt_kind
        || current.client_request_id != next.client_request_id
        || current.endpoint_id != next.endpoint_id
        || current.endpoint_fingerprint != next.endpoint_fingerprint
        || current.started_at != next.started_at
        || current.request_evidence != next.request_evidence
        || !optional_value_is_monotonic(&current.provider_request_id, &next.provider_request_id)
        || !optional_value_is_monotonic(&current.first_byte_at, &next.first_byte_at)
        || !optional_value_is_monotonic(&current.first_model_event_at, &next.first_model_event_at)
        || !optional_value_is_monotonic(
            &current.raw_stream_artifact_id,
            &next.raw_stream_artifact_id,
        )
        || !optional_value_is_monotonic(&current.failure_id, &next.failure_id)
        || !optional_value_is_monotonic(&current.finished_at, &next.finished_at)
        || !valid_attempt_transition(current.state, next.state)
    {
        return Err(PersistenceError::InvalidRequest(
            "transport attempt update changed immutable data or regressed state".to_owned(),
        ));
    }
    Ok(())
}

fn optional_value_is_monotonic<T: PartialEq>(current: &Option<T>, next: &Option<T>) -> bool {
    match (current, next) {
        (None, _) => true,
        (Some(current), Some(next)) => current == next,
        (Some(_), None) => false,
    }
}

fn valid_attempt_transition(current: TransportAttemptState, next: TransportAttemptState) -> bool {
    use TransportAttemptState::{
        AcquiringPermit, Authorized, AuthorizingNetwork, AwaitingFirstEvent, Cancelled, Completed,
        Connecting, DispatchPrepared, Failed, PreparingRequest, QueuedForEndpoint,
        ResponseCompleted, ResponseStarted, Streaming,
    };
    current == next
        || matches!(
            (current, next),
            (QueuedForEndpoint, AcquiringPermit)
                | (AcquiringPermit, PreparingRequest | Cancelled | Failed)
                | (PreparingRequest, AuthorizingNetwork | Cancelled | Failed)
                | (AuthorizingNetwork, Authorized | Connecting | Cancelled | Failed)
                | (Authorized, DispatchPrepared | Cancelled | Failed)
                | (DispatchPrepared, Connecting | Cancelled | Failed)
                | (Connecting, ResponseStarted | AwaitingFirstEvent | Cancelled | Failed)
                | (ResponseStarted, ResponseCompleted | Cancelled | Failed)
                | (ResponseCompleted, Completed | Cancelled | Failed)
                | (AwaitingFirstEvent, Streaming | Cancelled | Failed)
                | (Streaming, Completed | Cancelled | Failed)
        )
}

const fn attempt_kind_name(kind: TransportAttemptKind) -> &'static str {
    match kind {
        TransportAttemptKind::Initial => "initial",
        TransportAttemptKind::TransparentRetry => "transparent_retry",
        TransportAttemptKind::CompatibilityAdjustedRetry => "compatibility_adjusted_retry",
    }
}

const fn attempt_state_name(state: TransportAttemptState) -> &'static str {
    match state {
        TransportAttemptState::QueuedForEndpoint => "queued_for_endpoint",
        TransportAttemptState::AcquiringPermit => "acquiring_permit",
        TransportAttemptState::PreparingRequest => "preparing_request",
        TransportAttemptState::AuthorizingNetwork => "authorizing_network",
        TransportAttemptState::Authorized => "authorized",
        TransportAttemptState::DispatchPrepared => "dispatch_prepared",
        TransportAttemptState::Connecting => "connecting",
        TransportAttemptState::ResponseStarted => "response_started",
        TransportAttemptState::ResponseCompleted => "response_completed",
        TransportAttemptState::AwaitingFirstEvent => "awaiting_first_event",
        TransportAttemptState::Streaming => "streaming",
        TransportAttemptState::Completed => "completed",
        TransportAttemptState::Cancelled => "cancelled",
        TransportAttemptState::Failed => "failed",
    }
}
