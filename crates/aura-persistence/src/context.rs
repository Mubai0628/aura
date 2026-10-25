use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use aura_artifact_contract::{ArtifactAccess, ArtifactOwnerRef, ArtifactScope};
use aura_context_domain::{
    CompactionAnchorManifest, CompactionModelAttemptRecord, CompactionModelAttemptState,
    CompactionSourceSpan, ContextCompactionStateView, ContextPrivacyClass,
    ContextProjectionSnapshot, ContextRemediationClearReason, ContextRemediationPlan,
    ContextRemediationRecord, ContextRemediationState, ContextSearchFilters, ContextSourceRef,
    HistoryAtomicGroupRefV1, LegacyHistoryAtomicGroupVerificationV1, ManualCompactionRequestRecord,
    ManualCompactionRequestState, ModelEgressManifestV1, PromptBundleSnapshot,
    PromptInspectorProjection, StructuredCompactionRecord,
};
use aura_domain_common::{
    AgentRunId, ArtifactId, CommandInvocationId, ContentHash, CorrelationId, EventId,
    ModelExecutionSnapshotId, ModelStepId, SessionId, TransportAttemptId, UtcTimestamp,
    canonical_json_v1, canonical_json_v1_sha256,
};
use aura_model_domain::{
    CancellationSource, CompiledInstructionEnvelope, LogicalConversationItem,
    LogicalHistoricalToolCall, ModelExecutionSnapshot, TransportAttemptRecord,
    TransportAttemptState, TransportOperationContext,
};
use aura_projection_contract::ProjectionChange;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;
use serde_json::json;

use crate::{
    ArtifactWriteRequest, ModelProjectionDraft, PersistenceCoordinator, PersistenceError,
    PersistenceFaultPoint, PersistentEventDraft,
    artifact::insert_artifact_in_transaction,
    coordinator::{insert_event, to_sql_u64},
    model_projection::insert_model_projections,
    model_runtime::{
        insert_model_execution_snapshot_in_transaction, transition_transport_attempt_in_transaction,
    },
};

const COMPACTION_RESTART_BEFORE_MODEL_ATTEMPT: &str =
    "context.compaction.application_restarted_before_model_attempt";
const COMPACTION_RESTART_BEFORE_TRANSPORT: &str =
    "context.compaction.application_restarted_before_transport";
const COMPACTION_RESTART_BEFORE_DISPATCH: &str =
    "context.compaction.application_restarted_before_dispatch";
const COMPACTION_RESTART_TRANSPORT_FAILED: &str =
    "context.compaction.transport_failed_before_restart";
const COMPACTION_RESTART_TRANSPORT_CANCELLED: &str =
    "context.compaction.transport_cancelled_before_restart";
const COMPACTION_RESTART_AFTER_TRANSPORT_COMPLETED: &str =
    "context.compaction.application_restarted_after_transport_completed";
const COMPACTION_RESTART_SIDE_EFFECT_UNKNOWN: &str =
    "context.compaction.application_restarted_side_effect_unknown";
const COMPACTION_RESTART_AFTER_ATTEMPT_FAILED: &str =
    "context.compaction.application_restarted_after_attempt_failed";

