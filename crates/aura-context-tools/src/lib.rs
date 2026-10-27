#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use aura_artifact_contract::ArtifactAccess;
use aura_context_domain::{
    CompactionAnchorManifest, ContextCompactionStateView, ContextEventSelector,
    ContextReadCompactionInput, ContextReadCompactionResult, ContextReadEventsInput,
    ContextReadEventsResult, ContextReadToolResultInput, ContextReadToolResultResult,
    ContextReadToolResultView, ContextSearchInput, ContextSearchMatch, ContextSearchMode,
    ContextSearchResult, ContextSectionUsageView, ContextSourceRef, ContextStatusView,
    ModelEgressAuthorizationBasis, ModelEgressManifestV1, ModelEgressSourceKind,
    SourceAuthorizationBinding, TaskLineageContextStatusView,
};
use aura_context_runtime::{
    ModelEgressAuthorizationError, ModelEgressAuthorizationFacts, ModelEgressAuthorizer,
    PersistedContextEventActor, PersistedContextEventAuthorizationFact,
};
use aura_domain_common::{
    AgentRunId, ContentHash, ModelProfileId, SessionId, SessionSequence, canonical_json_v1,
    canonical_json_v1_sha256,
};
use aura_model_domain::{
    LogicalModelRequest, ResolvedModelAttachment, validate_resolved_attachments,
};
use aura_persistence::{
    ARTIFACT_RANGE_LIMIT, ArtifactMetadataRequest, ArtifactReadRequest,
    ContextCompactionStoredRecord, ContextEventStoredQuery, ContextEventStoredRow,
    ContextSearchStoredQuery, PersistenceCoordinator, PersistenceError,
};
use aura_security_contract::NetworkDataClass;
mod event_view;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

const CURSOR_SCHEMA_VERSION: u32 = 1;
const CURSOR_PREFIX: &str = "ctx1.";
const SEARCH_LIMIT_MAX: u32 = 50;
const SEARCH_EXCERPT_CHARS: usize = 320;
const EVENT_SELECTOR_MAX: usize = 32;
const EVENT_EXPANDED_MAX: usize = 128;
const EVENT_CHARACTER_HARD_LIMIT: u64 = 262_144;
const TOOL_RESULT_PAGE_CHARS: usize = 65_536;

#[derive(Clone)]
pub struct ContextToolServices {
    persistence: PersistenceCoordinator,
    session_id: SessionId,
    agent_run_id: AgentRunId,
}

#[derive(Debug, Error)]
pub enum ContextToolError {
    #[error("context.scope_mismatch")]
    ScopeMismatch,
    #[error("context.invalid_request")]
    InvalidRequest,
    #[error("context.cursor_invalid")]
    CursorInvalid,
    #[error("context.result_too_large")]
    ResultTooLarge,
    #[error("context.source_not_found")]
    SourceNotFound,
    #[error("context.persisted_fact_invalid")]
    PersistedFactInvalid,
    #[error("context.persistence_failed: {0}")]
    Persistence(#[from] PersistenceError),
}

impl PartialEq for ContextToolError {
    fn eq(&self, other: &Self) -> bool {
        self.code() == other.code()
    }
}

impl ContextToolError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ScopeMismatch => "context.scope_mismatch",
            Self::InvalidRequest => "context.invalid_request",
            Self::CursorInvalid => "context.cursor_invalid",
            Self::ResultTooLarge => "context.result_too_large",
            Self::SourceNotFound => "context.source_not_found",
            Self::PersistedFactInvalid => "context.persisted_fact_invalid",
            Self::Persistence(_) => "context.persistence_failed",
        }
    }
}

impl ContextToolServices {
    #[must_use]
    pub fn new(
        persistence: PersistenceCoordinator,
        session_id: SessionId,
        agent_run_id: AgentRunId,
    ) -> Self {
        Self { persistence, session_id, agent_run_id }
    }

