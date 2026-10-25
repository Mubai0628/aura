mod tool_calls;
use crate::tool_reference::resolved as resolved_tool;
use std::str::FromStr;
use tool_calls::{build_logical_assistant_tool_calls, load_tool_continuation_results};

use aura_artifact_contract::ArtifactAccess;
use aura_context_domain::{
    ContextProjectionSnapshot, ContextSourceRef, HistoryAtomicGroupKindV1,
    HistoryAtomicGroupPreimageV1, HistoryAtomicGroupRefV1, HistoryToolCallResultEntryV1,
    LegacyHistoryAtomicGroupVerificationV1, ModelEgressAuthorizationBasis,
    ProviderContinuationBindingV1,
};
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_json_jcs, canonical_sha256};
use aura_domain_common::{
    ArtifactId, ContentHash, ContentHashAlgorithm, EventId, ModelExecutionSnapshotId, SessionId,
    UtcTimestamp,
};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolWireVersionV1};
use aura_model_domain::{
    LogicalConversationItem, LogicalHistoricalToolCall, ModelApiFormat, ModelExecutionSnapshot,
    NormalizedModelOutput, NormalizedStopReason, ProviderContinuationState, TransportAttemptRecord,
    TransportAttemptState, TransportOperationContext,
};
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, NormalizedToolCallV1, ToolBatchRequestV1, ToolBatchResultV1,
    ToolCallStatusV1, ToolResultOutcomeV1, ToolResultV1,
};
use aura_tool_domain::ToolDefinitionRecordV1;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    ArtifactReadRequest, PersistenceCoordinator, PersistenceError,
    tool_entities::{decode_json, decode_verified_record, encode_json},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolHistoryAtomicGroupAssociationRequestV1 {
    pub tool_batch_id: ToolBatchId,
    pub assistant_response_event_id: EventId,
    pub assistant_response_content_hash: CanonicalSha256HashV1,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoredToolHistoryAtomicGroupAssociationV1 {
    Pending {
        tool_batch_id: ToolBatchId,
        assistant_response_event_id: EventId,
        assistant_response_content_hash: CanonicalSha256HashV1,
    },
    Finalized {
        tool_batch_id: ToolBatchId,
        preimage: HistoryAtomicGroupPreimageV1,
        group_ref: HistoryAtomicGroupRefV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyHistoryAtomicGroupSideMappingV1 {
    pub session_id: SessionId,
    pub legacy_atomic_group_id: String,
    pub verification: LegacyHistoryAtomicGroupVerificationV1,
    pub group_ref: Option<HistoryAtomicGroupRefV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedToolContinuationAuthorityV1 {
    pub originating_user_event_id: EventId,
    pub previous_model_execution_id: ModelExecutionSnapshotId,
    pub history_atomic_group: HistoryAtomicGroupPreimageV1,
    pub recent_history_items: Vec<LogicalConversationItem>,
    pub durable_result_tool_call_ids: Vec<ToolCallId>,
    pub provider_continuation: Option<ProviderContinuationBindingV1>,
}

struct ToolContinuationReadSnapshotV1 {
    originating_user_event_id: EventId,
    history_atomic_group: HistoryAtomicGroupPreimageV1,
    batch: ToolBatchRequestV1,
    execution: ModelExecutionSnapshot,
    previous_history: Vec<LogicalConversationItem>,
    assistant_content: String,
    result_rows: Vec<ToolContinuationResultReadV1>,
    provider_continuation: Option<ProviderContinuationBindingV1>,
}

struct ToolContinuationResultReadV1 {
    tool_call_id: ToolCallId,
    finalized_event_id: EventId,
    model_visible_result_artifact_id: ArtifactId,
    model_visible_result_byte_length: u64,
    canonical_name: String,
    is_error: bool,
}

impl PersistenceCoordinator {
    /// Associates a persisted model-visible Assistant Response with one exact
    /// admitted ToolBatch. The association remains pending until the batch and
    /// every unique admitted call have durable terminal results.
    pub fn commit_pending_tool_history_atomic_group_v1(
        &self,
        request: ToolHistoryAtomicGroupAssociationRequestV1,
    ) -> Result<StoredToolHistoryAtomicGroupAssociationV1, PersistenceError> {
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let batch = load_batch_request(&transaction, request.tool_batch_id)?;
            verify_assistant_response_event(&transaction, &batch, &request)?;
            if let Some(existing) = load_association(&transaction, request.tool_batch_id)? {
                match &existing {
                    StoredToolHistoryAtomicGroupAssociationV1::Pending {
                        assistant_response_event_id,
                        assistant_response_content_hash,
                        ..
                    } if *assistant_response_event_id == request.assistant_response_event_id
                        && assistant_response_content_hash
                            == &request.assistant_response_content_hash => {}
                    StoredToolHistoryAtomicGroupAssociationV1::Finalized { group_ref, .. }
                        if group_ref.assistant_response_event_id
                            == request.assistant_response_event_id => {}
                    _ => return Err(PersistenceError::ToolEntityStateConflict),
                }
                transaction.commit()?;
                return Ok(existing);
            }
            let changed = transaction.execute(
                "INSERT INTO tool_history_atomic_group_associations(
                    tool_batch_id, assistant_response_event_id,
                    assistant_response_content_hash_json, state, created_at
                 ) VALUES (?1, ?2, ?3, 'pending', ?4)",
                params![
                    request.tool_batch_id.to_string(),
                    request.assistant_response_event_id.to_string(),
                    encode_json(&request.assistant_response_content_hash)?,
                    request.created_at.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            transaction.commit()?;
            Ok(StoredToolHistoryAtomicGroupAssociationV1::Pending {
                tool_batch_id: request.tool_batch_id,
                assistant_response_event_id: request.assistant_response_event_id,
                assistant_response_content_hash: request.assistant_response_content_hash,
            })
        })
    }

    pub fn load_tool_history_atomic_group_v1(
        &self,
        tool_batch_id: ToolBatchId,
    ) -> Result<Option<StoredToolHistoryAtomicGroupAssociationV1>, PersistenceError> {
        self.with_reader(|connection| load_association(connection, tool_batch_id))
    }

    /// Reconstructs the exact durable P06 continuation authority for one
    /// terminal ToolBatch. This never creates a User Event or a continuation
    /// trigger; P08 remains the sole owner of that transition.
    pub fn load_verified_tool_continuation_authority_v1(
        &self,
        tool_batch_id: ToolBatchId,
        originating_user_event_id: EventId,
    ) -> Result<Option<VerifiedToolContinuationAuthorityV1>, PersistenceError> {
        let Some(snapshot) = self.with_reader(|connection| {
            load_tool_continuation_read_snapshot(
                connection,
                tool_batch_id,
                originating_user_event_id,
            )
        })?
        else {
            return Ok(None);
        };

        let mut recent_history_items = snapshot.previous_history;
        let assistant_tool_calls =
            build_logical_assistant_tool_calls(&snapshot.batch, &snapshot.result_rows)?;
        recent_history_items.push(LogicalConversationItem::Assistant {
            provider_history: None,
            source_event_id: snapshot.history_atomic_group.assistant_response_event_id,
            content: Some(snapshot.assistant_content.clone()),
            content_hash: Some(ContentHash::sha256(snapshot.assistant_content.as_bytes())),
            tool_calls: assistant_tool_calls,
        });
        let mut durable_result_tool_call_ids = Vec::with_capacity(snapshot.result_rows.len());
        for row in snapshot.result_rows {
            let length = usize::try_from(row.model_visible_result_byte_length)
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let bytes = self.read_artifact_range(ArtifactReadRequest {
                artifact_id: row.model_visible_result_artifact_id,
                session_id: Some(snapshot.history_atomic_group.session_id),
                required_access: ArtifactAccess::MODEL_READ,
                offset: 0,
                length,
                allow_quarantine: false,
            })?;
            if bytes.len() != length {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            let content =
                String::from_utf8(bytes).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            aura_domain_common::jcs::parse_json_strict(content.as_bytes())
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            recent_history_items.push(LogicalConversationItem::ToolResult {
                source_event_id: row.finalized_event_id,
                tool_call_id: row.tool_call_id,
                tool_name: row.canonical_name,
                content_hash: ContentHash::sha256(content.as_bytes()),
                content,
                is_error: row.is_error,
            });
            durable_result_tool_call_ids.push(row.tool_call_id);
        }

        Ok(Some(VerifiedToolContinuationAuthorityV1 {
            originating_user_event_id: snapshot.originating_user_event_id,
            previous_model_execution_id: snapshot.execution.id,
            history_atomic_group: snapshot.history_atomic_group,
            recent_history_items,
            durable_result_tool_call_ids,
            provider_continuation: snapshot.provider_continuation,
        }))
    }

    /// Records a non-destructive V0 side mapping. An opaque legacy group is
    /// deliberately retained as unverified unless a caller supplies the exact
    /// already-finalized V1 group fact that was reconstructed independently.
    pub fn record_legacy_history_atomic_group_mapping_v1(
        &self,
        mapping: LegacyHistoryAtomicGroupSideMappingV1,
        mapped_at: UtcTimestamp,
    ) -> Result<(), PersistenceError> {
        validate_legacy_mapping(&mapping)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (assistant_response_event_id, group_hash_json) = mapping
                .group_ref
                .as_ref()
                .map(|group_ref| -> Result<(Option<String>, Option<String>), PersistenceError> {
                    Ok((
                        Some(group_ref.assistant_response_event_id.to_string()),
                        Some(encode_json(&group_ref.group_hash)?),
                    ))
                })
                .transpose()?
                .unwrap_or((None, None));
            let changed = transaction.execute(
                "INSERT INTO context_legacy_history_atomic_group_mappings(
                    session_id, legacy_atomic_group_id, verification,
                    assistant_response_event_id, group_hash_json, mapped_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id, legacy_atomic_group_id) DO UPDATE SET
                    verification=excluded.verification,
                    assistant_response_event_id=excluded.assistant_response_event_id,
                    group_hash_json=excluded.group_hash_json,
                    mapped_at=excluded.mapped_at
                 WHERE verification=excluded.verification
                   AND COALESCE(assistant_response_event_id, '')
                       = COALESCE(excluded.assistant_response_event_id, '')
                   AND COALESCE(group_hash_json, '')=COALESCE(excluded.group_hash_json, '')",
                params![
                    mapping.session_id.to_string(),
                    mapping.legacy_atomic_group_id,
                    legacy_verification_name(mapping.verification),
                    assistant_response_event_id,
                    group_hash_json,
                    mapped_at.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            transaction.commit()?;
            Ok(())
        })
    }
}

pub(crate) fn finalize_tool_history_atomic_group_if_pending(
    transaction: &Transaction<'_>,
    tool_batch_id: ToolBatchId,
    finalized_at: UtcTimestamp,
) -> Result<Option<HistoryAtomicGroupRefV1>, PersistenceError> {
    let Some(association) = load_association(transaction, tool_batch_id)? else {
        return Ok(None);
    };
    if let StoredToolHistoryAtomicGroupAssociationV1::Finalized { group_ref, .. } = association {
        return Ok(Some(group_ref));
    }
    let StoredToolHistoryAtomicGroupAssociationV1::Pending {
        assistant_response_event_id,
        assistant_response_content_hash,
        ..
    } = association
    else {
        unreachable!("association variants were exhausted")
    };
    let batch = load_batch_request(transaction, tool_batch_id)?;
    let batch_result = load_completed_batch_result(transaction, &batch)?;
    let mut calls = Vec::with_capacity(batch.calls.len());
    for expected in &batch.calls {
        let row = transaction
            .query_row(
                "SELECT normalized_call_json, normalized_call_hash_json, status
                 FROM tool_calls WHERE tool_batch_id=?1 AND tool_call_id=?2 AND ordinal=?3",
                params![
                    tool_batch_id.to_string(),
                    expected.tool_call_id.to_string(),
                    i64::from(expected.ordinal),
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        if row.2 != call_status_name(ToolCallStatusV1::Finalized) {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
        let stored_call = decode_verified_record::<NormalizedToolCallV1>(&row.0, &row.1)?;
        if stored_call != *expected {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        let result_row = transaction
            .query_row(
                "SELECT result_json, result_hash_json FROM tool_results
                 WHERE tool_batch_id=?1 AND tool_call_id=?2 AND ordinal=?3",
                params![
                    tool_batch_id.to_string(),
                    expected.tool_call_id.to_string(),
                    i64::from(expected.ordinal),
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
        let result = decode_verified_record::<ToolResultV1>(&result_row.0, &result_row.1)?;
        if result.tool_call_id != expected.tool_call_id {
            return Err(PersistenceError::ToolEntityIntegrity);
        }
        calls.push(HistoryToolCallResultEntryV1 {
            ordinal: expected.ordinal,
            tool_call_id: expected.tool_call_id,
            tool_call_fact_hash: decode_json(&row.1)?,
            tool_result_fact_hash: decode_json(&result_row.1)?,
        });
    }
    let preimage = HistoryAtomicGroupPreimageV1 {
        kind: HistoryAtomicGroupKindV1,
        preimage_version: ToolWireVersionV1,
        session_id: batch.invocation_identity.session_id,
        turn_task_id: batch.invocation_identity.turn_task_id,
        agent_run_id: batch.invocation_identity.agent_run_id,
        model_step_id: batch.invocation_identity.model_step_id,
        assistant_response_event_id,
        assistant_response_content_hash,
        tool_batch_id,
        calls,
        batch_summary_hash: aura_domain_common::jcs::canonical_sha256(&batch_result)
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
    };
    let group_ref = preimage.group_ref().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let changed = transaction.execute(
        "UPDATE tool_history_atomic_group_associations
         SET state='finalized', preimage_json=?1, group_hash_json=?2,
             finalized_at=?3
         WHERE tool_batch_id=?4 AND state='pending'
           AND preimage_json IS NULL AND group_hash_json IS NULL",
        params![
            encode_json(&preimage)?,
            encode_json(&group_ref.group_hash)?,
            finalized_at.to_string(),
            tool_batch_id.to_string(),
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok(Some(group_ref))
}

fn load_tool_continuation_read_snapshot(
    connection: &rusqlite::Connection,
    tool_batch_id: ToolBatchId,
    originating_user_event_id: EventId,
) -> Result<Option<ToolContinuationReadSnapshotV1>, PersistenceError> {
    let Some(association) = load_association(connection, tool_batch_id)? else {
        return Ok(None);
    };
    let StoredToolHistoryAtomicGroupAssociationV1::Finalized {
        preimage: history_atomic_group,
        group_ref,
        ..
    } = association
    else {
        return Err(PersistenceError::ToolEntityStateConflict);
    };
    if history_atomic_group.group_ref().map_err(|_| PersistenceError::ToolEntityIntegrity)?
        != group_ref
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let batch = load_batch_request(connection, tool_batch_id)?;
    let _batch_result = load_completed_batch_result(connection, &batch)?;
    if batch.invocation_identity.session_id != history_atomic_group.session_id
        || batch.invocation_identity.turn_task_id != history_atomic_group.turn_task_id
        || batch.invocation_identity.agent_run_id != history_atomic_group.agent_run_id
        || batch.invocation_identity.model_step_id != history_atomic_group.model_step_id
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }

    let model_row = connection
        .query_row(
            "SELECT m.snapshot_json, o.output_json, o.raw_response_artifact_id
             FROM model_outputs o
             JOIN model_execution_snapshots m
               ON m.model_execution_snapshot_id=o.model_execution_snapshot_id
             WHERE o.model_step_id=?1",
            [history_atomic_group.model_step_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((execution_json, output_json, raw_response_artifact_id)) = model_row else {
        return Err(PersistenceError::ToolEntityStateConflict);
    };
    let execution = decode_json::<ModelExecutionSnapshot>(&execution_json)?;
    let output = decode_json::<NormalizedModelOutput>(&output_json)?;
    if execution.model_step_id != history_atomic_group.model_step_id
        || output.model_step_id != history_atomic_group.model_step_id
        || output.stop_reason != NormalizedStopReason::ToolCalls
        || output.tool_calls.len() != batch.calls.len()
        || output
            .tool_calls
            .iter()
            .zip(&batch.calls)
            .any(|(output_call, batch_call)| output_call.tool_call_id != batch_call.tool_call_id)
        || output.raw_response_ref.map(|value| value.to_string()) != raw_response_artifact_id
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    validate_previous_context_identity(connection, &execution, &history_atomic_group)?;
    let previous_history =
        load_previous_history(connection, &execution, originating_user_event_id)?;
    validate_originating_user_event(
        connection,
        &execution,
        history_atomic_group.session_id,
        originating_user_event_id,
        &previous_history,
    )?;
    let assistant_content = load_assistant_response_content(connection, &history_atomic_group)?;
    validate_completed_model_transport(connection, &execution, &output)?;
    let provider_continuation =
        load_provider_continuation_binding(connection, &execution, &output, &history_atomic_group)?;
    let result_rows = load_tool_continuation_results(connection, &batch, &history_atomic_group)?;
    Ok(Some(ToolContinuationReadSnapshotV1 {
        originating_user_event_id,
        history_atomic_group,
        batch,
        execution,
        previous_history,
        assistant_content,
        result_rows,
        provider_continuation,
    }))
}

fn validate_previous_context_identity(
    connection: &rusqlite::Connection,
    execution: &ModelExecutionSnapshot,
    group: &HistoryAtomicGroupPreimageV1,
) -> Result<(), PersistenceError> {
    let row = connection
        .query_row(
            "SELECT session_id, turn_task_id, agent_run_id, model_step_id
             FROM context_projection_snapshots WHERE context_snapshot_id=?1",
            [execution.context_snapshot_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityStateConflict)?;
    if row
        != (
            group.session_id.to_string(),
            group.turn_task_id.to_string(),
            group.agent_run_id.to_string(),
            group.model_step_id.to_string(),
        )
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn load_previous_history(
    connection: &rusqlite::Connection,
    execution: &ModelExecutionSnapshot,
    originating_user_event_id: EventId,
) -> Result<Vec<LogicalConversationItem>, PersistenceError> {
    let (snapshot_json, snapshot_hash_json) = connection
        .query_row(
            "SELECT snapshot_json, snapshot_hash_json
             FROM context_projection_snapshots WHERE context_snapshot_id=?1",
            [execution.context_snapshot_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityStateConflict)?;
    let snapshot = decode_json::<ContextProjectionSnapshot>(&snapshot_json)?;
    snapshot.validate_contract_shape().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if snapshot.id != execution.context_snapshot_id
        || snapshot.snapshot_hash != decode_json::<ContentHash>(&snapshot_hash_json)?
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let mut statement = connection.prepare(
        "SELECT item_json FROM context_projection_conversation_items
         WHERE context_snapshot_id=?1 ORDER BY ordinal",
    )?;
    let items = statement
        .query_map([execution.context_snapshot_id.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|encoded| decode_json::<LogicalConversationItem>(&encoded))
        .collect::<Result<Vec<_>, _>>()?;
    if items != snapshot.conversation_items
        || items.is_empty()
        || items.iter().any(|item| !logical_history_item_hash_is_valid(item))
        || !matches!(
            items.last(),
            Some(LogicalConversationItem::User { source_event_id, .. })
                if *source_event_id == originating_user_event_id
        )
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(items)
}

fn logical_history_item_hash_is_valid(item: &LogicalConversationItem) -> bool {
    match item {
        LogicalConversationItem::User { content, content_hash, .. }
        | LogicalConversationItem::ToolResult { content, content_hash, .. } => {
            ContentHash::sha256(content.as_bytes()) == *content_hash
        }
        LogicalConversationItem::Assistant { content, content_hash, tool_calls, .. } => {
            content.is_some() == content_hash.is_some()
                && content
                    .as_ref()
                    .zip(content_hash.as_ref())
                    .is_none_or(|(content, hash)| ContentHash::sha256(content.as_bytes()) == *hash)
                && tool_calls.iter().all(|call| {
                    ContentHash::sha256(call.arguments_json.as_bytes()) == call.arguments_hash
                        && serde_json::from_str::<serde_json::Value>(&call.arguments_json).is_ok()
                })
        }
    }
}

fn validate_originating_user_event(
    connection: &rusqlite::Connection,
    execution: &ModelExecutionSnapshot,
    session_id: SessionId,
    originating_user_event_id: EventId,
    previous_history: &[LogicalConversationItem],
) -> Result<(), PersistenceError> {
    let user_present = previous_history.iter().any(|item| {
        matches!(
            item,
            LogicalConversationItem::User { source_event_id, .. }
                if *source_event_id == originating_user_event_id
        )
    });
    if !user_present {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let row = connection
        .query_row(
            "SELECT e.session_id, e.actor, f.content_hash_json, f.byte_length,
                    f.model_visible, f.explicitly_user_authored
             FROM events e JOIN context_source_event_facts f USING(event_id)
             WHERE e.event_id=?1",
            [originating_user_event_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityStateConflict)?;
    let source_content_hash = decode_json::<ContentHash>(&row.2)?;
    if row.0 != session_id.to_string()
        || row.1 != "user"
        || source_content_hash.algorithm != ContentHashAlgorithm::Sha256
        || row.3 <= 0
        || !row.4
        || !row.5
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let mut statement = connection.prepare(
        "SELECT source_ref_json, authorization_basis_json,
                content_hash_json, byte_length
         FROM context_projection_source_authorizations
         WHERE context_snapshot_id=?1",
    )?;
    let matches = statement
        .query_map([execution.context_snapshot_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(source, basis, content_hash, byte_length)| {
            decode_json::<ContextSourceRef>(source).is_ok_and(|source| {
                source == ContextSourceRef::Event { event_id: originating_user_event_id }
            }) && decode_json::<ModelEgressAuthorizationBasis>(basis)
                .is_ok_and(|basis| basis == ModelEgressAuthorizationBasis::UserSubmission)
                && decode_json::<ContentHash>(content_hash)
                    .is_ok_and(|content_hash| content_hash == source_content_hash)
                && *byte_length == row.3
        })
        .count();
    if matches != 1 {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn load_assistant_response_content(
    connection: &rusqlite::Connection,
    group: &HistoryAtomicGroupPreimageV1,
) -> Result<String, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT e.session_id, e.turn_task_id, e.agent_run_id, e.event_type, e.actor,
                    e.payload_json, f.content_hash_json, f.byte_length,
                    f.model_visible, f.explicitly_user_authored
             FROM events e JOIN context_source_event_facts f USING(event_id)
             WHERE e.event_id=?1",
            [group.assistant_response_event_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, bool>(9)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityStateConflict)?;
    let payload = row.5.as_deref().ok_or(PersistenceError::ToolEntityIntegrity)?;
    let content = decode_json::<serde_json::Value>(payload)?
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let content_hash = ContentHash::sha256(content.as_bytes());
    if row.0 != group.session_id.to_string()
        || row.1.as_deref() != Some(&group.turn_task_id.to_string())
        || row.2.as_deref() != Some(&group.agent_run_id.to_string())
        || row.3 != "AssistantResponse"
        || !matches!(row.4.as_str(), "parent_agent" | "child_agent")
        || decode_json::<ContentHash>(&row.6)? != content_hash
        || content_hash.hex != group.assistant_response_content_hash.hex()
        || row.7
            != i64::try_from(content.len()).map_err(|_| PersistenceError::ToolEntityIntegrity)?
        || !row.8
        || row.9
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(content)
}

fn validate_completed_model_transport(
    connection: &rusqlite::Connection,
    execution: &ModelExecutionSnapshot,
    output: &NormalizedModelOutput,
) -> Result<(), PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT t.record_json
         FROM model_usage u JOIN transport_attempts t USING(transport_attempt_id)
         WHERE u.model_step_id=?1 AND u.model_execution_snapshot_id=?2",
    )?;
    let rows = statement
        .query_map([execution.model_step_id.to_string(), execution.id.to_string()], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() != 1 {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let attempt = decode_json::<TransportAttemptRecord>(&rows[0])?;
    attempt.request_evidence.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let operation_matches = matches!(
        attempt.request_evidence.operation_context,
        TransportOperationContext::AgentModelAttempt {
            model_step_id,
            model_execution_snapshot_id,
            context_snapshot_id,
            prompt_bundle_snapshot_id,
        } if model_step_id == execution.model_step_id
            && model_execution_snapshot_id == execution.id
            && context_snapshot_id == execution.context_snapshot_id
            && prompt_bundle_snapshot_id == execution.prompt_bundle_snapshot_id
    );
    if attempt.state != TransportAttemptState::Completed
        || attempt.endpoint_id != execution.endpoint_id
        || attempt.endpoint_fingerprint != execution.endpoint_fingerprint
        || attempt.raw_stream_artifact_id != output.raw_response_ref
        || !operation_matches
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn load_provider_continuation_binding(
    connection: &rusqlite::Connection,
    execution: &ModelExecutionSnapshot,
    output: &NormalizedModelOutput,
    group: &HistoryAtomicGroupPreimageV1,
) -> Result<Option<ProviderContinuationBindingV1>, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT model_execution_snapshot_id, endpoint_id, model_profile_id,
                    continuation_json
             FROM provider_continuation_refs WHERE model_step_id=?1",
            [execution.model_step_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    match (&output.provider_continuation, row) {
        (None, None) => Ok(None),
        (Some(state), Some(row)) => {
            let stored = decode_json::<ProviderContinuationState>(&row.3)?;
            let response_ref =
                output.raw_response_ref.ok_or(PersistenceError::ToolEntityIntegrity)?;
            if stored != *state
                || row.0 != execution.id.to_string()
                || row.1 != execution.endpoint_id.to_string()
                || row.2 != execution.model_profile_id.to_string()
                || !continuation_state_matches_api(state, execution.api_format)
                || !artifact_exists(connection, continuation_artifact_id(state))?
                || !artifact_exists(connection, response_ref)?
            {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            let attempt = load_completed_transport_attempt(connection, execution)?;
            Ok(Some(ProviderContinuationBindingV1 {
                state: state.clone(),
                provider_id: execution.provider_id,
                api_format: execution.api_format,
                endpoint_id: execution.endpoint_id,
                model_profile_id: execution.model_profile_id,
                endpoint_fingerprint: execution.endpoint_fingerprint.clone(),
                previous_model_execution_id: execution.id,
                provider_response_ref: response_ref,
                request_evidence_hash: canonical_sha256(&attempt.request_evidence)
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                response_evidence_hash: canonical_sha256(output)
                    .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                assistant_response_event_id: group.assistant_response_event_id,
                tool_batch_id: group.tool_batch_id,
            }))
        }
        _ => Err(PersistenceError::ToolEntityIntegrity),
    }
}

fn load_completed_transport_attempt(
    connection: &rusqlite::Connection,
    execution: &ModelExecutionSnapshot,
) -> Result<TransportAttemptRecord, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT t.record_json
         FROM model_usage u JOIN transport_attempts t USING(transport_attempt_id)
         WHERE u.model_step_id=?1 AND u.model_execution_snapshot_id=?2",
    )?;
    let rows = statement
        .query_map([execution.model_step_id.to_string(), execution.id.to_string()], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() != 1 {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let attempt = decode_json::<TransportAttemptRecord>(&rows[0])?;
    if attempt.state != TransportAttemptState::Completed {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(attempt)
}

const fn continuation_state_matches_api(
    state: &ProviderContinuationState,
    api_format: ModelApiFormat,
) -> bool {
    match state {
        ProviderContinuationState::OpenAiResponses { .. } => {
            matches!(api_format, ModelApiFormat::OpenAiResponses)
        }
        ProviderContinuationState::Anthropic { .. } => {
            matches!(api_format, ModelApiFormat::AnthropicMessages)
        }
        ProviderContinuationState::Opaque { .. } => true,
    }
}

const fn continuation_artifact_id(state: &ProviderContinuationState) -> ArtifactId {
    match state {
        ProviderContinuationState::OpenAiResponses { state_ref }
        | ProviderContinuationState::Anthropic { state_ref }
        | ProviderContinuationState::Opaque { state_ref, .. } => *state_ref,
    }
}

fn artifact_exists(
    connection: &rusqlite::Connection,
    artifact_id: ArtifactId,
) -> Result<bool, PersistenceError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM artifacts WHERE artifact_id=?1)",
            [artifact_id.to_string()],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn validate_result_event(
    connection: &rusqlite::Connection,
    batch: &ToolBatchRequestV1,
    event_id: EventId,
    tool_call_id: ToolCallId,
) -> Result<(), PersistenceError> {
    let row = connection
        .query_row(
            "SELECT session_id, turn_task_id, agent_run_id, event_type, payload_json
             FROM events WHERE event_id=?1",
            [event_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let payload = row.4.as_deref().ok_or(PersistenceError::ToolEntityIntegrity)?;
    let payload_value = decode_json::<serde_json::Value>(payload)?;
    let result_id = payload_value
        .get("result")
        .and_then(|value| value.get("tool_call_id"))
        .and_then(serde_json::Value::as_str);
    let expected_tool_call_id = tool_call_id.to_string();
    if row.0.as_deref() != Some(&batch.invocation_identity.session_id.to_string())
        || row.1.as_deref() != Some(&batch.invocation_identity.turn_task_id.to_string())
        || row.2.as_deref() != Some(&batch.invocation_identity.agent_run_id.to_string())
        || row.3 != "ToolResultFinalized"
        || result_id != Some(expected_tool_call_id.as_str())
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn validate_provider_call_binding(
    connection: &rusqlite::Connection,
    batch: &ToolBatchRequestV1,
    call: &NormalizedToolCallV1,
    definition: &ToolDefinitionRecordV1,
) -> Result<(), PersistenceError> {
    let row = connection
        .query_row(
            "SELECT model_execution_snapshot_id, tool_id, tool_definition_json
             FROM provider_tool_call_protocol_bindings WHERE tool_call_id=?1",
            [call.tool_call_id.to_string()],
            |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityStateConflict)?;
    let execution_id = connection.query_row(
        "SELECT model_execution_snapshot_id FROM model_outputs WHERE model_step_id=?1",
        [batch.invocation_identity.model_step_id.to_string()],
        |row| row.get::<_, String>(0),
    )?;
    if row.0 != execution_id
        || row.1 != definition.tool_id.to_string()
        || Some(decode_json::<aura_tool_contract::ToolDefinitionSnapshotRef>(&row.2)?)
            != call.definition_ref
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn verify_assistant_response_event(
    transaction: &Transaction<'_>,
    batch: &ToolBatchRequestV1,
    request: &ToolHistoryAtomicGroupAssociationRequestV1,
) -> Result<(), PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT e.session_id, e.turn_task_id, e.agent_run_id, e.actor,
                    f.content_hash_json, f.model_visible
             FROM events e
             JOIN context_source_event_facts f ON f.event_id=e.event_id
             WHERE e.event_id=?1",
            [request.assistant_response_event_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    if row.0.as_deref() != Some(&batch.invocation_identity.session_id.to_string())
        || row.1.as_deref() != Some(&batch.invocation_identity.turn_task_id.to_string())
        || row.2.as_deref() != Some(&batch.invocation_identity.agent_run_id.to_string())
        || !matches!(row.3.as_str(), "parent_agent" | "child_agent")
        || !row.5
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    let content_hash = decode_json::<ContentHash>(&row.4)?;
    if content_hash.algorithm != ContentHashAlgorithm::Sha256
        || content_hash.hex != request.assistant_response_content_hash.hex()
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(())
}

fn load_batch_request(
    transaction: &rusqlite::Connection,
    tool_batch_id: ToolBatchId,
) -> Result<ToolBatchRequestV1, PersistenceError> {
    let encoded = transaction
        .query_row(
            "SELECT request_json FROM tool_batches WHERE tool_batch_id=?1",
            [tool_batch_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    let batch = decode_json::<ToolBatchRequestV1>(&encoded)?;
    if batch.tool_batch_id != tool_batch_id {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(batch)
}

fn load_completed_batch_result(
    transaction: &rusqlite::Connection,
    batch: &ToolBatchRequestV1,
) -> Result<ToolBatchResultV1, PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT status, result_json, result_hash_json FROM tool_batches
             WHERE tool_batch_id=?1",
            [batch.tool_batch_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ToolEntityNotFound)?;
    if row.0 != "completed" {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    let (json, hash) = row.1.zip(row.2).ok_or(PersistenceError::ToolEntityIntegrity)?;
    let result = decode_verified_record::<ToolBatchResultV1>(&json, &hash)?;
    result.validate(batch).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    Ok(result)
}

fn load_association(
    transaction: &rusqlite::Connection,
    tool_batch_id: ToolBatchId,
) -> Result<Option<StoredToolHistoryAtomicGroupAssociationV1>, PersistenceError> {
    let row = transaction
        .query_row(
            "SELECT assistant_response_event_id, assistant_response_content_hash_json,
                    state, preimage_json, group_hash_json
             FROM tool_history_atomic_group_associations WHERE tool_batch_id=?1",
            [tool_batch_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?;
    row.map(|row| {
        let assistant_response_event_id = EventId::from_str(&row.0)?;
        let assistant_response_content_hash = decode_json(&row.1)?;
        match row.2.as_str() {
            "pending" if row.3.is_none() && row.4.is_none() => {
                Ok(StoredToolHistoryAtomicGroupAssociationV1::Pending {
                    tool_batch_id,
                    assistant_response_event_id,
                    assistant_response_content_hash,
                })
            }
            "finalized" => {
                let preimage = decode_json::<HistoryAtomicGroupPreimageV1>(
                    row.3.as_deref().ok_or(PersistenceError::ToolEntityIntegrity)?,
                )?;
                let stored_group_hash = decode_json::<CanonicalSha256HashV1>(
                    row.4.as_deref().ok_or(PersistenceError::ToolEntityIntegrity)?,
                )?;
                let group_ref =
                    preimage.group_ref().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                if preimage.tool_batch_id != tool_batch_id
                    || preimage.assistant_response_event_id != assistant_response_event_id
                    || preimage.assistant_response_content_hash != assistant_response_content_hash
                    || group_ref.group_hash != stored_group_hash
                {
                    return Err(PersistenceError::ToolEntityIntegrity);
                }
                Ok(StoredToolHistoryAtomicGroupAssociationV1::Finalized {
                    tool_batch_id,
                    preimage,
                    group_ref,
                })
            }
            _ => Err(PersistenceError::ToolEntityIntegrity),
        }
    })
    .transpose()
}

fn validate_legacy_mapping(
    mapping: &LegacyHistoryAtomicGroupSideMappingV1,
) -> Result<(), PersistenceError> {
    if mapping.legacy_atomic_group_id.trim().is_empty()
        || mapping.legacy_atomic_group_id.len() > 4_096
        || matches!(
            mapping.verification,
            LegacyHistoryAtomicGroupVerificationV1::VerifiedSideMapping
        ) != mapping.group_ref.is_some()
    {
        return Err(PersistenceError::InvalidRequest(
            "invalid legacy history atomic-group side mapping".to_owned(),
        ));
    }
    Ok(())
}

const fn legacy_verification_name(value: LegacyHistoryAtomicGroupVerificationV1) -> &'static str {
    match value {
        LegacyHistoryAtomicGroupVerificationV1::VerifiedSideMapping => "verified_side_mapping",
        LegacyHistoryAtomicGroupVerificationV1::LegacyUnverified => "legacy_unverified",
    }
}

const fn call_status_name(value: ToolCallStatusV1) -> &'static str {
    match value {
        ToolCallStatusV1::PendingAdmission => "pending_admission",
        ToolCallStatusV1::WaitingDependency => "waiting_dependency",
        ToolCallStatusV1::WaitingPermission => "waiting_permission",
        ToolCallStatusV1::WaitingApproval => "waiting_approval",
        ToolCallStatusV1::WaitingResource => "waiting_resource",
        ToolCallStatusV1::Queued => "queued",
        ToolCallStatusV1::Running => "running",
        ToolCallStatusV1::Cancelling => "cancelling",
        ToolCallStatusV1::Finalized => "finalized",
        ToolCallStatusV1::RecoveryRequired => "recovery_required",
    }
}