#[derive(Clone, Debug)]
pub struct ContextArtifactDraft {
    pub artifact_id: ArtifactId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct PromptBundleCommitRequest {
    pub session_id: SessionId,
    pub snapshot: PromptBundleSnapshot,
    pub artifacts: Vec<ContextArtifactDraft>,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptBundleCommitResult {
    pub snapshot: PromptBundleSnapshot,
    pub created: bool,
}

#[derive(Clone, Debug)]
pub struct ContextSearchChunkDraft {
    pub agent_run_id: Option<aura_domain_common::AgentRunId>,
    pub turn_task_id: Option<aura_domain_common::TurnTaskId>,
    pub source_type: String,
    pub source_id: String,
    pub session_sequence: aura_domain_common::SessionSequence,
    pub privacy_class: ContextPrivacyClass,
    pub source_hash: ContentHash,
    pub searchable_text: String,
}

#[derive(Clone, Debug)]
pub struct ContextProjectionCommitRequest {
    pub snapshot: ContextProjectionSnapshot,
    pub execution_snapshot: ModelExecutionSnapshot,
    pub instruction_envelope: CompiledInstructionEnvelope,
    pub model_egress_manifest: ModelEgressManifestV1,
    pub artifacts: Vec<ContextArtifactDraft>,
    pub prompt_inspector: PromptInspectorProjection,
    pub search_chunks: Vec<ContextSearchChunkDraft>,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextProjectionCommitResult {
    pub snapshot: ContextProjectionSnapshot,
    pub event_id: EventId,
    pub projection: ProjectionChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoredContextProjectionSnapshot {
    HistoricalV1 {
        context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
        snapshot_hash: ContentHash,
    },
    CurrentV2(Box<ContextProjectionSnapshot>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSearchStoredRow {
    pub chunk_id: i64,
    pub agent_run_id: Option<aura_domain_common::AgentRunId>,
    pub turn_task_id: Option<aura_domain_common::TurnTaskId>,
    pub source_type: String,
    pub source_id: String,
    pub session_sequence: aura_domain_common::SessionSequence,
    pub searchable_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSearchStoredPage {
    pub rows: Vec<ContextSearchStoredRow>,
    pub cutoff: aura_domain_common::SessionSequence,
    pub index_complete_through: aura_domain_common::SessionSequence,
    pub last_scanned: Option<(aura_domain_common::SessionSequence, i64)>,
}

#[derive(Clone, Debug)]
pub struct ContextSearchStoredQuery {
    pub session_id: SessionId,
    pub agent_run_id: aura_domain_common::AgentRunId,
    pub fts_query: String,
    pub filters: ContextSearchFilters,
    pub cutoff: Option<aura_domain_common::SessionSequence>,
    pub after: Option<(aura_domain_common::SessionSequence, i64)>,
    pub fetch_limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextEventStoredRow {
    pub event_id: EventId,
    pub session_sequence: aura_domain_common::SessionSequence,
    pub event_type: String,
    pub actor: String,
    pub history_atomic_group_ref: Option<HistoryAtomicGroupRefV1>,
    pub atomic_group_id: Option<String>,
    pub legacy_atomic_group_verification: Option<LegacyHistoryAtomicGroupVerificationV1>,
    pub model_visible_content: String,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug)]
pub struct ContextEventStoredQuery {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub cutoff: aura_domain_common::SessionSequence,
    pub event_ids: Vec<EventId>,
    pub sequence_range:
        Option<(aura_domain_common::SessionSequence, aura_domain_common::SessionSequence)>,
    pub turn_task_ids: Vec<aura_domain_common::TurnTaskId>,
    pub history_atomic_group_refs: Vec<HistoryAtomicGroupRefV1>,
    pub atomic_group_ids: Vec<String>,
}

#[derive(Clone, Debug)]
struct ContextEventSelectionRow {
    event_id: String,
    session_sequence: i64,
    event_type: String,
    actor: String,
    legacy_atomic_group_id: Option<String>,
    turn_task_id: Option<String>,
    direct_group_hash_json: Option<String>,
    legacy_verification: Option<String>,
    legacy_assistant_event_id: Option<String>,
    legacy_group_hash_json: Option<String>,
    model_visible_content: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ContextSourceEventCommitRequest {
    pub event: PersistentEventDraft,
    pub canonical_content: String,
    pub model_visible: bool,
    pub explicitly_user_authored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceEventStoredFact {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub actor: crate::EventActor,
    pub content_hash: ContentHash,
    pub byte_length: u64,
    pub model_visible: bool,
    pub explicitly_user_authored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextToolResultStored {
    pub tool_call_id: aura_domain_common::ToolCallId,
    pub tool_name: String,
    pub input: Option<String>,
    pub result: String,
    pub is_error: bool,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionSourceDraft {
    pub source_ref: ContextSourceRef,
    pub source_hash: ContentHash,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionCreateRequest {
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub sequence: u32,
    pub source_span: CompactionSourceSpan,
    pub sources: Vec<ContextCompactionSourceDraft>,
    pub focus_artifact: Option<ContextArtifactDraft>,
    pub requested_at: UtcTimestamp,
    pub prompt_inspector: PromptInspectorProjection,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionRunningRequest {
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub sequence: u32,
    pub attempt_ordinal: u8,
    pub attempt_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub model_step_id: ModelStepId,
    pub model_execution_snapshot_id: ModelExecutionSnapshotId,
    pub transport_attempt_id: Option<TransportAttemptId>,
    pub started_at: UtcTimestamp,
    pub prompt_inspector: PromptInspectorProjection,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionAttemptTerminalRequest {
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub sequence: u32,
    pub attempt_ordinal: u8,
    pub state: CompactionModelAttemptState,
    pub transport_attempt_id: Option<TransportAttemptId>,
    pub error_code: Option<String>,
    pub terminal_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionCompletedRequest {
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub sequence: u32,
    pub attempt_ordinal: u8,
    pub transport_attempt_id: TransportAttemptId,
    pub historical_summary: ContextArtifactDraft,
    pub active_turn_summary: ContextArtifactDraft,
    pub structured_compaction: ContextArtifactDraft,
    pub anchor_manifest: ContextArtifactDraft,
    pub anchor_manifest_value: CompactionAnchorManifest,
    pub record: StructuredCompactionRecord,
    pub completed_at: UtcTimestamp,
    pub prompt_inspector: PromptInspectorProjection,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionTerminalRequest {
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub sequence: u32,
    pub attempt_ordinal: Option<u8>,
    pub attempt_state: Option<CompactionModelAttemptState>,
    pub transport_attempt_id: Option<TransportAttemptId>,
    pub state: ContextCompactionStateView,
    pub error_code: String,
    pub completed_at: UtcTimestamp,
    pub prompt_inspector: PromptInspectorProjection,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextCompactionStoredRecord {
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub sequence: u32,
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub state: ContextCompactionStateView,
    pub model_step_id: Option<ModelStepId>,
    pub transport_attempt_id: Option<TransportAttemptId>,
    pub source_span: CompactionSourceSpan,
    pub historical_summary_artifact_id: Option<ArtifactId>,
    pub active_turn_summary_artifact_id: Option<ArtifactId>,
    pub structured_compaction_artifact_id: Option<ArtifactId>,
    pub anchor_manifest_artifact_id: Option<ArtifactId>,
    pub record: Option<StructuredCompactionRecord>,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextStartupRecoveryReport {
    pub failed_before_model_attempt: u64,
    pub failed_before_transport: u64,
    pub pre_dispatch_attempts_cancelled: u64,
    pub transport_failures_preserved: u64,
    pub transport_cancellations_preserved: u64,
    pub logical_attempt_failures_preserved: u64,
    pub completed_outputs_left_unaccepted: u64,
    pub unknown_side_effects: u64,
    pub manual_requests_terminalized: u64,
    pub prompt_inspector_projections_published: u64,
}

#[derive(Clone, Debug)]
struct ActiveContextCompaction {
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    sequence: u32,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    state: ContextCompactionStateView,
    model_step_id: Option<ModelStepId>,
    transport_attempt_id: Option<TransportAttemptId>,
}

#[derive(Clone, Debug)]
struct ContextCompactionRestartResolution {
    parent_state: ContextCompactionStateView,
    error_code: String,
    attempt_update: Option<CompactionModelAttemptRecord>,
    raw_attempt_to_cancel: Option<TransportAttemptRecord>,
    transport_attempt_id: Option<TransportAttemptId>,
}

#[derive(Clone, Debug)]
pub struct ContextRemediationRequiredRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub plan: ContextRemediationPlan,
    pub required_at: UtcTimestamp,
    pub prompt_inspector: PromptInspectorProjection,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextRemediationClearedRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub reason: ContextRemediationClearReason,
    pub cleared_at: UtcTimestamp,
    pub prompt_inspector: PromptInspectorProjection,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextRemediationCommitResult {
    pub record: ContextRemediationRecord,
    pub projection: Option<ProjectionChange>,
    pub changed: bool,
}

#[derive(Clone, Debug)]
pub struct ContextManualCompactionCreateRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub command_invocation_id: CommandInvocationId,
    pub command_event: PersistentEventDraft,
    pub command_artifact: ContextArtifactDraft,
    pub focus_artifact: Option<ContextArtifactDraft>,
}

#[derive(Clone, Debug)]
pub struct ContextManualCompactionRunningRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub command_invocation_id: CommandInvocationId,
    pub source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    pub compaction_sequence: u32,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ContextManualCompactionTerminalRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub command_invocation_id: CommandInvocationId,
    pub state: ManualCompactionRequestState,
    pub error_code: Option<String>,
    pub terminal_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextInternalArtifactMetadata {
    pub artifact_id: ArtifactId,
    pub media_type: String,
    pub byte_length: u64,
    pub content_hash: ContentHash,
}

impl PersistenceCoordinator {
    pub fn commit_prompt_bundle_snapshot(
        &self,
        request: PromptBundleCommitRequest,
    ) -> Result<PromptBundleCommitResult, PersistenceError> {
        validate_prompt_bundle_commit(&request)?;
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            validate_turn_session(&transaction, request.snapshot.turn_task_id, request.session_id)?;
            let existing = transaction
                .query_row(
                    "SELECT snapshot_json FROM prompt_bundle_snapshots WHERE turn_task_id=?1",
                    [request.snapshot.turn_task_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                let existing = serde_json::from_str::<PromptBundleSnapshot>(&existing)?;
                if existing != request.snapshot {
                    return Err(PersistenceError::InvalidRequest(
                        "turn task already has a different prompt bundle snapshot".to_owned(),
                    ));
                }
                return Ok(PromptBundleCommitResult { snapshot: existing, created: false });
            }
            insert_context_artifacts(
                &transaction,
                &artifact_root,
                request.session_id,
                request.artifacts,
            )?;
            transaction.execute(
                "INSERT INTO prompt_bundle_snapshots(
                    prompt_bundle_snapshot_id, turn_task_id, bundle_version, serializer_version,
                    settings_revision, bundle_manifest_hash_json, release_identity,
                    custom_addendum_artifact_id, custom_addendum_hash_json, snapshot_json, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    request.snapshot.id.to_string(),
                    request.snapshot.turn_task_id.to_string(),
                    request.snapshot.bundle_version.to_string(),
                    i64::from(request.snapshot.serializer_version),
                    to_sql_u64(request.snapshot.settings_revision.value())?,
                    serde_json::to_string(&request.snapshot.bundle_manifest_hash)?,
                    request.snapshot.release_identity,
                    request.snapshot.custom_addendum_artifact_id.map(|id| id.to_string()),
                    request
                        .snapshot
                        .custom_addendum_hash
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()?,
                    serde_json::to_string(&request.snapshot)?,
                    request.snapshot.created_at.to_string(),
                ],
            )?;
            for (index, module) in request.snapshot.module_snapshots.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO prompt_module_snapshots(
                        prompt_bundle_snapshot_id, ordinal, module_id, module_version, section,
                        activation_json, module_order, content_artifact_id, content_hash_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        request.snapshot.id.to_string(),
                        ordinal(index)?,
                        module.module_id,
                        module.module_version.to_string(),
                        enum_name(&module.section)?,
                        serde_json::to_string(&module.activation)?,
                        i64::from(module.order),
                        module.content_artifact_id.to_string(),
                        serde_json::to_string(&module.content_hash)?,
                    ],
                )?;
            }
            let mut event = PersistentEventDraft::system(
                "PromptBundleSnapshotCreated",
                request.correlation_id,
                json!({
                    "prompt_bundle_snapshot_id": request.snapshot.id,
                    "turn_task_id": request.snapshot.turn_task_id,
                    "bundle_manifest_hash": request.snapshot.bundle_manifest_hash,
                    "settings_revision": request.snapshot.settings_revision,
                }),
            );
            event.session_id = Some(request.session_id);
            event.turn_task_id = Some(request.snapshot.turn_task_id);
            insert_event(&transaction, &event)?;
            transaction.commit()?;
            Ok(PromptBundleCommitResult { snapshot: request.snapshot, created: true })
        })
    }

    pub fn prompt_bundle_snapshot_for_turn(
        &self,
        turn_task_id: aura_domain_common::TurnTaskId,
    ) -> Result<Option<PromptBundleSnapshot>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT snapshot_json FROM prompt_bundle_snapshots WHERE turn_task_id=?1",
                    [turn_task_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
                .transpose()
        })
    }

    pub fn commit_context_projection_snapshot(
        &self,
        request: ContextProjectionCommitRequest,
    ) -> Result<ContextProjectionCommitResult, PersistenceError> {
        self.commit_context_projection_snapshot_with_fault(request, None)
    }

    #[doc(hidden)]
    pub fn commit_context_projection_snapshot_with_fault(
        &self,
        request: ContextProjectionCommitRequest,
        fault: Option<PersistenceFaultPoint>,
    ) -> Result<ContextProjectionCommitResult, PersistenceError> {
        validate_context_projection_commit(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            validate_context_owners(&transaction, &request.snapshot)?;
            if transaction
                .query_row(
                    "SELECT 1 FROM context_projection_snapshots WHERE context_snapshot_id=?1",
                    [request.snapshot.id.to_string()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some()
            {
                return Err(PersistenceError::InvalidRequest(
                    "context projection snapshot identity already exists".to_owned(),
                ));
            }
            insert_model_execution_snapshot_in_transaction(
                &transaction,
                &request.execution_snapshot,
                false,
            )?;
            insert_context_artifacts(
                &transaction,
                &artifact_root,
                request.snapshot.session_id,
                request.artifacts,
            )?;
            insert_context_snapshot_row(&transaction, &request.snapshot)?;
            insert_context_children(
                &transaction,
                &request.snapshot,
                &request.model_egress_manifest,
            )?;
            insert_context_search_chunks(
                &transaction,
                request.snapshot.session_id,
                &request.search_chunks,
            )?;
            let event_id = EventId::new();
            let mut event = PersistentEventDraft::system(
                "ContextProjectionSnapshotCreated",
                request.correlation_id,
                json!({
                    "context_snapshot_id": request.snapshot.id,
                    "prompt_bundle_snapshot_id": request.snapshot.prompt_bundle_snapshot_id,
                    "model_step_id": request.snapshot.model_step_id,
                    "snapshot_hash": request.snapshot.snapshot_hash,
                }),
            );
            event.event_id = event_id;
            event.session_id = Some(request.snapshot.session_id);
            event.turn_task_id = Some(request.snapshot.turn_task_id);
            event.agent_run_id = Some(request.snapshot.agent_run_id);
            insert_event(&transaction, &event)?;
            let (mut projections, _) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::PromptInspector(Box::new(request.prompt_inspector))],
            )?;
            let projection = projections.pop().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "prompt inspector projection was not materialized".to_owned(),
                )
            })?;
            if fault == Some(PersistenceFaultPoint::BeforeCommit) {
                return Err(PersistenceError::InvalidRequest(
                    "injected context persistence failure before commit".to_owned(),
                ));
            }
            transaction.commit()?;
            Ok(ContextProjectionCommitResult { snapshot: request.snapshot, event_id, projection })
        })
    }

    pub fn context_projection_snapshot(
        &self,
        context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    ) -> Result<Option<ContextProjectionSnapshot>, PersistenceError> {
        match self.stored_context_projection_snapshot(context_snapshot_id)? {
            Some(StoredContextProjectionSnapshot::CurrentV2(snapshot)) => Ok(Some(*snapshot)),
            Some(StoredContextProjectionSnapshot::HistoricalV1 { .. }) => {
                Err(PersistenceError::HistoricalContextProjectionSnapshot)
            }
            None => Ok(None),
        }
    }

    pub fn stored_context_projection_snapshot(
        &self,
        context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    ) -> Result<Option<StoredContextProjectionSnapshot>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT snapshot_version, snapshot_hash_json, snapshot_json
                     FROM context_projection_snapshots
                     WHERE context_snapshot_id=?1",
                    [context_snapshot_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?
                .map(|(snapshot_version, snapshot_hash_json, snapshot_json)| {
                    let snapshot_hash = serde_json::from_str::<ContentHash>(&snapshot_hash_json)?;
                    match snapshot_version {
                        1 => Ok(StoredContextProjectionSnapshot::HistoricalV1 {
                            context_snapshot_id,
                            snapshot_hash,
                        }),
                        2 => {
                            let snapshot =
                                serde_json::from_str::<ContextProjectionSnapshot>(&snapshot_json)?;
                            snapshot.validate_contract_shape().map_err(|error| {
                                PersistenceError::InvalidRequest(format!(
                                    "persisted context projection is invalid: {error}"
                                ))
                            })?;
                            if snapshot.id != context_snapshot_id
                                || snapshot.snapshot_version != 2
                                || snapshot.snapshot_hash != snapshot_hash
                            {
                                return Err(PersistenceError::ContextProjectionIntegrity);
                            }
                            Ok(StoredContextProjectionSnapshot::CurrentV2(Box::new(snapshot)))
                        }
                        _ => Err(PersistenceError::ContextProjectionIntegrity),
                    }
                })
                .transpose()
        })
    }

    pub fn latest_context_projection_snapshot(
        &self,
        session_id: SessionId,
        agent_run_id: aura_domain_common::AgentRunId,
    ) -> Result<Option<ContextProjectionSnapshot>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT snapshot_json FROM context_projection_snapshots
                     WHERE session_id=?1 AND agent_run_id=?2 AND snapshot_version=2
                     ORDER BY created_at DESC, context_snapshot_id DESC LIMIT 1",
                    params![session_id.to_string(), agent_run_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| {
                    let snapshot = serde_json::from_str::<ContextProjectionSnapshot>(&value)?;
                    snapshot.validate_contract_shape().map_err(|error| {
                        PersistenceError::InvalidRequest(format!(
                            "persisted context projection is invalid: {error}"
                        ))
                    })?;
                    Ok(snapshot)
                })
                .transpose()
        })
    }

    pub fn search_context_rows(
        &self,
        query: ContextSearchStoredQuery,
    ) -> Result<ContextSearchStoredPage, PersistenceError> {
        if query.fts_query.trim().is_empty() || query.fetch_limit == 0 || query.fetch_limit > 512 {
            return Err(PersistenceError::InvalidRequest(
                "context search request is outside its bounded limits".to_owned(),
            ));
        }
        self.with_reader(|connection| {
            let watermark = load_context_search_watermark(connection, query.session_id)?;
            let cutoff = query.cutoff.unwrap_or(watermark.0);
            let (after_sequence, after_chunk) = match query.after {
                Some((sequence, chunk)) => (to_sql_u64(sequence.value())?, chunk),
                None => (i64::MAX, i64::MAX),
            };
            let mut statement = connection.prepare(
                "WITH fts_matches(chunk_id) AS MATERIALIZED (
                     SELECT CAST(chunk_id AS INTEGER)
                     FROM context_search_fts
                     WHERE context_search_fts MATCH ?1
                 )
                 SELECT c.chunk_id, c.agent_run_id, c.turn_task_id, c.source_type, c.source_id,
                        c.session_sequence, c.searchable_text
                 FROM fts_matches f
                 JOIN context_search_chunks c ON c.chunk_id=f.chunk_id
                 WHERE c.session_id=?2
                   AND c.agent_run_id=?3
                   AND c.session_sequence<=?4
                   AND (c.session_sequence<?5 OR (c.session_sequence=?5 AND c.chunk_id<?6))
                 ORDER BY c.session_sequence DESC, c.chunk_id DESC
                 LIMIT ?7",
            )?;
            let rows = statement
                .query_map(
                    params![
                        query.fts_query,
                        query.session_id.to_string(),
                        query.agent_run_id.to_string(),
                        to_sql_u64(cutoff.value())?,
                        after_sequence,
                        after_chunk,
                        i64::try_from(query.fetch_limit).map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "context search limit exceeds SQLite range".to_owned(),
                            )
                        })?,
                    ],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, String>(6)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let last_scanned = rows
                .last()
                .map(|row| {
                    u64::try_from(row.5)
                        .map(|sequence| (aura_domain_common::SessionSequence::new(sequence), row.0))
                        .map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "negative context search sequence".to_owned(),
                            )
                        })
                })
                .transpose()?;
            let mut results = Vec::new();
            for row in rows {
                if !query.filters.source_types.is_empty()
                    && !query.filters.source_types.contains(&row.3)
                {
                    continue;
                }
                if query.filters.turn_task_id.is_some_and(|expected| {
                    row.2
                        .as_deref()
                        .and_then(|value| aura_domain_common::TurnTaskId::from_str(value).ok())
                        != Some(expected)
                }) {
                    continue;
                }
                if !query.filters.include_reasoning && row.3 == "reasoning" {
                    continue;
                }
                results.push(ContextSearchStoredRow {
                    chunk_id: row.0,
                    agent_run_id: row.1.as_deref().map(FromStr::from_str).transpose()?,
                    turn_task_id: row.2.as_deref().map(FromStr::from_str).transpose()?,
                    source_type: row.3,
                    source_id: row.4,
                    session_sequence: aura_domain_common::SessionSequence::new(
                        u64::try_from(row.5).map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "negative context search sequence".to_owned(),
                            )
                        })?,
                    ),
                    searchable_text: row.6,
                });
            }
            Ok(ContextSearchStoredPage {
                rows: results,
                cutoff,
                index_complete_through: watermark.0,
                last_scanned,
            })
        })
    }

    pub fn context_event_rows(
        &self,
        query: ContextEventStoredQuery,
    ) -> Result<Vec<ContextEventStoredRow>, PersistenceError> {
        if query.event_ids.len()
            + query.turn_task_ids.len()
            + query.history_atomic_group_refs.len()
            + query.atomic_group_ids.len()
            > 32
        {
            return Err(PersistenceError::InvalidRequest(
                "context event selector limit exceeded".to_owned(),
            ));
        }
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT e.event_id, e.session_sequence, e.event_type, e.actor,
                        e.atomic_group_id, e.turn_task_id,
                        direct.group_hash_json, legacy.verification,
                        legacy.assistant_response_event_id, legacy.group_hash_json,
                        (SELECT searchable_text FROM context_search_chunks c
                         WHERE c.session_id=e.session_id
                           AND c.source_id=e.event_id
                           AND c.privacy_class='model_visible'
                         ORDER BY c.chunk_id DESC LIMIT 1)
                 FROM events e
                 LEFT JOIN tool_history_atomic_group_associations direct
                   ON direct.assistant_response_event_id=e.event_id
                  AND direct.state='finalized'
                 LEFT JOIN context_legacy_history_atomic_group_mappings legacy
                   ON legacy.session_id=e.session_id
                  AND legacy.legacy_atomic_group_id=e.atomic_group_id
                 WHERE e.session_id=?1 AND e.agent_run_id=?2
                   AND e.session_sequence IS NOT NULL AND e.session_sequence<=?3
                 ORDER BY e.session_sequence, e.global_sequence",
            )?;
            let rows = statement
                .query_map(
                    params![
                        query.session_id.to_string(),
                        query.agent_run_id.to_string(),
                        to_sql_u64(query.cutoff.value())?,
                    ],
                    |row| {
                        Ok(ContextEventSelectionRow {
                            event_id: row.get(0)?,
                            session_sequence: row.get(1)?,
                            event_type: row.get(2)?,
                            actor: row.get(3)?,
                            legacy_atomic_group_id: row.get(4)?,
                            turn_task_id: row.get(5)?,
                            direct_group_hash_json: row.get(6)?,
                            legacy_verification: row.get(7)?,
                            legacy_assistant_event_id: row.get(8)?,
                            legacy_group_hash_json: row.get(9)?,
                            model_visible_content: row.get(10)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            let mut selected_groups = query.atomic_group_ids.clone();
            let mut selected_history_groups = query.history_atomic_group_refs.clone();
            for row in &rows {
                let event_id = EventId::from_str(&row.event_id)?;
                let sequence = u64::try_from(row.session_sequence).map_err(|_| {
                    PersistenceError::InvalidRequest("negative event sequence".to_owned())
                })?;
                let history_group_ref = history_group_ref_from_row(row)?;
                let selected = query.event_ids.contains(&event_id)
                    || query.sequence_range.is_some_and(|(start, end)| {
                        sequence >= start.value() && sequence <= end.value()
                    })
                    || row.turn_task_id.as_deref().is_some_and(|id| {
                        query.turn_task_ids.iter().any(|expected| id == expected.to_string())
                    })
                    || history_group_ref
                        .as_ref()
                        .is_some_and(|group| selected_history_groups.contains(group));
                if selected
                    && let Some(group) = &row.legacy_atomic_group_id
                    && !selected_groups.contains(group)
                {
                    selected_groups.push(group.clone());
                }
                if selected
                    && let Some(group) = history_group_ref
                    && !selected_history_groups.contains(&group)
                {
                    selected_history_groups.push(group);
                }
            }
            let mut result = Vec::new();
            for row in rows {
                let event_id = EventId::from_str(&row.event_id)?;
                let sequence = u64::try_from(row.session_sequence).map_err(|_| {
                    PersistenceError::InvalidRequest("negative event sequence".to_owned())
                })?;
                let history_atomic_group_ref = history_group_ref_from_row(&row)?;
                let legacy_atomic_group_verification = legacy_verification_from_row(&row)?;
                let selected = query.event_ids.contains(&event_id)
                    || query.sequence_range.is_some_and(|(start, end)| {
                        sequence >= start.value() && sequence <= end.value()
                    })
                    || row.turn_task_id.as_deref().is_some_and(|id| {
                        query.turn_task_ids.iter().any(|expected| id == expected.to_string())
                    })
                    || row
                        .legacy_atomic_group_id
                        .as_ref()
                        .is_some_and(|group| selected_groups.contains(group))
                    || history_atomic_group_ref
                        .as_ref()
                        .is_some_and(|group| selected_history_groups.contains(group));
                if selected && let Some(content) = row.model_visible_content {
                    result.push(ContextEventStoredRow {
                        event_id,
                        session_sequence: aura_domain_common::SessionSequence::new(sequence),
                        event_type: row.event_type,
                        actor: row.actor,
                        history_atomic_group_ref,
                        atomic_group_id: row.legacy_atomic_group_id,
                        legacy_atomic_group_verification,
                        content_hash: ContentHash::sha256(content.as_bytes()),
                        model_visible_content: content,
                    });
                }
            }
            Ok(result)
        })
    }

    pub fn context_event_sequence_cutoff(
        &self,
        session_id: SessionId,
    ) -> Result<aura_domain_common::SessionSequence, PersistenceError> {
        self.with_reader(|connection| {
            let value = connection.query_row(
                "SELECT COALESCE(MAX(session_sequence), 0) FROM events WHERE session_id=?1",
                [session_id.to_string()],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(aura_domain_common::SessionSequence::new(u64::try_from(value).map_err(|_| {
                PersistenceError::InvalidRequest("negative persisted event sequence".to_owned())
            })?))
        })
    }

    pub fn commit_context_source_event(
        &self,
        request: ContextSourceEventCommitRequest,
    ) -> Result<EventId, PersistenceError> {
        validate_context_source_event(&request)?;
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let event_id = request.event.event_id;
            let session_id = request.event.session_id.ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "context source event requires a Session owner".to_owned(),
                )
            })?;
            insert_event(&transaction, &request.event)?;
            transaction.execute(
                "INSERT INTO context_source_event_facts(
                    event_id, session_id, content_hash_json, byte_length,
                    model_visible, explicitly_user_authored, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    event_id.to_string(),
                    session_id.to_string(),
                    serde_json::to_string(&ContentHash::sha256(
                        request.canonical_content.as_bytes()
                    ))?,
                    to_sql_u64(u64::try_from(request.canonical_content.len()).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "context source event length exceeds u64".to_owned(),
                        )
                    })?)?,
                    i64::from(request.model_visible),
                    i64::from(request.explicitly_user_authored),
                    request.event.created_at.to_string(),
                ],
            )?;
            transaction.commit()?;
            Ok(event_id)
        })
    }

    pub fn context_source_event_fact(
        &self,
        event_id: EventId,
    ) -> Result<Option<ContextSourceEventStoredFact>, PersistenceError> {
        self.with_reader(|connection| {
            connection
                .query_row(
                    "SELECT f.session_id, e.actor, f.content_hash_json, f.byte_length,
                            f.model_visible, f.explicitly_user_authored
                     FROM context_source_event_facts f
                     JOIN events e ON e.event_id=f.event_id
                     WHERE f.event_id=?1",
                    [event_id.to_string()],
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
                .map(|row| {
                    Ok(ContextSourceEventStoredFact {
                        event_id,
                        session_id: SessionId::from_str(&row.0)?,
                        actor: parse_event_actor(&row.1)?,
                        content_hash: serde_json::from_str(&row.2)?,
                        byte_length: u64::try_from(row.3).map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "persisted context source byte length is negative".to_owned(),
                            )
                        })?,
                        model_visible: row.4,
                        explicitly_user_authored: row.5,
                    })
                })
                .transpose()
        })
    }

    pub fn context_tool_result(
        &self,
        session_id: SessionId,
        agent_run_id: AgentRunId,
        tool_call_id: aura_domain_common::ToolCallId,
    ) -> Result<Option<ContextToolResultStored>, PersistenceError> {
        self.with_reader(|connection| {
            let result = connection
                .query_row(
                    "SELECT i.context_snapshot_id, i.item_json
                     FROM context_projection_conversation_items i
                     JOIN context_projection_snapshots s
                       ON s.context_snapshot_id=i.context_snapshot_id
                     WHERE s.session_id=?1 AND s.agent_run_id=?2 AND i.tool_call_id=?3
                     ORDER BY s.created_at DESC, i.ordinal DESC LIMIT 1",
                    params![
                        session_id.to_string(),
                        agent_run_id.to_string(),
                        tool_call_id.to_string(),
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let Some((context_snapshot_id, item_json)) = result else {
                return Ok(None);
            };
            let item = serde_json::from_str::<LogicalConversationItem>(&item_json)?;
            let LogicalConversationItem::ToolResult {
                tool_call_id: stored_tool_call_id,
                tool_name,
                content,
                content_hash,
                is_error,
                ..
            } = item
            else {
                return Err(PersistenceError::InvalidRequest(
                    "persisted tool result row has the wrong item kind".to_owned(),
                ));
            };
            if stored_tool_call_id != tool_call_id
                || content_hash != ContentHash::sha256(content.as_bytes())
            {
                return Err(PersistenceError::InvalidRequest(
                    "persisted tool result identity or hash is inconsistent".to_owned(),
                ));
            }
            let mut statement = connection.prepare(
                "SELECT item_json FROM context_projection_conversation_items
                 WHERE context_snapshot_id=?1 AND item_kind='assistant'
                 ORDER BY ordinal",
            )?;
            let assistant_items = statement
                .query_map([context_snapshot_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            let input = assistant_items
                .into_iter()
                .map(|encoded| serde_json::from_str::<LogicalConversationItem>(&encoded))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .find_map(|assistant| match assistant {
                    LogicalConversationItem::Assistant { tool_calls, .. } => tool_calls
                        .into_iter()
                        .find(|call| call.tool_call_id == tool_call_id)
                        .map(|call: LogicalHistoricalToolCall| call.arguments_json),
                    _ => None,
                });
            Ok(Some(ContextToolResultStored {
                tool_call_id,
                tool_name,
                input,
                result: content,
                is_error,
                content_hash,
            }))
        })
    }

    pub fn latest_prompt_inspector_projection(
        &self,
        session_id: SessionId,
    ) -> Result<Option<PromptInspectorProjection>, PersistenceError> {
        self.latest_model_projection(
            aura_projection_contract::ProjectionKind::PromptInspector,
            aura_projection_contract::ProjectionScope::Session { session_id },
        )?
        .map(|change| match change {
            ProjectionChange::PromptInspector { payload, .. } => Ok(*payload),
            _ => Err(PersistenceError::InvalidRequest(
                "persisted prompt inspector route has the wrong projection kind".to_owned(),
            )),
        })
        .transpose()
    }

    pub fn create_manual_context_compaction_request(
        &self,
        request: ContextManualCompactionCreateRequest,
    ) -> Result<ManualCompactionRequestRecord, PersistenceError> {
        let (command_hash, focus_hash) = validate_manual_compaction_create(&request)?;
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (turn_task_id, workspace_id) =
                validate_agent_run_session(&transaction, request.agent_run_id, request.session_id)?;
            if request.command_event.turn_task_id != Some(turn_task_id)
                || request.command_event.workspace_id != Some(workspace_id)
            {
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction command owner is invalid".to_owned(),
                ));
            }
            if let Some(existing) = load_manual_compaction_request(
                &transaction,
                request.session_id,
                request.command_invocation_id,
            )? {
                if existing.agent_run_id != request.agent_run_id
                    || existing.command_event_id != request.command_event.event_id
                    || existing.focus_hash != focus_hash
                    || existing.created_at != request.command_event.created_at
                {
                    return Err(PersistenceError::InvalidRequest(
                        "manual context compaction request identity changed".to_owned(),
                    ));
                }
                return Ok(existing);
            }

            let focus_artifact_id = request.focus_artifact.as_ref().map(|value| value.artifact_id);
            let mut artifacts = vec![request.command_artifact.clone()];
            artifacts.extend(request.focus_artifact.clone());
            insert_context_artifacts(&transaction, &artifact_root, request.session_id, artifacts)?;

            let mut command_event = request.command_event;
            command_event.payload_json = Some(json!({
                "command_invocation_id": request.command_invocation_id,
                "focus_hash": focus_hash,
            }));
            command_event.payload_artifact_ref = Some(request.command_artifact.artifact_id);
            command_event.searchable_text = None;
            insert_event(&transaction, &command_event)?;
            transaction.execute(
                "INSERT INTO context_source_event_facts(
                    event_id, session_id, content_hash_json, byte_length,
                    model_visible, explicitly_user_authored, created_at
                 ) VALUES (?1, ?2, ?3, ?4, 0, 1, ?5)",
                params![
                    command_event.event_id.to_string(),
                    request.session_id.to_string(),
                    serde_json::to_string(&command_hash)?,
                    to_sql_u64(u64::try_from(request.command_artifact.bytes.len()).map_err(
                        |_| PersistenceError::InvalidRequest(
                            "manual context compaction command exceeds u64".to_owned(),
                        ),
                    )?)?,
                    command_event.created_at.to_string(),
                ],
            )?;
            transaction.execute(
                "INSERT INTO context_manual_compaction_requests(
                    session_id, agent_run_id, request_key, command_event_id,
                    focus_artifact_id, focus_hash_json, state,
                    source_context_snapshot_id, compaction_sequence,
                    error_code, created_at, terminal_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'requested', NULL, NULL, NULL, ?7, NULL)",
                params![
                    request.session_id.to_string(),
                    request.agent_run_id.to_string(),
                    request.command_invocation_id.to_string(),
                    command_event.event_id.to_string(),
                    focus_artifact_id.map(|id| id.to_string()),
                    focus_hash.as_ref().map(serde_json::to_string).transpose()?,
                    command_event.created_at.to_string(),
                ],
            )?;
            let result = ManualCompactionRequestRecord {
                session_id: request.session_id,
                agent_run_id: request.agent_run_id,
                command_invocation_id: request.command_invocation_id,
                command_event_id: command_event.event_id,
                focus_artifact_id,
                focus_hash,
                state: ManualCompactionRequestState::Requested,
                source_context_snapshot_id: None,
                compaction_sequence: None,
                error_code: None,
                created_at: command_event.created_at,
                terminal_at: None,
            };
            transaction.commit()?;
            Ok(result)
        })
    }

    pub fn mark_manual_context_compaction_running(
        &self,
        request: ContextManualCompactionRunningRequest,
    ) -> Result<ManualCompactionRequestRecord, PersistenceError> {
        if request.compaction_sequence == 0 {
            return Err(PersistenceError::InvalidRequest(
                "manual context compaction sequence is invalid".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let existing = load_manual_compaction_request(
                &transaction,
                request.session_id,
                request.command_invocation_id,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "manual context compaction request is missing".to_owned(),
                )
            })?;
            if existing.agent_run_id != request.agent_run_id {
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction AgentRun changed".to_owned(),
                ));
            }
            if existing.state == ManualCompactionRequestState::Running {
                if existing.source_context_snapshot_id == Some(request.source_context_snapshot_id)
                    && existing.compaction_sequence == Some(request.compaction_sequence)
                {
                    return Ok(existing);
                }
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction binding changed".to_owned(),
                ));
            }
            if existing.state != ManualCompactionRequestState::Requested {
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction request is already terminal".to_owned(),
                ));
            }
            validate_compaction_owner_and_state(
                &transaction,
                request.session_id,
                request.agent_run_id,
                request.source_context_snapshot_id,
                request.compaction_sequence,
                &["requested", "running"],
            )?;
            let changed = transaction.execute(
                "UPDATE context_manual_compaction_requests
                 SET state='running', source_context_snapshot_id=?4, compaction_sequence=?5
                 WHERE session_id=?1 AND agent_run_id=?2 AND request_key=?3 AND state='requested'",
                params![
                    request.session_id.to_string(),
                    request.agent_run_id.to_string(),
                    request.command_invocation_id.to_string(),
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.compaction_sequence),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction request changed concurrently".to_owned(),
                ));
            }
            let mut event = PersistentEventDraft::system(
                "ManualContextCompactionStarted",
                request.correlation_id,
                json!({
                    "command_invocation_id": request.command_invocation_id,
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "compaction_sequence": request.compaction_sequence,
                }),
            );
            event.session_id = Some(request.session_id);
            event.agent_run_id = Some(request.agent_run_id);
            insert_event(&transaction, &event)?;
            let result = load_manual_compaction_request(
                &transaction,
                request.session_id,
                request.command_invocation_id,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "manual context compaction running state was not persisted".to_owned(),
                )
            })?;
            transaction.commit()?;
            Ok(result)
        })
    }

    pub fn finish_manual_context_compaction_request(
        &self,
        request: ContextManualCompactionTerminalRequest,
    ) -> Result<ManualCompactionRequestRecord, PersistenceError> {
        if !matches!(
            request.state,
            ManualCompactionRequestState::Completed
                | ManualCompactionRequestState::Failed
                | ManualCompactionRequestState::Cancelled
        ) || (request.state == ManualCompactionRequestState::Completed
            && request.error_code.is_some())
            || (request.state != ManualCompactionRequestState::Completed
                && request.error_code.as_deref().is_none_or(str::is_empty))
        {
            return Err(PersistenceError::InvalidRequest(
                "manual context compaction terminal state is invalid".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let existing = load_manual_compaction_request(
                &transaction,
                request.session_id,
                request.command_invocation_id,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "manual context compaction request is missing".to_owned(),
                )
            })?;
            if existing.agent_run_id != request.agent_run_id {
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction AgentRun changed".to_owned(),
                ));
            }
            if matches!(
                existing.state,
                ManualCompactionRequestState::Completed
                    | ManualCompactionRequestState::Failed
                    | ManualCompactionRequestState::Cancelled
            ) {
                if existing.state == request.state
                    && existing.error_code == request.error_code
                    && existing.terminal_at == Some(request.terminal_at)
                {
                    return Ok(existing);
                }
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction terminal outcome changed".to_owned(),
                ));
            }
            if let (Some(source_context_snapshot_id), Some(sequence)) =
                (existing.source_context_snapshot_id, existing.compaction_sequence)
            {
                let expected_state = match request.state {
                    ManualCompactionRequestState::Completed => "completed",
                    ManualCompactionRequestState::Failed => "failed",
                    ManualCompactionRequestState::Cancelled => "cancelled",
                    ManualCompactionRequestState::Requested
                    | ManualCompactionRequestState::Running => unreachable!(),
                };
                validate_compaction_owner_and_state(
                    &transaction,
                    request.session_id,
                    request.agent_run_id,
                    source_context_snapshot_id,
                    sequence,
                    &[expected_state],
                )?;
            } else if request.state == ManualCompactionRequestState::Completed {
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction cannot complete before a safe-point binding"
                        .to_owned(),
                ));
            }
            let state = manual_compaction_state_name(request.state);
            let changed = transaction.execute(
                "UPDATE context_manual_compaction_requests
                 SET state=?4, error_code=?5, terminal_at=?6
                 WHERE session_id=?1 AND agent_run_id=?2 AND request_key=?3
                   AND state IN ('requested', 'running')",
                params![
                    request.session_id.to_string(),
                    request.agent_run_id.to_string(),
                    request.command_invocation_id.to_string(),
                    state,
                    request.error_code,
                    request.terminal_at.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "manual context compaction request changed concurrently".to_owned(),
                ));
            }
            let event_type = match request.state {
                ManualCompactionRequestState::Completed => "ManualContextCompactionCompleted",
                ManualCompactionRequestState::Failed => "ManualContextCompactionFailed",
                ManualCompactionRequestState::Cancelled => "ManualContextCompactionCancelled",
                ManualCompactionRequestState::Requested | ManualCompactionRequestState::Running => {
                    unreachable!()
                }
            };
            let mut event = PersistentEventDraft::system(
                event_type,
                request.correlation_id,
                json!({
                    "command_invocation_id": request.command_invocation_id,
                    "error_code": request.error_code,
                }),
            );
            event.session_id = Some(request.session_id);
            event.agent_run_id = Some(request.agent_run_id);
            insert_event(&transaction, &event)?;
            let result = load_manual_compaction_request(
                &transaction,
                request.session_id,
                request.command_invocation_id,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "manual context compaction terminal state was not persisted".to_owned(),
                )
            })?;
            transaction.commit()?;
            Ok(result)
        })
    }

    pub fn pending_manual_context_compaction_requests(
        &self,
        session_id: SessionId,
        agent_run_id: AgentRunId,
    ) -> Result<Vec<ManualCompactionRequestRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let mut statement = connection.prepare(
                "SELECT request_key FROM context_manual_compaction_requests
                 WHERE session_id=?1 AND agent_run_id=?2 AND state IN ('requested', 'running')
                 ORDER BY created_at, request_key",
            )?;
            let keys = statement
                .query_map(params![session_id.to_string(), agent_run_id.to_string()], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            keys.into_iter()
                .map(|key| {
                    let key = CommandInvocationId::from_str(&key)?;
                    load_manual_compaction_request(connection, session_id, key)?.ok_or_else(|| {
                        PersistenceError::InvalidRequest(
                            "manual context compaction index is inconsistent".to_owned(),
                        )
                    })
                })
                .collect()
        })
    }

    /// Reconciles P06 compaction work owned by a previous Core runtime.
    ///
    /// This recovery never dispatches or retries a Provider request, never
    /// accepts an unvalidated compaction result, and never replaces the LKG.
    /// Pre-dispatch TransportAttempts can be proven not to have crossed the
    /// durable dispatch boundary and are forward-cancelled. Attempts at or
    /// after `DispatchPrepared` retain their raw state and are represented by
    /// the P06 logical `unknown_side_effect` terminal state.
    pub fn reconcile_context_after_core_restart(
        &self,
        correlation_id: CorrelationId,
        restarted_at: UtcTimestamp,
    ) -> Result<ContextStartupRecoveryReport, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let active = load_active_context_compactions(&transaction)?;
            let mut report = ContextStartupRecoveryReport::default();
            let mut projection_drafts = Vec::with_capacity(active.len());

            for compaction in active {
                let resolution = resolve_context_compaction_restart(
                    &transaction,
                    &compaction,
                    correlation_id,
                    restarted_at,
                )?;

                if let Some(mut raw_attempt) = resolution.raw_attempt_to_cancel.clone() {
                    raw_attempt.state = TransportAttemptState::Cancelled;
                    raw_attempt.finished_at = Some(restarted_at);
                    transition_transport_attempt_in_transaction(
                        &transaction,
                        correlation_id,
                        &raw_attempt,
                        Some(CancellationSource::CoreShutdown),
                    )?;
                    report.pre_dispatch_attempts_cancelled =
                        report.pre_dispatch_attempts_cancelled.checked_add(1).ok_or_else(|| {
                            PersistenceError::InvalidRequest(
                                "context startup recovery counter exhausted".to_owned(),
                            )
                        })?;
                }

                if let Some(attempt) = &resolution.attempt_update {
                    let changed = transaction.execute(
                        "UPDATE context_compaction_model_attempts
                         SET transport_attempt_id=?4, state=?5, error_code=?6, terminal_at=?7
                         WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
                           AND attempt_ordinal=?3 AND state='running'",
                        params![
                            attempt.source_context_snapshot_id.to_string(),
                            i64::from(attempt.compaction_sequence),
                            i64::from(attempt.attempt_ordinal),
                            attempt.transport_attempt_id.map(|id| id.to_string()),
                            compaction_model_attempt_state_name(attempt.state),
                            attempt.error_code,
                            attempt.terminal_at.map(|value| value.to_string()),
                        ],
                    )?;
                    if changed != 1 {
                        return Err(PersistenceError::InvalidRequest(
                            "context restart recovery lost its running model attempt".to_owned(),
                        ));
                    }
                    let event_type = match attempt.state {
                        CompactionModelAttemptState::Failed => {
                            "ContextCompactionModelAttemptFailed"
                        }
                        CompactionModelAttemptState::Cancelled => {
                            "ContextCompactionModelAttemptCancelled"
                        }
                        CompactionModelAttemptState::UnknownSideEffect => {
                            "ContextCompactionModelAttemptSideEffectUnknown"
                        }
                        CompactionModelAttemptState::Running
                        | CompactionModelAttemptState::Completed => {
                            return Err(PersistenceError::InvalidRequest(
                                "context restart recovery produced an invalid attempt terminal"
                                    .to_owned(),
                            ));
                        }
                    };
                    let mut event = PersistentEventDraft::system(
                        event_type,
                        correlation_id,
                        json!({
                            "source_context_snapshot_id": attempt.source_context_snapshot_id,
                            "sequence": attempt.compaction_sequence,
                            "attempt_ordinal": attempt.attempt_ordinal,
                            "transport_attempt_id": attempt.transport_attempt_id,
                            "error_code": resolution.error_code,
                            "recovery_reason": "application_restarted",
                        }),
                    );
                    event.session_id = Some(compaction.session_id);
                    event.agent_run_id = Some(compaction.agent_run_id);
                    insert_event(&transaction, &event)?;
                }

                let persisted_state = context_compaction_state_name(resolution.parent_state)?;
                let changed = transaction.execute(
                    "UPDATE context_compactions
                     SET state=?3, error_code=?4, transport_attempt_id=?5, completed_at=?6
                     WHERE source_context_snapshot_id=?1 AND sequence=?2
                       AND state IN ('requested', 'running')",
                    params![
                        compaction.source_context_snapshot_id.to_string(),
                        i64::from(compaction.sequence),
                        persisted_state,
                        resolution.error_code,
                        resolution.transport_attempt_id.map(|id| id.to_string()),
                        restarted_at.to_string(),
                    ],
                )?;
                if changed != 1 {
                    return Err(PersistenceError::InvalidRequest(
                        "context restart recovery lost its active compaction".to_owned(),
                    ));
                }

                let event_type = if resolution.parent_state == ContextCompactionStateView::Cancelled
                {
                    "ContextCompactionCancelled"
                } else {
                    "ContextCompactionFailed"
                };
                let mut event = PersistentEventDraft::system(
                    event_type,
                    correlation_id,
                    json!({
                        "source_context_snapshot_id": compaction.source_context_snapshot_id,
                        "sequence": compaction.sequence,
                        "transport_attempt_id": resolution.transport_attempt_id,
                        "error_code": resolution.error_code,
                        "recovery_reason": "application_restarted",
                    }),
                );
                event.session_id = Some(compaction.session_id);
                event.agent_run_id = Some(compaction.agent_run_id);
                insert_event(&transaction, &event)?;

                report.manual_requests_terminalized = report
                    .manual_requests_terminalized
                    .checked_add(terminalize_bound_manual_compaction_requests(
                        &transaction,
                        &compaction,
                        resolution.parent_state,
                        &resolution.error_code,
                        correlation_id,
                        restarted_at,
                    )?)
                    .ok_or_else(|| {
                        PersistenceError::InvalidRequest(
                            "context startup recovery counter exhausted".to_owned(),
                        )
                    })?;

                let mut prompt_inspector =
                    load_latest_prompt_inspector_for_recovery(&transaction, compaction.session_id)?
                        .ok_or_else(|| {
                            PersistenceError::InvalidRequest(
                                "active context compaction has no Prompt Inspector projection"
                                    .to_owned(),
                            )
                        })?;
                if prompt_inspector.active_agent_run_id != Some(compaction.agent_run_id)
                    || prompt_inspector.context_snapshot_id
                        != Some(compaction.source_context_snapshot_id)
                    || prompt_inspector.compaction.source_context_snapshot_id
                        != Some(compaction.source_context_snapshot_id)
                {
                    return Err(PersistenceError::InvalidRequest(
                        "active context compaction Prompt Inspector identity is inconsistent"
                            .to_owned(),
                    ));
                }
                prompt_inspector.compaction.state = resolution.parent_state;
                projection_drafts
                    .push(ModelProjectionDraft::PromptInspector(Box::new(prompt_inspector)));

                increment_context_recovery_report(&mut report, &resolution.error_code)?;
            }

            if !projection_drafts.is_empty() {
                let (changes, _) =
                    insert_model_projections(&transaction, runtime_instance_id, projection_drafts)?;
                report.prompt_inspector_projections_published = u64::try_from(changes.len())
                    .map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "context recovery projection count exceeds u64".to_owned(),
                        )
                    })?;
            }

            transaction.commit()?;
            Ok(report)
        })
    }

    pub fn create_context_compaction(
        &self,
        request: ContextCompactionCreateRequest,
    ) -> Result<ProjectionChange, PersistenceError> {
        validate_context_compaction_create(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (session_id, agent_run_id) =
                context_snapshot_owner(&transaction, request.source_context_snapshot_id)?;
            validate_compaction_projection(
                &request.prompt_inspector,
                session_id,
                agent_run_id,
                request.source_context_snapshot_id,
                ContextCompactionStateView::Required,
            )?;
            let focus_artifact_id = request.focus_artifact.as_ref().map(|value| value.artifact_id);
            if let Some(artifact) = request.focus_artifact {
                insert_context_artifacts(&transaction, &artifact_root, session_id, vec![artifact])?;
            }
            transaction.execute(
                "INSERT INTO context_compactions(
                    source_context_snapshot_id, sequence, state, source_span_json,
                    focus_artifact_id, requested_at
                 ) VALUES (?1, ?2, 'requested', ?3, ?4, ?5)",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    serde_json::to_string(&request.source_span)?,
                    focus_artifact_id.map(|id| id.to_string()),
                    request.requested_at.to_string(),
                ],
            )?;
            for (index, source) in request.sources.iter().enumerate() {
                transaction.execute(
                    "INSERT INTO context_compaction_sources(
                        source_context_snapshot_id, compaction_sequence, ordinal,
                        source_ref_json, source_hash_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        request.source_context_snapshot_id.to_string(),
                        i64::from(request.sequence),
                        ordinal(index)?,
                        serde_json::to_string(&source.source_ref)?,
                        serde_json::to_string(&source.source_hash)?,
                    ],
                )?;
            }
            let mut event = PersistentEventDraft::system(
                "ContextCompactionRequested",
                request.correlation_id,
                json!({
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "sequence": request.sequence,
                }),
            );
            event.session_id = Some(session_id);
            event.agent_run_id = Some(agent_run_id);
            insert_event(&transaction, &event)?;
            let (mut projections, _) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::PromptInspector(Box::new(request.prompt_inspector))],
            )?;
            let projection = projections.pop().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "compaction requested projection was not materialized".to_owned(),
                )
            })?;
            transaction.commit()?;
            Ok(projection)
        })
    }

    pub fn mark_context_compaction_running(
        &self,
        request: ContextCompactionRunningRequest,
    ) -> Result<ProjectionChange, PersistenceError> {
        if !(1..=2).contains(&request.attempt_ordinal)
            || request.attempt_context_snapshot_id == request.source_context_snapshot_id
        {
            return Err(PersistenceError::InvalidRequest(
                "context compaction model attempt identity is invalid".to_owned(),
            ));
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (session_id, agent_run_id) =
                context_snapshot_owner(&transaction, request.source_context_snapshot_id)?;
            validate_compaction_projection(
                &request.prompt_inspector,
                session_id,
                agent_run_id,
                request.source_context_snapshot_id,
                ContextCompactionStateView::Running,
            )?;
            validate_context_compaction_attempt_graph(
                &transaction,
                session_id,
                agent_run_id,
                &request,
            )?;
            let (parent_state, attempt_count) = transaction
                .query_row(
                    "SELECT c.state, COUNT(a.attempt_ordinal)
                     FROM context_compactions c
                     LEFT JOIN context_compaction_model_attempts a
                       ON a.source_context_snapshot_id=c.source_context_snapshot_id
                      AND a.compaction_sequence=c.sequence
                     WHERE c.source_context_snapshot_id=?1 AND c.sequence=?2
                     GROUP BY c.state",
                    params![
                        request.source_context_snapshot_id.to_string(),
                        i64::from(request.sequence),
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?
                .ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "context compaction operation is missing".to_owned(),
                    )
                })?;
            let expected_ordinal = u8::try_from(attempt_count + 1).map_err(|_| {
                PersistenceError::InvalidRequest(
                    "context compaction attempt count is invalid".to_owned(),
                )
            })?;
            if !matches!(parent_state.as_str(), "requested" | "running")
                || expected_ordinal != request.attempt_ordinal
            {
                return Err(PersistenceError::InvalidRequest(
                    "context compaction attempt order is invalid".to_owned(),
                ));
            }
            if request.attempt_ordinal > 1 {
                let prior_state = transaction
                    .query_row(
                        "SELECT state FROM context_compaction_model_attempts
                         WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
                           AND attempt_ordinal=?3",
                        params![
                            request.source_context_snapshot_id.to_string(),
                            i64::from(request.sequence),
                            i64::from(request.attempt_ordinal - 1),
                        ],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                if !matches!(prior_state.as_deref(), Some("failed")) {
                    return Err(PersistenceError::InvalidRequest(
                        "context compaction retry requires a failed prior attempt".to_owned(),
                    ));
                }
            }
            let changed = transaction.execute(
                "UPDATE context_compactions
                 SET state='running', model_step_id=?3, transport_attempt_id=?4
                 WHERE source_context_snapshot_id=?1 AND sequence=?2
                   AND state IN ('requested', 'running')",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    request.model_step_id.to_string(),
                    request.transport_attempt_id.map(|id| id.to_string()),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "context compaction is not active".to_owned(),
                ));
            }
            transaction.execute(
                "INSERT INTO context_compaction_model_attempts(
                    source_context_snapshot_id, compaction_sequence, attempt_ordinal,
                    attempt_context_snapshot_id, model_step_id, model_execution_snapshot_id,
                    transport_attempt_id, state, error_code, started_at, terminal_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', NULL, ?8, NULL)",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    i64::from(request.attempt_ordinal),
                    request.attempt_context_snapshot_id.to_string(),
                    request.model_step_id.to_string(),
                    request.model_execution_snapshot_id.to_string(),
                    request.transport_attempt_id.map(|id| id.to_string()),
                    request.started_at.to_string(),
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "ContextCompactionRunning",
                request.correlation_id,
                json!({
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "sequence": request.sequence,
                    "attempt_ordinal": request.attempt_ordinal,
                    "attempt_context_snapshot_id": request.attempt_context_snapshot_id,
                    "model_step_id": request.model_step_id,
                    "model_execution_snapshot_id": request.model_execution_snapshot_id,
                    "transport_attempt_id": request.transport_attempt_id,
                }),
            );
            event.session_id = Some(session_id);
            event.agent_run_id = Some(agent_run_id);
            insert_event(&transaction, &event)?;
            let (mut projections, _) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::PromptInspector(Box::new(request.prompt_inspector))],
            )?;
            let projection = projections.pop().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "compaction running projection was not materialized".to_owned(),
                )
            })?;
            transaction.commit()?;
            Ok(projection)
        })
    }

    pub fn finish_context_compaction_model_attempt(
        &self,
        request: ContextCompactionAttemptTerminalRequest,
    ) -> Result<CompactionModelAttemptRecord, PersistenceError> {
        if !(1..=2).contains(&request.attempt_ordinal)
            || request.state == CompactionModelAttemptState::Running
            || (request.state == CompactionModelAttemptState::Completed
                && (request.transport_attempt_id.is_none() || request.error_code.is_some()))
            || (request.state != CompactionModelAttemptState::Completed
                && request.error_code.as_deref().is_none_or(str::is_empty))
        {
            return Err(PersistenceError::InvalidRequest(
                "context compaction model attempt terminal state is invalid".to_owned(),
            ));
        }
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let mut existing = load_context_compaction_model_attempt(
                &transaction,
                request.source_context_snapshot_id,
                request.sequence,
                request.attempt_ordinal,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "context compaction model attempt is missing".to_owned(),
                )
            })?;
            if existing.state != CompactionModelAttemptState::Running {
                if existing.state == request.state
                    && existing.transport_attempt_id == request.transport_attempt_id
                    && existing.error_code == request.error_code
                    && existing.terminal_at == Some(request.terminal_at)
                {
                    return Ok(existing);
                }
                return Err(PersistenceError::InvalidRequest(
                    "context compaction model attempt terminal outcome changed".to_owned(),
                ));
            }
            validate_context_compaction_transport_attempt(
                &transaction,
                &existing,
                request.state,
                request.transport_attempt_id,
            )?;
            let state = compaction_model_attempt_state_name(request.state);
            let changed = transaction.execute(
                "UPDATE context_compaction_model_attempts
                 SET transport_attempt_id=?4, state=?5, error_code=?6, terminal_at=?7
                 WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
                   AND attempt_ordinal=?3 AND state='running'",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    i64::from(request.attempt_ordinal),
                    request.transport_attempt_id.map(|id| id.to_string()),
                    state,
                    request.error_code,
                    request.terminal_at.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "context compaction model attempt changed concurrently".to_owned(),
                ));
            }
            transaction.execute(
                "UPDATE context_compactions
                 SET transport_attempt_id=?3
                 WHERE source_context_snapshot_id=?1 AND sequence=?2 AND state='running'",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    request.transport_attempt_id.map(|id| id.to_string()),
                ],
            )?;
            let event_type = match request.state {
                CompactionModelAttemptState::Completed => "ContextCompactionModelAttemptCompleted",
                CompactionModelAttemptState::Failed => "ContextCompactionModelAttemptFailed",
                CompactionModelAttemptState::Cancelled => "ContextCompactionModelAttemptCancelled",
                CompactionModelAttemptState::UnknownSideEffect => {
                    "ContextCompactionModelAttemptSideEffectUnknown"
                }
                CompactionModelAttemptState::Running => unreachable!(),
            };
            let (session_id, agent_run_id) =
                context_snapshot_owner(&transaction, request.source_context_snapshot_id)?;
            let mut event = PersistentEventDraft::system(
                event_type,
                request.correlation_id,
                json!({
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "sequence": request.sequence,
                    "attempt_ordinal": request.attempt_ordinal,
                    "transport_attempt_id": request.transport_attempt_id,
                    "error_code": request.error_code,
                }),
            );
            event.session_id = Some(session_id);
            event.agent_run_id = Some(agent_run_id);
            insert_event(&transaction, &event)?;
            existing.state = request.state;
            existing.transport_attempt_id = request.transport_attempt_id;
            existing.error_code = request.error_code;
            existing.terminal_at = Some(request.terminal_at);
            transaction.commit()?;
            Ok(existing)
        })
    }

    pub fn complete_context_compaction(
        &self,
        request: ContextCompactionCompletedRequest,
    ) -> Result<ProjectionChange, PersistenceError> {
        validate_context_compaction_completed(&request)?;
        let runtime_instance_id = self.runtime_instance_id();
        let artifact_root = self.artifact_root().to_path_buf();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (session_id, agent_run_id) =
                context_snapshot_owner(&transaction, request.source_context_snapshot_id)?;
            validate_compaction_projection(
                &request.prompt_inspector,
                session_id,
                agent_run_id,
                request.source_context_snapshot_id,
                ContextCompactionStateView::Completed,
            )?;
            let persisted_source_span = transaction
                .query_row(
                    "SELECT source_span_json FROM context_compactions
                     WHERE source_context_snapshot_id=?1 AND sequence=?2 AND state='running'",
                    params![
                        request.source_context_snapshot_id.to_string(),
                        i64::from(request.sequence),
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|value| serde_json::from_str::<CompactionSourceSpan>(&value))
                .transpose()?;
            if persisted_source_span.as_ref() != Some(&request.record.source_span) {
                return Err(PersistenceError::InvalidRequest(
                    "completed context compaction source span changed".to_owned(),
                ));
            }
            validate_persisted_compaction_sources(
                &transaction,
                request.source_context_snapshot_id,
                request.sequence,
                &request.anchor_manifest_value,
            )?;
            let attempt = load_context_compaction_model_attempt(
                &transaction,
                request.source_context_snapshot_id,
                request.sequence,
                request.attempt_ordinal,
            )?
            .filter(|attempt| attempt.state == CompactionModelAttemptState::Running)
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "completed context compaction has no running model attempt".to_owned(),
                )
            })?;
            validate_context_compaction_transport_attempt(
                &transaction,
                &attempt,
                CompactionModelAttemptState::Completed,
                Some(request.transport_attempt_id),
            )?;
            let artifacts = vec![
                request.historical_summary.clone(),
                request.active_turn_summary.clone(),
                request.structured_compaction.clone(),
                request.anchor_manifest.clone(),
            ];
            insert_context_artifacts(&transaction, &artifact_root, session_id, artifacts)?;
            let attempt_changed = transaction.execute(
                "UPDATE context_compaction_model_attempts
                 SET transport_attempt_id=?4, state='completed', error_code=NULL, terminal_at=?5
                 WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
                   AND attempt_ordinal=?3 AND state='running'",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    i64::from(request.attempt_ordinal),
                    request.transport_attempt_id.to_string(),
                    request.completed_at.to_string(),
                ],
            )?;
            if attempt_changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "completed context compaction model attempt changed concurrently".to_owned(),
                ));
            }
            let changed = transaction.execute(
                "UPDATE context_compactions
                 SET state='completed', historical_summary_artifact_id=?3,
                     active_turn_summary_artifact_id=?4,
                     structured_compaction_artifact_id=?5,
                     anchor_manifest_artifact_id=?6, record_json=?7,
                     error_code=NULL, transport_attempt_id=?8, completed_at=?9
                 WHERE source_context_snapshot_id=?1 AND sequence=?2 AND state='running'",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    request.historical_summary.artifact_id.to_string(),
                    request.active_turn_summary.artifact_id.to_string(),
                    request.structured_compaction.artifact_id.to_string(),
                    request.anchor_manifest.artifact_id.to_string(),
                    serde_json::to_string(&request.record)?,
                    request.transport_attempt_id.to_string(),
                    request.completed_at.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "context compaction is not in running state".to_owned(),
                ));
            }
            transaction.execute(
                "INSERT INTO context_compaction_lkg(
                    session_id, agent_run_id, source_context_snapshot_id,
                    compaction_sequence, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(session_id, agent_run_id) DO UPDATE SET
                    source_context_snapshot_id=excluded.source_context_snapshot_id,
                    compaction_sequence=excluded.compaction_sequence,
                    updated_at=excluded.updated_at",
                params![
                    session_id.to_string(),
                    agent_run_id.to_string(),
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    request.completed_at.to_string(),
                ],
            )?;
            insert_context_search_chunks(
                &transaction,
                session_id,
                &[
                    ContextSearchChunkDraft {
                        agent_run_id: Some(agent_run_id),
                        turn_task_id: None,
                        source_type: "compaction".to_owned(),
                        source_id: format!(
                            "{}:{}:historical",
                            request.source_context_snapshot_id, request.sequence
                        ),
                        session_sequence: request.record.source_span.end_sequence,
                        privacy_class: ContextPrivacyClass::ModelVisible,
                        source_hash: request.record.historical_summary_hash.clone(),
                        searchable_text: String::from_utf8(
                            request.historical_summary.bytes.clone(),
                        )
                        .map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "historical summary artifact is not UTF-8".to_owned(),
                            )
                        })?,
                    },
                    ContextSearchChunkDraft {
                        agent_run_id: Some(agent_run_id),
                        turn_task_id: None,
                        source_type: "compaction".to_owned(),
                        source_id: format!(
                            "{}:{}:structured",
                            request.source_context_snapshot_id, request.sequence
                        ),
                        session_sequence: request.record.source_span.end_sequence,
                        privacy_class: ContextPrivacyClass::ModelVisible,
                        source_hash: request.record.structured_markdown_hash.clone(),
                        searchable_text: String::from_utf8(
                            request.structured_compaction.bytes.clone(),
                        )
                        .map_err(|_| {
                            PersistenceError::InvalidRequest(
                                "structured compaction artifact is not UTF-8".to_owned(),
                            )
                        })?,
                    },
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "ContextCompactionCompleted",
                request.correlation_id,
                json!({
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "sequence": request.sequence,
                    "attempt_ordinal": request.attempt_ordinal,
                    "transport_attempt_id": request.transport_attempt_id,
                    "source_span": request.record.source_span,
                    "anchor_manifest_hash": request.record.anchor_manifest_hash,
                }),
            );
            event.session_id = Some(session_id);
            event.agent_run_id = Some(agent_run_id);
            insert_event(&transaction, &event)?;
            let (mut projections, _) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::PromptInspector(Box::new(request.prompt_inspector))],
            )?;
            let projection = projections.pop().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "compaction completed projection was not materialized".to_owned(),
                )
            })?;
            transaction.commit()?;
            Ok(projection)
        })
    }

    pub fn finish_context_compaction(
        &self,
        request: ContextCompactionTerminalRequest,
    ) -> Result<ProjectionChange, PersistenceError> {
        if !matches!(
            request.state,
            ContextCompactionStateView::Failed | ContextCompactionStateView::Cancelled
        ) || request.error_code.trim().is_empty()
            || match (request.attempt_ordinal, request.attempt_state) {
                (None, None) => request.transport_attempt_id.is_some(),
                (Some(ordinal), Some(attempt_state)) => {
                    !(1..=2).contains(&ordinal)
                        || attempt_state == CompactionModelAttemptState::Running
                        || attempt_state == CompactionModelAttemptState::Completed
                        || (request.state == ContextCompactionStateView::Cancelled
                            && attempt_state != CompactionModelAttemptState::Cancelled)
                        || (request.state == ContextCompactionStateView::Failed
                            && !matches!(
                                attempt_state,
                                CompactionModelAttemptState::Failed
                                    | CompactionModelAttemptState::UnknownSideEffect
                            ))
                }
                _ => true,
            }
        {
            return Err(PersistenceError::InvalidRequest(
                "context compaction terminal request is invalid".to_owned(),
            ));
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (session_id, agent_run_id) =
                context_snapshot_owner(&transaction, request.source_context_snapshot_id)?;
            validate_compaction_projection(
                &request.prompt_inspector,
                session_id,
                agent_run_id,
                request.source_context_snapshot_id,
                request.state,
            )?;
            if let (Some(attempt_ordinal), Some(attempt_state)) =
                (request.attempt_ordinal, request.attempt_state)
            {
                let attempt = load_context_compaction_model_attempt(
                    &transaction,
                    request.source_context_snapshot_id,
                    request.sequence,
                    attempt_ordinal,
                )?
                .filter(|attempt| attempt.state == CompactionModelAttemptState::Running)
                .ok_or_else(|| {
                    PersistenceError::InvalidRequest(
                        "terminal context compaction has no running model attempt".to_owned(),
                    )
                })?;
                validate_context_compaction_transport_attempt(
                    &transaction,
                    &attempt,
                    attempt_state,
                    request.transport_attempt_id,
                )?;
                let attempt_changed = transaction.execute(
                    "UPDATE context_compaction_model_attempts
                     SET transport_attempt_id=?4, state=?5, error_code=?6, terminal_at=?7
                     WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
                       AND attempt_ordinal=?3 AND state='running'",
                    params![
                        request.source_context_snapshot_id.to_string(),
                        i64::from(request.sequence),
                        i64::from(attempt_ordinal),
                        request.transport_attempt_id.map(|id| id.to_string()),
                        compaction_model_attempt_state_name(attempt_state),
                        request.error_code,
                        request.completed_at.to_string(),
                    ],
                )?;
                if attempt_changed != 1 {
                    return Err(PersistenceError::InvalidRequest(
                        "terminal context compaction model attempt changed concurrently".to_owned(),
                    ));
                }
            } else {
                let running_attempt_exists = transaction.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM context_compaction_model_attempts
                        WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
                          AND state='running'
                    )",
                    params![
                        request.source_context_snapshot_id.to_string(),
                        i64::from(request.sequence),
                    ],
                    |row| row.get::<_, bool>(0),
                )?;
                if running_attempt_exists {
                    return Err(PersistenceError::InvalidRequest(
                        "terminal context compaction omitted its running model attempt".to_owned(),
                    ));
                }
            }
            let state = context_compaction_state_name(request.state)?;
            let changed = transaction.execute(
                "UPDATE context_compactions
                 SET state=?3, error_code=?4, transport_attempt_id=?5, completed_at=?6
                 WHERE source_context_snapshot_id=?1 AND sequence=?2
                   AND state IN ('requested', 'running')",
                params![
                    request.source_context_snapshot_id.to_string(),
                    i64::from(request.sequence),
                    state,
                    request.error_code,
                    request.transport_attempt_id.map(|id| id.to_string()),
                    request.completed_at.to_string(),
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "context compaction has already reached a terminal state".to_owned(),
                ));
            }
            let event_type = if request.state == ContextCompactionStateView::Failed {
                "ContextCompactionFailed"
            } else {
                "ContextCompactionCancelled"
            };
            let mut event = PersistentEventDraft::system(
                event_type,
                request.correlation_id,
                json!({
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "sequence": request.sequence,
                    "attempt_ordinal": request.attempt_ordinal,
                    "attempt_state": request.attempt_state,
                    "transport_attempt_id": request.transport_attempt_id,
                    "error_code": request.error_code,
                }),
            );
            event.session_id = Some(session_id);
            event.agent_run_id = Some(agent_run_id);
            insert_event(&transaction, &event)?;
            let (mut projections, _) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::PromptInspector(Box::new(request.prompt_inspector))],
            )?;
            let projection = projections.pop().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "compaction terminal projection was not materialized".to_owned(),
                )
            })?;
            transaction.commit()?;
            Ok(projection)
        })
    }

    pub fn context_compaction(
        &self,
        session_id: SessionId,
        agent_run_id: AgentRunId,
        source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
        sequence: u32,
    ) -> Result<Option<ContextCompactionStoredRecord>, PersistenceError> {
        self.with_reader(|connection| {
            load_context_compaction(
                connection,
                session_id,
                agent_run_id,
                source_context_snapshot_id,
                sequence,
            )
        })
    }

    pub fn latest_context_compaction(
        &self,
        session_id: SessionId,
        agent_run_id: AgentRunId,
    ) -> Result<Option<ContextCompactionStoredRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let key = connection
                .query_row(
                    "SELECT source_context_snapshot_id, compaction_sequence
                     FROM context_compaction_lkg
                     WHERE session_id=?1 AND agent_run_id=?2",
                    params![session_id.to_string(), agent_run_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let Some((snapshot_id, sequence)) = key else {
                return Ok(None);
            };
            load_context_compaction(
                connection,
                session_id,
                agent_run_id,
                aura_domain_common::ContextProjectionSnapshotId::from_str(&snapshot_id)?,
                u32::try_from(sequence).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted compaction sequence is invalid".to_owned(),
                    )
                })?,
            )
        })
    }

    pub fn require_context_remediation(
        &self,
        request: ContextRemediationRequiredRequest,
    ) -> Result<ContextRemediationCommitResult, PersistenceError> {
        validate_context_remediation_plan(&request.plan)?;
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let owner = context_snapshot_owner(&transaction, request.source_context_snapshot_id)?;
            if owner != (request.session_id, request.agent_run_id) {
                return Err(PersistenceError::InvalidRequest(
                    "context remediation source owner is inconsistent".to_owned(),
                ));
            }
            validate_context_remediation_projection(
                &request.prompt_inspector,
                request.session_id,
                request.agent_run_id,
                request.source_context_snapshot_id,
                Some(&request.plan),
            )?;
            if let Some(existing) = load_context_remediation(
                &transaction,
                request.session_id,
                request.agent_run_id,
                request.source_context_snapshot_id,
            )? {
                if existing.state == ContextRemediationState::Awaiting
                    && existing.plan == request.plan
                    && existing.required_at == request.required_at
                {
                    transaction.commit()?;
                    return Ok(ContextRemediationCommitResult {
                        record: existing,
                        projection: None,
                        changed: false,
                    });
                }
                return Err(PersistenceError::InvalidRequest(
                    "context remediation identity already exists with different facts".to_owned(),
                ));
            }
            let another_active = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM context_remediations
                    WHERE session_id=?1 AND agent_run_id=?2 AND state='awaiting'
                 )",
                params![request.session_id.to_string(), request.agent_run_id.to_string()],
                |row| row.get::<_, bool>(0),
            )?;
            if another_active {
                return Err(PersistenceError::InvalidRequest(
                    "another context remediation is already active for this AgentRun".to_owned(),
                ));
            }
            transaction.execute(
                "INSERT INTO context_remediations(
                    session_id, agent_run_id, source_context_snapshot_id, state,
                    plan_json, required_at, cleared_at, clear_reason
                 ) VALUES (?1, ?2, ?3, 'awaiting', ?4, ?5, NULL, NULL)",
                params![
                    request.session_id.to_string(),
                    request.agent_run_id.to_string(),
                    request.source_context_snapshot_id.to_string(),
                    serde_json::to_string(&request.plan)?,
                    request.required_at.to_string(),
                ],
            )?;
            let mut event = PersistentEventDraft::system(
                "ContextRemediationRequired",
                request.correlation_id,
                json!({
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "error_code": request.plan.overflow.error_code,
                    "overflow_tokens": request.plan.overflow.overflow_tokens,
                    "action_count": request.plan.actions.len(),
                }),
            );
            event.session_id = Some(request.session_id);
            event.agent_run_id = Some(request.agent_run_id);
            insert_event(&transaction, &event)?;
            let (mut projections, _) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::PromptInspector(Box::new(request.prompt_inspector))],
            )?;
            let projection = projections.pop().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "context remediation projection was not materialized".to_owned(),
                )
            })?;
            let record = ContextRemediationRecord {
                session_id: request.session_id,
                agent_run_id: request.agent_run_id,
                source_context_snapshot_id: request.source_context_snapshot_id,
                plan: request.plan,
                state: ContextRemediationState::Awaiting,
                required_at: request.required_at,
                cleared_at: None,
                clear_reason: None,
            };
            transaction.commit()?;
            Ok(ContextRemediationCommitResult {
                record,
                projection: Some(projection),
                changed: true,
            })
        })
    }

    pub fn clear_context_remediation(
        &self,
        request: ContextRemediationClearedRequest,
    ) -> Result<ContextRemediationCommitResult, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            validate_context_remediation_projection(
                &request.prompt_inspector,
                request.session_id,
                request.agent_run_id,
                request.source_context_snapshot_id,
                None,
            )?;
            let existing = load_context_remediation(
                &transaction,
                request.session_id,
                request.agent_run_id,
                request.source_context_snapshot_id,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "context remediation is missing for the requested source".to_owned(),
                )
            })?;
            if existing.state == ContextRemediationState::Cleared {
                if existing.clear_reason == Some(request.reason) {
                    transaction.commit()?;
                    return Ok(ContextRemediationCommitResult {
                        record: existing,
                        projection: None,
                        changed: false,
                    });
                }
                return Err(PersistenceError::InvalidRequest(
                    "context remediation already has another clear reason".to_owned(),
                ));
            }
            let reason = context_remediation_clear_reason_name(request.reason);
            let changed = transaction.execute(
                "UPDATE context_remediations
                 SET state='cleared', cleared_at=?4, clear_reason=?5
                 WHERE session_id=?1 AND agent_run_id=?2 AND source_context_snapshot_id=?3
                   AND state='awaiting'",
                params![
                    request.session_id.to_string(),
                    request.agent_run_id.to_string(),
                    request.source_context_snapshot_id.to_string(),
                    request.cleared_at.to_string(),
                    reason,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::InvalidRequest(
                    "context remediation is not awaiting user action".to_owned(),
                ));
            }
            let mut event = PersistentEventDraft::system(
                "ContextRemediationCleared",
                request.correlation_id,
                json!({
                    "source_context_snapshot_id": request.source_context_snapshot_id,
                    "reason": request.reason,
                }),
            );
            event.session_id = Some(request.session_id);
            event.agent_run_id = Some(request.agent_run_id);
            insert_event(&transaction, &event)?;
            let (mut projections, _) = insert_model_projections(
                &transaction,
                runtime_instance_id,
                vec![ModelProjectionDraft::PromptInspector(Box::new(request.prompt_inspector))],
            )?;
            let projection = projections.pop().ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "context remediation clear projection was not materialized".to_owned(),
                )
            })?;
            let record = ContextRemediationRecord {
                state: ContextRemediationState::Cleared,
                cleared_at: Some(request.cleared_at),
                clear_reason: Some(request.reason),
                ..existing
            };
            transaction.commit()?;
            Ok(ContextRemediationCommitResult {
                record,
                projection: Some(projection),
                changed: true,
            })
        })
    }

    pub fn active_context_remediation(
        &self,
        session_id: SessionId,
        agent_run_id: AgentRunId,
    ) -> Result<Option<ContextRemediationRecord>, PersistenceError> {
        self.with_reader(|connection| {
            let snapshot_id = connection
                .query_row(
                    "SELECT source_context_snapshot_id FROM context_remediations
                     WHERE session_id=?1 AND agent_run_id=?2 AND state='awaiting'",
                    params![session_id.to_string(), agent_run_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            snapshot_id
                .map(|snapshot_id| {
                    load_context_remediation(
                        connection,
                        session_id,
                        agent_run_id,
                        aura_domain_common::ContextProjectionSnapshotId::from_str(&snapshot_id)?,
                    )
                })
                .transpose()
                .map(Option::flatten)
        })
    }

    pub fn context_internal_artifact_metadata(
        &self,
        session_id: SessionId,
        artifact_id: ArtifactId,
    ) -> Result<ContextInternalArtifactMetadata, PersistenceError> {
        self.with_reader(|connection| {
            load_context_internal_artifact_metadata(connection, session_id, artifact_id)
        })
    }

    pub fn read_context_internal_artifact_range(
        &self,
        session_id: SessionId,
        artifact_id: ArtifactId,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, PersistenceError> {
        self.context_internal_artifact_metadata(session_id, artifact_id)?;
        self.read_internal_artifact_range(artifact_id, offset, length)
    }
}

fn history_group_ref_from_row(
    row: &ContextEventSelectionRow,
) -> Result<Option<HistoryAtomicGroupRefV1>, PersistenceError> {
    let direct = row
        .direct_group_hash_json
        .as_deref()
        .map(|hash| -> Result<HistoryAtomicGroupRefV1, PersistenceError> {
            Ok(HistoryAtomicGroupRefV1 {
                assistant_response_event_id: EventId::from_str(&row.event_id)?,
                group_hash: serde_json::from_str(hash)?,
            })
        })
        .transpose()?;
    let legacy = match row.legacy_verification.as_deref() {
        None | Some("legacy_unverified") => None,
        Some("verified_side_mapping") => Some(HistoryAtomicGroupRefV1 {
            assistant_response_event_id: EventId::from_str(
                row.legacy_assistant_event_id
                    .as_deref()
                    .ok_or(PersistenceError::ToolEntityIntegrity)?,
            )?,
            group_hash: serde_json::from_str(
                row.legacy_group_hash_json
                    .as_deref()
                    .ok_or(PersistenceError::ToolEntityIntegrity)?,
            )?,
        }),
        Some(_) => return Err(PersistenceError::ToolEntityIntegrity),
    };
    if direct.is_some() && legacy.is_some() && direct != legacy {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(direct.or(legacy))
}

fn legacy_verification_from_row(
    row: &ContextEventSelectionRow,
) -> Result<Option<LegacyHistoryAtomicGroupVerificationV1>, PersistenceError> {
    match row.legacy_verification.as_deref() {
        None => Ok(row
            .legacy_atomic_group_id
            .as_ref()
            .map(|_| LegacyHistoryAtomicGroupVerificationV1::LegacyUnverified)),
        Some("verified_side_mapping") => {
            Ok(Some(LegacyHistoryAtomicGroupVerificationV1::VerifiedSideMapping))
        }
        Some("legacy_unverified") => {
            if row.legacy_assistant_event_id.is_some() || row.legacy_group_hash_json.is_some() {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            Ok(Some(LegacyHistoryAtomicGroupVerificationV1::LegacyUnverified))
        }
        Some(_) => Err(PersistenceError::ToolEntityIntegrity),
    }
}

fn validate_prompt_bundle_commit(
    request: &PromptBundleCommitRequest,
) -> Result<(), PersistenceError> {
    request.snapshot.validate_contract_shape().map_err(|error| {
        PersistenceError::InvalidRequest(format!("prompt bundle snapshot is invalid: {error}"))
    })?;
    let expected = request
        .snapshot
        .module_snapshots
        .iter()
        .map(|module| (module.content_artifact_id, module.content_hash.clone()))
        .chain(
            request
                .snapshot
                .custom_addendum_artifact_id
                .zip(request.snapshot.custom_addendum_hash.clone()),
        )
        .collect::<BTreeMap<_, _>>();
    validate_artifact_drafts(&request.artifacts, &expected)
}

fn validate_context_projection_commit(
    request: &ContextProjectionCommitRequest,
) -> Result<(), PersistenceError> {
    request.snapshot.validate_contract_shape().map_err(|error| {
        PersistenceError::InvalidRequest(format!("context projection snapshot is invalid: {error}"))
    })?;
    request.instruction_envelope.validate_contract_shape().map_err(|error| {
        PersistenceError::InvalidRequest(format!("instruction envelope is invalid: {error}"))
    })?;
    request.model_egress_manifest.validate_contract_shape().map_err(|error| {
        PersistenceError::InvalidRequest(format!("model egress manifest is invalid: {error}"))
    })?;
    if request.snapshot.id != request.model_egress_manifest.context_snapshot_id
        || request.execution_snapshot.context_snapshot_id != request.snapshot.id
        || request.execution_snapshot.prompt_bundle_snapshot_id
            != request.snapshot.prompt_bundle_snapshot_id
        || request.execution_snapshot.model_step_id != request.snapshot.model_step_id
        || request.execution_snapshot.model_profile_id != request.snapshot.model_profile_id
        || request.execution_snapshot.runtime_effective_window_tokens
            != request.snapshot.budget.runtime_effective_window_tokens
        || request.execution_snapshot.request_options.effective_max_output_tokens
            != request.snapshot.output_request.effective_max_output_tokens
        || request.snapshot.instruction_envelope_hash
            != request.instruction_envelope.instruction_hash
        || request.snapshot.model_egress_manifest_hash
            != request.model_egress_manifest.manifest_hash
        || request.prompt_inspector.session_id != request.snapshot.session_id
        || request.prompt_inspector.context_snapshot_id != Some(request.snapshot.id)
    {
        return Err(PersistenceError::InvalidRequest(
            "context projection linked identities are inconsistent".to_owned(),
        ));
    }
    let mut expected = request
        .snapshot
        .sections
        .iter()
        .map(|section| (section.content_artifact_id, section.content_hash.clone()))
        .collect::<BTreeMap<_, _>>();
    expected.insert(
        request.snapshot.instruction_envelope_artifact_id,
        request.snapshot.instruction_envelope_hash.clone(),
    );
    let manifest_bytes = canonical_json_v1(&request.model_egress_manifest).map_err(|error| {
        PersistenceError::InvalidRequest(format!("model egress manifest is not canonical: {error}"))
    })?;
    let provided_manifest = request
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == request.snapshot.model_egress_manifest_artifact_id)
        .ok_or_else(|| {
            PersistenceError::InvalidRequest("model egress manifest artifact is missing".to_owned())
        })?;
    if provided_manifest.bytes != manifest_bytes {
        return Err(PersistenceError::InvalidRequest(
            "model egress manifest artifact bytes are not canonical".to_owned(),
        ));
    }
    expected.insert(
        request.snapshot.model_egress_manifest_artifact_id,
        ContentHash::sha256(&provided_manifest.bytes),
    );
    validate_artifact_drafts(&request.artifacts, &expected)?;
    let envelope = request
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == request.snapshot.instruction_envelope_artifact_id)
        .ok_or_else(|| {
            PersistenceError::InvalidRequest("instruction envelope artifact is missing".to_owned())
        })?;
    if envelope.bytes != request.instruction_envelope.canonical_text.as_bytes() {
        return Err(PersistenceError::InvalidRequest(
            "instruction envelope artifact bytes are not canonical".to_owned(),
        ));
    }
    if request
        .search_chunks
        .iter()
        .any(|chunk| chunk.searchable_text.is_empty() || chunk.source_type.trim().is_empty())
    {
        return Err(PersistenceError::InvalidRequest("context search chunk is invalid".to_owned()));
    }
    Ok(())
}

fn validate_artifact_drafts(
    artifacts: &[ContextArtifactDraft],
    expected: &BTreeMap<ArtifactId, ContentHash>,
) -> Result<(), PersistenceError> {
    if artifacts.len() != expected.len() {
        return Err(PersistenceError::InvalidRequest(
            "context artifact set does not match snapshot references".to_owned(),
        ));
    }
    let mut actual = BTreeMap::new();
    for artifact in artifacts {
        if artifact.media_type.trim().is_empty()
            || actual.insert(artifact.artifact_id, ContentHash::sha256(&artifact.bytes)).is_some()
        {
            return Err(PersistenceError::InvalidRequest(
                "context artifact set contains invalid or duplicate entries".to_owned(),
            ));
        }
    }
    if &actual != expected {
        return Err(PersistenceError::InvalidRequest(
            "context artifact content hash does not match the frozen snapshot".to_owned(),
        ));
    }
    Ok(())
}

fn insert_context_artifacts(
    transaction: &Transaction<'_>,
    artifact_root: &std::path::Path,
    session_id: SessionId,
    artifacts: Vec<ContextArtifactDraft>,
) -> Result<(), PersistenceError> {
    for artifact in artifacts {
        insert_artifact_in_transaction(
            transaction,
            artifact_root,
            artifact.artifact_id,
            ArtifactWriteRequest {
                scope: ArtifactScope::Session,
                media_type: artifact.media_type,
                access: ArtifactAccess::NONE,
                owner: ArtifactOwnerRef::Session { session_id },
                bytes: artifact.bytes,
            },
        )?;
    }
    Ok(())
}

fn insert_context_snapshot_row(
    transaction: &Transaction<'_>,
    snapshot: &ContextProjectionSnapshot,
) -> Result<(), PersistenceError> {
    transaction.execute(
        "INSERT INTO context_projection_snapshots(
            context_snapshot_id, session_id, turn_task_id, agent_run_id, model_step_id,
            prompt_bundle_snapshot_id, model_profile_id, runtime_effective_window_tokens,
            output_reserve_tokens, safety_reserve_tokens, request_input_budget_tokens,
            estimated_input_tokens, history_cutoff, instruction_envelope_artifact_id,
            instruction_envelope_hash_json, model_egress_policy_version,
            model_egress_manifest_artifact_id, model_egress_manifest_hash_json, budget_json,
            output_request_json, context_usage_json, active_task_lineage_id,
            snapshot_hash_json, snapshot_json, created_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25
         )",
        params![
            snapshot.id.to_string(),
            snapshot.session_id.to_string(),
            snapshot.turn_task_id.to_string(),
            snapshot.agent_run_id.to_string(),
            snapshot.model_step_id.to_string(),
            snapshot.prompt_bundle_snapshot_id.to_string(),
            snapshot.model_profile_id.to_string(),
            to_sql_u64(snapshot.budget.runtime_effective_window_tokens)?,
            to_sql_u64(snapshot.budget.output_reserve_tokens)?,
            to_sql_u64(snapshot.budget.safety_reserve_tokens)?,
            to_sql_u64(snapshot.budget.request_input_budget_tokens)?,
            to_sql_u64(snapshot.budget.estimated_input_tokens)?,
            to_sql_u64(snapshot.history_cutoff.value())?,
            snapshot.instruction_envelope_artifact_id.to_string(),
            serde_json::to_string(&snapshot.instruction_envelope_hash)?,
            i64::from(snapshot.model_egress_policy_version.get()),
            snapshot.model_egress_manifest_artifact_id.to_string(),
            serde_json::to_string(&snapshot.model_egress_manifest_hash)?,
            serde_json::to_string(&snapshot.budget)?,
            serde_json::to_string(&snapshot.output_request)?,
            serde_json::to_string(&snapshot.context_usage)?,
            snapshot.active_task_lineage_id.map(|id| id.to_string()),
            serde_json::to_string(&snapshot.snapshot_hash)?,
            serde_json::to_string(snapshot)?,
            snapshot.created_at.to_string(),
        ],
    )?;
    transaction.execute(
        "UPDATE context_projection_snapshots
         SET snapshot_version=?2 WHERE context_snapshot_id=?1",
        params![snapshot.id.to_string(), i64::from(snapshot.snapshot_version)],
    )?;
    Ok(())
}

fn insert_context_children(
    transaction: &Transaction<'_>,
    snapshot: &ContextProjectionSnapshot,
    manifest: &ModelEgressManifestV1,
) -> Result<(), PersistenceError> {
    for section in &snapshot.sections {
        transaction.execute(
            "INSERT INTO context_projection_sections(
                context_snapshot_id, physical_ordinal, section_kind, content_artifact_id,
                content_hash_json, character_count, byte_count, estimated_tokens,
                source_refs_json, compressibility, privacy_class, is_empty_marker
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                snapshot.id.to_string(),
                i64::from(section.physical_ordinal),
                enum_name(&section.kind)?,
                section.content_artifact_id.to_string(),
                serde_json::to_string(&section.content_hash)?,
                to_sql_u64(section.character_count)?,
                to_sql_u64(section.byte_count)?,
                to_sql_u64(section.estimated_tokens)?,
                serde_json::to_string(&section.source_refs)?,
                enum_name(&section.compressibility)?,
                enum_name(&section.privacy_class)?,
                i64::from(section.is_empty_marker),
            ],
        )?;
    }
    transaction.execute(
        "INSERT INTO context_projection_tool_registry_v2(
            context_snapshot_id, tool_registry_snapshot_id, registry_revision,
            registry_content_hash_json, tool_set_hash_json, schema_set_json,
            effective_bindings_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            snapshot.id.to_string(),
            snapshot.tool_schema_set.registry_snapshot_ref.tool_registry_snapshot_id.to_string(),
            to_sql_u64(snapshot.tool_schema_set.registry_snapshot_ref.registry_revision.value())?,
            serde_json::to_string(&snapshot.tool_schema_set.registry_snapshot_ref.content_hash)?,
            serde_json::to_string(&snapshot.tool_schema_set.tool_set_hash)?,
            serde_json::to_string(&snapshot.tool_schema_set)?,
            serde_json::to_string(&snapshot.effective_tool_bindings)?,
        ],
    )?;
    for (index, tool) in snapshot.tool_schema_set.tools.iter().enumerate() {
        transaction.execute(
            "INSERT INTO context_projection_tool_schemas(
                context_snapshot_id, ordinal, tool_id, tool_version_json,
                schema_hash_json, tool_schema_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                snapshot.id.to_string(),
                ordinal(index)?,
                tool.tool_id.to_string(),
                serde_json::to_string(&tool.tool_version)?,
                serde_json::to_string(&tool.schema_hash)?,
                serde_json::to_string(tool)?,
            ],
        )?;
    }
    for (index, item) in snapshot.conversation_items.iter().enumerate() {
        let (kind, source_event_id, tool_call_id) = conversation_identity(item);
        transaction.execute(
            "INSERT INTO context_projection_conversation_items(
                context_snapshot_id, ordinal, item_kind, source_event_id, tool_call_id, item_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                snapshot.id.to_string(),
                ordinal(index)?,
                kind,
                source_event_id.to_string(),
                tool_call_id.map(|id| id.to_string()),
                serde_json::to_string(item)?,
            ],
        )?;
    }
    for (index, attachment) in snapshot.attachments.iter().enumerate() {
        transaction.execute(
            "INSERT INTO context_projection_attachments(
                context_snapshot_id, ordinal, source_event_id, artifact_id, content_hash_json,
                byte_length, media_type, attachment_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                snapshot.id.to_string(),
                ordinal(index)?,
                attachment.source_event_id.to_string(),
                attachment.artifact_id.to_string(),
                serde_json::to_string(&attachment.content_hash)?,
                to_sql_u64(attachment.byte_length)?,
                attachment.media_type,
                serde_json::to_string(attachment)?,
            ],
        )?;
    }
    for item in &manifest.items {
        transaction.execute(
            "INSERT INTO context_projection_source_authorizations(
                context_snapshot_id, ordinal, source_kind, data_class, source_ref_json,
                authorization_basis_json, content_hash_json, byte_length
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                snapshot.id.to_string(),
                i64::from(item.ordinal),
                enum_name(&item.source_kind)?,
                enum_name(&item.data_class)?,
                serde_json::to_string(&item.source_ref)?,
                serde_json::to_string(&item.authorization_basis)?,
                serde_json::to_string(&item.content_hash)?,
                to_sql_u64(item.byte_length)?,
            ],
        )?;
    }
    Ok(())
}