    pub fn status(&self) -> Result<ContextStatusView, ContextToolError> {
        let snapshot = self
            .persistence
            .latest_context_projection_snapshot(self.session_id, self.agent_run_id)?;
        let inspector = self.persistence.latest_prompt_inspector_projection(self.session_id)?;
        let inspector =
            inspector.filter(|value| value.active_agent_run_id == Some(self.agent_run_id));
        let Some(snapshot) = snapshot else {
            return Ok(ContextStatusView {
                session_id: self.session_id,
                agent_run_id: self.agent_run_id,
                current_context_snapshot_id: None,
                prompt_bundle_snapshot_id: None,
                runtime_effective_window_tokens: 0,
                request_input_budget_tokens: 0,
                estimated_input_tokens: 0,
                section_usage: Vec::new(),
                compaction_state: inspector
                    .as_ref()
                    .map_or(ContextCompactionStateView::NotRequired, |value| {
                        value.compaction.state
                    }),
                retention_exceptions: inspector
                    .as_ref()
                    .map_or_else(Vec::new, |value| value.compaction.retention_exceptions.clone()),
                active_task_lineage: inspector
                    .as_ref()
                    .and_then(|value| value.active_task_lineage.as_ref())
                    .map(task_lineage_status),
                overflow: inspector.and_then(|value| value.overflow),
            });
        };
        let section_usage = snapshot
            .sections
            .iter()
            .map(|section| ContextSectionUsageView {
                kind: section.kind,
                estimated_tokens: section.estimated_tokens,
            })
            .collect();
        Ok(ContextStatusView {
            session_id: self.session_id,
            agent_run_id: self.agent_run_id,
            current_context_snapshot_id: Some(snapshot.id),
            prompt_bundle_snapshot_id: Some(snapshot.prompt_bundle_snapshot_id),
            runtime_effective_window_tokens: snapshot.budget.runtime_effective_window_tokens,
            request_input_budget_tokens: snapshot.budget.request_input_budget_tokens,
            estimated_input_tokens: snapshot.budget.estimated_input_tokens,
            section_usage,
            compaction_state: inspector
                .as_ref()
                .map_or(ContextCompactionStateView::NotRequired, |value| value.compaction.state),
            retention_exceptions: inspector
                .as_ref()
                .map_or_else(Vec::new, |value| value.compaction.retention_exceptions.clone()),
            active_task_lineage: inspector
                .as_ref()
                .and_then(|value| value.active_task_lineage.as_ref())
                .map(task_lineage_status),
            overflow: inspector.and_then(|value| value.overflow),
        })
    }

