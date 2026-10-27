use std::{collections::BTreeSet, future::Future, pin::Pin};

use aura_context_domain::{
    CompactionAnchor, CompactionAnchorKind, CompactionAnchorManifest, CompactionModelAttemptState,
    CompactionSourceSpan, ContextRetentionException, ContextSourceRef,
    MANUAL_COMPACTION_FOCUS_MAX_CHARS, ModelEgressManifestV1,
};
use aura_domain_common::{
    AgentRunId, ContentHash, ContextProjectionSnapshotId, ModelProfileId, SessionId,
    TransportAttemptId, canonical_json_v1, canonical_json_v1_sha256,
};
use aura_model_domain::{
    LogicalModelRequest, ModelExecutionSnapshot, NormalizedModelOutput, NormalizedStopReason,
};
use thiserror::Error;

mod attempt;
pub use attempt::CompactionRequestShape;
use attempt::validate_prepared_compaction_attempt;

const HEADINGS: [&str; 7] = [
    "# Historical-Snapshot-Summary",
    "# Active-Turn-Progress-Summary",
    "# Structured-Compaction",
    "## Summary",
    "## Full-Turn-Session-Line",
    "## Active-Turn-Segment-Line",
    "## Truth-Index",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionSourceDraft {
    pub kind: CompactionAnchorKind,
    pub source_ref: ContextSourceRef,
    pub source_hash: ContentHash,
    pub preview: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedCompactionOutput {
    pub historical_snapshot: String,
    pub active_turn_summary: String,
    pub structured_compaction: String,
    pub referenced_anchors: Vec<String>,
    pub full_markdown: String,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CompactionValidationError {
    #[error("context.compaction.focus_too_long")]
    FocusTooLong,
    #[error("context.compaction.focus_control_character")]
    FocusControlCharacter,
    #[error("context.compaction.anchor_manifest_invalid")]
    AnchorManifestInvalid,
    #[error("context.compaction.output_empty")]
    OutputEmpty,
    #[error("context.compaction.output_too_large")]
    OutputTooLarge,
    #[error("context.compaction.heading_invalid")]
    HeadingInvalid,
    #[error("context.compaction.historical_snapshot_empty")]
    HistoricalSnapshotEmpty,
    #[error("context.compaction.unknown_anchor")]
    UnknownAnchor,
    #[error("context.compaction.duplicate_anchor")]
    DuplicateAnchor,
    #[error("context.compaction.truth_index_invalid")]
    TruthIndexInvalid,
}

pub fn validate_manual_compaction_focus(
    focus: Option<&str>,
) -> Result<Option<String>, CompactionValidationError> {
    let Some(focus) = focus else {
        return Ok(None);
    };
    let normalized = focus.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.chars().count() > MANUAL_COMPACTION_FOCUS_MAX_CHARS {
        return Err(CompactionValidationError::FocusTooLong);
    }
    if normalized.chars().any(|character| {
        (character <= '\u{001f}' && !matches!(character, '\n' | '\t')) || character == '\u{007f}'
    }) {
        return Err(CompactionValidationError::FocusControlCharacter);
    }
    Ok((!normalized.trim().is_empty()).then_some(normalized))
}

pub fn build_anchor_manifest(
    sources: Vec<CompactionSourceDraft>,
) -> Result<CompactionAnchorManifest, CompactionValidationError> {
    let mut source_keys = BTreeSet::new();
    for source in &sources {
        let source_key = canonical_json_v1(&(&source.source_ref, &source.source_hash))
            .map_err(|_| CompactionValidationError::AnchorManifestInvalid)?;
        if !source_keys.insert(source_key) {
            return Err(CompactionValidationError::AnchorManifestInvalid);
        }
    }
    let anchors = sources
        .into_iter()
        .enumerate()
        .map(|(index, source)| CompactionAnchor {
            anchor: format!("a{:06}", index + 1),
            kind: source.kind,
            source_ref: source.source_ref,
            source_hash: source.source_hash,
            preview: source.preview,
        })
        .collect::<Vec<_>>();
    let manifest_hash = canonical_json_v1_sha256(&anchors)
        .map_err(|_| CompactionValidationError::AnchorManifestInvalid)?;
    Ok(CompactionAnchorManifest { anchors, manifest_hash })
}

pub fn parse_and_validate_compaction_output(
    output: &str,
    output_character_limit: u64,
    manifest: &CompactionAnchorManifest,
    has_compressible_sources: bool,
) -> Result<ParsedCompactionOutput, CompactionValidationError> {
    if output.is_empty() {
        return Err(CompactionValidationError::OutputEmpty);
    }
    if crate::compaction_size::compaction_output_too_large(output, output_character_limit) {
        return Err(CompactionValidationError::OutputTooLarge);
    }
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let heading_lines = normalized.lines().filter(|line| line.starts_with('#')).collect::<Vec<_>>();
    if heading_lines != HEADINGS {
        return Err(CompactionValidationError::HeadingInvalid);
    }
    let positions = HEADINGS
        .iter()
        .map(|heading| normalized.find(heading).ok_or(CompactionValidationError::HeadingInvalid))
        .collect::<Result<Vec<_>, _>>()?;
    if !positions.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(CompactionValidationError::HeadingInvalid);
    }
    let historical_snapshot = body_between(&normalized, positions[0], HEADINGS[0], positions[1]);
    if has_compressible_sources && historical_snapshot.trim().is_empty() {
        return Err(CompactionValidationError::HistoricalSnapshotEmpty);
    }
    let active_turn_summary = body_between(&normalized, positions[1], HEADINGS[1], positions[2]);
    let structured_compaction = normalized[positions[2]..].trim_end().to_owned();
    let allowed =
        manifest.anchors.iter().map(|anchor| anchor.anchor.as_str()).collect::<BTreeSet<_>>();
    let truth_index = body_after(&normalized, positions[6], HEADINGS[6]);
    let mut referenced_anchors = Vec::new();
    let mut truth_set = BTreeSet::new();
    for line in truth_index.lines().filter(|line| !line.trim().is_empty()) {
        let anchor = line
            .strip_prefix("- ")
            .filter(|value| is_anchor_token(value))
            .ok_or(CompactionValidationError::TruthIndexInvalid)?;
        if !allowed.contains(anchor) {
            return Err(CompactionValidationError::UnknownAnchor);
        }
        if !truth_set.insert(anchor.to_owned()) {
            return Err(CompactionValidationError::DuplicateAnchor);
        }
        referenced_anchors.push(anchor.to_owned());
    }
    if truth_set != allowed.into_iter().map(ToOwned::to_owned).collect() {
        return Err(CompactionValidationError::TruthIndexInvalid);
    }
    let all_references = referenced_anchor_tokens(&normalized).into_iter().collect::<BTreeSet<_>>();
    if all_references != truth_set {
        return Err(CompactionValidationError::TruthIndexInvalid);
    }
    Ok(ParsedCompactionOutput {
        historical_snapshot,
        active_turn_summary,
        structured_compaction,
        referenced_anchors,
        full_markdown: normalized,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionAttemptDisposition {
    RetryOnce,
    Interrupt,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionOperationSpec {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub model_profile_id: ModelProfileId,
    pub source_context_snapshot_id: ContextProjectionSnapshotId,
    pub sequence: u32,
    pub source_span: CompactionSourceSpan,
    pub anchor_manifest: CompactionAnchorManifest,
    pub output_character_limit: u64,
    pub retained_recent_tokens: u64,
    pub retention_exceptions: Vec<ContextRetentionException>,
}

#[derive(Clone, Debug)]
pub struct PreparedCompactionModelAttempt {
    pub attempt_ordinal: u8,
    pub logical_request: LogicalModelRequest,
    pub execution_snapshot: ModelExecutionSnapshot,
    pub egress_manifest: ModelEgressManifestV1,
    pub shape: CompactionRequestShape,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactionExecutionFailure {
    pub code: String,
    pub retryable: bool,
    pub side_effect_unknown: bool,
}

#[derive(Clone, Debug)]
pub enum CompactionModelCallTerminal {
    Completed { output: Box<NormalizedModelOutput>, transport_attempt_id: TransportAttemptId },
    Failed { error_code: String, transport_attempt_id: Option<TransportAttemptId>, retryable: bool },
    Cancelled { error_code: String, transport_attempt_id: Option<TransportAttemptId> },
    UnknownSideEffect { error_code: String, transport_attempt_id: TransportAttemptId },
}

#[derive(Clone, Debug)]
pub struct ValidatedCompactionCommit {
    pub operation: ContextCompactionOperationSpec,
    pub attempt: PreparedCompactionModelAttempt,
    pub transport_attempt_id: TransportAttemptId,
    pub parsed: ParsedCompactionOutput,
}

#[derive(Clone, Debug)]
pub struct CompactionAttemptFailureCommit {
    pub operation: ContextCompactionOperationSpec,
    pub attempt: Option<PreparedCompactionModelAttempt>,
    pub state: CompactionModelAttemptState,
    pub transport_attempt_id: Option<TransportAttemptId>,
    pub error_code: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextCompactionExecutionOutcome {
    Completed { attempt_ordinal: u8, transport_attempt_id: TransportAttemptId },
    Failed { failure: CompactionExecutionFailure, completed_attempts: u8 },
    Cancelled { error_code: String, completed_attempts: u8 },
    PersistenceInterrupted { error_code: String },
}

/// The factory and the sink perform the application's real synchronous store work - they
/// read and write the journal for the attempt they are describing - and this used to be synchronous,
/// so whichever reactive task happened to poll `execute_context_compaction` was the thread parked on
/// the database lock. Both traits are awaitable now, and the application implementation runs each
/// step whole on its own bounded synchronous owner. The attempt ordinal, the operation and the
/// commit value each step is given are unchanged: this is a change of *where the wait happens*, not
/// of what is written or in which order.
pub trait CompactionAttemptFactory: Send + Sync {
    fn prepare_attempt<'a>(
        &'a self,
        operation: &'a ContextCompactionOperationSpec,
        attempt_ordinal: u8,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<PreparedCompactionModelAttempt, CompactionExecutionFailure>>
                + Send
                + 'a,
        >,
    >;
}

pub trait CompactionModelCaller: Send + Sync {
    fn call<'a>(
        &'a self,
        attempt: &'a PreparedCompactionModelAttempt,
    ) -> Pin<Box<dyn Future<Output = CompactionModelCallTerminal> + Send + 'a>>;
}

pub trait CompactionLifecycleSink: Send + Sync {
    fn mark_attempt_running<'a>(
        &'a self,
        operation: &'a ContextCompactionOperationSpec,
        attempt: &'a PreparedCompactionModelAttempt,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn finish_retryable_attempt<'a>(
        &'a self,
        failure: &'a CompactionAttemptFailureCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn commit_completed<'a>(
        &'a self,
        completed: &'a ValidatedCompactionCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn finish_operation<'a>(
        &'a self,
        failure: &'a CompactionAttemptFailureCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

pub async fn execute_context_compaction<Factory, Caller, Sink, Facts>(
    operation: ContextCompactionOperationSpec,
    factory: &Factory,
    caller: &Caller,
    sink: &Sink,
    authorization_facts: &Facts,
) -> ContextCompactionExecutionOutcome
where
    Factory: CompactionAttemptFactory,
    Caller: CompactionModelCaller,
    Sink: CompactionLifecycleSink,
    Facts: crate::ModelEgressAuthorizationFacts,
{
    if let Err(failure) = validate_compaction_operation(&operation) {
        return finish_without_attempt(operation, sink, failure, 0).await;
    }
    let mut context_snapshot_ids = BTreeSet::new();
    let mut model_step_ids = BTreeSet::new();
    for attempt_ordinal in 1..=2 {
        let attempt = match factory.prepare_attempt(&operation, attempt_ordinal).await {
            Ok(attempt) => attempt,
            Err(failure) => {
                return finish_without_attempt(
                    operation,
                    sink,
                    failure,
                    attempt_ordinal.saturating_sub(1),
                )
                .await;
            }
        };
        if let Err(failure) = validate_prepared_compaction_attempt(
            &operation,
            &attempt,
            attempt_ordinal,
            &mut context_snapshot_ids,
            &mut model_step_ids,
        ) {
            return finish_without_attempt(
                operation,
                sink,
                failure,
                attempt_ordinal.saturating_sub(1),
            )
            .await;
        }
        if let Err(error) = crate::ModelEgressAuthorizer::new(authorization_facts)
            .validate(operation.session_id, operation.model_profile_id, &attempt.egress_manifest)
            .await
        {
            return finish_without_attempt(
                operation,
                sink,
                CompactionExecutionFailure {
                    code: error.to_string(),
                    retryable: false,
                    side_effect_unknown: false,
                },
                attempt_ordinal.saturating_sub(1),
            )
            .await;
        }
        if let Err(error_code) = sink.mark_attempt_running(&operation, &attempt).await {
            return ContextCompactionExecutionOutcome::PersistenceInterrupted { error_code };
        }

        match caller.call(&attempt).await {
            CompactionModelCallTerminal::Completed { output, transport_attempt_id } => {
                let parsed = normalized_compaction_output(&attempt, output).and_then(|output| {
                    parse_and_validate_compaction_output(
                        &output,
                        operation.output_character_limit,
                        &operation.anchor_manifest,
                        !operation.anchor_manifest.anchors.is_empty(),
                    )
                    .map_err(|error| CompactionExecutionFailure {
                        code: error.to_string(),
                        retryable: true,
                        side_effect_unknown: false,
                    })
                });
                match parsed {
                    Ok(parsed) => {
                        let completed = ValidatedCompactionCommit {
                            operation,
                            attempt,
                            transport_attempt_id,
                            parsed,
                        };
                        if let Err(error_code) = sink.commit_completed(&completed).await {
                            return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                                error_code,
                            };
                        }
                        return ContextCompactionExecutionOutcome::Completed {
                            attempt_ordinal,
                            transport_attempt_id,
                        };
                    }
                    Err(failure) => {
                        let commit = CompactionAttemptFailureCommit {
                            operation: operation.clone(),
                            attempt: Some(attempt),
                            state: CompactionModelAttemptState::Failed,
                            transport_attempt_id: Some(transport_attempt_id),
                            error_code: failure.code.clone(),
                        };
                        if attempt_ordinal == 1 {
                            if let Err(error_code) = sink.finish_retryable_attempt(&commit).await {
                                return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                                    error_code,
                                };
                            }
                            continue;
                        }
                        return finish_with_attempt(commit, sink, failure, attempt_ordinal).await;
                    }
                }
            }
            CompactionModelCallTerminal::Failed { error_code, transport_attempt_id, retryable } => {
                let failure = CompactionExecutionFailure {
                    code: error_code.clone(),
                    retryable,
                    side_effect_unknown: false,
                };
                let commit = CompactionAttemptFailureCommit {
                    operation: operation.clone(),
                    attempt: Some(attempt),
                    state: CompactionModelAttemptState::Failed,
                    transport_attempt_id,
                    error_code,
                };
                if attempt_ordinal == 1 && retryable {
                    if let Err(error_code) = sink.finish_retryable_attempt(&commit).await {
                        return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                            error_code,
                        };
                    }
                    continue;
                }
                return finish_with_attempt(commit, sink, failure, attempt_ordinal).await;
            }
            CompactionModelCallTerminal::Cancelled { error_code, transport_attempt_id } => {
                let commit = CompactionAttemptFailureCommit {
                    operation,
                    attempt: Some(attempt),
                    state: CompactionModelAttemptState::Cancelled,
                    transport_attempt_id,
                    error_code: error_code.clone(),
                };
                if let Err(error_code) = sink.finish_operation(&commit).await {
                    return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                        error_code,
                    };
                }
                return ContextCompactionExecutionOutcome::Cancelled {
                    error_code,
                    completed_attempts: attempt_ordinal,
                };
            }
            CompactionModelCallTerminal::UnknownSideEffect { error_code, transport_attempt_id } => {
                let failure = CompactionExecutionFailure {
                    code: error_code.clone(),
                    retryable: false,
                    side_effect_unknown: true,
                };
                let commit = CompactionAttemptFailureCommit {
                    operation,
                    attempt: Some(attempt),
                    state: CompactionModelAttemptState::UnknownSideEffect,
                    transport_attempt_id: Some(transport_attempt_id),
                    error_code,
                };
                return finish_with_attempt(commit, sink, failure, attempt_ordinal).await;
            }
        }
    }
    ContextCompactionExecutionOutcome::Failed {
        failure: CompactionExecutionFailure {
            code: "context.compaction.retry_exhausted".to_owned(),
            retryable: false,
            side_effect_unknown: false,
        },
        completed_attempts: 2,
    }
}

fn validate_compaction_operation(
    operation: &ContextCompactionOperationSpec,
) -> Result<(), CompactionExecutionFailure> {
    let source_refs = operation
        .anchor_manifest
        .anchors
        .iter()
        .map(|anchor| anchor.source_ref.clone())
        .collect::<Vec<_>>();
    if operation.sequence == 0
        || operation.output_character_limit == 0
        || operation.source_span.start_sequence > operation.source_span.end_sequence
        || operation.source_span.source_refs != source_refs
        || canonical_json_v1_sha256(&operation.anchor_manifest.anchors)
            .map_or(true, |hash| hash != operation.anchor_manifest.manifest_hash)
    {
        return Err(CompactionExecutionFailure {
            code: "context.compaction.operation_invalid".to_owned(),
            retryable: false,
            side_effect_unknown: false,
        });
    }
    Ok(())
}

fn normalized_compaction_output(
    attempt: &PreparedCompactionModelAttempt,
    output: Box<NormalizedModelOutput>,
) -> Result<String, CompactionExecutionFailure> {
    let output = *output;
    if output.model_step_id != attempt.logical_request.model_step_id
        || !output.tool_calls.is_empty()
        || output.refusal.is_some()
        || !matches!(
            output.stop_reason,
            NormalizedStopReason::EndTurn | NormalizedStopReason::StopSequence
        )
        || output.assistant_blocks.is_empty()
    {
        return Err(CompactionExecutionFailure {
            code: "context.compaction.model_output_invalid".to_owned(),
            retryable: true,
            side_effect_unknown: false,
        });
    }
    Ok(output.assistant_blocks.into_iter().map(|block| block.text).collect())
}

async fn finish_without_attempt<Sink: CompactionLifecycleSink>(
    operation: ContextCompactionOperationSpec,
    sink: &Sink,
    failure: CompactionExecutionFailure,
    completed_attempts: u8,
) -> ContextCompactionExecutionOutcome {
    let commit = CompactionAttemptFailureCommit {
        operation,
        attempt: None,
        state: CompactionModelAttemptState::Failed,
        transport_attempt_id: None,
        error_code: failure.code.clone(),
    };
    if let Err(error_code) = sink.finish_operation(&commit).await {
        return ContextCompactionExecutionOutcome::PersistenceInterrupted { error_code };
    }
    ContextCompactionExecutionOutcome::Failed { failure, completed_attempts }
}

async fn finish_with_attempt<Sink: CompactionLifecycleSink>(
    commit: CompactionAttemptFailureCommit,
    sink: &Sink,
    failure: CompactionExecutionFailure,
    completed_attempts: u8,
) -> ContextCompactionExecutionOutcome {
    if let Err(error_code) = sink.finish_operation(&commit).await {
        return ContextCompactionExecutionOutcome::PersistenceInterrupted { error_code };
    }
    ContextCompactionExecutionOutcome::Failed { failure, completed_attempts }
}

#[must_use]
pub const fn disposition_after_failure(completed_attempts: u8) -> CompactionAttemptDisposition {
    if completed_attempts < 2 {
        CompactionAttemptDisposition::RetryOnce
    } else {
        CompactionAttemptDisposition::Interrupt
    }
}

fn body_between(text: &str, start: usize, heading: &str, end: usize) -> String {
    text[start + heading.len()..end].trim().to_owned()
}

fn body_after(text: &str, start: usize, heading: &str) -> String {
    text[start + heading.len()..].trim().to_owned()
}

fn is_anchor_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'a' && bytes[1..].iter().all(u8::is_ascii_digit)
}

fn referenced_anchor_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut anchors = BTreeSet::new();
    let mut index = 0;
    while index + 7 <= bytes.len() {
        if bytes[index] == b'a'
            && bytes[index + 1..index + 7].iter().all(u8::is_ascii_digit)
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
            && (index + 7 == bytes.len() || !bytes[index + 7].is_ascii_alphanumeric())
        {
            if let Ok(anchor) = std::str::from_utf8(&bytes[index..index + 7]) {
                anchors.insert(anchor.to_owned());
            }
            index += 7;
        } else {
            index += 1;
        }
    }
    anchors.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicU8, Ordering},
        },
    };

    use aura_context_domain::{ModelEgressSourceKind, SourceAuthorizationBinding};
    use aura_domain_common::ToolRegistrySnapshotId;
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{
        EndpointId, EventId, ModelContentBlockId, ModelExecutionSnapshotId, ModelStepId,
        PromptBundleSnapshotId, ProviderId, Revision, SessionSequence, UtcTimestamp,
    };
    use aura_model_domain::{
        AssistantContentBlock, CompiledInstructionEnvelope, EndpointAuthScheme,
        LogicalInstructionBlock, LogicalInstructionKind, ModelApiFormat, ModelOutputFormat,
        ModelOutputRequest, ModelUsage, ResolvedModelRequestOptions, UsageSource,
    };
    use aura_security_contract::NetworkDataClass;
    use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1};

    use super::*;

    fn manifest() -> CompactionAnchorManifest {
        build_anchor_manifest(vec![CompactionSourceDraft {
            kind: CompactionAnchorKind::Event,
            source_ref: ContextSourceRef::Event { event_id: EventId::new() },
            source_hash: ContentHash::sha256(b"event"),
            preview: "event".to_owned(),
        }])
        .expect("manifest")
    }

    fn valid_output() -> String {
        "# Historical-Snapshot-Summary\nHistory a000001\n\n# Active-Turn-Progress-Summary\nActive\n\n# Structured-Compaction\n## Summary\nSummary\n## Full-Turn-Session-Line\nLine\n## Active-Turn-Segment-Line\nSegment\n## Truth-Index\n- a000001\n".to_owned()
    }

    fn operation() -> ContextCompactionOperationSpec {
        let anchor_manifest = manifest();
        ContextCompactionOperationSpec {
            session_id: SessionId::new(),
            agent_run_id: AgentRunId::new(),
            model_profile_id: ModelProfileId::new(),
            source_context_snapshot_id: ContextProjectionSnapshotId::new(),
            sequence: 1,
            source_span: CompactionSourceSpan {
                start_sequence: SessionSequence::new(1),
                end_sequence: SessionSequence::new(1),
                source_refs: anchor_manifest
                    .anchors
                    .iter()
                    .map(|anchor| anchor.source_ref.clone())
                    .collect(),
            },
            anchor_manifest,
            output_character_limit: 10_000,
            retained_recent_tokens: 0,
            retention_exceptions: Vec::new(),
        }
    }

    fn instruction_envelope() -> CompiledInstructionEnvelope {
        let kinds = [
            LogicalInstructionKind::Core,
            LogicalInstructionKind::Permission,
            LogicalInstructionKind::ToolSummary,
            LogicalInstructionKind::WorkspaceAgents,
            LogicalInstructionKind::HistoricalSnapshot,
            LogicalInstructionKind::AgentMode,
            LogicalInstructionKind::Hook,
            LogicalInstructionKind::TaskState,
        ];
        let blocks = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let content = format!("compaction-block-{index}");
                LogicalInstructionBlock {
                    kind,
                    content_hash: ContentHash::sha256(content.as_bytes()),
                    content,
                    source_event_ids: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        let canonical_text =
            blocks.iter().map(|block| block.content.as_str()).collect::<Vec<_>>().join("\n");
        CompiledInstructionEnvelope::new(1, blocks, canonical_text).expect("instruction envelope")
    }

    fn prepared_attempt(
        operation: &ContextCompactionOperationSpec,
        attempt_ordinal: u8,
    ) -> PreparedCompactionModelAttempt {
        let context_snapshot_id = ContextProjectionSnapshotId::new();
        let model_step_id = ModelStepId::new();
        let execution_snapshot_id = ModelExecutionSnapshotId::new();
        let logical_request = LogicalModelRequest {
            model_step_id,
            execution_snapshot_id,
            context_snapshot_id,
            instruction_envelope: instruction_envelope(),
            conversation_items: Vec::new(),
            fixed_tool_ids: Vec::new(),
            tool_schema_set: ToolSchemaSetSnapshotV1::new(
                ToolRegistrySnapshotRef {
                    tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                    registry_revision: Revision::INITIAL,
                    content_hash: CanonicalHashV1::sha256(b"compaction-test-registry"),
                },
                Vec::new(),
            )
            .expect("schema set"),
            attachments: Vec::new(),
            provider_continuation: None,
            output_request: ModelOutputRequest {
                format: ModelOutputFormat::Text,
                effective_max_output_tokens: 1_024,
            },
        };
        let execution_snapshot = ModelExecutionSnapshot {
            id: execution_snapshot_id,
            model_step_id,
            provider_id: ProviderId::new(),
            endpoint_id: EndpointId::new(),
            model_profile_id: operation.model_profile_id,
            provider_display_name: "Compaction Fixture Provider".to_owned(),
            endpoint_display_name: "Compaction Fixture Endpoint".to_owned(),
            model_name: "compaction-fixture-model".to_owned(),
            readable_ref: "Compaction Fixture Provider / compaction-fixture-model".to_owned(),
            api_format: ModelApiFormat::OpenAiResponses,
            auth_scheme: EndpointAuthScheme::None,
            base_url: "https://api.example.test/v1".to_owned(),
            resolved_request_url: "https://api.example.test/v1/responses".to_owned(),
            openai_organization_id: None,
            openai_project_id: None,
            anthropic_workspace_id: None,
            chat_output_token_field: None,
            chat_instruction_role: None,
            endpoint_config_revision: 1,
            model_config_revision: 1,
            runtime_effective_window_tokens: 32_768,
            historical_snapshot_max_tokens: 1_024,
            request_options: ResolvedModelRequestOptions::output_only(1_024),
            historical_tool_call_wire_bindings: Vec::new(),
            credential_id: None,
            credential_revision: None,
            credential_binding_fingerprint: None,
            context_snapshot_id,
            prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
            endpoint_fingerprint: ContentHash::sha256(b"compaction-fixture-endpoint"),
            created_at: UtcTimestamp::now(),
        };
        PreparedCompactionModelAttempt {
            attempt_ordinal,
            logical_request,
            execution_snapshot,
            egress_manifest: ModelEgressManifestV1::new(context_snapshot_id, Vec::new())
                .expect("egress manifest"),
            shape: CompactionRequestShape::Isolated,
        }
    }

    fn completed_output(
        attempt: &PreparedCompactionModelAttempt,
        text: String,
    ) -> NormalizedModelOutput {
        NormalizedModelOutput {
            model_step_id: attempt.logical_request.model_step_id,
            assistant_blocks: vec![AssistantContentBlock {
                block_id: ModelContentBlockId::new(),
                text,
                provider_reference: None,
            }],
            reasoning_blocks: Vec::new(),
            tool_calls: Vec::new(),
            refusal: None,
            stop_reason: NormalizedStopReason::EndTurn,
            usage: ModelUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                reasoning_tokens: None,
                cached_input_tokens: None,
                cache_write_tokens: None,
                source: UsageSource::LocalEstimated,
            },
            provider_continuation: None,
            provider_request_id: None,
            raw_response_ref: None,
        }
    }

    struct Factory;

    impl CompactionAttemptFactory for Factory {
        fn prepare_attempt<'a>(
            &'a self,
            operation: &'a ContextCompactionOperationSpec,
            attempt_ordinal: u8,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<PreparedCompactionModelAttempt, CompactionExecutionFailure>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async move { Ok(prepared_attempt(operation, attempt_ordinal)) })
        }
    }

    struct Caller {
        calls: AtomicU8,
        terminals: Mutex<VecDeque<CompactionModelCallTerminal>>,
    }

    impl Caller {
        fn new(terminals: Vec<CompactionModelCallTerminal>) -> Self {
            Self { calls: AtomicU8::new(0), terminals: Mutex::new(terminals.into()) }
        }
    }

    impl CompactionModelCaller for Caller {
        fn call<'a>(
            &'a self,
            attempt: &'a PreparedCompactionModelAttempt,
        ) -> Pin<Box<dyn Future<Output = CompactionModelCallTerminal> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let terminal = self
                .terminals
                .lock()
                .expect("caller terminals")
                .pop_front()
                .expect("one terminal per call");
            let terminal = match terminal {
                CompactionModelCallTerminal::Completed { output, transport_attempt_id } => {
                    CompactionModelCallTerminal::Completed {
                        output: Box::new(NormalizedModelOutput {
                            model_step_id: attempt.logical_request.model_step_id,
                            ..*output
                        }),
                        transport_attempt_id,
                    }
                }
                other => other,
            };
            Box::pin(async move { terminal })
        }
    }

    #[derive(Default)]
    struct SinkState {
        running: Vec<u8>,
        retry_failures: Vec<CompactionModelAttemptState>,
        completed: Vec<u8>,
        terminal: Vec<CompactionModelAttemptState>,
    }

    #[derive(Default)]
    struct Sink {
        state: Mutex<SinkState>,
    }

    impl CompactionLifecycleSink for Sink {
        fn mark_attempt_running<'a>(
            &'a self,
            _operation: &'a ContextCompactionOperationSpec,
            attempt: &'a PreparedCompactionModelAttempt,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                self.state.lock().expect("sink state").running.push(attempt.attempt_ordinal);
                Ok(())
            })
        }

        fn finish_retryable_attempt<'a>(
            &'a self,
            failure: &'a CompactionAttemptFailureCommit,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                self.state.lock().expect("sink state").retry_failures.push(failure.state);
                Ok(())
            })
        }

        fn commit_completed<'a>(
            &'a self,
            completed: &'a ValidatedCompactionCommit,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                self.state
                    .lock()
                    .expect("sink state")
                    .completed
                    .push(completed.attempt.attempt_ordinal);
                Ok(())
            })
        }

        fn finish_operation<'a>(
            &'a self,
            failure: &'a CompactionAttemptFailureCommit,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                self.state.lock().expect("sink state").terminal.push(failure.state);
                Ok(())
            })
        }
    }

    struct Facts;

    impl crate::ModelEgressAuthorizationFacts for Facts {
        fn session_model_channel_is_authorized(
            &self,
            _session_id: SessionId,
            _model_profile_id: ModelProfileId,
        ) -> bool {
            true
        }

        async fn persisted_event(
            &self,
            _event_id: EventId,
        ) -> Result<
            Option<crate::PersistedContextEventAuthorizationFact>,
            crate::ModelEgressAuthorizationError,
        > {
            Ok(None)
        }

        fn source_grant_binding_is_valid(
            &self,
            _session_id: SessionId,
            _source_ref: &ContextSourceRef,
            _source_kind: ModelEgressSourceKind,
            _data_class: NetworkDataClass,
            _binding: &SourceAuthorizationBinding,
        ) -> Result<bool, crate::ModelEgressAuthorizationError> {
            Ok(false)
        }
    }

    #[test]
    fn strict_markdown_and_anchor_validation_accepts_only_the_frozen_shape() {
        let parsed =
            parse_and_validate_compaction_output(&valid_output(), 10_000, &manifest(), true)
                .expect("parsed");
        assert_eq!(parsed.referenced_anchors, vec!["a000001"]);

        let unknown = valid_output().replace("a000001", "a999999");
        assert_eq!(
            parse_and_validate_compaction_output(&unknown, 10_000, &manifest(), true),
            Err(CompactionValidationError::UnknownAnchor)
        );

        let duplicate = valid_output().replace("- a000001", "- a000001\n- a000001");
        assert_eq!(
            parse_and_validate_compaction_output(&duplicate, 10_000, &manifest(), true),
            Err(CompactionValidationError::DuplicateAnchor)
        );

        let missing = valid_output().replace("- a000001", "");
        assert_eq!(
            parse_and_validate_compaction_output(&missing, 10_000, &manifest(), true),
            Err(CompactionValidationError::TruthIndexInvalid)
        );
    }

    #[test]
    fn manual_focus_is_normalized_bounded_and_not_trimmed() {
        assert_eq!(
            validate_manual_compaction_focus(Some(" keep\r\nthis ")).expect("focus"),
            Some(" keep\nthis ".to_owned())
        );
        assert_eq!(
            validate_manual_compaction_focus(Some(&"x".repeat(4_097))),
            Err(CompactionValidationError::FocusTooLong)
        );
    }

    #[tokio::test]
    async fn invalid_first_output_retries_once_with_a_new_attempt_then_commits() {
        let operation = operation();
        let first = prepared_attempt(&operation, 1);
        let second = prepared_attempt(&operation, 2);
        let caller = Caller::new(vec![
            CompactionModelCallTerminal::Completed {
                output: Box::new(completed_output(&first, "invalid".to_owned())),
                transport_attempt_id: TransportAttemptId::new(),
            },
            CompactionModelCallTerminal::Completed {
                output: Box::new(completed_output(&second, valid_output())),
                transport_attempt_id: TransportAttemptId::new(),
            },
        ]);
        let sink = Sink::default();

        let outcome = execute_context_compaction(operation, &Factory, &caller, &sink, &Facts).await;

        assert!(matches!(
            outcome,
            ContextCompactionExecutionOutcome::Completed { attempt_ordinal: 2, .. }
        ));
        assert_eq!(caller.calls.load(Ordering::SeqCst), 2);
        let state = sink.state.lock().expect("sink state");
        assert_eq!(state.running, vec![1, 2]);
        assert_eq!(state.retry_failures, vec![CompactionModelAttemptState::Failed]);
        assert_eq!(state.completed, vec![2]);
        assert!(state.terminal.is_empty());
    }

    #[tokio::test]
    async fn unknown_side_effect_never_retries() {
        let caller = Caller::new(vec![CompactionModelCallTerminal::UnknownSideEffect {
            error_code: "transport.side_effect_unknown".to_owned(),
            transport_attempt_id: TransportAttemptId::new(),
        }]);
        let sink = Sink::default();

        let outcome =
            execute_context_compaction(operation(), &Factory, &caller, &sink, &Facts).await;

        assert!(matches!(
            outcome,
            ContextCompactionExecutionOutcome::Failed {
                failure: CompactionExecutionFailure { side_effect_unknown: true, .. },
                completed_attempts: 1,
            }
        ));
        assert_eq!(caller.calls.load(Ordering::SeqCst), 1);
        let state = sink.state.lock().expect("sink state");
        assert!(state.retry_failures.is_empty());
        assert_eq!(state.terminal, vec![CompactionModelAttemptState::UnknownSideEffect]);
    }

    #[tokio::test]
    async fn cancellation_is_terminal_and_never_retries() {
        let caller = Caller::new(vec![CompactionModelCallTerminal::Cancelled {
            error_code: "transport.cancelled".to_owned(),
            transport_attempt_id: Some(TransportAttemptId::new()),
        }]);
        let sink = Sink::default();

        let outcome =
            execute_context_compaction(operation(), &Factory, &caller, &sink, &Facts).await;

        assert_eq!(
            outcome,
            ContextCompactionExecutionOutcome::Cancelled {
                error_code: "transport.cancelled".to_owned(),
                completed_attempts: 1,
            }
        );
        assert_eq!(caller.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            sink.state.lock().expect("sink state").terminal,
            vec![CompactionModelAttemptState::Cancelled]
        );
    }
}