fn insert_context_search_chunks(
    transaction: &Transaction<'_>,
    session_id: SessionId,
    chunks: &[ContextSearchChunkDraft],
) -> Result<(), PersistenceError> {
    let mut high_watermark = 0_u64;
    for chunk in chunks {
        if chunk.privacy_class != ContextPrivacyClass::ModelVisible {
            continue;
        }
        transaction.execute(
            "INSERT INTO context_search_chunks(
                session_id, agent_run_id, turn_task_id, source_type, source_id,
                session_sequence, privacy_class, source_hash_json, searchable_text, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(session_id, source_type, source_id, source_hash_json) DO NOTHING",
            params![
                session_id.to_string(),
                chunk.agent_run_id.map(|id| id.to_string()),
                chunk.turn_task_id.map(|id| id.to_string()),
                chunk.source_type,
                chunk.source_id,
                to_sql_u64(chunk.session_sequence.value())?,
                enum_name(&chunk.privacy_class)?,
                serde_json::to_string(&chunk.source_hash)?,
                chunk.searchable_text,
                UtcTimestamp::now().to_string(),
            ],
        )?;
        if transaction.changes() > 0 {
            let chunk_id = transaction.last_insert_rowid();
            transaction.execute(
                "INSERT INTO context_search_fts(
                    chunk_id, session_id, agent_run_id, source_type, source_id,
                    session_sequence, searchable_text
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    chunk_id.to_string(),
                    session_id.to_string(),
                    chunk.agent_run_id.map(|id| id.to_string()),
                    chunk.source_type,
                    chunk.source_id,
                    chunk.session_sequence.to_string(),
                    chunk.searchable_text,
                ],
            )?;
        }
        high_watermark = high_watermark.max(chunk.session_sequence.value());
    }
    transaction.execute(
        "INSERT INTO context_search_watermarks(
            session_id, indexed_through_sequence, complete, updated_at
         ) VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(session_id) DO UPDATE SET
            indexed_through_sequence=MAX(indexed_through_sequence, excluded.indexed_through_sequence),
            complete=excluded.complete,
            updated_at=excluded.updated_at",
        params![
            session_id.to_string(),
            to_sql_u64(high_watermark)?,
            UtcTimestamp::now().to_string(),
        ],
    )?;
    Ok(())
}