    pub fn search(
        &self,
        input: ContextSearchInput,
    ) -> Result<ContextSearchResult, ContextToolError> {
        self.validate_scope(input.session_id, input.agent_run_id)?;
        let fts_query = build_fts_query(&input.query, input.mode)?;
        if input.limit == 0 || input.limit > SEARCH_LIMIT_MAX {
            return Err(ContextToolError::InvalidRequest);
        }
        if input.filters.agent_run_id.is_some_and(|value| value != self.agent_run_id) {
            return Err(ContextToolError::ScopeMismatch);
        }
        let request_hash = canonical_json_v1_sha256(&SearchCursorBinding {
            query: input.query.clone(),
            mode: input.mode,
            filters: input.filters.clone(),
        })
        .map_err(|_| ContextToolError::InvalidRequest)?;
        let cursor = input.cursor.as_deref().map(decode_cursor::<SearchCursor>).transpose()?;
        let (cutoff, mut after) = if let Some(cursor) = cursor {
            validate_cursor_header(
                &cursor.header,
                CursorOperation::Search,
                self.session_id,
                self.agent_run_id,
                &request_hash,
            )?;
            (Some(cursor.cutoff), Some((cursor.last_sequence, cursor.last_chunk_id)))
        } else {
            (None, None)
        };
        let mut effective_cutoff = cutoff;
        let mut matches = Vec::new();
        let requested =
            usize::try_from(input.limit).map_err(|_| ContextToolError::InvalidRequest)?;
        let mut exhausted = false;
        let mut index_complete_through = SessionSequence::new(0);
        while matches.len() < requested && !exhausted {
            let remaining = requested - matches.len();
            let fetch_limit = remaining.saturating_mul(4).clamp(32, 512);
            let page = self.persistence.search_context_rows(ContextSearchStoredQuery {
                session_id: self.session_id,
                agent_run_id: self.agent_run_id,
                fts_query: fts_query.clone(),
                filters: input.filters.clone(),
                cutoff: effective_cutoff,
                after,
                fetch_limit,
            })?;
            effective_cutoff = Some(page.cutoff);
            index_complete_through = page.index_complete_through;
            if page.rows.is_empty() {
                if let Some(last_scanned) = page.last_scanned {
                    after = Some(last_scanned);
                    continue;
                }
                exhausted = true;
                continue;
            }
            for row in page.rows {
                if matches.len() == requested {
                    break;
                }
                after = Some((row.session_sequence, row.chunk_id));
                matches.push(search_match(row, &input.query, input.mode));
            }
            if matches.len() < requested {
                after = page.last_scanned.or(after);
                exhausted = page.last_scanned.is_none();
            }
        }
        let next_cursor = if matches.len() == requested && !exhausted {
            let cutoff = effective_cutoff.ok_or(ContextToolError::PersistedFactInvalid)?;
            after
                .map(|(last_sequence, last_chunk_id)| {
                    encode_cursor(&SearchCursor {
                        header: CursorHeader::new(
                            CursorOperation::Search,
                            self.session_id,
                            self.agent_run_id,
                            request_hash,
                        ),
                        cutoff,
                        last_sequence,
                        last_chunk_id,
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(ContextSearchResult { matches, next_cursor, index_complete_through })
    }

    pub fn read_events(
        &self,
        input: ContextReadEventsInput,
    ) -> Result<ContextReadEventsResult, ContextToolError> {
        self.validate_scope(input.session_id, input.agent_run_id)?;
        if input.selectors.is_empty()
            || input.selectors.len() > EVENT_SELECTOR_MAX
            || input.character_limit == 0
            || input.character_limit > EVENT_CHARACTER_HARD_LIMIT
        {
            return Err(ContextToolError::InvalidRequest);
        }
        let request_hash = canonical_json_v1_sha256(&EventsCursorBinding {
            selectors: input.selectors.clone(),
            character_limit: input.character_limit,
        })
        .map_err(|_| ContextToolError::InvalidRequest)?;
        let cursor = input.cursor.as_deref().map(decode_cursor::<EventCursor>).transpose()?;
        let (cutoff, after) = if let Some(cursor) = cursor {
            validate_cursor_header(
                &cursor.header,
                CursorOperation::ReadEvents,
                self.session_id,
                self.agent_run_id,
                &request_hash,
            )?;
            (cursor.cutoff, Some(cursor.last_sequence))
        } else {
            (self.persistence.context_event_sequence_cutoff(self.session_id)?, None)
        };
        let query = event_query(self.session_id, self.agent_run_id, cutoff, &input.selectors)?;
        let rows = self.persistence.context_event_rows(query)?;
        let units = atomic_event_units(rows, after);
        let mut events = Vec::new();
        let mut character_count = 0_u64;
        let mut last_sequence = after;
        let mut has_more = false;
        for unit in units {
            if unit.len() > EVENT_EXPANDED_MAX {
                return Err(ContextToolError::ResultTooLarge);
            }
            let unit_characters = unit.iter().try_fold(0_u64, |total, row| {
                total.checked_add(u64::try_from(row.model_visible_content.chars().count()).ok()?)
            });
            let Some(unit_characters) = unit_characters else {
                return Err(ContextToolError::ResultTooLarge);
            };
            if unit_characters > EVENT_CHARACTER_HARD_LIMIT {
                return Err(ContextToolError::ResultTooLarge);
            }
            if !events.is_empty()
                && (character_count.saturating_add(unit_characters) > input.character_limit
                    || events.len().saturating_add(unit.len()) > EVENT_EXPANDED_MAX)
            {
                has_more = true;
                break;
            }
            character_count = character_count
                .checked_add(unit_characters)
                .ok_or(ContextToolError::ResultTooLarge)?;
            for row in unit {
                last_sequence = Some(row.session_sequence);
                events.push(event_view::whole(row));
            }
        }
        let next_cursor = if has_more {
            last_sequence
                .map(|last_sequence| {
                    encode_cursor(&EventCursor {
                        header: CursorHeader::new(
                            CursorOperation::ReadEvents,
                            self.session_id,
                            self.agent_run_id,
                            request_hash,
                        ),
                        cutoff,
                        last_sequence,
                    })
                })
                .transpose()?
        } else {
            None
        };
        Ok(ContextReadEventsResult { events, next_cursor, character_count })
    }

    pub fn read_tool_result(
        &self,
        input: ContextReadToolResultInput,
    ) -> Result<ContextReadToolResultResult, ContextToolError> {
        self.validate_scope(input.session_id, input.agent_run_id)?;
        let stored = self
            .persistence
            .context_tool_result(self.session_id, self.agent_run_id, input.tool_call_id)?
            .ok_or(ContextToolError::SourceNotFound)?;
        let request_hash = canonical_json_v1_sha256(&ToolResultCursorBinding {
            tool_call_id: input.tool_call_id,
            include_input: input.include_input,
            content_hash: stored.content_hash.clone(),
        })
        .map_err(|_| ContextToolError::InvalidRequest)?;
        let cursor = input.cursor.as_deref().map(decode_cursor::<ToolResultCursor>).transpose()?;
        let offset = if let Some(cursor) = cursor {
            validate_cursor_header(
                &cursor.header,
                CursorOperation::ReadToolResult,
                self.session_id,
                self.agent_run_id,
                &request_hash,
            )?;
            cursor.byte_offset
        } else {
            0
        };
        let (result, next_offset) = take_text_page(&stored.result, offset, TOOL_RESULT_PAGE_CHARS)?;
        let next_cursor = next_offset
            .map(|byte_offset| {
                encode_cursor(&ToolResultCursor {
                    header: CursorHeader::new(
                        CursorOperation::ReadToolResult,
                        self.session_id,
                        self.agent_run_id,
                        request_hash,
                    ),
                    byte_offset,
                })
            })
            .transpose()?;
        Ok(ContextReadToolResultResult {
            value: ContextReadToolResultView {
                tool_call_id: stored.tool_call_id,
                tool_name: stored.tool_name,
                input: (input.include_input && offset == 0).then_some(stored.input).flatten(),
                result,
                is_error: stored.is_error,
                content_hash: stored.content_hash,
            },
            next_cursor,
        })
    }

    pub fn read_compaction(
        &self,
        input: ContextReadCompactionInput,
    ) -> Result<ContextReadCompactionResult, ContextToolError> {
        self.validate_scope(input.session_id, input.agent_run_id)?;
        let stored = self
            .persistence
            .context_compaction(
                self.session_id,
                self.agent_run_id,
                input.source_context_snapshot_id,
                input.sequence,
            )?
            .ok_or(ContextToolError::SourceNotFound)?;
        if stored.state != ContextCompactionStateView::Completed {
            return Err(ContextToolError::SourceNotFound);
        }
        self.materialize_compaction(stored)
    }

    fn materialize_compaction(
        &self,
        stored: ContextCompactionStoredRecord,
    ) -> Result<ContextReadCompactionResult, ContextToolError> {
        let record = stored.record.ok_or(ContextToolError::PersistedFactInvalid)?;
        if record.source_span != stored.source_span {
            return Err(ContextToolError::PersistedFactInvalid);
        }
        let historical_snapshot = self.read_text_artifact(
            stored.historical_summary_artifact_id,
            "text/markdown; charset=utf-8",
            &record.historical_summary_hash,
        )?;
        let active_turn_summary = self.read_text_artifact(
            stored.active_turn_summary_artifact_id,
            "text/markdown; charset=utf-8",
            &record.active_turn_summary_hash,
        )?;
        let structured_compaction = self.read_text_artifact(
            stored.structured_compaction_artifact_id,
            "text/markdown; charset=utf-8",
            &record.structured_markdown_hash,
        )?;
        let anchor_text =
            self.read_artifact(stored.anchor_manifest_artifact_id, "application/json")?;
        let anchor_manifest = serde_json::from_slice::<CompactionAnchorManifest>(&anchor_text)
            .map_err(|_| ContextToolError::PersistedFactInvalid)?;
        if canonical_json_v1_sha256(&anchor_manifest.anchors)
            .map_err(|_| ContextToolError::PersistedFactInvalid)?
            != record.anchor_manifest_hash
            || anchor_manifest.manifest_hash != record.anchor_manifest_hash
        {
            return Err(ContextToolError::PersistedFactInvalid);
        }
        Ok(ContextReadCompactionResult {
            source_context_snapshot_id: stored.source_context_snapshot_id,
            sequence: stored.sequence,
            historical_snapshot,
            active_turn_summary,
            structured_compaction,
            source_span: stored.source_span,
            anchor_manifest,
        })
    }

    fn read_text_artifact(
        &self,
        artifact_id: Option<aura_domain_common::ArtifactId>,
        media_type: &str,
        expected_hash: &ContentHash,
    ) -> Result<String, ContextToolError> {
        let bytes = self.read_artifact(artifact_id, media_type)?;
        if ContentHash::sha256(&bytes) != *expected_hash {
            return Err(ContextToolError::PersistedFactInvalid);
        }
        String::from_utf8(bytes).map_err(|_| ContextToolError::PersistedFactInvalid)
    }

    fn read_artifact(
        &self,
        artifact_id: Option<aura_domain_common::ArtifactId>,
        media_type: &str,
    ) -> Result<Vec<u8>, ContextToolError> {
        let artifact_id = artifact_id.ok_or(ContextToolError::PersistedFactInvalid)?;
        let metadata =
            self.persistence.context_internal_artifact_metadata(self.session_id, artifact_id)?;
        if metadata.media_type != media_type {
            return Err(ContextToolError::PersistedFactInvalid);
        }
        let capacity =
            usize::try_from(metadata.byte_length).map_err(|_| ContextToolError::ResultTooLarge)?;
        let mut bytes = Vec::with_capacity(capacity);
        let mut offset = 0_u64;
        while offset < metadata.byte_length {
            let remaining = metadata.byte_length - offset;
            let length = usize::try_from(remaining.min(ARTIFACT_RANGE_LIMIT as u64))
                .map_err(|_| ContextToolError::ResultTooLarge)?;
            let chunk = self.persistence.read_context_internal_artifact_range(
                self.session_id,
                artifact_id,
                offset,
                length,
            )?;
            if chunk.is_empty() {
                return Err(ContextToolError::PersistedFactInvalid);
            }
            offset = offset
                .checked_add(
                    u64::try_from(chunk.len()).map_err(|_| ContextToolError::ResultTooLarge)?,
                )
                .ok_or(ContextToolError::ResultTooLarge)?;
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != capacity || ContentHash::blake3(&bytes) != metadata.content_hash {
            return Err(ContextToolError::PersistedFactInvalid);
        }
        Ok(bytes)
    }

    fn validate_scope(
        &self,
        session_id: SessionId,
        agent_run_id: AgentRunId,
    ) -> Result<(), ContextToolError> {
        if session_id != self.session_id || agent_run_id != self.agent_run_id {
            return Err(ContextToolError::ScopeMismatch);
        }
        Ok(())
    }
}

pub trait ModelEgressPolicyAuthority: Send + Sync {
    fn session_model_channel_is_authorized(
        &self,
        session_id: SessionId,
        model_profile_id: ModelProfileId,
    ) -> bool;

    fn source_grant_binding_is_valid(
        &self,
        session_id: SessionId,
        source_ref: &ContextSourceRef,
        source_kind: ModelEgressSourceKind,
        data_class: NetworkDataClass,
        binding: &SourceAuthorizationBinding,
    ) -> Result<bool, ModelEgressAuthorizationError>;
}

pub struct PersistenceModelEgressFacts<A> {
    persistence: PersistenceCoordinator,
    authority: A,
}

impl<A> PersistenceModelEgressFacts<A> {
    #[must_use]
    pub const fn new(persistence: PersistenceCoordinator, authority: A) -> Self {
        Self { persistence, authority }
    }
}

impl<A> ModelEgressAuthorizationFacts for PersistenceModelEgressFacts<A>
where
    A: ModelEgressPolicyAuthority,
{
    fn session_model_channel_is_authorized(
        &self,
        session_id: SessionId,
        model_profile_id: ModelProfileId,
    ) -> bool {
        self.authority.session_model_channel_is_authorized(session_id, model_profile_id)
    }

    async fn persisted_event(
        &self,
        event_id: aura_domain_common::EventId,
    ) -> Result<Option<PersistedContextEventAuthorizationFact>, ModelEgressAuthorizationError> {
        self.persistence
            .context_source_event_fact(event_id)
            .map_err(|_| ModelEgressAuthorizationError::FactStore)
            .map(|fact| {
                fact.map(|fact| PersistedContextEventAuthorizationFact {
                    event_id: fact.event_id,
                    session_id: fact.session_id,
                    actor: match fact.actor {
                        aura_persistence::EventActor::User => PersistedContextEventActor::User,
                        aura_persistence::EventActor::ParentAgent => {
                            PersistedContextEventActor::ParentAgent
                        }
                        aura_persistence::EventActor::ChildAgent => {
                            PersistedContextEventActor::ChildAgent
                        }
                        aura_persistence::EventActor::SystemRuntime => {
                            PersistedContextEventActor::SystemRuntime
                        }
                    },
                    content_hash: fact.content_hash,
                    byte_length: fact.byte_length,
                    model_visible: fact.model_visible,
                    explicitly_user_authored: fact.explicitly_user_authored,
                })
            })
    }

    fn source_grant_binding_is_valid(
        &self,
        session_id: SessionId,
        source_ref: &ContextSourceRef,
        source_kind: ModelEgressSourceKind,
        data_class: NetworkDataClass,
        binding: &SourceAuthorizationBinding,
    ) -> Result<bool, ModelEgressAuthorizationError> {
        self.authority.source_grant_binding_is_valid(
            session_id,
            source_ref,
            source_kind,
            data_class,
            binding,
        )
    }
}

#[derive(Debug, Error)]
pub enum AttachmentResolutionError {
    #[error("context.attachment_egress_not_authorized")]
    Egress(#[from] ModelEgressAuthorizationError),
    #[error("context.attachment_manifest_mismatch")]
    ManifestMismatch,
    #[error("context.attachment_access_denied")]
    AccessDenied,
    #[error("context.attachment_snapshot_mismatch")]
    SnapshotMismatch,
    #[error("context.attachment_too_large")]
    TooLarge,
    #[error("context.attachment_persistence_failed")]
    Persistence(#[from] PersistenceError),
}

pub async fn authorize_and_resolve_attachments<F>(
    persistence: &PersistenceCoordinator,
    facts: &F,
    session_id: SessionId,
    model_profile_id: ModelProfileId,
    request: &LogicalModelRequest,
    manifest: &ModelEgressManifestV1,
) -> Result<Vec<ResolvedModelAttachment>, AttachmentResolutionError>
where
    F: ModelEgressAuthorizationFacts,
{
    if manifest.context_snapshot_id != request.context_snapshot_id {
        return Err(AttachmentResolutionError::ManifestMismatch);
    }
    ModelEgressAuthorizer::new(facts).validate(session_id, model_profile_id, manifest).await?;
    let mut resolved = Vec::with_capacity(request.attachments.len());
    for snapshot in &request.attachments {
        let matching = manifest
            .items
            .iter()
            .filter(|item| {
                item.source_ref == ContextSourceRef::Artifact { artifact_id: snapshot.artifact_id }
                    && item.content_hash == snapshot.content_hash
                    && item.byte_length == snapshot.byte_length
                    && matches!(
                        item.source_kind,
                        ModelEgressSourceKind::ExternalFileAttachment
                            | ModelEgressSourceKind::ArtifactAttachment
                            | ModelEgressSourceKind::BrowserAttachment
                            | ModelEgressSourceKind::DiagnosticAttachment
                    )
                    && matches!(
                        item.authorization_basis,
                        ModelEgressAuthorizationBasis::SourceGrants { .. }
                    )
            })
            .count();
        if matching != 1 {
            return Err(AttachmentResolutionError::ManifestMismatch);
        }
        let record = persistence
            .artifact_record(ArtifactMetadataRequest {
                artifact_id: snapshot.artifact_id,
                session_id: Some(session_id),
                required_access: ArtifactAccess::MODEL_READ,
                allow_quarantine: false,
            })
            .map_err(map_attachment_persistence_error)?;
        if record.media_type != snapshot.media_type || record.byte_length != snapshot.byte_length {
            return Err(AttachmentResolutionError::SnapshotMismatch);
        }
        let capacity =
            usize::try_from(record.byte_length).map_err(|_| AttachmentResolutionError::TooLarge)?;
        let mut bytes = Vec::with_capacity(capacity);
        let mut offset = 0_u64;
        while offset < record.byte_length {
            let length =
                usize::try_from((record.byte_length - offset).min(ARTIFACT_RANGE_LIMIT as u64))
                    .map_err(|_| AttachmentResolutionError::TooLarge)?;
            let chunk = persistence
                .read_artifact_range(ArtifactReadRequest {
                    artifact_id: snapshot.artifact_id,
                    session_id: Some(session_id),
                    required_access: ArtifactAccess::MODEL_READ,
                    offset,
                    length,
                    allow_quarantine: false,
                })
                .map_err(map_attachment_persistence_error)?;
            if chunk.is_empty() {
                return Err(AttachmentResolutionError::SnapshotMismatch);
            }
            offset = offset
                .checked_add(
                    u64::try_from(chunk.len()).map_err(|_| AttachmentResolutionError::TooLarge)?,
                )
                .ok_or(AttachmentResolutionError::TooLarge)?;
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() != capacity || ContentHash::sha256(&bytes) != snapshot.content_hash {
            return Err(AttachmentResolutionError::SnapshotMismatch);
        }
        resolved.push(ResolvedModelAttachment { snapshot: snapshot.clone(), bytes: bytes.into() });
    }
    validate_resolved_attachments(request, &resolved)
        .map_err(|_| AttachmentResolutionError::SnapshotMismatch)?;
    Ok(resolved)
}

fn map_attachment_persistence_error(error: PersistenceError) -> AttachmentResolutionError {
    match error {
        PersistenceError::ArtifactAccessDenied
        | PersistenceError::QuarantineAccessRequired
        | PersistenceError::ArtifactNotFound => AttachmentResolutionError::AccessDenied,
        other => AttachmentResolutionError::Persistence(other),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CursorHeader {
    schema_version: u32,
    operation: CursorOperation,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    request_hash: ContentHash,
}

impl CursorHeader {
    fn new(
        operation: CursorOperation,
        session_id: SessionId,
        agent_run_id: AgentRunId,
        request_hash: ContentHash,
    ) -> Self {
        Self {
            schema_version: CURSOR_SCHEMA_VERSION,
            operation,
            session_id,
            agent_run_id,
            request_hash,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CursorOperation {
    Search,
    ReadEvents,
    ReadToolResult,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SearchCursor {
    header: CursorHeader,
    cutoff: SessionSequence,
    last_sequence: SessionSequence,
    last_chunk_id: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EventCursor {
    header: CursorHeader,
    cutoff: SessionSequence,
    last_sequence: SessionSequence,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolResultCursor {
    header: CursorHeader,
    byte_offset: usize,
}

#[derive(Serialize)]
struct SearchCursorBinding {
    query: String,
    mode: ContextSearchMode,
    filters: aura_context_domain::ContextSearchFilters,
}

#[derive(Serialize)]
struct EventsCursorBinding {
    selectors: Vec<ContextEventSelector>,
    character_limit: u64,
}

#[derive(Serialize)]
struct ToolResultCursorBinding {
    tool_call_id: aura_domain_common::ToolCallId,
    include_input: bool,
    content_hash: ContentHash,
}

fn validate_cursor_header(
    header: &CursorHeader,
    operation: CursorOperation,
    session_id: SessionId,
    agent_run_id: AgentRunId,
    request_hash: &ContentHash,
) -> Result<(), ContextToolError> {
    if header.schema_version != CURSOR_SCHEMA_VERSION
        || header.operation != operation
        || header.session_id != session_id
        || header.agent_run_id != agent_run_id
        || &header.request_hash != request_hash
    {
        return Err(ContextToolError::CursorInvalid);
    }
    Ok(())
}

fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, ContextToolError> {
    let bytes = canonical_json_v1(cursor).map_err(|_| ContextToolError::CursorInvalid)?;
    let mut encoded = String::with_capacity(CURSOR_PREFIX.len() + bytes.len() * 2);
    encoded.push_str(CURSOR_PREFIX);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").map_err(|_| ContextToolError::CursorInvalid)?;
    }
    Ok(encoded)
}

fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, ContextToolError> {
    let encoded = cursor.strip_prefix(CURSOR_PREFIX).ok_or(ContextToolError::CursorInvalid)?;
    if encoded.is_empty() || !encoded.len().is_multiple_of(2) {
        return Err(ContextToolError::CursorInvalid);
    }
    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).map_err(|_| ContextToolError::CursorInvalid)?;
        bytes.push(u8::from_str_radix(pair, 16).map_err(|_| ContextToolError::CursorInvalid)?);
    }
    serde_json::from_slice(&bytes).map_err(|_| ContextToolError::CursorInvalid)
}

fn build_fts_query(query: &str, mode: ContextSearchMode) -> Result<String, ContextToolError> {
    let query = query.trim();
    if query.is_empty() || query.contains('\0') || query.contains('\r') {
        return Err(ContextToolError::InvalidRequest);
    }
    let quote = |value: &str| format!("\"{}\"", value.replace('"', "\"\""));
    match mode {
        ContextSearchMode::ExactPhrase => Ok(quote(query)),
        ContextSearchMode::Natural => {
            let terms = query.split_whitespace().map(quote).collect::<Vec<_>>();
            if terms.is_empty() {
                Err(ContextToolError::InvalidRequest)
            } else {
                Ok(terms.join(" AND "))
            }
        }
    }
}

fn search_match(
    row: aura_persistence::ContextSearchStoredRow,
    query: &str,
    mode: ContextSearchMode,
) -> ContextSearchMatch {
    let needle = match mode {
        ContextSearchMode::ExactPhrase => query.trim(),
        ContextSearchMode::Natural => query.split_whitespace().next().unwrap_or(query),
    };
    let (match_start, match_end) = find_match_char_range(&row.searchable_text, needle);
    let excerpt_start = match_start.saturating_sub(SEARCH_EXCERPT_CHARS / 3);
    let excerpt_end =
        excerpt_start.saturating_add(SEARCH_EXCERPT_CHARS).min(row.searchable_text.chars().count());
    let excerpt = row
        .searchable_text
        .chars()
        .skip(excerpt_start)
        .take(excerpt_end.saturating_sub(excerpt_start))
        .collect::<String>();
    ContextSearchMatch {
        source_type: row.source_type,
        source_id: row.source_id,
        sequence: row.session_sequence,
        excerpt,
        match_start: u64::try_from(match_start.saturating_sub(excerpt_start)).unwrap_or(u64::MAX),
        match_end: u64::try_from(match_end.saturating_sub(excerpt_start)).unwrap_or(u64::MAX),
    }
}

fn find_match_char_range(haystack: &str, needle: &str) -> (usize, usize) {
    if needle.is_empty() {
        return (0, 0);
    }
    if let Some(byte_start) = haystack.find(needle) {
        let start = haystack[..byte_start].chars().count();
        return (start, start + needle.chars().count());
    }
    let lower_haystack = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    if let Some(byte_start) = lower_haystack.find(&lower_needle) {
        let start = lower_haystack[..byte_start].chars().count();
        return (start, start + lower_needle.chars().count());
    }
    (0, needle.chars().count().min(haystack.chars().count()))
}

fn event_query(
    session_id: SessionId,
    agent_run_id: AgentRunId,
    cutoff: SessionSequence,
    selectors: &[ContextEventSelector],
) -> Result<ContextEventStoredQuery, ContextToolError> {
    let mut event_ids = Vec::new();
    let mut sequence_range: Option<(SessionSequence, SessionSequence)> = None;
    let mut turn_task_ids = Vec::new();
    let mut history_atomic_group_refs = Vec::new();
    let mut atomic_group_ids = Vec::new();
    for selector in selectors {
        match selector {
            ContextEventSelector::Event { event_id } => event_ids.push(*event_id),
            ContextEventSelector::SequenceRange { start, end } => {
                if start > end || sequence_range.is_some() {
                    return Err(ContextToolError::InvalidRequest);
                }
                sequence_range = Some((*start, *end));
            }
            ContextEventSelector::TurnTask { turn_task_id } => turn_task_ids.push(*turn_task_id),
            ContextEventSelector::HistoryAtomicGroup { group_ref } => {
                history_atomic_group_refs.push(group_ref.clone());
            }
            ContextEventSelector::AtomicGroup { atomic_group_id } => {
                if atomic_group_id.trim().is_empty() {
                    return Err(ContextToolError::InvalidRequest);
                }
                atomic_group_ids.push(atomic_group_id.clone());
            }
        }
    }
    event_ids.sort();
    event_ids.dedup();
    turn_task_ids.sort();
    turn_task_ids.dedup();
    history_atomic_group_refs.sort_by(|left, right| {
        left.assistant_response_event_id
            .cmp(&right.assistant_response_event_id)
            .then_with(|| left.group_hash.hex().cmp(right.group_hash.hex()))
    });
    history_atomic_group_refs.dedup();
    atomic_group_ids.sort();
    atomic_group_ids.dedup();
    Ok(ContextEventStoredQuery {
        session_id,
        agent_run_id,
        cutoff,
        event_ids,
        sequence_range,
        turn_task_ids,
        history_atomic_group_refs,
        atomic_group_ids,
    })
}

fn atomic_event_units(
    rows: Vec<ContextEventStoredRow>,
    after: Option<SessionSequence>,
) -> Vec<Vec<ContextEventStoredRow>> {
    let mut grouped = BTreeMap::<String, Vec<ContextEventStoredRow>>::new();
    for row in &rows {
        if let Some(group) = context_event_group_key(row) {
            grouped.entry(group).or_default().push(row.clone());
        }
    }
    let mut emitted = BTreeSet::new();
    let mut units = Vec::new();
    for row in rows {
        if after.is_some_and(|after| row.session_sequence <= after) {
            continue;
        }
        if let Some(group) = context_event_group_key(&row) {
            if emitted.insert(group.clone()) {
                units.push(grouped.remove(&group).unwrap_or_default());
            }
        } else {
            units.push(vec![row]);
        }
    }
    units
}

fn context_event_group_key(row: &ContextEventStoredRow) -> Option<String> {
    row.history_atomic_group_ref
        .as_ref()
        .map(|group_ref| {
            format!("v1:{}:{}", group_ref.assistant_response_event_id, group_ref.group_hash.hex())
        })
        .or_else(|| row.atomic_group_id.as_ref().map(|group| format!("v0:{group}")))
}

fn take_text_page(
    text: &str,
    byte_offset: usize,
    character_limit: usize,
) -> Result<(String, Option<usize>), ContextToolError> {
    if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
        return Err(ContextToolError::CursorInvalid);
    }
    let remainder = &text[byte_offset..];
    let mut end = remainder.len();
    if let Some((index, _)) = remainder.char_indices().nth(character_limit) {
        end = index;
    }
    let next = (end < remainder.len())
        .then(|| byte_offset.checked_add(end).ok_or(ContextToolError::ResultTooLarge))
        .transpose()?;
    Ok((remainder[..end].to_owned(), next))
}

fn task_lineage_status(
    value: &aura_context_domain::PromptInspectorTaskLineageView,
) -> TaskLineageContextStatusView {
    TaskLineageContextStatusView {
        lineage_id: value.lineage_id,
        revision: value.revision,
        state: value.state.clone(),
    }
}

#[cfg(test)]
mod tests {
    use aura_context_domain::{ContextSearchFilters, ContextSearchMode};
    use aura_domain_common::{AgentRunId, ContentHash, SessionId, SessionSequence};

    use super::*;

    #[test]
    fn cursors_are_canonical_and_bound_to_request_identity() {
        let session_id = SessionId::new();
        let agent_run_id = AgentRunId::new();
        let request_hash = ContentHash::sha256(b"request");
        let cursor = SearchCursor {
            header: CursorHeader::new(
                CursorOperation::Search,
                session_id,
                agent_run_id,
                request_hash.clone(),
            ),
            cutoff: SessionSequence::new(42),
            last_sequence: SessionSequence::new(21),
            last_chunk_id: 7,
        };
        let encoded = encode_cursor(&cursor).expect("cursor");
        let decoded = decode_cursor::<SearchCursor>(&encoded).expect("decoded");
        validate_cursor_header(
            &decoded.header,
            CursorOperation::Search,
            session_id,
            agent_run_id,
            &request_hash,
        )
        .expect("bound");
        assert_eq!(decoded.cutoff, SessionSequence::new(42));
        assert_eq!(
            validate_cursor_header(
                &decoded.header,
                CursorOperation::ReadEvents,
                session_id,
                agent_run_id,
                &request_hash,
            ),
            Err(ContextToolError::CursorInvalid)
        );
    }

    #[test]
    fn fts_modes_quote_input_without_exposing_a_dsl() {
        assert_eq!(
            build_fts_query("alpha beta", ContextSearchMode::Natural).expect("natural"),
            "\"alpha\" AND \"beta\""
        );
        assert_eq!(
            build_fts_query("alpha beta", ContextSearchMode::ExactPhrase).expect("phrase"),
            "\"alpha beta\""
        );
        let _filters = ContextSearchFilters::default();
    }

    #[test]
    fn text_pages_preserve_utf8_boundaries() {
        let text = format!("{}end", "你".repeat(65_536));
        let (first, next) = take_text_page(&text, 0, 65_536).expect("first page");
        assert_eq!(first.chars().count(), 65_536);
        let (second, end) = take_text_page(&text, next.expect("cursor"), 65_536).expect("second");
        assert_eq!(second, "end");
        assert_eq!(end, None);
    }
}