fn validate_turn_session(
    transaction: &Transaction<'_>,
    turn_task_id: aura_domain_common::TurnTaskId,
    session_id: SessionId,
) -> Result<(), PersistenceError> {
    let actual = transaction
        .query_row(
            "SELECT session_id FROM turn_tasks WHERE turn_task_id=?1",
            [turn_task_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if actual.as_deref() != Some(session_id.to_string().as_str()) {
        return Err(PersistenceError::InvalidRequest(
            "prompt bundle turn task does not belong to the session".to_owned(),
        ));
    }
    Ok(())
}

fn validate_context_owners(
    transaction: &Transaction<'_>,
    snapshot: &ContextProjectionSnapshot,
) -> Result<(), PersistenceError> {
    validate_turn_session(transaction, snapshot.turn_task_id, snapshot.session_id)?;
    let owner = transaction
        .query_row(
            "SELECT t.session_id
             FROM agent_runs a JOIN turn_tasks t ON t.turn_task_id=a.turn_task_id
             WHERE a.agent_run_id=?1 AND a.turn_task_id=?2",
            params![snapshot.agent_run_id.to_string(), snapshot.turn_task_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if owner.as_deref() != Some(snapshot.session_id.to_string().as_str()) {
        return Err(PersistenceError::InvalidRequest(
            "context snapshot agent run does not belong to the turn task and session".to_owned(),
        ));
    }
    let prompt_snapshot = transaction
        .query_row(
            "SELECT snapshot_json FROM prompt_bundle_snapshots WHERE prompt_bundle_snapshot_id=?1",
            [snapshot.prompt_bundle_snapshot_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let prompt_snapshot = prompt_snapshot
        .map(|value| serde_json::from_str::<PromptBundleSnapshot>(&value))
        .transpose()?;
    let Some(prompt_snapshot) = prompt_snapshot else {
        return Err(PersistenceError::InvalidRequest(
            "context snapshot prompt bundle is missing".to_owned(),
        ));
    };
    if prompt_snapshot.turn_task_id != snapshot.turn_task_id {
        return Err(PersistenceError::InvalidRequest(
            "context snapshot prompt bundle does not belong to the turn task".to_owned(),
        ));
    }
    let available_modules = prompt_snapshot
        .module_snapshots
        .into_iter()
        .map(|module| module.module_id)
        .collect::<std::collections::BTreeSet<_>>();
    if snapshot
        .active_prompt_module_ids
        .iter()
        .any(|module_id| !available_modules.contains(module_id))
    {
        return Err(PersistenceError::InvalidRequest(
            "context snapshot activates a module outside its prompt bundle".to_owned(),
        ));
    }
    Ok(())
}

fn load_context_search_watermark(
    connection: &rusqlite::Connection,
    session_id: SessionId,
) -> Result<(aura_domain_common::SessionSequence, bool), PersistenceError> {
    let value = connection
        .query_row(
            "SELECT indexed_through_sequence, complete FROM context_search_watermarks
             WHERE session_id=?1",
            [session_id.to_string()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?
        .unwrap_or((0, true));
    Ok((
        aura_domain_common::SessionSequence::new(u64::try_from(value.0).map_err(|_| {
            PersistenceError::InvalidRequest("negative context search watermark".to_owned())
        })?),
        value.1,
    ))
}

fn conversation_identity(
    item: &LogicalConversationItem,
) -> (&'static str, EventId, Option<aura_domain_common::ToolCallId>) {
    match item {
        LogicalConversationItem::User { source_event_id, .. } => ("user", *source_event_id, None),
        LogicalConversationItem::Assistant { source_event_id, .. } => {
            ("assistant", *source_event_id, None)
        }
        LogicalConversationItem::ToolResult { source_event_id, tool_call_id, .. } => {
            ("tool_result", *source_event_id, Some(*tool_call_id))
        }
    }
}

fn enum_name<T: Serialize>(value: &T) -> Result<String, PersistenceError> {
    serde_json::to_value(value)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| PersistenceError::InvalidRequest("enum wire value is invalid".to_owned()))
}

fn ordinal(index: usize) -> Result<i64, PersistenceError> {
    i64::try_from(index + 1)
        .map_err(|_| PersistenceError::InvalidRequest("context ordinal exceeds SQLite".to_owned()))
}

fn validate_context_source_event(
    request: &ContextSourceEventCommitRequest,
) -> Result<(), PersistenceError> {
    if request.event.session_id.is_none()
        || request.canonical_content.is_empty()
        || request.canonical_content.starts_with('\u{feff}')
        || request.canonical_content.contains('\r')
        || request.canonical_content.chars().any(|character| {
            (character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
                || character == '\u{007f}'
        })
        || match (request.model_visible, request.event.searchable_text.as_deref()) {
            (true, Some(text)) => text != request.canonical_content,
            (false, None) => false,
            _ => true,
        }
        || (request.explicitly_user_authored && request.event.actor != crate::EventActor::User)
    {
        return Err(PersistenceError::InvalidRequest(
            "context source event facts are invalid".to_owned(),
        ));
    }
    Ok(())
}

fn validate_manual_compaction_create(
    request: &ContextManualCompactionCreateRequest,
) -> Result<(ContentHash, Option<ContentHash>), PersistenceError> {
    if request.command_event.session_id != Some(request.session_id)
        || request.command_event.agent_run_id != Some(request.agent_run_id)
        || request.command_event.event_type != "ManualCompactionRequested"
        || request.command_event.event_version != 1
        || request.command_event.actor != crate::EventActor::User
        || request.command_event.payload_json.is_some()
        || request.command_event.payload_artifact_ref.is_some()
        || request.command_event.searchable_text.is_some()
        || request.command_artifact.media_type != "text/plain; charset=utf-8"
        || request
            .focus_artifact
            .as_ref()
            .is_some_and(|focus| focus.artifact_id == request.command_artifact.artifact_id)
    {
        return Err(PersistenceError::InvalidRequest(
            "manual context compaction command identity is invalid".to_owned(),
        ));
    }
    let command = std::str::from_utf8(&request.command_artifact.bytes).map_err(|_| {
        PersistenceError::InvalidRequest(
            "manual context compaction command is not UTF-8".to_owned(),
        )
    })?;
    if command.starts_with('\u{feff}')
        || command.contains('\r')
        || command.chars().any(|character| {
            (character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
                || character == '\u{007f}'
        })
    {
        return Err(PersistenceError::InvalidRequest(
            "manual context compaction command contains forbidden characters".to_owned(),
        ));
    }
    let remainder = command.strip_prefix("/compact").ok_or_else(|| {
        PersistenceError::InvalidRequest(
            "manual context compaction command prefix is invalid".to_owned(),
        )
    })?;
    let focus = if remainder.is_empty() {
        None
    } else {
        Some(remainder.strip_prefix(' ').ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "manual context compaction command syntax is invalid".to_owned(),
            )
        })?)
        .filter(|value| !value.trim().is_empty())
    };
    if focus.is_some_and(|value| {
        value.chars().count() > aura_context_domain::MANUAL_COMPACTION_FOCUS_MAX_CHARS
    }) {
        return Err(PersistenceError::InvalidRequest(
            "manual context compaction focus exceeds 4096 characters".to_owned(),
        ));
    }
    match (focus, request.focus_artifact.as_ref()) {
        (None, None) => {}
        (Some(expected), Some(artifact))
            if artifact.media_type == "text/plain; charset=utf-8"
                && artifact.bytes == expected.as_bytes() => {}
        _ => {
            return Err(PersistenceError::InvalidRequest(
                "manual context compaction focus artifact is inconsistent".to_owned(),
            ));
        }
    }
    Ok((
        ContentHash::sha256(&request.command_artifact.bytes),
        request.focus_artifact.as_ref().map(|artifact| ContentHash::sha256(&artifact.bytes)),
    ))
}

fn validate_agent_run_session(
    transaction: &Transaction<'_>,
    agent_run_id: AgentRunId,
    session_id: SessionId,
) -> Result<(aura_domain_common::TurnTaskId, aura_domain_common::WorkspaceId), PersistenceError> {
    let owner = transaction
        .query_row(
            "SELECT a.turn_task_id, s.workspace_id
             FROM agent_runs a
             JOIN turn_tasks t ON t.turn_task_id=a.turn_task_id
             JOIN sessions s ON s.session_id=t.session_id
             WHERE a.agent_run_id=?1 AND t.session_id=?2",
            params![agent_run_id.to_string(), session_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    owner
        .map(|(turn_task_id, workspace_id)| -> Result<_, PersistenceError> {
            Ok((
                aura_domain_common::TurnTaskId::from_str(&turn_task_id)?,
                aura_domain_common::WorkspaceId::from_str(&workspace_id)?,
            ))
        })
        .transpose()?
        .ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "manual context compaction AgentRun does not belong to the Session".to_owned(),
            )
        })
}

fn validate_compaction_owner_and_state(
    transaction: &Transaction<'_>,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    sequence: u32,
    allowed_states: &[&str],
) -> Result<(), PersistenceError> {
    let state = transaction
        .query_row(
            "SELECT c.state
             FROM context_compactions c
             JOIN context_projection_snapshots s
               ON s.context_snapshot_id=c.source_context_snapshot_id
             WHERE c.source_context_snapshot_id=?1 AND c.sequence=?2
               AND s.session_id=?3 AND s.agent_run_id=?4",
            params![
                source_context_snapshot_id.to_string(),
                i64::from(sequence),
                session_id.to_string(),
                agent_run_id.to_string(),
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if !state.as_deref().is_some_and(|state| allowed_states.contains(&state)) {
        return Err(PersistenceError::InvalidRequest(
            "manual context compaction binding is missing or has the wrong state".to_owned(),
        ));
    }
    Ok(())
}

fn manual_compaction_state_name(state: ManualCompactionRequestState) -> &'static str {
    match state {
        ManualCompactionRequestState::Requested => "requested",
        ManualCompactionRequestState::Running => "running",
        ManualCompactionRequestState::Completed => "completed",
        ManualCompactionRequestState::Failed => "failed",
        ManualCompactionRequestState::Cancelled => "cancelled",
    }
}

fn parse_manual_compaction_state(
    state: &str,
) -> Result<ManualCompactionRequestState, PersistenceError> {
    match state {
        "requested" => Ok(ManualCompactionRequestState::Requested),
        "running" => Ok(ManualCompactionRequestState::Running),
        "completed" => Ok(ManualCompactionRequestState::Completed),
        "failed" => Ok(ManualCompactionRequestState::Failed),
        "cancelled" => Ok(ManualCompactionRequestState::Cancelled),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted manual context compaction state is invalid".to_owned(),
        )),
    }
}

fn load_manual_compaction_request(
    connection: &rusqlite::Connection,
    session_id: SessionId,
    command_invocation_id: CommandInvocationId,
) -> Result<Option<ManualCompactionRequestRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT agent_run_id, command_event_id, focus_artifact_id, focus_hash_json,
                    state, source_context_snapshot_id, compaction_sequence,
                    error_code, created_at, terminal_at
             FROM context_manual_compaction_requests
             WHERE session_id=?1 AND request_key=?2",
            params![session_id.to_string(), command_invocation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?
        .map(|row| {
            let source_context_snapshot_id =
                row.5.as_deref().map(FromStr::from_str).transpose()?;
            let compaction_sequence = row
                .6
                .map(|sequence| {
                    u32::try_from(sequence).map_err(|_| {
                        PersistenceError::InvalidRequest(
                            "persisted manual compaction sequence is invalid".to_owned(),
                        )
                    })
                })
                .transpose()?;
            if source_context_snapshot_id.is_some() != compaction_sequence.is_some() {
                return Err(PersistenceError::InvalidRequest(
                    "persisted manual context compaction binding is incomplete".to_owned(),
                ));
            }
            Ok(ManualCompactionRequestRecord {
                session_id,
                agent_run_id: AgentRunId::from_str(&row.0)?,
                command_invocation_id,
                command_event_id: EventId::from_str(&row.1)?,
                focus_artifact_id: row.2.as_deref().map(FromStr::from_str).transpose()?,
                focus_hash: row.3.as_deref().map(serde_json::from_str).transpose()?,
                state: parse_manual_compaction_state(&row.4)?,
                source_context_snapshot_id,
                compaction_sequence,
                error_code: row.7,
                created_at: UtcTimestamp::from_str(&row.8).map_err(|error| {
                    PersistenceError::InvalidRequest(format!(
                        "persisted manual context compaction created_at is invalid: {error}"
                    ))
                })?,
                terminal_at: row
                    .9
                    .as_deref()
                    .map(|value| {
                        UtcTimestamp::from_str(value).map_err(|error| {
                            PersistenceError::InvalidRequest(format!(
                                "persisted manual context compaction terminal_at is invalid: {error}"
                            ))
                        })
                    })
                    .transpose()?,
            })
        })
        .transpose()
}

fn context_snapshot_owner(
    transaction: &Transaction<'_>,
    context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
) -> Result<(SessionId, AgentRunId), PersistenceError> {
    transaction
        .query_row(
            "SELECT session_id, agent_run_id FROM context_projection_snapshots
             WHERE context_snapshot_id=?1",
            [context_snapshot_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map(|row| {
            Ok::<_, PersistenceError>((SessionId::from_str(&row.0)?, AgentRunId::from_str(&row.1)?))
        })
        .transpose()?
        .ok_or_else(|| {
            PersistenceError::InvalidRequest("context compaction source is missing".to_owned())
        })
}

fn load_active_context_compactions(
    transaction: &Transaction<'_>,
) -> Result<Vec<ActiveContextCompaction>, PersistenceError> {
    let mut statement = transaction.prepare(
        "SELECT c.source_context_snapshot_id, c.sequence, s.session_id, s.agent_run_id,
                c.state, c.model_step_id, c.transport_attempt_id
         FROM context_compactions c
         JOIN context_projection_snapshots s
           ON s.context_snapshot_id=c.source_context_snapshot_id
         WHERE c.state IN ('requested', 'running')
         ORDER BY c.requested_at, c.source_context_snapshot_id, c.sequence",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|row| {
            Ok(ActiveContextCompaction {
                source_context_snapshot_id: FromStr::from_str(&row.0)?,
                sequence: u32::try_from(row.1).map_err(|_| {
                    PersistenceError::InvalidRequest(
                        "persisted context compaction sequence is invalid".to_owned(),
                    )
                })?,
                session_id: FromStr::from_str(&row.2)?,
                agent_run_id: FromStr::from_str(&row.3)?,
                state: parse_context_compaction_state(&row.4)?,
                model_step_id: row.5.as_deref().map(FromStr::from_str).transpose()?,
                transport_attempt_id: row.6.as_deref().map(FromStr::from_str).transpose()?,
            })
        })
        .collect()
}

fn resolve_context_compaction_restart(
    transaction: &Transaction<'_>,
    compaction: &ActiveContextCompaction,
    _correlation_id: CorrelationId,
    restarted_at: UtcTimestamp,
) -> Result<ContextCompactionRestartResolution, PersistenceError> {
    let attempts = load_context_compaction_model_attempts(
        transaction,
        compaction.source_context_snapshot_id,
        compaction.sequence,
    )?;
    if compaction.state == ContextCompactionStateView::Required {
        if !attempts.is_empty()
            || compaction.model_step_id.is_some()
            || compaction.transport_attempt_id.is_some()
        {
            return Err(PersistenceError::InvalidRequest(
                "requested context compaction has model-attempt evidence".to_owned(),
            ));
        }
        return Ok(ContextCompactionRestartResolution {
            parent_state: ContextCompactionStateView::Failed,
            error_code: COMPACTION_RESTART_BEFORE_MODEL_ATTEMPT.to_owned(),
            attempt_update: None,
            raw_attempt_to_cancel: None,
            transport_attempt_id: None,
        });
    }
    if compaction.state != ContextCompactionStateView::Running || attempts.is_empty() {
        return Err(PersistenceError::InvalidRequest(
            "running context compaction has no model-attempt evidence".to_owned(),
        ));
    }
    validate_context_compaction_attempt_sequence(&attempts)?;
    let latest = attempts.last().expect("attempt sequence is non-empty");
    validate_context_compaction_attempt_record_graph(
        transaction,
        compaction.session_id,
        compaction.agent_run_id,
        latest,
    )?;
    if compaction.model_step_id != Some(latest.model_step_id) {
        return Err(PersistenceError::InvalidRequest(
            "active context compaction points at the wrong ModelStep".to_owned(),
        ));
    }
    if let (Some(parent), Some(attempt)) =
        (compaction.transport_attempt_id, latest.transport_attempt_id)
        && parent != attempt
    {
        return Err(PersistenceError::InvalidRequest(
            "active context compaction TransportAttempt binding is inconsistent".to_owned(),
        ));
    }

    if latest.state != CompactionModelAttemptState::Running {
        return resolve_terminal_context_compaction_attempt_restart(
            transaction,
            compaction,
            latest,
        );
    }

    let raw_attempts = load_agent_model_transport_attempts(transaction, latest)?;
    validate_agent_model_transport_attempt_sequence(&raw_attempts)?;
    let Some(raw_attempt) = raw_attempts.last() else {
        if latest.transport_attempt_id.is_some() || compaction.transport_attempt_id.is_some() {
            return Err(PersistenceError::InvalidRequest(
                "context compaction references a missing TransportAttempt".to_owned(),
            ));
        }
        let mut attempt = latest.clone();
        attempt.state = CompactionModelAttemptState::Failed;
        attempt.error_code = Some(COMPACTION_RESTART_BEFORE_TRANSPORT.to_owned());
        attempt.terminal_at = Some(restarted_at);
        return Ok(ContextCompactionRestartResolution {
            parent_state: ContextCompactionStateView::Failed,
            error_code: COMPACTION_RESTART_BEFORE_TRANSPORT.to_owned(),
            attempt_update: Some(attempt),
            raw_attempt_to_cancel: None,
            transport_attempt_id: None,
        });
    };
    if latest.transport_attempt_id.is_some_and(|id| id != raw_attempt.id)
        || compaction.transport_attempt_id.is_some_and(|id| id != raw_attempt.id)
    {
        return Err(PersistenceError::InvalidRequest(
            "context compaction points at a non-current TransportAttempt".to_owned(),
        ));
    }

    let (parent_state, attempt_state, error_code, raw_attempt_to_cancel) = match raw_attempt.state {
        TransportAttemptState::QueuedForEndpoint
        | TransportAttemptState::AcquiringPermit
        | TransportAttemptState::PreparingRequest
        | TransportAttemptState::AuthorizingNetwork
        | TransportAttemptState::Authorized => (
            ContextCompactionStateView::Cancelled,
            CompactionModelAttemptState::Cancelled,
            COMPACTION_RESTART_BEFORE_DISPATCH,
            Some(raw_attempt.clone()),
        ),
        TransportAttemptState::DispatchPrepared
        | TransportAttemptState::Connecting
        | TransportAttemptState::ResponseStarted
        | TransportAttemptState::ResponseCompleted
        | TransportAttemptState::AwaitingFirstEvent
        | TransportAttemptState::Streaming => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::UnknownSideEffect,
                Some(raw_attempt.id),
            )?;
            (
                ContextCompactionStateView::Failed,
                CompactionModelAttemptState::UnknownSideEffect,
                COMPACTION_RESTART_SIDE_EFFECT_UNKNOWN,
                None,
            )
        }
        TransportAttemptState::Completed => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::Failed,
                Some(raw_attempt.id),
            )?;
            (
                ContextCompactionStateView::Failed,
                CompactionModelAttemptState::Failed,
                COMPACTION_RESTART_AFTER_TRANSPORT_COMPLETED,
                None,
            )
        }
        TransportAttemptState::Failed => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::Failed,
                Some(raw_attempt.id),
            )?;
            (
                ContextCompactionStateView::Failed,
                CompactionModelAttemptState::Failed,
                COMPACTION_RESTART_TRANSPORT_FAILED,
                None,
            )
        }
        TransportAttemptState::Cancelled => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::Cancelled,
                Some(raw_attempt.id),
            )?;
            (
                ContextCompactionStateView::Cancelled,
                CompactionModelAttemptState::Cancelled,
                COMPACTION_RESTART_TRANSPORT_CANCELLED,
                None,
            )
        }
    };
    let mut attempt = latest.clone();
    attempt.transport_attempt_id = Some(raw_attempt.id);
    attempt.state = attempt_state;
    attempt.error_code = Some(error_code.to_owned());
    attempt.terminal_at = Some(restarted_at);
    Ok(ContextCompactionRestartResolution {
        parent_state,
        error_code: error_code.to_owned(),
        attempt_update: Some(attempt),
        raw_attempt_to_cancel,
        transport_attempt_id: Some(raw_attempt.id),
    })
}

fn resolve_terminal_context_compaction_attempt_restart(
    transaction: &Transaction<'_>,
    compaction: &ActiveContextCompaction,
    latest: &CompactionModelAttemptRecord,
) -> Result<ContextCompactionRestartResolution, PersistenceError> {
    let resolution = match latest.state {
        CompactionModelAttemptState::Completed => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::Completed,
                latest.transport_attempt_id,
            )?;
            (ContextCompactionStateView::Failed, COMPACTION_RESTART_AFTER_TRANSPORT_COMPLETED)
        }
        CompactionModelAttemptState::Failed => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::Failed,
                latest.transport_attempt_id,
            )?;
            (ContextCompactionStateView::Failed, COMPACTION_RESTART_AFTER_ATTEMPT_FAILED)
        }
        CompactionModelAttemptState::Cancelled => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::Cancelled,
                latest.transport_attempt_id,
            )?;
            (ContextCompactionStateView::Cancelled, COMPACTION_RESTART_TRANSPORT_CANCELLED)
        }
        CompactionModelAttemptState::UnknownSideEffect => {
            validate_context_compaction_transport_attempt(
                transaction,
                latest,
                CompactionModelAttemptState::UnknownSideEffect,
                latest.transport_attempt_id,
            )?;
            (ContextCompactionStateView::Failed, COMPACTION_RESTART_SIDE_EFFECT_UNKNOWN)
        }
        CompactionModelAttemptState::Running => {
            return Err(PersistenceError::InvalidRequest(
                "running context compaction attempt was routed as terminal".to_owned(),
            ));
        }
    };
    if compaction.transport_attempt_id.is_some()
        && compaction.transport_attempt_id != latest.transport_attempt_id
    {
        return Err(PersistenceError::InvalidRequest(
            "terminal context compaction attempt binding contradicts its parent".to_owned(),
        ));
    }
    Ok(ContextCompactionRestartResolution {
        parent_state: resolution.0,
        error_code: resolution.1.to_owned(),
        attempt_update: None,
        raw_attempt_to_cancel: None,
        transport_attempt_id: latest.transport_attempt_id,
    })
}

fn load_context_compaction_model_attempts(
    transaction: &Transaction<'_>,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    sequence: u32,
) -> Result<Vec<CompactionModelAttemptRecord>, PersistenceError> {
    let mut statement = transaction.prepare(
        "SELECT attempt_ordinal FROM context_compaction_model_attempts
         WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
         ORDER BY attempt_ordinal",
    )?;
    let ordinals = statement
        .query_map(params![source_context_snapshot_id.to_string(), i64::from(sequence)], |row| {
            row.get::<_, i64>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    ordinals
        .into_iter()
        .map(|ordinal| {
            let ordinal = u8::try_from(ordinal).map_err(|_| {
                PersistenceError::InvalidRequest(
                    "persisted context compaction attempt ordinal is invalid".to_owned(),
                )
            })?;
            load_context_compaction_model_attempt(
                transaction,
                source_context_snapshot_id,
                sequence,
                ordinal,
            )?
            .ok_or_else(|| {
                PersistenceError::InvalidRequest(
                    "context compaction attempt index is inconsistent".to_owned(),
                )
            })
        })
        .collect()
}

fn validate_context_compaction_attempt_sequence(
    attempts: &[CompactionModelAttemptRecord],
) -> Result<(), PersistenceError> {
    if attempts.is_empty()
        || attempts.len() > 2
        || attempts.iter().enumerate().any(|(index, attempt)| {
            usize::from(attempt.attempt_ordinal) != index + 1
                || (index + 1 < attempts.len()
                    && attempt.state != CompactionModelAttemptState::Failed)
        })
    {
        return Err(PersistenceError::InvalidRequest(
            "context compaction attempt history is not canonical".to_owned(),
        ));
    }
    Ok(())
}

fn validate_context_compaction_attempt_record_graph(
    transaction: &Transaction<'_>,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    attempt: &CompactionModelAttemptRecord,
) -> Result<(), PersistenceError> {
    let graph = transaction
        .query_row(
            "SELECT s.session_id, s.agent_run_id, s.model_step_id,
                    s.prompt_bundle_snapshot_id, m.snapshot_json
             FROM context_projection_snapshots s
             JOIN model_execution_snapshots m ON m.model_execution_snapshot_id=?2
             WHERE s.context_snapshot_id=?1",
            params![
                attempt.attempt_context_snapshot_id.to_string(),
                attempt.model_execution_snapshot_id.to_string(),
            ],
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
        .optional()?;
    let Some((stored_session, stored_agent_run, model_step, prompt_bundle, mex)) = graph else {
        return Err(PersistenceError::InvalidRequest(
            "context compaction attempt cps/mex graph is missing".to_owned(),
        ));
    };
    let mex = serde_json::from_str::<ModelExecutionSnapshot>(&mex)?;
    if stored_session != session_id.to_string()
        || stored_agent_run != agent_run_id.to_string()
        || model_step != attempt.model_step_id.to_string()
        || prompt_bundle != mex.prompt_bundle_snapshot_id.to_string()
        || mex.id != attempt.model_execution_snapshot_id
        || mex.model_step_id != attempt.model_step_id
        || mex.context_snapshot_id != attempt.attempt_context_snapshot_id
    {
        return Err(PersistenceError::InvalidRequest(
            "context compaction attempt cps/mex graph is inconsistent".to_owned(),
        ));
    }
    Ok(())
}

fn load_agent_model_transport_attempts(
    transaction: &Transaction<'_>,
    attempt: &CompactionModelAttemptRecord,
) -> Result<Vec<TransportAttemptRecord>, PersistenceError> {
    let operation_context = TransportOperationContext::AgentModelAttempt {
        model_step_id: attempt.model_step_id,
        model_execution_snapshot_id: attempt.model_execution_snapshot_id,
        context_snapshot_id: attempt.attempt_context_snapshot_id,
        prompt_bundle_snapshot_id: transaction
            .query_row(
                "SELECT prompt_bundle_snapshot_id FROM context_projection_snapshots
             WHERE context_snapshot_id=?1",
                [attempt.attempt_context_snapshot_id.to_string()],
                |row| row.get::<_, String>(0),
            )?
            .parse()?,
    };
    let operation_context_json = serde_json::to_string(&operation_context)?;
    let mut statement = transaction.prepare(
        "SELECT record_json FROM transport_attempts
         WHERE operation_context_json=?1
         ORDER BY attempt_number, started_at, transport_attempt_id",
    )?;
    statement
        .query_map([operation_context_json], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|encoded| {
            let record = serde_json::from_str::<TransportAttemptRecord>(&encoded)?;
            if record.request_evidence.operation_context != operation_context {
                return Err(PersistenceError::InvalidRequest(
                    "Agent model TransportAttempt operation identity is inconsistent".to_owned(),
                ));
            }
            Ok(record)
        })
        .collect()
}

fn validate_agent_model_transport_attempt_sequence(
    attempts: &[TransportAttemptRecord],
) -> Result<(), PersistenceError> {
    if attempts.iter().enumerate().any(|(index, attempt)| {
        usize::try_from(attempt.attempt_number).ok() != Some(index + 1)
            || (index + 1 < attempts.len() && attempt.state != TransportAttemptState::Failed)
    }) {
        return Err(PersistenceError::InvalidRequest(
            "Agent model TransportAttempt history is not canonical".to_owned(),
        ));
    }
    Ok(())
}

fn terminalize_bound_manual_compaction_requests(
    transaction: &Transaction<'_>,
    compaction: &ActiveContextCompaction,
    parent_state: ContextCompactionStateView,
    error_code: &str,
    correlation_id: CorrelationId,
    terminal_at: UtcTimestamp,
) -> Result<u64, PersistenceError> {
    let mut statement = transaction.prepare(
        "SELECT request_key FROM context_manual_compaction_requests
         WHERE session_id=?1 AND agent_run_id=?2
           AND source_context_snapshot_id=?3 AND compaction_sequence=?4
           AND state IN ('requested', 'running')
         ORDER BY request_key",
    )?;
    let keys = statement
        .query_map(
            params![
                compaction.session_id.to_string(),
                compaction.agent_run_id.to_string(),
                compaction.source_context_snapshot_id.to_string(),
                i64::from(compaction.sequence),
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let (state, event_type) = if parent_state == ContextCompactionStateView::Cancelled {
        ("cancelled", "ManualContextCompactionCancelled")
    } else {
        ("failed", "ManualContextCompactionFailed")
    };
    for key in &keys {
        let changed = transaction.execute(
            "UPDATE context_manual_compaction_requests
             SET state=?4, error_code=?5, terminal_at=?6
             WHERE session_id=?1 AND agent_run_id=?2 AND request_key=?3
               AND state IN ('requested', 'running')",
            params![
                compaction.session_id.to_string(),
                compaction.agent_run_id.to_string(),
                key,
                state,
                error_code,
                terminal_at.to_string(),
            ],
        )?;
        if changed != 1 {
            return Err(PersistenceError::InvalidRequest(
                "manual context compaction changed during restart recovery".to_owned(),
            ));
        }
        let command_invocation_id = CommandInvocationId::from_str(key)?;
        let mut event = PersistentEventDraft::system(
            event_type,
            correlation_id,
            json!({
                "command_invocation_id": command_invocation_id,
                "error_code": error_code,
                "recovery_reason": "application_restarted",
            }),
        );
        event.session_id = Some(compaction.session_id);
        event.agent_run_id = Some(compaction.agent_run_id);
        insert_event(transaction, &event)?;
    }
    u64::try_from(keys.len()).map_err(|_| {
        PersistenceError::InvalidRequest(
            "manual context compaction recovery count exceeds u64".to_owned(),
        )
    })
}

fn load_latest_prompt_inspector_for_recovery(
    transaction: &Transaction<'_>,
    session_id: SessionId,
) -> Result<Option<PromptInspectorProjection>, PersistenceError> {
    let mut statement = transaction.prepare(
        "SELECT payload_json FROM projection_outbox
         WHERE projection_kind='prompt_inspector'
         ORDER BY outbox_id DESC",
    )?;
    let encoded =
        statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for encoded in encoded {
        let change = serde_json::from_str::<ProjectionChange>(&encoded)?;
        change.validate().map_err(|error| {
            PersistenceError::InvalidRequest(format!(
                "persisted Prompt Inspector projection is invalid: {error}"
            ))
        })?;
        if let ProjectionChange::PromptInspector { payload, .. } = change
            && payload.session_id == session_id
        {
            return Ok(Some(*payload));
        }
    }
    Ok(None)
}

fn increment_context_recovery_report(
    report: &mut ContextStartupRecoveryReport,
    error_code: &str,
) -> Result<(), PersistenceError> {
    let counter = match error_code {
        COMPACTION_RESTART_BEFORE_MODEL_ATTEMPT => &mut report.failed_before_model_attempt,
        COMPACTION_RESTART_BEFORE_TRANSPORT => &mut report.failed_before_transport,
        COMPACTION_RESTART_TRANSPORT_FAILED => &mut report.transport_failures_preserved,
        COMPACTION_RESTART_TRANSPORT_CANCELLED | COMPACTION_RESTART_BEFORE_DISPATCH => {
            &mut report.transport_cancellations_preserved
        }
        COMPACTION_RESTART_AFTER_TRANSPORT_COMPLETED => {
            &mut report.completed_outputs_left_unaccepted
        }
        COMPACTION_RESTART_SIDE_EFFECT_UNKNOWN => &mut report.unknown_side_effects,
        COMPACTION_RESTART_AFTER_ATTEMPT_FAILED => &mut report.logical_attempt_failures_preserved,
        _ => {
            return Err(PersistenceError::InvalidRequest(
                "context startup recovery produced an unknown stable error code".to_owned(),
            ));
        }
    };
    *counter = counter.checked_add(1).ok_or_else(|| {
        PersistenceError::InvalidRequest("context startup recovery counter exhausted".to_owned())
    })?;
    Ok(())
}

fn validate_context_compaction_create(
    request: &ContextCompactionCreateRequest,
) -> Result<(), PersistenceError> {
    if request.sequence == 0
        || request.source_span.start_sequence > request.source_span.end_sequence
        || request.sources.is_empty()
        || request.sources.len() != request.source_span.source_refs.len()
        || !request
            .sources
            .iter()
            .zip(&request.source_span.source_refs)
            .all(|(source, expected)| &source.source_ref == expected)
    {
        return Err(PersistenceError::InvalidRequest(
            "context compaction source plan is invalid".to_owned(),
        ));
    }
    if let Some(focus) = &request.focus_artifact {
        let focus_text = std::str::from_utf8(&focus.bytes).map_err(|_| {
            PersistenceError::InvalidRequest("context compaction focus is not UTF-8".to_owned())
        })?;
        if focus.media_type != "text/plain; charset=utf-8"
            || focus_text.chars().count() > aura_context_domain::MANUAL_COMPACTION_FOCUS_MAX_CHARS
            || focus_text.contains('\r')
            || focus_text.chars().any(|character| {
                (character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
                    || character == '\u{007f}'
            })
        {
            return Err(PersistenceError::InvalidRequest(
                "context compaction focus is invalid".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_compaction_projection(
    projection: &PromptInspectorProjection,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    state: ContextCompactionStateView,
) -> Result<(), PersistenceError> {
    if projection.session_id != session_id
        || projection.active_agent_run_id != Some(agent_run_id)
        || projection.context_snapshot_id != Some(source_context_snapshot_id)
        || projection.compaction.state != state
        || projection.compaction.source_context_snapshot_id != Some(source_context_snapshot_id)
    {
        return Err(PersistenceError::InvalidRequest(
            "context compaction projection does not match the operation".to_owned(),
        ));
    }
    Ok(())
}

fn validate_context_compaction_completed(
    request: &ContextCompactionCompletedRequest,
) -> Result<(), PersistenceError> {
    if request.sequence == 0
        || !(1..=2).contains(&request.attempt_ordinal)
        || request.record.source_context_snapshot_id != request.source_context_snapshot_id
        || request.record.sequence != request.sequence
    {
        return Err(PersistenceError::InvalidRequest(
            "completed context compaction identity is invalid".to_owned(),
        ));
    }
    let artifacts = [
        &request.historical_summary,
        &request.active_turn_summary,
        &request.structured_compaction,
        &request.anchor_manifest,
    ];
    let artifact_ids = artifacts.iter().map(|value| value.artifact_id).collect::<BTreeSet<_>>();
    if artifact_ids.len() != artifacts.len()
        || request.historical_summary.media_type != "text/markdown; charset=utf-8"
        || request.active_turn_summary.media_type != "text/markdown; charset=utf-8"
        || request.structured_compaction.media_type != "text/markdown; charset=utf-8"
        || request.anchor_manifest.media_type != "application/json"
        || ContentHash::sha256(&request.historical_summary.bytes)
            != request.record.historical_summary_hash
        || ContentHash::sha256(&request.active_turn_summary.bytes)
            != request.record.active_turn_summary_hash
        || ContentHash::sha256(&request.structured_compaction.bytes)
            != request.record.structured_markdown_hash
        || request.anchor_manifest_value.manifest_hash != request.record.anchor_manifest_hash
        || canonical_json_v1_sha256(&request.anchor_manifest_value.anchors).map_err(|error| {
            PersistenceError::InvalidRequest(format!(
                "context compaction anchors are not canonical: {error}"
            ))
        })? != request.anchor_manifest_value.manifest_hash
        || canonical_json_v1(&request.anchor_manifest_value).map_err(|error| {
            PersistenceError::InvalidRequest(format!(
                "context compaction anchor manifest is not canonical: {error}"
            ))
        })? != request.anchor_manifest.bytes
    {
        return Err(PersistenceError::InvalidRequest(
            "completed context compaction artifacts are inconsistent".to_owned(),
        ));
    }
    Ok(())
}

fn validate_persisted_compaction_sources(
    transaction: &Transaction<'_>,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    sequence: u32,
    manifest: &CompactionAnchorManifest,
) -> Result<(), PersistenceError> {
    let mut statement = transaction.prepare(
        "SELECT ordinal, source_ref_json, source_hash_json
         FROM context_compaction_sources
         WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
         ORDER BY ordinal ASC",
    )?;
    let rows = statement
        .query_map(params![source_context_snapshot_id.to_string(), i64::from(sequence)], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() != manifest.anchors.len() {
        return Err(PersistenceError::InvalidRequest(
            "completed context compaction source evidence changed".to_owned(),
        ));
    }
    for (index, ((ordinal, source_ref_json, source_hash_json), anchor)) in
        rows.into_iter().zip(&manifest.anchors).enumerate()
    {
        let expected_ordinal = i64::try_from(index + 1).map_err(|_| {
            PersistenceError::InvalidRequest(
                "completed context compaction source ordinal overflowed".to_owned(),
            )
        })?;
        let source_ref = serde_json::from_str::<ContextSourceRef>(&source_ref_json)?;
        let source_hash = serde_json::from_str::<ContentHash>(&source_hash_json)?;
        if ordinal != expected_ordinal
            || anchor.anchor != format!("a{:06}", index + 1)
            || anchor.source_ref != source_ref
            || anchor.source_hash != source_hash
        {
            return Err(PersistenceError::InvalidRequest(
                "completed context compaction source evidence changed".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_context_compaction_attempt_graph(
    transaction: &Transaction<'_>,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    request: &ContextCompactionRunningRequest,
) -> Result<(), PersistenceError> {
    let graph = transaction
        .query_row(
            "SELECT s.session_id, s.agent_run_id, s.model_step_id, s.prompt_bundle_snapshot_id,
                    m.snapshot_json
             FROM context_projection_snapshots s
             JOIN model_execution_snapshots m ON m.model_execution_snapshot_id=?2
             WHERE s.context_snapshot_id=?1",
            params![
                request.attempt_context_snapshot_id.to_string(),
                request.model_execution_snapshot_id.to_string(),
            ],
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
        .optional()?;
    let Some((stored_session_id, stored_agent_run_id, model_step_id, prompt_bundle_id, mex)) =
        graph
    else {
        return Err(PersistenceError::InvalidRequest(
            "context compaction attempt cps/mex graph is missing".to_owned(),
        ));
    };
    let mex = serde_json::from_str::<ModelExecutionSnapshot>(&mex)?;
    if stored_session_id != session_id.to_string()
        || stored_agent_run_id != agent_run_id.to_string()
        || model_step_id != request.model_step_id.to_string()
        || mex.id != request.model_execution_snapshot_id
        || mex.model_step_id != request.model_step_id
        || mex.context_snapshot_id != request.attempt_context_snapshot_id
        || prompt_bundle_id != mex.prompt_bundle_snapshot_id.to_string()
    {
        return Err(PersistenceError::InvalidRequest(
            "context compaction attempt cps/mex graph is inconsistent".to_owned(),
        ));
    }
    Ok(())
}

fn validate_context_compaction_transport_attempt(
    transaction: &Transaction<'_>,
    attempt: &CompactionModelAttemptRecord,
    terminal_state: CompactionModelAttemptState,
    transport_attempt_id: Option<TransportAttemptId>,
) -> Result<(), PersistenceError> {
    let Some(transport_attempt_id) = transport_attempt_id else {
        if matches!(
            terminal_state,
            CompactionModelAttemptState::Failed | CompactionModelAttemptState::Cancelled
        ) {
            return Ok(());
        }
        return Err(PersistenceError::InvalidRequest(
            "context compaction terminal outcome requires a TransportAttempt".to_owned(),
        ));
    };
    let persisted = transaction
        .query_row(
            "SELECT record_json FROM transport_attempts WHERE transport_attempt_id=?1",
            [transport_attempt_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            PersistenceError::InvalidRequest(
                "context compaction TransportAttempt is missing".to_owned(),
            )
        })?;
    let persisted = serde_json::from_str::<TransportAttemptRecord>(&persisted)?;
    let operation_matches = matches!(
        persisted.request_evidence.operation_context,
        TransportOperationContext::AgentModelAttempt {
            model_step_id,
            model_execution_snapshot_id,
            context_snapshot_id,
            ..
        } if model_step_id == attempt.model_step_id
            && model_execution_snapshot_id == attempt.model_execution_snapshot_id
            && context_snapshot_id == attempt.attempt_context_snapshot_id
    );
    let state_matches = match terminal_state {
        CompactionModelAttemptState::Completed => {
            persisted.state == TransportAttemptState::Completed
        }
        CompactionModelAttemptState::Failed => matches!(
            persisted.state,
            TransportAttemptState::Failed | TransportAttemptState::Completed
        ),
        CompactionModelAttemptState::Cancelled => {
            persisted.state == TransportAttemptState::Cancelled
        }
        CompactionModelAttemptState::UnknownSideEffect => matches!(
            persisted.state,
            TransportAttemptState::DispatchPrepared
                | TransportAttemptState::Connecting
                | TransportAttemptState::ResponseStarted
                | TransportAttemptState::ResponseCompleted
                | TransportAttemptState::AwaitingFirstEvent
                | TransportAttemptState::Streaming
        ),
        CompactionModelAttemptState::Running => false,
    };
    if !operation_matches || !state_matches {
        return Err(PersistenceError::InvalidRequest(
            "context compaction TransportAttempt evidence is inconsistent".to_owned(),
        ));
    }
    if terminal_state == CompactionModelAttemptState::Completed
        || (terminal_state == CompactionModelAttemptState::Failed
            && persisted.state == TransportAttemptState::Completed)
    {
        let output_exists = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM model_outputs
                WHERE model_step_id=?1 AND model_execution_snapshot_id=?2
            )",
            params![
                attempt.model_step_id.to_string(),
                attempt.model_execution_snapshot_id.to_string(),
            ],
            |row| row.get::<_, bool>(0),
        )?;
        if !output_exists {
            return Err(PersistenceError::InvalidRequest(
                "completed context compaction attempt has no persisted model output".to_owned(),
            ));
        }
    }
    Ok(())
}

fn compaction_model_attempt_state_name(state: CompactionModelAttemptState) -> &'static str {
    match state {
        CompactionModelAttemptState::Running => "running",
        CompactionModelAttemptState::Completed => "completed",
        CompactionModelAttemptState::Failed => "failed",
        CompactionModelAttemptState::Cancelled => "cancelled",
        CompactionModelAttemptState::UnknownSideEffect => "unknown_side_effect",
    }
}

fn parse_compaction_model_attempt_state(
    state: &str,
) -> Result<CompactionModelAttemptState, PersistenceError> {
    match state {
        "running" => Ok(CompactionModelAttemptState::Running),
        "completed" => Ok(CompactionModelAttemptState::Completed),
        "failed" => Ok(CompactionModelAttemptState::Failed),
        "cancelled" => Ok(CompactionModelAttemptState::Cancelled),
        "unknown_side_effect" => Ok(CompactionModelAttemptState::UnknownSideEffect),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted context compaction model attempt state is invalid".to_owned(),
        )),
    }
}

fn load_context_compaction_model_attempt(
    connection: &rusqlite::Connection,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    sequence: u32,
    attempt_ordinal: u8,
) -> Result<Option<CompactionModelAttemptRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT attempt_context_snapshot_id, model_step_id, model_execution_snapshot_id,
                    transport_attempt_id, state, error_code, started_at, terminal_at
             FROM context_compaction_model_attempts
             WHERE source_context_snapshot_id=?1 AND compaction_sequence=?2
               AND attempt_ordinal=?3",
            params![
                source_context_snapshot_id.to_string(),
                i64::from(sequence),
                i64::from(attempt_ordinal),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?
        .map(|row| {
            Ok(CompactionModelAttemptRecord {
                source_context_snapshot_id,
                compaction_sequence: sequence,
                attempt_ordinal,
                attempt_context_snapshot_id: FromStr::from_str(&row.0)?,
                model_step_id: FromStr::from_str(&row.1)?,
                model_execution_snapshot_id: FromStr::from_str(&row.2)?,
                transport_attempt_id: row.3.as_deref().map(FromStr::from_str).transpose()?,
                state: parse_compaction_model_attempt_state(&row.4)?,
                error_code: row.5,
                started_at: UtcTimestamp::from_str(&row.6).map_err(|error| {
                    PersistenceError::InvalidRequest(format!(
                        "persisted context compaction attempt started_at is invalid: {error}"
                    ))
                })?,
                terminal_at: row
                    .7
                    .as_deref()
                    .map(|value| {
                        UtcTimestamp::from_str(value).map_err(|error| {
                            PersistenceError::InvalidRequest(format!(
                                "persisted context compaction attempt terminal_at is invalid: {error}"
                            ))
                        })
                    })
                    .transpose()?,
            })
        })
        .transpose()
}

fn context_compaction_state_name(
    state: ContextCompactionStateView,
) -> Result<&'static str, PersistenceError> {
    match state {
        ContextCompactionStateView::Required => Ok("requested"),
        ContextCompactionStateView::Running => Ok("running"),
        ContextCompactionStateView::Completed => Ok("completed"),
        ContextCompactionStateView::Failed => Ok("failed"),
        ContextCompactionStateView::Cancelled => Ok("cancelled"),
        ContextCompactionStateView::NotRequired => Err(PersistenceError::InvalidRequest(
            "not_required is not a persisted context compaction state".to_owned(),
        )),
    }
}

fn parse_context_compaction_state(
    state: &str,
) -> Result<ContextCompactionStateView, PersistenceError> {
    match state {
        "requested" => Ok(ContextCompactionStateView::Required),
        "running" => Ok(ContextCompactionStateView::Running),
        "completed" => Ok(ContextCompactionStateView::Completed),
        "failed" => Ok(ContextCompactionStateView::Failed),
        "cancelled" => Ok(ContextCompactionStateView::Cancelled),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted context compaction state is invalid".to_owned(),
        )),
    }
}

fn load_context_compaction(
    connection: &rusqlite::Connection,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    sequence: u32,
) -> Result<Option<ContextCompactionStoredRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT c.state, c.model_step_id, c.transport_attempt_id, c.source_span_json,
                    c.historical_summary_artifact_id, c.active_turn_summary_artifact_id,
                    c.structured_compaction_artifact_id, c.anchor_manifest_artifact_id,
                    c.record_json, c.error_code
             FROM context_compactions c
             JOIN context_projection_snapshots s
               ON s.context_snapshot_id=c.source_context_snapshot_id
             WHERE c.source_context_snapshot_id=?1 AND c.sequence=?2
               AND s.session_id=?3 AND s.agent_run_id=?4",
            params![
                source_context_snapshot_id.to_string(),
                i64::from(sequence),
                session_id.to_string(),
                agent_run_id.to_string(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?
        .map(|row| {
            Ok(ContextCompactionStoredRecord {
                source_context_snapshot_id,
                sequence,
                session_id,
                agent_run_id,
                state: parse_context_compaction_state(&row.0)?,
                model_step_id: row.1.as_deref().map(FromStr::from_str).transpose()?,
                transport_attempt_id: row.2.as_deref().map(FromStr::from_str).transpose()?,
                source_span: serde_json::from_str(&row.3)?,
                historical_summary_artifact_id: row
                    .4
                    .as_deref()
                    .map(FromStr::from_str)
                    .transpose()?,
                active_turn_summary_artifact_id: row
                    .5
                    .as_deref()
                    .map(FromStr::from_str)
                    .transpose()?,
                structured_compaction_artifact_id: row
                    .6
                    .as_deref()
                    .map(FromStr::from_str)
                    .transpose()?,
                anchor_manifest_artifact_id: row.7.as_deref().map(FromStr::from_str).transpose()?,
                record: row.8.as_deref().map(serde_json::from_str).transpose()?,
                error_code: row.9,
            })
        })
        .transpose()
}

fn validate_context_remediation_plan(
    plan: &ContextRemediationPlan,
) -> Result<(), PersistenceError> {
    if plan.overflow.error_code != "context.non_compressible_overflow"
        || plan.overflow.overflow_tokens == 0
        || plan.actions.is_empty()
        || plan.actions != plan.overflow.remediation
    {
        return Err(PersistenceError::InvalidRequest(
            "context remediation plan is not a canonical non-compressible overflow plan".to_owned(),
        ));
    }
    Ok(())
}

fn validate_context_remediation_projection(
    projection: &PromptInspectorProjection,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
    active_plan: Option<&ContextRemediationPlan>,
) -> Result<(), PersistenceError> {
    let overflow_matches = match active_plan {
        Some(plan) => projection.overflow.as_ref() == Some(&plan.overflow),
        None => projection.overflow.is_none(),
    };
    if projection.session_id != session_id
        || projection.active_agent_run_id != Some(agent_run_id)
        || projection.context_snapshot_id != Some(source_context_snapshot_id)
        || !overflow_matches
    {
        return Err(PersistenceError::InvalidRequest(
            "context remediation Prompt Inspector projection is inconsistent".to_owned(),
        ));
    }
    Ok(())
}

fn context_remediation_clear_reason_name(reason: ContextRemediationClearReason) -> &'static str {
    match reason {
        ContextRemediationClearReason::RecheckSucceeded => "recheck_succeeded",
        ContextRemediationClearReason::Cancelled => "cancelled",
    }
}

fn parse_context_remediation_clear_reason(
    reason: &str,
) -> Result<ContextRemediationClearReason, PersistenceError> {
    match reason {
        "recheck_succeeded" => Ok(ContextRemediationClearReason::RecheckSucceeded),
        "cancelled" => Ok(ContextRemediationClearReason::Cancelled),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted context remediation clear reason is invalid".to_owned(),
        )),
    }
}

fn load_context_remediation(
    connection: &rusqlite::Connection,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    source_context_snapshot_id: aura_domain_common::ContextProjectionSnapshotId,
) -> Result<Option<ContextRemediationRecord>, PersistenceError> {
    connection
        .query_row(
            "SELECT state, plan_json, required_at, cleared_at, clear_reason
             FROM context_remediations
             WHERE session_id=?1 AND agent_run_id=?2 AND source_context_snapshot_id=?3",
            params![
                session_id.to_string(),
                agent_run_id.to_string(),
                source_context_snapshot_id.to_string(),
            ],
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
        .optional()?
        .map(|row| {
            let state = match row.0.as_str() {
                "awaiting" => ContextRemediationState::Awaiting,
                "cleared" => ContextRemediationState::Cleared,
                _ => {
                    return Err(PersistenceError::InvalidRequest(
                        "persisted context remediation state is invalid".to_owned(),
                    ));
                }
            };
            let cleared_at = row
                .3
                .as_deref()
                .map(|value| {
                    UtcTimestamp::from_str(value).map_err(|error| {
                        PersistenceError::InvalidRequest(format!(
                            "persisted context remediation cleared_at is invalid: {error}"
                        ))
                    })
                })
                .transpose()?;
            let clear_reason =
                row.4.as_deref().map(parse_context_remediation_clear_reason).transpose()?;
            if (state == ContextRemediationState::Awaiting
                && (cleared_at.is_some() || clear_reason.is_some()))
                || (state == ContextRemediationState::Cleared
                    && (cleared_at.is_none() || clear_reason.is_none()))
            {
                return Err(PersistenceError::InvalidRequest(
                    "persisted context remediation terminal fields are inconsistent".to_owned(),
                ));
            }
            Ok(ContextRemediationRecord {
                session_id,
                agent_run_id,
                source_context_snapshot_id,
                plan: serde_json::from_str(&row.1)?,
                state,
                required_at: UtcTimestamp::from_str(&row.2).map_err(|error| {
                    PersistenceError::InvalidRequest(format!(
                        "persisted context remediation required_at is invalid: {error}"
                    ))
                })?,
                cleared_at,
                clear_reason,
            })
        })
        .transpose()
}

fn load_context_internal_artifact_metadata(
    connection: &rusqlite::Connection,
    session_id: SessionId,
    artifact_id: ArtifactId,
) -> Result<ContextInternalArtifactMetadata, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT a.media_type, a.byte_length, a.content_hash_algorithm, a.content_hash_hex
             FROM artifacts a
             JOIN artifact_owners o ON o.artifact_id=a.artifact_id
             WHERE a.artifact_id=?1
               AND o.owner_kind='direct' AND o.owner_type='session' AND o.owner_id=?2
               AND (
                    EXISTS(SELECT 1 FROM prompt_module_snapshots p
                           WHERE p.content_artifact_id=a.artifact_id)
                 OR EXISTS(SELECT 1 FROM prompt_bundle_snapshots p
                           WHERE p.custom_addendum_artifact_id=a.artifact_id)
                 OR EXISTS(SELECT 1 FROM context_projection_sections s
                           WHERE s.content_artifact_id=a.artifact_id)
                 OR EXISTS(SELECT 1 FROM context_projection_snapshots s
                           WHERE s.instruction_envelope_artifact_id=a.artifact_id
                              OR s.model_egress_manifest_artifact_id=a.artifact_id)
                 OR EXISTS(SELECT 1 FROM context_compactions c
                           WHERE c.historical_summary_artifact_id=a.artifact_id
                              OR c.active_turn_summary_artifact_id=a.artifact_id
                              OR c.structured_compaction_artifact_id=a.artifact_id
                              OR c.anchor_manifest_artifact_id=a.artifact_id
                              OR c.focus_artifact_id=a.artifact_id)
               )",
            params![artifact_id.to_string(), session_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(PersistenceError::ArtifactAccessDenied)?;
    let algorithm = match row.2.as_str() {
        "sha256" => aura_domain_common::ContentHashAlgorithm::Sha256,
        "blake3" => aura_domain_common::ContentHashAlgorithm::Blake3,
        _ => return Err(PersistenceError::ArtifactIntegrity),
    };
    Ok(ContextInternalArtifactMetadata {
        artifact_id,
        media_type: row.0,
        byte_length: u64::try_from(row.1).map_err(|_| PersistenceError::ArtifactIntegrity)?,
        content_hash: ContentHash::new(algorithm, row.3)?,
    })
}

fn parse_event_actor(value: &str) -> Result<crate::EventActor, PersistenceError> {
    match value {
        "user" => Ok(crate::EventActor::User),
        "parent_agent" => Ok(crate::EventActor::ParentAgent),
        "child_agent" => Ok(crate::EventActor::ChildAgent),
        "system_runtime" => Ok(crate::EventActor::SystemRuntime),
        _ => Err(PersistenceError::InvalidRequest(
            "persisted context event actor is invalid".to_owned(),
        )),
    }
}
