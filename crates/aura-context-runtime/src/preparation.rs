use std::collections::{BTreeMap, BTreeSet};

use aura_context_domain::{
    ContextBudgetError, ContextBudgetSnapshot, ContextCompactionStateView, ContextCompressibility,
    ContextOverflowDiagnosticView, ContextPreparationRequest, ContextPreparationTriggerV1,
    ContextProjectionSnapshot, ContextRemediationPlan, ContextSectionKind, ContextSectionSnapshot,
    ContextSnapshotError, ContextSourceRef, ContextUsageKind, ContextUsageSegmentView,
    ContextUsageSummaryView, HistoryAtomicGroupPreimageV1, ModelEgressAuthorizationBasis,
    ModelEgressContractError, ModelEgressManifestV1, ModelEgressSourceKind, PreparedLogicalContext,
    PromptBundleSnapshot, PromptInspectorAttachmentView, PromptInspectorBudgetView,
    PromptInspectorCacheUsageView, PromptInspectorCompactionView, PromptInspectorEgressClassView,
    PromptInspectorModelEgressView, PromptInspectorProjection, PromptInspectorSectionPlacement,
    PromptInspectorSectionView, PromptInspectorSkillView, PromptInspectorTaskLineageView,
    PromptInspectorToolView, ProviderContinuationBindingV1,
};
use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, EndpointId, EventId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId,
    Revision, SessionId, TurnTaskId, UtcTimestamp, canonical_json_v1,
};
use aura_domain_common::{ToolCallId, ToolId};
use aura_model_domain::{
    ContextWindowResolution, LogicalConversationItem, ModelApiFormat, ModelAttachmentSnapshot,
    ModelExecutionSnapshot, ProviderContinuationState,
};
use aura_security_contract::NetworkDataClass;
use aura_tool_contract::{EffectiveToolBindingSnapshot, ToolSchemaSetSnapshotV1};
use thiserror::Error;

use crate::attachment_sources::validate_attachment_sources;
use crate::current_user::{current_user_content, input_batch_content};
use crate::{
    CanonicalTextBudgetItem, ContextEstimatorRegistry, ContextSectionAssembly, ContextSectionError,
    ContextSectionInput, MeasuredRequest, ModelEgressSourceDraft, ModelEstimatorKey,
    NativeAttachmentBudgetItem, assemble_context_sections, build_model_egress_manifest,
};

#[derive(Clone, Debug)]
pub struct ContextToolInput {
    pub tool_id: ToolId,
    pub compressibility: ContextCompressibility,
}

#[derive(Clone, Debug)]
pub struct PromptInspectorInput {
    pub skills: Vec<PromptInspectorSkillView>,
    pub active_task_lineage: Option<PromptInspectorTaskLineageView>,
    pub compaction: PromptInspectorCompactionView,
    pub cache_usage: PromptInspectorCacheUsageView,
    pub active_tool_call_ids: Vec<ToolCallId>,
    /// The frozen activity projection, supplied by the caller that sampled it.
    ///
    /// It is `None` when this step produced no activity fragment. Because the caller froze the
    /// snapshot once for the whole model step, the view here and the fragment in the request
    /// body describe the same instant - they are not two reads that could disagree.
    pub activity: Option<aura_context_domain::PromptInspectorActivityView>,
    pub attachments_resolved: bool,
}

impl Default for PromptInspectorInput {
    fn default() -> Self {
        Self {
            skills: Vec::new(),
            active_task_lineage: None,
            compaction: PromptInspectorCompactionView {
                state: ContextCompactionStateView::NotRequired,
                source_context_snapshot_id: None,
                retained_recent_tokens: 0,
                retention_exceptions: Vec::new(),
            },
            cache_usage: PromptInspectorCacheUsageView {
                actual_input_tokens: None,
                actual_cached_input_tokens: None,
                stable_prefix_estimate_tokens: 0,
            },
            active_tool_call_ids: Vec::new(),
            activity: None,
            attachments_resolved: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ContextPreparationInput {
    pub request: ContextPreparationRequest,
    pub prompt_bundle_snapshot: PromptBundleSnapshot,
    pub active_prompt_module_ids: Vec<String>,
    pub section_inputs: Vec<ContextSectionInput>,
    pub recent_history_items: Vec<LogicalConversationItem>,
    pub trigger: ContextPreparationTriggerV1,
    pub tool_continuation_facts: Option<ToolContinuationFactsV1>,
    pub core_continuation_facts: Option<CoreContinuationFactsV1>,
    pub tool_schema_set: ToolSchemaSetSnapshotV1,
    pub effective_tool_bindings: Vec<EffectiveToolBindingSnapshot>,
    pub tools: Vec<ContextToolInput>,
    pub attachments: Vec<ModelAttachmentSnapshot>,
    pub attachment_egress_sources: Vec<ModelEgressSourceDraft>,
    pub model_estimator_key: ModelEstimatorKey,
    /// Exact target selected by Core before P06 compilation. It lets P06
    /// reject Provider continuation state that belongs to another Provider,
    /// API format, Endpoint, Profile, or Endpoint fingerprint.
    pub provider_target: ContextProviderTargetV1,
    pub inspector: PromptInspectorInput,
    pub previous_snapshot: Option<ContextProjectionSnapshot>,
    /// The provider's count of this model's previous request ([`MeasuredRequest`]).
    pub previous_measurement: Option<MeasuredRequest>,
    pub created_at: UtcTimestamp,
    /// Bound from stored user text before compaction removes that text from the
    /// model-visible history. A summary string is not one of these proofs.
    pub compacted_source_proofs: Vec<aura_context_domain::UserSourceProof>,
    /// Earlier compaction hops the visible summary may follow. They are not
    /// copied back into the model-visible sections.
    pub coverage_hops: Vec<aura_context_domain::CoverageHop>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextProviderTargetV1 {
    pub provider_id: ProviderId,
    pub api_format: ModelApiFormat,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub endpoint_fingerprint: ContentHash,
}

/// Core-owned facts loaded from durable P05/P07/P08 records. Callers outside
/// that trusted assembly path cannot use the trigger alone to authorize a Tool
/// continuation.
#[derive(Clone, Debug)]
pub struct ToolContinuationFactsV1 {
    pub history_atomic_group: HistoryAtomicGroupPreimageV1,
    pub previous_model_step_legally_terminal: bool,
    pub assistant_response_durable: bool,
    pub tool_batch_terminal: bool,
    pub durable_result_tool_call_ids: Vec<ToolCallId>,
    pub verified_provider_continuation: Option<ProviderContinuationBindingV1>,
    /// Additions consumed for this turn that belong after the completed group.
    /// Only this ordered suffix may follow the group; anything else is rejected.
    pub accepted_additions: Vec<AcceptedTurnAdditionV1>,
}

/// Reloaded from the Agent Store checkpoint, never accepted from a model DTO.
#[derive(Clone, Debug)]
pub struct CoreContinuationFactsV1 {
    pub session_id: aura_domain_common::SessionId,
    pub turn_task_id: aura_domain_common::TurnTaskId,
    pub agent_run_id: aura_domain_common::AgentRunId,
    pub previous_model_step_id: ModelStepId,
    pub assistant_response_event_id: EventId,
    pub checkpoint_hash: aura_domain_common::jcs::CanonicalSha256HashV1,
    /// Whether the continued response carried model-visible content at all. A
    /// response with neither text nor Tool calls is intentionally absent from the
    /// model-visible history, so continuing it must not fabricate an empty
    /// assistant message just to satisfy the shape check.
    pub previous_response_visible: bool,
    /// Same suffix rule as a tool continuation: these additions, in this order,
    /// are the only items that may follow the continued assistant response.
    pub accepted_additions: Vec<AcceptedTurnAdditionV1>,
}

/// One addition Core already consumed into the current turn. The compiler
/// accepts it only as a suffix of a complete group, or as a hole proven by the
/// compaction anchors of this preparation. It is not a new turn root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedTurnAdditionV1 {
    pub user_event_id: EventId,
    pub content_hash: ContentHash,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub revision: Revision,
    pub order: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedContextArtifactDraft {
    pub artifact_id: ArtifactId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct PreparedContextDraft {
    pub context: PreparedLogicalContext,
    pub artifacts: Vec<PreparedContextArtifactDraft>,
    pub prompt_inspector: PromptInspectorProjection,
}

#[derive(Clone, Debug)]
pub enum ContextPreparationCompilationOutcome {
    Prepared(Box<PreparedContextDraft>),
    /// With the request this step would have sent ([`crate::RequestPrefix`]).
    CompactionRequired {
        plan: ContextRemediationPlan,
        budget: ContextBudgetSnapshot,
        prefix: Box<crate::RequestPrefix>,
    },
    AwaitingRemediation {
        plan: ContextRemediationPlan,
        budget: ContextBudgetSnapshot,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextWindowRebuildDirective {
    pub failed_execution_snapshot_id: ModelExecutionSnapshotId,
    pub failed_context_snapshot_id: ContextProjectionSnapshotId,
    pub failed_model_step_id: ModelStepId,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    pub model_profile_id: ModelProfileId,
    pub runtime_effective_window_tokens: u64,
    pub provider_error_ref: ArtifactId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextWindowResolutionDirective {
    Rebuild(ContextWindowRebuildDirective),
    AwaitingUserCorrection {
        failed_execution_snapshot_id: ModelExecutionSnapshotId,
        model_profile_id: ModelProfileId,
        provider_error_ref: ArtifactId,
    },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextWindowRebuildError {
    #[error("context.window_resolution.failed_execution_mismatch")]
    FailedExecutionMismatch,
    #[error("context.window_resolution.model_profile_mismatch")]
    ModelProfileMismatch,
    #[error("context.window_resolution.invalid_window")]
    InvalidWindow,
    #[error("context.window_resolution.new_model_step_required")]
    NewModelStepRequired,
    #[error("context.window_resolution.previous_snapshot_required")]
    PreviousSnapshotRequired,
    #[error("context.window_resolution.scope_mismatch")]
    ScopeMismatch,
    #[error("context.window_resolution.prompt_bundle_mismatch")]
    PromptBundleMismatch,
    #[error(transparent)]
    Preparation(#[from] ContextPreparationError),
}

pub fn resolve_context_window_resolution(
    failed_execution: &ModelExecutionSnapshot,
    resolution: &ContextWindowResolution,
) -> Result<ContextWindowResolutionDirective, ContextWindowRebuildError> {
    let (
        failed_execution_snapshot_id,
        model_profile_id,
        provider_error_ref,
        runtime_effective_window_tokens,
    ) = match resolution {
        ContextWindowResolution::P06RebuildRequired {
            failed_execution_snapshot_id,
            model_profile_id,
            provider_error_ref,
            current_effective_window_tokens,
            ..
        } => (
            *failed_execution_snapshot_id,
            *model_profile_id,
            *provider_error_ref,
            Some(*current_effective_window_tokens),
        ),
        ContextWindowResolution::AutomaticCandidateSelected {
            failed_execution_snapshot_id,
            model_profile_id,
            provider_error_ref,
            candidate_window_tokens,
            ..
        } => (
            *failed_execution_snapshot_id,
            *model_profile_id,
            *provider_error_ref,
            Some(*candidate_window_tokens),
        ),
        ContextWindowResolution::RuntimeCorrectionApplied {
            failed_execution_snapshot_id,
            model_profile_id,
            provider_error_ref,
            corrected_window_tokens,
            ..
        } => (
            *failed_execution_snapshot_id,
            *model_profile_id,
            *provider_error_ref,
            Some(*corrected_window_tokens),
        ),
        ContextWindowResolution::UserCorrectionRequired {
            failed_execution_snapshot_id,
            model_profile,
            provider_error_ref,
            ..
        }
        | ContextWindowResolution::AutomaticCandidatesExhausted {
            failed_execution_snapshot_id,
            model_profile,
            provider_error_ref,
            ..
        } => {
            if *failed_execution_snapshot_id != failed_execution.id {
                return Err(ContextWindowRebuildError::FailedExecutionMismatch);
            }
            if model_profile.id != failed_execution.model_profile_id {
                return Err(ContextWindowRebuildError::ModelProfileMismatch);
            }
            return Ok(ContextWindowResolutionDirective::AwaitingUserCorrection {
                failed_execution_snapshot_id: *failed_execution_snapshot_id,
                model_profile_id: model_profile.id,
                provider_error_ref: *provider_error_ref,
            });
        }
    };
    if failed_execution_snapshot_id != failed_execution.id {
        return Err(ContextWindowRebuildError::FailedExecutionMismatch);
    }
    if model_profile_id != failed_execution.model_profile_id {
        return Err(ContextWindowRebuildError::ModelProfileMismatch);
    }
    let runtime_effective_window_tokens = runtime_effective_window_tokens
        .filter(|tokens| *tokens > 0)
        .ok_or(ContextWindowRebuildError::InvalidWindow)?;
    Ok(ContextWindowResolutionDirective::Rebuild(ContextWindowRebuildDirective {
        failed_execution_snapshot_id,
        failed_context_snapshot_id: failed_execution.context_snapshot_id,
        failed_model_step_id: failed_execution.model_step_id,
        prompt_bundle_snapshot_id: failed_execution.prompt_bundle_snapshot_id,
        model_profile_id,
        runtime_effective_window_tokens,
        provider_error_ref,
    }))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextPreparationError {
    #[error("context.preparation.prompt_bundle_invalid")]
    PromptBundleInvalid,
    #[error("context.preparation.prompt_bundle_owner_mismatch")]
    PromptBundleOwnerMismatch,
    #[error("context.preparation.active_prompt_modules_invalid")]
    ActivePromptModulesInvalid,
    #[error("context.preparation.sections_invalid")]
    SectionsInvalid,
    #[error("context.preparation.tool_snapshot_invalid")]
    ToolSnapshotInvalid,
    #[error("context.preparation.conversation_invalid")]
    ConversationInvalid,
    #[error("context.preparation.current_user_source_missing")]
    CurrentUserSourceMissing,
    #[error("context.preparation.tool_continuation_facts_missing")]
    ToolContinuationFactsMissing,
    #[error("context.preparation.tool_continuation_invalid")]
    ToolContinuationInvalid,
    #[error("context.preparation.provider_continuation_binding_mismatch")]
    ProviderContinuationBindingMismatch,
    #[error("context.preparation.attachment_source_invalid")]
    AttachmentSourceInvalid,
    #[error("context.preparation.egress_invalid")]
    EgressInvalid,
    #[error("context.preparation.budget_invalid")]
    BudgetInvalid,
    #[error("context.preparation.snapshot_invalid")]
    SnapshotInvalid,
    #[error("context.preparation.canonical_serialization_failed")]
    CanonicalSerialization,
    #[error("context.preparation.arithmetic_overflow")]
    ArithmeticOverflow,
    #[error("context.preparation.coverage_budget_exhausted")]
    CoverageBudgetExhausted,
}

pub struct ContextPreparationCompiler<'a> {
    estimators: &'a ContextEstimatorRegistry,
}

impl<'a> ContextPreparationCompiler<'a> {
    #[must_use]
    pub const fn new(estimators: &'a ContextEstimatorRegistry) -> Self {
        Self { estimators }
    }

    pub fn compile(
        &self,
        mut input: ContextPreparationInput,
    ) -> Result<ContextPreparationCompilationOutcome, ContextPreparationError> {
        validate_prompt_bundle(&mut input)?;
        validate_and_sort_tools(
            &mut input.tools,
            &input.tool_schema_set,
            &input.effective_tool_bindings,
        )?;
        validate_recent_history(&input.recent_history_items)?;
        validate_attachment_sources(&input.attachments, &input.attachment_egress_sources)?;
        validate_provider_target(&input)?;

        let assembly = assemble_context_sections(std::mem::take(&mut input.section_inputs))
            .map_err(map_section_error)?;
        validate_recent_history_sources(&assembly, &input.recent_history_items)?;
        let mut conversation_items = std::mem::take(&mut input.recent_history_items);
        let history_budget_len = conversation_items.len();
        let provider_continuation = match &input.trigger {
            ContextPreparationTriggerV1::UserBatch { user_event_ids } => {
                if user_event_ids.is_empty()
                    || user_event_ids.iter().collect::<std::collections::BTreeSet<_>>().len()
                        != user_event_ids.len()
                    || input.tool_continuation_facts.is_some()
                    || input.core_continuation_facts.is_some()
                {
                    return Err(ContextPreparationError::CurrentUserSourceMissing);
                }
                for id in user_event_ids {
                    if conversation_items
                        .iter()
                        .any(|item| conversation_source_event_id(item) == *id)
                    {
                        return Err(ContextPreparationError::CurrentUserSourceMissing);
                    }
                    validate_current_user_source(&assembly, *id)?;
                    let content = input_batch_content(&assembly, *id)?;
                    conversation_items.push(LogicalConversationItem::User {
                        source_event_id: *id,
                        content_hash: ContentHash::sha256(content.as_bytes()),
                        content,
                    });
                }
                None
            }
            ContextPreparationTriggerV1::UserSubmission { user_event_id } => {
                if input.tool_continuation_facts.is_some()
                    || input.core_continuation_facts.is_some()
                    || conversation_items
                        .iter()
                        .any(|item| conversation_source_event_id(item) == *user_event_id)
                {
                    return Err(ContextPreparationError::ConversationInvalid);
                }
                validate_current_user_source(&assembly, *user_event_id)?;
                let current_user_content = current_user_content(&assembly, *user_event_id)?;
                conversation_items.push(LogicalConversationItem::User {
                    source_event_id: *user_event_id,
                    content_hash: ContentHash::sha256(current_user_content.as_bytes()),
                    content: current_user_content,
                });
                None
            }
            ContextPreparationTriggerV1::ToolContinuation { .. } => {
                if input.core_continuation_facts.is_some() {
                    return Err(ContextPreparationError::ConversationInvalid);
                }
                let facts = input
                    .tool_continuation_facts
                    .as_ref()
                    .ok_or(ContextPreparationError::ToolContinuationFactsMissing)?;
                let history = addition_body(
                    &conversation_items,
                    &facts.accepted_additions,
                    &assembly,
                    &input.request,
                    &input.compacted_source_proofs,
                    &input.coverage_hops,
                    ContextPreparationError::ToolContinuationInvalid,
                )?;
                validate_tool_continuation(
                    &input.request,
                    &input.trigger,
                    facts,
                    history,
                    &input.provider_target,
                )?
            }
            ContextPreparationTriggerV1::CoreContinuation {
                originating_user_event_id,
                previous_model_step_id,
                assistant_response_event_id,
                checkpoint_hash,
            } => {
                let facts = input
                    .core_continuation_facts
                    .as_ref()
                    .ok_or(ContextPreparationError::ConversationInvalid)?;
                let history = addition_body(
                    &conversation_items,
                    &facts.accepted_additions,
                    &assembly,
                    &input.request,
                    &input.compacted_source_proofs,
                    &input.coverage_hops,
                    ContextPreparationError::ConversationInvalid,
                )?;
                if input.tool_continuation_facts.is_some()
                    || facts.session_id != input.request.session_id || facts.turn_task_id != input.request.turn_task_id || facts.agent_run_id != input.request.agent_run_id
                    || facts.previous_model_step_id != *previous_model_step_id || input.request.model_step_id == *previous_model_step_id
                    || facts.assistant_response_event_id != *assistant_response_event_id || facts.checkpoint_hash != *checkpoint_hash
                    || !history.iter().any(|item| matches!(item, LogicalConversationItem::User { source_event_id, .. } if source_event_id == originating_user_event_id))
                    || !core_continuation_history_matches(history, facts, *assistant_response_event_id)
                { return Err(ContextPreparationError::ConversationInvalid); }
                None
            }
        };

        let context_snapshot_id = ContextProjectionSnapshotId::new();
        let mut egress_sources =
            assembly.egress_fragments.iter().map(ModelEgressSourceDraft::from).collect::<Vec<_>>();
        egress_sources.extend(tool_egress_sources(&input.tool_schema_set)?);
        egress_sources.append(&mut input.attachment_egress_sources);
        let model_egress_manifest =
            build_model_egress_manifest(context_snapshot_id, egress_sources)
                .map_err(map_egress_error)?;

        let pending_budget = build_pending_budget_items(
            &assembly,
            &input.tool_schema_set,
            &input.tools,
            &conversation_items[..history_budget_len],
            &input.attachments,
        )?;
        let (text_items, native_attachments) = estimator_inputs(&pending_budget)?;
        let budget = self
            .estimators
            .estimate_budget_measured(
                &input.model_estimator_key,
                input.request.runtime_effective_window_tokens,
                crate::trigger_output_reserve(
                    input.request.output_request.effective_max_output_tokens,
                ),
                &text_items,
                &native_attachments,
                input.previous_measurement.as_ref(),
            )
            .map_err(map_budget_error)?;
        let accounting = build_accounting(&pending_budget, &budget)?;
        let overflow = overflow_diagnostic(&accounting, &budget)?;
        // History
        // is compacted only when this request and the output it may write do not fit the window,
        // and only when compacting is one of the remedies; otherwise the user decides.
        if budget.estimated_input_tokens > budget.request_input_budget_tokens {
            let plan = remediation_plan(overflow.ok_or(ContextPreparationError::BudgetInvalid)?);
            if accounting.compressible_tokens > 0
                && plan.actions.iter().any(|action| action == "compact_sealed_history")
            {
                let prefix = Box::new(crate::RequestPrefix::of_compilation(
                    &assembly.instruction_envelope,
                    conversation_items,
                    &input,
                    provider_continuation.is_some(),
                    &model_egress_manifest,
                    budget.estimated_input_tokens,
                ));
                return Ok(ContextPreparationCompilationOutcome::CompactionRequired {
                    plan,
                    budget,
                    prefix,
                });
            }
            return Ok(ContextPreparationCompilationOutcome::AwaitingRemediation { plan, budget });
        }
        // A request that fits goes out with the room the window has left for its output.
        let output = &mut input.request.output_request.effective_max_output_tokens;
        let budget = crate::with_room_for_output(budget, *output).map_err(map_budget_error)?;
        *output = budget.output_reserve_tokens;

        build_prepared_context(
            input,
            assembly,
            conversation_items,
            provider_continuation,
            model_egress_manifest,
            budget,
            accounting,
            context_snapshot_id,
        )
        .map(|draft| ContextPreparationCompilationOutcome::Prepared(Box::new(draft)))
    }

    pub fn compile_after_context_window_resolution(
        &self,
        input: ContextPreparationInput,
        directive: &ContextWindowRebuildDirective,
    ) -> Result<ContextPreparationCompilationOutcome, ContextWindowRebuildError> {
        let previous = input
            .previous_snapshot
            .as_ref()
            .ok_or(ContextWindowRebuildError::PreviousSnapshotRequired)?;
        if previous.id != directive.failed_context_snapshot_id
            || previous.model_step_id != directive.failed_model_step_id
        {
            return Err(ContextWindowRebuildError::PreviousSnapshotRequired);
        }
        if input.request.model_step_id == directive.failed_model_step_id {
            return Err(ContextWindowRebuildError::NewModelStepRequired);
        }
        if input.request.model_profile_id != directive.model_profile_id
            || previous.model_profile_id != directive.model_profile_id
        {
            return Err(ContextWindowRebuildError::ModelProfileMismatch);
        }
        if input.request.runtime_effective_window_tokens
            != directive.runtime_effective_window_tokens
        {
            return Err(ContextWindowRebuildError::InvalidWindow);
        }
        if input.request.session_id != previous.session_id
            || input.request.turn_task_id != previous.turn_task_id
            || input.request.agent_run_id != previous.agent_run_id
        {
            return Err(ContextWindowRebuildError::ScopeMismatch);
        }
        if input.prompt_bundle_snapshot.id != directive.prompt_bundle_snapshot_id
            || previous.prompt_bundle_snapshot_id != directive.prompt_bundle_snapshot_id
            || input.prompt_bundle_snapshot.turn_task_id != previous.turn_task_id
        {
            return Err(ContextWindowRebuildError::PromptBundleMismatch);
        }
        self.compile(input).map_err(ContextWindowRebuildError::from)
    }
}

#[derive(Clone, Copy, Debug)]
struct BudgetAssignment {
    usage: ContextUsageKind,
    section: ContextSectionKind,
    compressibility: ContextCompressibility,
    tool_index: Option<usize>,
    attachment_index: Option<usize>,
}

#[derive(Clone, Debug)]
enum PendingBudgetPayload {
    Text(String),
    NativeAttachment(ModelAttachmentSnapshot),
}

#[derive(Clone, Debug)]
struct PendingBudgetItem {
    ordinal: u32,
    payload: PendingBudgetPayload,
    assignment: BudgetAssignment,
}

#[derive(Clone, Debug)]
struct ContextAccounting {
    usage: ContextUsageSummaryView,
    section_tokens: BTreeMap<ContextSectionKind, u64>,
    tool_tokens: Vec<u64>,
    attachment_tokens: Vec<u64>,
    non_compressible_tokens: u64,
    compressible_tokens: u64,
    user_reducible_tokens: u64,
}

fn validate_prompt_bundle(
    input: &mut ContextPreparationInput,
) -> Result<(), ContextPreparationError> {
    input
        .prompt_bundle_snapshot
        .validate_contract_shape()
        .map_err(|_| ContextPreparationError::PromptBundleInvalid)?;
    if input.prompt_bundle_snapshot.turn_task_id != input.request.turn_task_id {
        return Err(ContextPreparationError::PromptBundleOwnerMismatch);
    }
    let available = input
        .prompt_bundle_snapshot
        .module_snapshots
        .iter()
        .map(|module| module.module_id.as_str())
        .collect::<BTreeSet<_>>();
    input.active_prompt_module_ids.sort();
    let original_len = input.active_prompt_module_ids.len();
    input.active_prompt_module_ids.dedup();
    if input.active_prompt_module_ids.is_empty()
        || input.active_prompt_module_ids.len() != original_len
        || input
            .active_prompt_module_ids
            .iter()
            .any(|module_id| !available.contains(module_id.as_str()))
    {
        return Err(ContextPreparationError::ActivePromptModulesInvalid);
    }
    Ok(())
}

fn validate_and_sort_tools(
    tools: &mut [ContextToolInput],
    schema_set: &ToolSchemaSetSnapshotV1,
    effective_bindings: &[EffectiveToolBindingSnapshot],
) -> Result<(), ContextPreparationError> {
    schema_set.validate().map_err(|_| ContextPreparationError::ToolSnapshotInvalid)?;
    tools.sort_by_key(|tool| tool.tool_id);
    if !tools.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id)
        || tools.len() != schema_set.tools.len()
        || effective_bindings.len() != schema_set.tools.len()
        || !effective_bindings.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id)
    {
        return Err(ContextPreparationError::ToolSnapshotInvalid);
    }
    for ((tool, schema), binding) in tools.iter().zip(&schema_set.tools).zip(effective_bindings) {
        if tool.tool_id != schema.tool_id
            || binding.tool_id != schema.tool_id
            || binding.tool_version != schema.tool_version
        {
            return Err(ContextPreparationError::ToolSnapshotInvalid);
        }
    }
    Ok(())
}

/// The continued response must be represented when it carried model-visible
/// content: once authorized additions are removed, the conversation ends with
/// that assistant item and no pending Tool call. When a complete response
/// carried neither text nor Tool calls, the history legitimately has no item
/// for it, so the continuation continues the last real item instead.
fn core_continuation_history_matches(
    items: &[LogicalConversationItem],
    facts: &CoreContinuationFactsV1,
    assistant_response_event_id: EventId,
) -> bool {
    if facts.previous_response_visible {
        return matches!(
            items.last(),
            Some(LogicalConversationItem::Assistant { source_event_id, tool_calls, .. })
                if *source_event_id == assistant_response_event_id && tool_calls.is_empty()
        );
    }
    !items.iter().any(|item| {
        matches!(item, LogicalConversationItem::Assistant { source_event_id, .. }
            if *source_event_id == assistant_response_event_id)
    })
}

fn validate_recent_history(
    items: &[LogicalConversationItem],
) -> Result<(), ContextPreparationError> {
    for item in items {
        match item {
            LogicalConversationItem::User { content, content_hash, .. }
            | LogicalConversationItem::ToolResult { content, content_hash, .. } => {
                if ContentHash::sha256(content.as_bytes()) != *content_hash {
                    return Err(ContextPreparationError::ConversationInvalid);
                }
            }
            LogicalConversationItem::Assistant { content, content_hash, tool_calls, .. } => {
                if content.is_some() != content_hash.is_some()
                    || content.as_ref().zip(content_hash.as_ref()).is_some_and(|(content, hash)| {
                        ContentHash::sha256(content.as_bytes()) != *hash
                    })
                    || tool_calls.iter().any(|call| {
                        serde_json::from_str::<serde_json::Value>(&call.arguments_json).is_err()
                            || ContentHash::sha256(call.arguments_json.as_bytes())
                                != call.arguments_hash
                    })
                    || (content.is_none() && tool_calls.is_empty())
                {
                    return Err(ContextPreparationError::ConversationInvalid);
                }
            }
        }
    }
    Ok(())
}

fn validate_tool_continuation(
    request: &ContextPreparationRequest,
    trigger: &ContextPreparationTriggerV1,
    facts: &ToolContinuationFactsV1,
    items: &[LogicalConversationItem],
    provider_target: &ContextProviderTargetV1,
) -> Result<Option<ProviderContinuationState>, ContextPreparationError> {
    let ContextPreparationTriggerV1::ToolContinuation {
        originating_user_event_id,
        previous_model_step_id,
        assistant_response_event_id,
        tool_batch_id,
        history_atomic_group_ref,
        provider_continuation,
    } = trigger
    else {
        return Err(ContextPreparationError::ToolContinuationInvalid);
    };
    facts
        .history_atomic_group
        .validate()
        .map_err(|_| ContextPreparationError::ToolContinuationInvalid)?;
    let computed_group_ref = facts
        .history_atomic_group
        .group_ref()
        .map_err(|_| ContextPreparationError::ToolContinuationInvalid)?;
    let group = &facts.history_atomic_group;
    if computed_group_ref != *history_atomic_group_ref
        || history_atomic_group_ref.assistant_response_event_id != *assistant_response_event_id
        || group.session_id != request.session_id
        || group.turn_task_id != request.turn_task_id
        || group.agent_run_id != request.agent_run_id
        || group.model_step_id != *previous_model_step_id
        || group.assistant_response_event_id != *assistant_response_event_id
        || group.tool_batch_id != *tool_batch_id
        || request.model_step_id == *previous_model_step_id
        || !facts.previous_model_step_legally_terminal
        || !facts.assistant_response_durable
        || !facts.tool_batch_terminal
    {
        return Err(ContextPreparationError::ToolContinuationInvalid);
    }

    let expected_call_ids = group.calls.iter().map(|entry| entry.tool_call_id).collect::<Vec<_>>();
    let mut durable_result_ids = facts.durable_result_tool_call_ids.clone();
    durable_result_ids.sort_unstable();
    durable_result_ids.dedup();
    let mut expected_sorted = expected_call_ids.clone();
    expected_sorted.sort_unstable();
    if durable_result_ids != expected_sorted || expected_call_ids.len() + 2 > items.len() {
        return Err(ContextPreparationError::ToolContinuationInvalid);
    }

    let prefix_len = items.len() - expected_call_ids.len() - 1;
    let suffix = &items[prefix_len..];
    if !items[..prefix_len].iter().any(|item| matches!(item,
        LogicalConversationItem::User { source_event_id, .. } if *source_event_id == *originating_user_event_id)) {
        return Err(ContextPreparationError::ToolContinuationInvalid);
    }
    let assistant_calls = match &suffix[0] {
        LogicalConversationItem::Assistant { source_event_id, tool_calls, .. }
            if *source_event_id == *assistant_response_event_id =>
        {
            tool_calls
        }
        _ => return Err(ContextPreparationError::ToolContinuationInvalid),
    };
    if assistant_calls.iter().map(|call| call.tool_call_id).collect::<Vec<_>>() != expected_call_ids
        || suffix[1..].iter().zip(&expected_call_ids).any(|(item, expected)| {
            !matches!(
                item,
                LogicalConversationItem::ToolResult { tool_call_id, .. }
                    if tool_call_id == expected
            )
        })
    {
        return Err(ContextPreparationError::ToolContinuationInvalid);
    }

    match provider_continuation {
        Some(binding)
            if facts.verified_provider_continuation.as_ref() == Some(binding)
                && binding.assistant_response_event_id == *assistant_response_event_id
                && binding.tool_batch_id == *tool_batch_id
                && binding.provider_id == provider_target.provider_id
                && binding.api_format == provider_target.api_format
                && binding.endpoint_id == provider_target.endpoint_id
                && binding.model_profile_id == provider_target.model_profile_id
                && binding.endpoint_fingerprint == provider_target.endpoint_fingerprint =>
        {
            Ok(Some(binding.state.clone()))
        }
        Some(_) => Err(ContextPreparationError::ProviderContinuationBindingMismatch),
        None => Ok(None),
    }
}

fn validate_provider_target(
    input: &ContextPreparationInput,
) -> Result<(), ContextPreparationError> {
    if input.provider_target.provider_id != input.model_estimator_key.provider_id
        || input.provider_target.model_profile_id != input.request.model_profile_id
    {
        return Err(ContextPreparationError::ConversationInvalid);
    }
    Ok(())
}

fn validate_current_user_source(
    assembly: &ContextSectionAssembly,
    event_id: EventId,
) -> Result<(), ContextPreparationError> {
    let present = assembly.egress_fragments.iter().any(|fragment| {
        fragment.source_ref == aura_context_domain::ContextSourceRef::Event { event_id }
            && fragment.source_kind == ModelEgressSourceKind::UserEvent
            && matches!(fragment.authorization_basis, ModelEgressAuthorizationBasis::UserSubmission)
    });
    present.then_some(()).ok_or(ContextPreparationError::CurrentUserSourceMissing)
}

fn validate_recent_history_sources(
    assembly: &ContextSectionAssembly,
    items: &[LogicalConversationItem],
) -> Result<(), ContextPreparationError> {
    for item in items {
        let event_id = conversation_source_event_id(item);
        let expected = match item {
            LogicalConversationItem::User { .. } => ModelEgressSourceKind::UserEvent,
            LogicalConversationItem::Assistant { .. } => ModelEgressSourceKind::AssistantEvent,
            LogicalConversationItem::ToolResult { .. } => ModelEgressSourceKind::ToolResult,
        };
        if !assembly.egress_fragments.iter().any(|fragment| {
            fragment.source_ref == aura_context_domain::ContextSourceRef::Event { event_id }
                && fragment.source_kind == expected
        }) {
            return Err(ContextPreparationError::ConversationInvalid);
        }
    }
    Ok(())
}

fn tool_egress_sources(
    schema_set: &ToolSchemaSetSnapshotV1,
) -> Result<Vec<ModelEgressSourceDraft>, ContextPreparationError> {
    schema_set
        .tools
        .iter()
        .map(|tool| {
            let bytes = canonical_json_v1(tool)
                .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
            Ok(ModelEgressSourceDraft {
                source_ref: aura_context_domain::ContextSourceRef::ToolSchema {
                    tool_id: tool.tool_id,
                    tool_version: tool.tool_version.clone(),
                },
                source_kind: ModelEgressSourceKind::ToolSchema,
                data_class: NetworkDataClass::PublicContent,
                content_hash: ContentHash::sha256(&bytes),
                byte_length: u64::try_from(bytes.len())
                    .map_err(|_| ContextPreparationError::ArithmeticOverflow)?,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            })
        })
        .collect()
}

fn build_pending_budget_items(
    assembly: &ContextSectionAssembly,
    schema_set: &ToolSchemaSetSnapshotV1,
    tools: &[ContextToolInput],
    recent_history: &[LogicalConversationItem],
    attachments: &[ModelAttachmentSnapshot],
) -> Result<Vec<PendingBudgetItem>, ContextPreparationError> {
    let mut items = Vec::new();
    for (index, block) in assembly.instruction_envelope.ordered_blocks.iter().enumerate() {
        let section = instruction_section(block.kind);
        let mut content = block.content.clone();
        if index + 1 < assembly.instruction_envelope.ordered_blocks.len() {
            content.push('\n');
        }
        push_text_budget_item(
            &mut items,
            content,
            instruction_usage(block.kind),
            section,
            budget_compressibility(assembly, section)?,
            None,
        )?;
    }
    for (index, (tool, schema)) in tools.iter().zip(&schema_set.tools).enumerate() {
        let bytes = canonical_json_v1(schema)
            .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
        let content = String::from_utf8(bytes)
            .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
        push_text_budget_item(
            &mut items,
            content,
            ContextUsageKind::ToolsAndResults,
            ContextSectionKind::LoadedToolSchemas,
            tool.compressibility,
            Some(index),
        )?;
    }
    for item in recent_history {
        match item {
            LogicalConversationItem::User { content, .. } => push_text_budget_item(
                &mut items,
                content.clone(),
                ContextUsageKind::Conversation,
                ContextSectionKind::RecentHistory,
                ContextCompressibility::CompressibleHistory,
                None,
            )?,
            LogicalConversationItem::Assistant {
                content, tool_calls, provider_history, ..
            } => {
                if let Some(history) = provider_history {
                    push_text_budget_item(
                        &mut items,
                        serde_json::to_string(history)
                            .map_err(|_| ContextPreparationError::CanonicalSerialization)?,
                        ContextUsageKind::Conversation,
                        ContextSectionKind::RecentHistory,
                        ContextCompressibility::CompressibleHistory,
                        None,
                    )?;
                }
                if let Some(content) = content {
                    push_text_budget_item(
                        &mut items,
                        content.clone(),
                        ContextUsageKind::Conversation,
                        ContextSectionKind::RecentHistory,
                        ContextCompressibility::CompressibleHistory,
                        None,
                    )?;
                }
                for call in tool_calls {
                    let bytes = canonical_json_v1(call)
                        .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
                    push_text_budget_item(
                        &mut items,
                        String::from_utf8(bytes)
                            .map_err(|_| ContextPreparationError::CanonicalSerialization)?,
                        ContextUsageKind::ToolsAndResults,
                        ContextSectionKind::RecentHistory,
                        ContextCompressibility::CompressibleHistory,
                        None,
                    )?;
                }
            }
            LogicalConversationItem::ToolResult { content, .. } => push_text_budget_item(
                &mut items,
                content.clone(),
                ContextUsageKind::ToolsAndResults,
                ContextSectionKind::RecentHistory,
                ContextCompressibility::CompressibleHistory,
                None,
            )?,
        }
    }
    let current_kinds = [
        ContextSectionKind::FileReferencesAndPendingAttachments,
        ContextSectionKind::CurrentTurnWorkingSet,
        ContextSectionKind::SteeredUserAdditions,
    ];
    for (index, kind) in current_kinds.into_iter().enumerate() {
        let section = assembly
            .sections
            .iter()
            .find(|section| section.kind == kind)
            .ok_or(ContextPreparationError::SectionsInvalid)?;
        let mut content = section.canonical_content.clone();
        if index + 1 < current_kinds.len() {
            content.push('\n');
        }
        push_text_budget_item(
            &mut items,
            content,
            if kind == ContextSectionKind::FileReferencesAndPendingAttachments {
                ContextUsageKind::BackgroundAndCompaction
            } else {
                ContextUsageKind::Conversation
            },
            kind,
            if section.is_empty_marker {
                ContextCompressibility::NeverCompress
            } else {
                section.compressibility
            },
            None,
        )?;
    }
    for (index, attachment) in attachments.iter().enumerate() {
        let ordinal = next_ordinal(&items)?;
        items.push(PendingBudgetItem {
            ordinal,
            payload: PendingBudgetPayload::NativeAttachment(attachment.clone()),
            assignment: BudgetAssignment {
                usage: ContextUsageKind::BackgroundAndCompaction,
                section: ContextSectionKind::FileReferencesAndPendingAttachments,
                compressibility: ContextCompressibility::UserReducibleOnly,
                tool_index: None,
                attachment_index: Some(index),
            },
        });
    }
    Ok(items)
}

fn push_text_budget_item(
    items: &mut Vec<PendingBudgetItem>,
    content: String,
    usage: ContextUsageKind,
    section: ContextSectionKind,
    compressibility: ContextCompressibility,
    tool_index: Option<usize>,
) -> Result<(), ContextPreparationError> {
    let ordinal = next_ordinal(items)?;
    items.push(PendingBudgetItem {
        ordinal,
        payload: PendingBudgetPayload::Text(content),
        assignment: BudgetAssignment {
            usage,
            section,
            compressibility,
            tool_index,
            attachment_index: None,
        },
    });
    Ok(())
}

fn next_ordinal(items: &[PendingBudgetItem]) -> Result<u32, ContextPreparationError> {
    u32::try_from(items.len() + 1).map_err(|_| ContextPreparationError::ArithmeticOverflow)
}

fn estimator_inputs(
    items: &[PendingBudgetItem],
) -> Result<(Vec<CanonicalTextBudgetItem>, Vec<NativeAttachmentBudgetItem>), ContextPreparationError>
{
    let mut text = Vec::new();
    let mut native = Vec::new();
    for item in items {
        match &item.payload {
            PendingBudgetPayload::Text(content) => text.push(CanonicalTextBudgetItem {
                ordinal: item.ordinal,
                content_hash: ContentHash::sha256(content.as_bytes()),
                canonical_text: content.clone(),
            }),
            PendingBudgetPayload::NativeAttachment(attachment) => {
                native.push(NativeAttachmentBudgetItem {
                    ordinal: item.ordinal,
                    attachment: attachment.clone(),
                });
            }
        }
    }
    Ok((text, native))
}

fn build_accounting(
    pending: &[PendingBudgetItem],
    budget: &ContextBudgetSnapshot,
) -> Result<ContextAccounting, ContextPreparationError> {
    let estimates = budget
        .item_estimates
        .iter()
        .map(|estimate| (estimate.ordinal, estimate))
        .collect::<BTreeMap<_, _>>();
    let mut usage_tokens = BTreeMap::<ContextUsageKind, u64>::new();
    let mut section_tokens = BTreeMap::<ContextSectionKind, u64>::new();
    let tool_count = pending
        .iter()
        .filter_map(|item| item.assignment.tool_index)
        .max()
        .map_or(0, |index| index + 1);
    let attachment_count = pending
        .iter()
        .filter_map(|item| item.assignment.attachment_index)
        .max()
        .map_or(0, |index| index + 1);
    let mut tool_tokens = vec![0_u64; tool_count];
    let mut attachment_tokens = vec![0_u64; attachment_count];
    let mut non_compressible_tokens = 0_u64;
    let mut compressible_tokens = 0_u64;
    let mut user_reducible_tokens = 0_u64;
    for item in pending {
        let tokens = estimates
            .get(&item.ordinal)
            .ok_or(ContextPreparationError::BudgetInvalid)?
            .estimated_tokens;
        checked_add_map(&mut usage_tokens, item.assignment.usage, tokens)?;
        checked_add_map(&mut section_tokens, item.assignment.section, tokens)?;
        if let Some(index) = item.assignment.tool_index {
            tool_tokens[index] = tool_tokens[index]
                .checked_add(tokens)
                .ok_or(ContextPreparationError::ArithmeticOverflow)?;
        }
        if let Some(index) = item.assignment.attachment_index {
            attachment_tokens[index] = attachment_tokens[index]
                .checked_add(tokens)
                .ok_or(ContextPreparationError::ArithmeticOverflow)?;
        }
        match item.assignment.compressibility {
            ContextCompressibility::NeverCompress => {
                non_compressible_tokens = non_compressible_tokens
                    .checked_add(tokens)
                    .ok_or(ContextPreparationError::ArithmeticOverflow)?;
            }
            ContextCompressibility::CompressibleHistory
            | ContextCompressibility::CompressibleSealedActiveSegment => {
                compressible_tokens = compressible_tokens
                    .checked_add(tokens)
                    .ok_or(ContextPreparationError::ArithmeticOverflow)?;
            }
            ContextCompressibility::UserReducibleOnly => {
                user_reducible_tokens = user_reducible_tokens
                    .checked_add(tokens)
                    .ok_or(ContextPreparationError::ArithmeticOverflow)?;
            }
        }
    }
    let estimated = budget.item_estimates.iter().any(|estimate| {
        estimate.estimator.exactness != aura_context_domain::TokenEstimateExactness::Exact
    });
    let segments = ContextUsageKind::ORDER
        .into_iter()
        .map(|kind| ContextUsageSegmentView {
            kind,
            label: usage_label(kind).to_owned(),
            estimated_tokens: usage_tokens.get(&kind).copied().unwrap_or_default(),
        })
        .collect();
    let usage = ContextUsageSummaryView {
        runtime_effective_window_tokens: budget.runtime_effective_window_tokens,
        used_input_tokens: budget.estimated_input_tokens,
        estimated,
        segments,
    };
    if !usage.is_consistent() {
        return Err(ContextPreparationError::BudgetInvalid);
    }
    Ok(ContextAccounting {
        usage,
        section_tokens,
        tool_tokens,
        attachment_tokens,
        non_compressible_tokens,
        compressible_tokens,
        user_reducible_tokens,
    })
}

// Keep every immutable compilation product explicit at the final sealing boundary.
#[allow(clippy::too_many_arguments)]
fn build_prepared_context(
    input: ContextPreparationInput,
    assembly: ContextSectionAssembly,
    conversation_items: Vec<LogicalConversationItem>,
    provider_continuation: Option<ProviderContinuationState>,
    model_egress_manifest: ModelEgressManifestV1,
    budget: ContextBudgetSnapshot,
    accounting: ContextAccounting,
    context_snapshot_id: ContextProjectionSnapshotId,
) -> Result<PreparedContextDraft, ContextPreparationError> {
    let instruction_artifact_id = ArtifactId::new();
    let manifest_artifact_id = ArtifactId::new();
    let mut artifacts = Vec::with_capacity(assembly.sections.len() + 2);
    let mut sections = Vec::with_capacity(assembly.sections.len());
    for section in &assembly.sections {
        let artifact_id = ArtifactId::new();
        artifacts.push(PreparedContextArtifactDraft {
            artifact_id,
            media_type: "application/vnd.aura.context-section+text; charset=utf-8".to_owned(),
            bytes: section.canonical_content.as_bytes().to_vec(),
        });
        sections.push(ContextSectionSnapshot {
            kind: section.kind,
            physical_ordinal: section.physical_ordinal,
            content_artifact_id: artifact_id,
            content_hash: section.content_hash.clone(),
            character_count: section.character_count,
            byte_count: section.byte_count,
            estimated_tokens: accounting
                .section_tokens
                .get(&section.kind)
                .copied()
                .unwrap_or_default(),
            source_refs: section.source_refs.clone(),
            compressibility: section.compressibility,
            privacy_class: section.privacy_class,
            is_empty_marker: section.is_empty_marker,
        });
    }
    artifacts.push(PreparedContextArtifactDraft {
        artifact_id: instruction_artifact_id,
        media_type: "application/vnd.aura.instruction-envelope+text; charset=utf-8".to_owned(),
        bytes: assembly.instruction_envelope.canonical_text.as_bytes().to_vec(),
    });
    let manifest_bytes = canonical_json_v1(&model_egress_manifest)
        .map_err(|_| ContextPreparationError::CanonicalSerialization)?;
    artifacts.push(PreparedContextArtifactDraft {
        artifact_id: manifest_artifact_id,
        media_type: "application/vnd.aura.model-egress-manifest+json".to_owned(),
        bytes: manifest_bytes,
    });

    let snapshot = ContextProjectionSnapshot {
        snapshot_version: 2,
        id: context_snapshot_id,
        session_id: input.request.session_id,
        turn_task_id: input.request.turn_task_id,
        agent_run_id: input.request.agent_run_id,
        model_step_id: input.request.model_step_id,
        prompt_bundle_snapshot_id: input.prompt_bundle_snapshot.id,
        active_prompt_module_ids: input.active_prompt_module_ids.clone(),
        model_profile_id: input.request.model_profile_id,
        budget: budget.clone(),
        sections,
        tool_schema_set: input.tool_schema_set.clone(),
        effective_tool_bindings: input.effective_tool_bindings.clone(),
        conversation_items: conversation_items.clone(),
        attachments: input.attachments.clone(),
        active_task_lineage_id: input
            .inspector
            .active_task_lineage
            .as_ref()
            .map(|lineage| lineage.lineage_id),
        instruction_envelope_artifact_id: instruction_artifact_id,
        instruction_envelope_hash: assembly.instruction_envelope.instruction_hash.clone(),
        model_egress_policy_version: model_egress_manifest.policy_version,
        model_egress_manifest_artifact_id: manifest_artifact_id,
        model_egress_manifest_hash: model_egress_manifest.manifest_hash.clone(),
        output_request: input.request.output_request.clone(),
        history_cutoff: input.request.history_cutoff,
        context_usage: accounting.usage.clone(),
        snapshot_hash: ContentHash::sha256(b"pending"),
        created_at: input.created_at,
    }
    .seal()
    .map_err(map_snapshot_error)?;

    let prompt_inspector =
        build_prompt_inspector(&input, &assembly, &snapshot, &model_egress_manifest, &accounting)?;
    let context = PreparedLogicalContext {
        model_step_id: input.request.model_step_id,
        context_snapshot_id,
        prompt_bundle_snapshot_id: input.prompt_bundle_snapshot.id,
        instruction_blocks: assembly.instruction_envelope.ordered_blocks.clone(),
        instruction_envelope: assembly.instruction_envelope,
        conversation_items,
        tool_schema_set: input.tool_schema_set,
        attachments: input.attachments,
        provider_continuation,
        output_request: input.request.output_request,
        model_egress_manifest,
        snapshot,
    };
    Ok(PreparedContextDraft { context, artifacts, prompt_inspector })
}

fn build_prompt_inspector(
    input: &ContextPreparationInput,
    assembly: &ContextSectionAssembly,
    snapshot: &ContextProjectionSnapshot,
    manifest: &ModelEgressManifestV1,
    accounting: &ContextAccounting,
) -> Result<PromptInspectorProjection, ContextPreparationError> {
    let previous = input.previous_snapshot.as_ref();
    let sections = assembly
        .sections
        .iter()
        .zip(&snapshot.sections)
        .map(|(draft, section)| {
            let source_kinds =
                draft.source_kinds.iter().map(enum_wire_name).collect::<Result<Vec<_>, _>>()?;
            Ok(PromptInspectorSectionView {
                kind: section.kind,
                physical_ordinal: section.physical_ordinal,
                content_hash: section.content_hash.clone(),
                estimated_tokens: section.estimated_tokens,
                character_count: section.character_count,
                byte_count: section.byte_count,
                changed_from_previous_step: previous
                    .and_then(|previous| {
                        previous.sections.iter().find(|value| value.kind == section.kind)
                    })
                    .is_none_or(|previous| previous.content_hash != section.content_hash),
                source_kinds,
                compressibility: section.compressibility,
                privacy_class: section.privacy_class,
                is_empty_marker: section.is_empty_marker,
                placement: section_placement(section.kind),
            })
        })
        .collect::<Result<Vec<_>, ContextPreparationError>>()?;
    let tools = input
        .tool_schema_set
        .tools
        .iter()
        .enumerate()
        .map(|(index, tool)| PromptInspectorToolView {
            tool_id: tool.tool_id,
            canonical_name: tool.canonical_name.to_string(),
            schema_hash: tool.schema_hash.clone(),
            estimated_tokens: accounting.tool_tokens.get(index).copied().unwrap_or_default(),
        })
        .collect();
    let attachments = input
        .attachments
        .iter()
        .enumerate()
        .map(|(index, attachment)| PromptInspectorAttachmentView {
            display_name: attachment.display_name.clone(),
            media_type: attachment.media_type.clone(),
            byte_length: attachment.byte_length,
            content_hash: attachment.content_hash.clone(),
            estimated_tokens: accounting.attachment_tokens.get(index).copied(),
            resolved: input.inspector.attachments_resolved,
        })
        .collect();
    let budget_view = inspector_budget(&snapshot.budget);
    let model_egress = inspector_egress(manifest, input.inspector.attachments_resolved)?;
    let mut active_tool_call_ids = input.inspector.active_tool_call_ids.clone();
    active_tool_call_ids.sort_unstable();
    active_tool_call_ids.dedup();
    Ok(PromptInspectorProjection {
        session_id: snapshot.session_id,
        revision: Revision::INITIAL,
        active_agent_run_id: Some(snapshot.agent_run_id),
        active_model_step_id: Some(snapshot.model_step_id),
        prompt_bundle_snapshot_id: Some(snapshot.prompt_bundle_snapshot_id),
        context_snapshot_id: Some(snapshot.id),
        model_profile_id: Some(snapshot.model_profile_id),
        budget: Some(budget_view),
        sections,
        tools,
        skills: input.inspector.skills.clone(),
        attachments,
        context_usage: Some(snapshot.context_usage.clone()),
        cache_usage: input.inspector.cache_usage.clone(),
        active_task_lineage: input.inspector.active_task_lineage.clone(),
        compaction: input.inspector.compaction.clone(),
        model_egress: Some(model_egress),
        overflow: None,
        active_tool_call_ids,
        // The activity projection is supplied by the caller that froze the snapshot, so
        // the Inspector view and the fragment are built from one sampling - never two reads.
        activity: input.inspector.activity.clone(),
        reduction: None,
    })
}

fn inspector_budget(budget: &ContextBudgetSnapshot) -> PromptInspectorBudgetView {
    let identities = budget
        .item_estimates
        .iter()
        .map(|item| {
            (
                item.estimator.estimator_id.clone(),
                item.estimator.estimator_version,
                item.estimator.exactness == aura_context_domain::TokenEstimateExactness::Exact,
            )
        })
        .collect::<BTreeSet<_>>();
    let exact = identities.iter().all(|identity| identity.2);
    let (estimator_id, estimator_version) = if let Some(identity) =
        (identities.len() == 1).then(|| identities.iter().next()).flatten()
    {
        (identity.0.clone(), identity.1)
    } else {
        ("mixed_context_estimators_v1".to_owned(), 1)
    };
    PromptInspectorBudgetView {
        runtime_effective_window_tokens: budget.runtime_effective_window_tokens,
        estimated_input_tokens: budget.estimated_input_tokens,
        output_reserve_tokens: budget.output_reserve_tokens,
        safety_reserve_tokens: budget.safety_reserve_tokens,
        request_input_budget_tokens: budget.request_input_budget_tokens,
        estimator_id,
        estimator_version,
        exact,
        policy: budget.policy.clone(),
    }
}

fn inspector_egress(
    manifest: &ModelEgressManifestV1,
    attachments_resolved: bool,
) -> Result<PromptInspectorModelEgressView, ContextPreparationError> {
    let mut classes = BTreeMap::<NetworkDataClass, (u32, u64)>::new();
    for item in &manifest.items {
        let entry = classes.entry(item.data_class).or_default();
        entry.0 = entry.0.checked_add(1).ok_or(ContextPreparationError::ArithmeticOverflow)?;
        entry.1 = entry
            .1
            .checked_add(item.byte_length)
            .ok_or(ContextPreparationError::ArithmeticOverflow)?;
    }
    let classes = classes
        .into_iter()
        .map(|(data_class, (item_count, byte_length))| {
            let disposition = data_class
                .model_channel_disposition(manifest.policy_version)
                .map_err(|_| ContextPreparationError::EgressInvalid)?;
            Ok(PromptInspectorEgressClassView { data_class, disposition, item_count, byte_length })
        })
        .collect::<Result<Vec<_>, ContextPreparationError>>()?;
    Ok(PromptInspectorModelEgressView {
        policy_version: manifest.policy_version.get(),
        manifest_hash: manifest.manifest_hash.clone(),
        classes,
        unresolved_source_kinds: Vec::new(),
        attachments_resolved,
    })
}

fn overflow_diagnostic(
    accounting: &ContextAccounting,
    budget: &ContextBudgetSnapshot,
) -> Result<Option<ContextOverflowDiagnosticView>, ContextPreparationError> {
    if budget.estimated_input_tokens <= budget.request_input_budget_tokens {
        return Ok(None);
    }
    let overflow_tokens = budget
        .estimated_input_tokens
        .checked_sub(budget.request_input_budget_tokens)
        .ok_or(ContextPreparationError::ArithmeticOverflow)?;
    let non_compressible_overflow =
        accounting.non_compressible_tokens > budget.request_input_budget_tokens;
    let remediation = if non_compressible_overflow {
        vec!["reduce_user_managed_context".to_owned()]
    } else {
        vec!["compact_sealed_history".to_owned(), "reduce_user_managed_context".to_owned()]
    };
    Ok(Some(ContextOverflowDiagnosticView {
        request_input_budget_tokens: budget.request_input_budget_tokens,
        estimated_input_tokens: budget.estimated_input_tokens,
        non_compressible_tokens: accounting.non_compressible_tokens,
        compressible_history_tokens: accounting.compressible_tokens,
        user_reducible_tokens: accounting.user_reducible_tokens,
        overflow_tokens,
        error_code: if non_compressible_overflow {
            "context.non_compressible_overflow".to_owned()
        } else {
            "context.request_budget_exceeded".to_owned()
        },
        remediation,
    }))
}

fn remediation_plan(overflow: ContextOverflowDiagnosticView) -> ContextRemediationPlan {
    ContextRemediationPlan { actions: overflow.remediation.clone(), overflow }
}

fn checked_add_map<K: Ord>(
    values: &mut BTreeMap<K, u64>,
    key: K,
    value: u64,
) -> Result<(), ContextPreparationError> {
    let entry = values.entry(key).or_default();
    *entry = entry.checked_add(value).ok_or(ContextPreparationError::ArithmeticOverflow)?;
    Ok(())
}

fn budget_compressibility(
    assembly: &ContextSectionAssembly,
    kind: ContextSectionKind,
) -> Result<ContextCompressibility, ContextPreparationError> {
    assembly
        .sections
        .iter()
        .find(|section| section.kind == kind)
        .map(|section| {
            if section.is_empty_marker {
                ContextCompressibility::NeverCompress
            } else {
                section.compressibility
            }
        })
        .ok_or(ContextPreparationError::SectionsInvalid)
}

const fn instruction_section(
    kind: aura_model_domain::LogicalInstructionKind,
) -> ContextSectionKind {
    match kind {
        aura_model_domain::LogicalInstructionKind::Core => ContextSectionKind::CoreSystemPrompt,
        aura_model_domain::LogicalInstructionKind::Permission => {
            ContextSectionKind::PermissionAndSecurity
        }
        aura_model_domain::LogicalInstructionKind::ToolSummary => {
            ContextSectionKind::LoadedToolSchemas
        }
        aura_model_domain::LogicalInstructionKind::WorkspaceAgents => {
            ContextSectionKind::WorkspaceAgentsInstructions
        }
        aura_model_domain::LogicalInstructionKind::HistoricalSnapshot => {
            ContextSectionKind::HistoricalSnapshotSummary
        }
        aura_model_domain::LogicalInstructionKind::AgentMode => ContextSectionKind::AgentModePrompt,
        aura_model_domain::LogicalInstructionKind::Hook => ContextSectionKind::HookInjections,
        aura_model_domain::LogicalInstructionKind::TaskState => {
            ContextSectionKind::CurrentTaskState
        }
    }
}

const fn instruction_usage(kind: aura_model_domain::LogicalInstructionKind) -> ContextUsageKind {
    match kind {
        aura_model_domain::LogicalInstructionKind::Core
        | aura_model_domain::LogicalInstructionKind::Permission
        | aura_model_domain::LogicalInstructionKind::WorkspaceAgents
        | aura_model_domain::LogicalInstructionKind::AgentMode
        | aura_model_domain::LogicalInstructionKind::Hook => ContextUsageKind::SystemAndRules,
        aura_model_domain::LogicalInstructionKind::ToolSummary => ContextUsageKind::ToolsAndResults,
        aura_model_domain::LogicalInstructionKind::HistoricalSnapshot => {
            ContextUsageKind::BackgroundAndCompaction
        }
        aura_model_domain::LogicalInstructionKind::TaskState => ContextUsageKind::Conversation,
    }
}

const fn section_placement(kind: ContextSectionKind) -> PromptInspectorSectionPlacement {
    match kind {
        ContextSectionKind::CoreSystemPrompt
        | ContextSectionKind::PermissionAndSecurity
        | ContextSectionKind::LoadedToolSchemas
        | ContextSectionKind::WorkspaceAgentsInstructions
        | ContextSectionKind::HistoricalSnapshotSummary
        | ContextSectionKind::AgentModePrompt
        | ContextSectionKind::HookInjections
        | ContextSectionKind::CurrentTaskState => {
            PromptInspectorSectionPlacement::InstructionEnvelope
        }
        ContextSectionKind::RecentHistory => PromptInspectorSectionPlacement::Conversation,
        ContextSectionKind::FileReferencesAndPendingAttachments => {
            PromptInspectorSectionPlacement::Attachment
        }
        ContextSectionKind::CurrentTurnWorkingSet | ContextSectionKind::SteeredUserAdditions => {
            PromptInspectorSectionPlacement::CurrentUserPart
        }
    }
}

const fn usage_label(kind: ContextUsageKind) -> &'static str {
    match kind {
        ContextUsageKind::SystemAndRules => "系统与规则",
        ContextUsageKind::ToolsAndResults => "工具与结果",
        ContextUsageKind::Conversation => "对话内容",
        ContextUsageKind::BackgroundAndCompaction => "背景与压缩历史",
    }
}

fn additions_are_authorized(
    additions: &[AcceptedTurnAdditionV1],
    request: &ContextPreparationRequest,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut previous_order = None;
    additions.iter().all(|addition| {
        let ordered = previous_order.is_none_or(|previous| addition.order > previous);
        previous_order = Some(addition.order);
        ordered
            && addition.revision.value() >= 1
            && addition.session_id == request.session_id
            && addition.turn_task_id == request.turn_task_id
            && seen.insert(addition.user_event_id)
    })
}

fn user_submission_matches(
    assembly: &ContextSectionAssembly,
    addition: &AcceptedTurnAdditionV1,
) -> bool {
    assembly.egress_fragments.iter().any(|fragment| {
        fragment.source_ref == ContextSourceRef::Event { event_id: addition.user_event_id }
            && fragment.source_kind == ModelEgressSourceKind::UserEvent
            && matches!(fragment.authorization_basis, ModelEgressAuthorizationBasis::UserSubmission)
            && ContentHash::sha256(fragment.content.as_bytes()) == addition.content_hash
    })
}

/// Authorized additions may follow a complete group and nothing else may.
/// A missing authorized addition is accepted only when this preparation's
/// compaction summary anchors that exact event and hash.
fn addition_body<'a>(
    items: &'a [LogicalConversationItem],
    additions: &[AcceptedTurnAdditionV1],
    assembly: &ContextSectionAssembly,
    request: &ContextPreparationRequest,
    proofs: &[aura_context_domain::UserSourceProof],
    hops: &[aura_context_domain::CoverageHop],
    invalid: ContextPreparationError,
) -> Result<&'a [LogicalConversationItem], ContextPreparationError> {
    if !additions_are_authorized(additions, request) {
        return Err(invalid);
    }
    if additions.is_empty() {
        return Ok(items);
    }
    let mut end = items.len();
    let mut tail = Vec::new();
    while end > 0 {
        let LogicalConversationItem::User { source_event_id, content_hash, .. } = &items[end - 1]
        else {
            break;
        };
        let Some(addition) =
            additions.iter().find(|addition| addition.user_event_id == *source_event_id)
        else {
            break;
        };
        if addition.content_hash != *content_hash || !user_submission_matches(assembly, addition) {
            return Err(invalid);
        }
        tail.push(addition.user_event_id);
        end -= 1;
    }
    tail.reverse();
    let present = additions.iter().filter(|addition| {
        items.iter().any(|item| conversation_source_event_id(item) == addition.user_event_id)
    });
    if present.map(|addition| addition.user_event_id).ne(tail.iter().copied()) {
        return Err(invalid);
    }
    for addition in additions {
        if tail.contains(&addition.user_event_id) {
            continue;
        }
        match crate::addition_coverage::compaction_covers(assembly, addition, proofs, hops) {
            crate::addition_coverage::AdditionCoverage::Covered => {}
            crate::addition_coverage::AdditionCoverage::Missing => return Err(invalid),
            crate::addition_coverage::AdditionCoverage::BudgetExhausted => {
                return Err(ContextPreparationError::CoverageBudgetExhausted);
            }
        }
    }
    Ok(&items[..end])
}

fn conversation_source_event_id(item: &LogicalConversationItem) -> EventId {
    match item {
        LogicalConversationItem::User { source_event_id, .. }
        | LogicalConversationItem::Assistant { source_event_id, .. }
        | LogicalConversationItem::ToolResult { source_event_id, .. } => *source_event_id,
    }
}

fn enum_wire_name<T: serde::Serialize>(value: &T) -> Result<String, ContextPreparationError> {
    serde_json::to_value(value)
        .map_err(|_| ContextPreparationError::CanonicalSerialization)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or(ContextPreparationError::CanonicalSerialization)
}

fn map_section_error(_error: ContextSectionError) -> ContextPreparationError {
    ContextPreparationError::SectionsInvalid
}

fn map_egress_error(_error: ModelEgressContractError) -> ContextPreparationError {
    ContextPreparationError::EgressInvalid
}

fn map_budget_error(error: ContextBudgetError) -> ContextPreparationError {
    match error {
        ContextBudgetError::ArithmeticOverflow => ContextPreparationError::ArithmeticOverflow,
        _ => ContextPreparationError::BudgetInvalid,
    }
}

fn map_snapshot_error(_error: ContextSnapshotError) -> ContextPreparationError {
    ContextPreparationError::SnapshotInvalid
}

#[cfg(test)]
mod tests {
    use aura_context_domain::{
        ContextCompressibility, ContextPrivacyClass, ModelEgressAuthorizationBasis,
        ModelEgressSourceKind, PromptActivationCondition, PromptModuleSnapshot, PromptSectionClass,
    };
    use aura_domain_common::CanonicalSemVer;
    use aura_domain_common::ToolRegistrySnapshotId;
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{
        AgentRunId, ArtifactId, CorrelationId, ModelProfileId, ModelStepId, PromptBundleSnapshotId,
        ProviderId, Revision, SessionId, SessionSequence, TurnTaskId, WorkspaceId,
    };
    use aura_model_domain::{ModelOutputFormat, ModelOutputRequest};
    use aura_security_contract::{InvocationIdentity, NetworkDataClass};
    use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1};

    use crate::ContextFragment;

    use super::*;

    fn prompt_bundle(turn_task_id: TurnTaskId) -> PromptBundleSnapshot {
        let module_content = "core\n";
        PromptBundleSnapshot {
            id: PromptBundleSnapshotId::new(),
            turn_task_id,
            bundle_version: CanonicalSemVer::new("1.0.0").expect("bundle version"),
            serializer_version: 1,
            module_snapshots: vec![PromptModuleSnapshot {
                module_id: "core.base".to_owned(),
                module_version: CanonicalSemVer::new("1.0.0").expect("module version"),
                section: PromptSectionClass::Core,
                activation: PromptActivationCondition::Always,
                order: 1,
                content_hash: ContentHash::sha256(module_content.as_bytes()),
                content_artifact_id: ArtifactId::new(),
            }],
            custom_addendum_artifact_id: None,
            custom_addendum_hash: None,
            settings_revision: Revision::INITIAL,
            bundle_manifest_hash: ContentHash::sha256(b"manifest"),
            release_identity: "test-release".to_owned(),
            created_at: UtcTimestamp::now(),
        }
    }

    fn input(core: String) -> ContextPreparationInput {
        let session_id = SessionId::new();
        let turn_task_id = TurnTaskId::new();
        let agent_run_id = AgentRunId::new();
        let provider_id = ProviderId::new();
        let endpoint_id = EndpointId::new();
        let model_profile_id = ModelProfileId::new();
        let user_event_id = EventId::new();
        let mut identity =
            InvocationIdentity::runtime_user(WorkspaceId::new(), session_id, CorrelationId::new());
        identity.turn_task_id = Some(turn_task_id);
        identity.agent_run_id = Some(agent_run_id);
        ContextPreparationInput {
            request: ContextPreparationRequest {
                invocation_identity: identity,
                session_id,
                turn_task_id,
                agent_run_id,
                model_step_id: ModelStepId::new(),
                model_profile_id,
                runtime_effective_window_tokens: 32_768,
                output_request: ModelOutputRequest {
                    format: ModelOutputFormat::Text,
                    effective_max_output_tokens: 1_024,
                },
                history_cutoff: SessionSequence::new(9),
            },
            prompt_bundle_snapshot: prompt_bundle(turn_task_id),
            active_prompt_module_ids: vec!["core.base".to_owned()],
            section_inputs: vec![
                ContextSectionInput {
                    kind: ContextSectionKind::CoreSystemPrompt,
                    fragments: vec![ContextFragment {
                        source_ref: aura_context_domain::ContextSourceRef::PromptModule {
                            module_id: "core.base".to_owned(),
                            content_hash: ContentHash::sha256(core.as_bytes()),
                        },
                        source_kind: ModelEgressSourceKind::AuraCore,
                        data_class: NetworkDataClass::PublicContent,
                        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                        content: core,
                    }],
                    compressibility: ContextCompressibility::NeverCompress,
                    privacy_class: ContextPrivacyClass::ModelVisible,
                },
                ContextSectionInput {
                    kind: ContextSectionKind::CurrentTurnWorkingSet,
                    fragments: vec![ContextFragment {
                        source_ref: aura_context_domain::ContextSourceRef::Event {
                            event_id: user_event_id,
                        },
                        source_kind: ModelEgressSourceKind::UserEvent,
                        data_class: NetworkDataClass::PublicQuery,
                        authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
                        content: "Implement the frozen plan.".to_owned(),
                    }],
                    compressibility: ContextCompressibility::NeverCompress,
                    privacy_class: ContextPrivacyClass::ModelVisible,
                },
            ],
            recent_history_items: Vec::new(),
            trigger: ContextPreparationTriggerV1::UserSubmission { user_event_id },
            tool_continuation_facts: None,
            core_continuation_facts: None,
            tool_schema_set: ToolSchemaSetSnapshotV1::new(
                ToolRegistrySnapshotRef {
                    tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                    registry_revision: Revision::INITIAL,
                    content_hash: CanonicalHashV1::sha256(b"preparation-test-registry"),
                },
                Vec::new(),
            )
            .expect("schema set"),
            effective_tool_bindings: Vec::new(),
            tools: Vec::new(),
            attachments: Vec::new(),
            attachment_egress_sources: Vec::new(),
            model_estimator_key: ModelEstimatorKey {
                provider_id,
                provider_model_name: "fixture-model".to_owned(),
            },
            provider_target: ContextProviderTargetV1 {
                provider_id,
                api_format: ModelApiFormat::OpenAiResponses,
                endpoint_id,
                model_profile_id,
                endpoint_fingerprint: ContentHash::sha256(b"preparation-test-endpoint"),
            },
            inspector: PromptInspectorInput::default(),
            previous_snapshot: None,
            created_at: UtcTimestamp::now(),
            compacted_source_proofs: Vec::new(),
            previous_measurement: None,
            coverage_hops: Vec::new(),
        }
    }

    fn tool_continuation_input(with_provider_continuation: bool) -> ContextPreparationInput {
        let mut input = input("Core rules".to_owned());
        let user_event_id = match &input.trigger {
            ContextPreparationTriggerV1::UserSubmission { user_event_id } => *user_event_id,
            _ => unreachable!("base fixture uses a UserSubmission trigger"),
        };
        let assistant_response_event_id = EventId::new();
        let result_event_id = EventId::new();
        let tool_batch_id = aura_domain_common::ToolBatchId::new();
        let tool_call_id = ToolCallId::new();
        let arguments_json = "{}".to_owned();
        let assistant_content = "Calling the fixture tool.".to_owned();
        let result_content = r#"{"ok":true}"#.to_owned();
        let group = HistoryAtomicGroupPreimageV1 {
            kind: aura_context_domain::HistoryAtomicGroupKindV1,
            preimage_version: aura_domain_common::ToolWireVersionV1,
            session_id: input.request.session_id,
            turn_task_id: input.request.turn_task_id,
            agent_run_id: input.request.agent_run_id,
            model_step_id: input.request.model_step_id,
            assistant_response_event_id,
            assistant_response_content_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(
                assistant_content.as_bytes(),
            ),
            tool_batch_id,
            calls: vec![aura_context_domain::HistoryToolCallResultEntryV1 {
                ordinal: 1,
                tool_call_id,
                tool_call_fact_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(
                    b"call",
                ),
                tool_result_fact_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(
                    b"result",
                ),
            }],
            batch_summary_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"batch"),
        };
        let group_ref = group.group_ref().expect("history group");
        let provider_continuation = with_provider_continuation.then(|| {
            let state = ProviderContinuationState::OpenAiResponses { state_ref: ArtifactId::new() };
            ProviderContinuationBindingV1 {
                state,
                provider_id: input.provider_target.provider_id,
                api_format: input.provider_target.api_format,
                endpoint_id: input.provider_target.endpoint_id,
                model_profile_id: input.provider_target.model_profile_id,
                endpoint_fingerprint: input.provider_target.endpoint_fingerprint.clone(),
                previous_model_execution_id: ModelExecutionSnapshotId::new(),
                provider_response_ref: ArtifactId::new(),
                request_evidence_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(
                    b"request",
                ),
                response_evidence_hash: aura_domain_common::jcs::CanonicalSha256HashV1::sha256(
                    b"response",
                ),
                assistant_response_event_id,
                tool_batch_id,
            }
        });
        input.request.model_step_id = ModelStepId::new();
        input.trigger = ContextPreparationTriggerV1::ToolContinuation {
            originating_user_event_id: user_event_id,
            previous_model_step_id: group.model_step_id,
            assistant_response_event_id,
            tool_batch_id,
            history_atomic_group_ref: group_ref,
            provider_continuation: provider_continuation.clone(),
        };
        input.tool_continuation_facts = Some(ToolContinuationFactsV1 {
            history_atomic_group: group,
            previous_model_step_legally_terminal: true,
            assistant_response_durable: true,
            tool_batch_terminal: true,
            durable_result_tool_call_ids: vec![tool_call_id],
            verified_provider_continuation: provider_continuation,
            accepted_additions: vec![],
        });
        input.recent_history_items = vec![
            LogicalConversationItem::User {
                source_event_id: user_event_id,
                content: "Implement the frozen plan.".to_owned(),
                content_hash: ContentHash::sha256(b"Implement the frozen plan."),
            },
            LogicalConversationItem::Assistant {
                provider_history: None,
                source_event_id: assistant_response_event_id,
                content: Some(assistant_content.clone()),
                content_hash: Some(ContentHash::sha256(assistant_content.as_bytes())),
                tool_calls: vec![aura_model_domain::LogicalHistoricalToolCall {
                    tool_call_id,
                    tool_id: Some(ToolId::new()),
                    canonical_name: "fixture.read".to_owned(),
                    arguments_hash: ContentHash::sha256(arguments_json.as_bytes()),
                    arguments_json,
                }],
            },
            LogicalConversationItem::ToolResult {
                source_event_id: result_event_id,
                tool_call_id,
                tool_name: "fixture.read".to_owned(),
                content_hash: ContentHash::sha256(result_content.as_bytes()),
                content: result_content.clone(),
                is_error: false,
            },
        ];
        let history_section = input
            .section_inputs
            .iter_mut()
            .find(|section| section.kind == ContextSectionKind::CurrentTurnWorkingSet)
            .expect("base current-turn section");
        history_section.kind = ContextSectionKind::RecentHistory;
        history_section.fragments.extend([
            ContextFragment {
                source_ref: aura_context_domain::ContextSourceRef::Event {
                    event_id: assistant_response_event_id,
                },
                source_kind: ModelEgressSourceKind::AssistantEvent,
                data_class: NetworkDataClass::PublicContent,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: assistant_content,
            },
            ContextFragment {
                source_ref: aura_context_domain::ContextSourceRef::Event {
                    event_id: result_event_id,
                },
                source_kind: ModelEgressSourceKind::ToolResult,
                data_class: NetworkDataClass::PublicContent,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: result_content,
            },
        ]);
        input
    }

    #[test]
    fn preparation_freezes_one_consistent_context_egress_budget_and_inspector() {
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        let outcome = compiler.compile(input("Core rules".to_owned())).expect("compile");
        let ContextPreparationCompilationOutcome::Prepared(prepared) = outcome else {
            panic!("fixture must fit without compaction");
        };
        assert_eq!(prepared.context.snapshot.sections.len(), 12);
        assert_eq!(prepared.context.snapshot.active_prompt_module_ids, vec!["core.base"]);
        assert_eq!(prepared.context.snapshot.context_usage.segments.len(), 4);
        assert!(prepared.context.snapshot.context_usage.is_consistent());
        assert_eq!(
            prepared.context.snapshot.budget.estimated_input_tokens,
            prepared.context.snapshot.context_usage.used_input_tokens
        );
        prepared.context.snapshot.validate_contract_shape().expect("snapshot");
        prepared.context.model_egress_manifest.validate_contract_shape().expect("egress manifest");
        assert_eq!(prepared.artifacts.len(), 14);
        assert_eq!(
            prepared.prompt_inspector.context_snapshot_id,
            Some(prepared.context.snapshot.id)
        );
        let current_user = prepared.context.conversation_items.last().expect("current user");
        let LogicalConversationItem::User { content, .. } = current_user else {
            panic!("last conversation item must be current user");
        };
        // The turn's first step sends its message as later ones do.
        assert_eq!(content, "Implement the frozen plan.");
        assert!(!content.contains("Current-Turn-Working-Set"));
    }

    #[test]
    fn tool_continuation_without_provider_state_rebuilds_full_history_and_continues() {
        let input = tool_continuation_input(false);
        let originating_user_event_id = match &input.trigger {
            ContextPreparationTriggerV1::ToolContinuation { originating_user_event_id, .. } => {
                *originating_user_event_id
            }
            _ => unreachable!(),
        };
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        let ContextPreparationCompilationOutcome::Prepared(prepared) =
            compiler.compile(input).expect("compile full-history continuation")
        else {
            panic!("continuation fixture must fit")
        };
        assert!(prepared.context.provider_continuation.is_none());
        assert_eq!(prepared.context.conversation_items.len(), 3);
        assert!(matches!(
            &prepared.context.conversation_items[0],
            LogicalConversationItem::User { source_event_id, .. }
                if *source_event_id == originating_user_event_id
        ));
        assert_eq!(
            prepared
                .context
                .conversation_items
                .iter()
                .filter(|item| matches!(item, LogicalConversationItem::User { .. }))
                .count(),
            1,
            "ToolContinuation must not create or copy a new User Event",
        );
    }

    #[test]
    fn provider_continuation_rejects_an_endpoint_binding_mismatch() {
        let mut input = tool_continuation_input(true);
        input.provider_target.endpoint_id = EndpointId::new();
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        assert!(matches!(
            compiler.compile(input),
            Err(ContextPreparationError::ProviderContinuationBindingMismatch)
        ));
    }

    #[test]
    fn non_compressible_overflow_requires_remediation_without_creating_a_snapshot() {
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        let outcome = compiler.compile(input("x".repeat(40_000))).expect("compile");
        let ContextPreparationCompilationOutcome::AwaitingRemediation { plan, .. } = outcome else {
            panic!("non-compressible overflow must not request compaction");
        };
        assert_eq!(plan.overflow.error_code, "context.non_compressible_overflow");
        assert!(plan.overflow.overflow_tokens > 0);
    }

    #[test]
    fn context_window_rebuild_requires_new_model_step_and_creates_new_cps() {
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        let original_input = input("Core rules".to_owned());
        let ContextPreparationCompilationOutcome::Prepared(original) =
            compiler.compile(original_input.clone()).expect("compile original context")
        else {
            panic!("original context must be prepared");
        };
        let directive = ContextWindowRebuildDirective {
            failed_execution_snapshot_id: ModelExecutionSnapshotId::new(),
            failed_context_snapshot_id: original.context.snapshot.id,
            failed_model_step_id: original.context.snapshot.model_step_id,
            prompt_bundle_snapshot_id: original.context.snapshot.prompt_bundle_snapshot_id,
            model_profile_id: original.context.snapshot.model_profile_id,
            runtime_effective_window_tokens: 32_768,
            provider_error_ref: ArtifactId::new(),
        };

        let mut reused_step = original_input.clone();
        reused_step.previous_snapshot = Some(original.context.snapshot.clone());
        assert_eq!(
            compiler
                .compile_after_context_window_resolution(reused_step, &directive)
                .expect_err("failed ModelStep must not be reused"),
            ContextWindowRebuildError::NewModelStepRequired
        );

        let mut rebuild = original_input;
        rebuild.request.model_step_id = ModelStepId::new();
        rebuild.previous_snapshot = Some(original.context.snapshot.clone());
        let ContextPreparationCompilationOutcome::Prepared(rebuilt) = compiler
            .compile_after_context_window_resolution(rebuild, &directive)
            .expect("compile rebuilt context")
        else {
            panic!("rebuilt fixture must fit without compaction");
        };
        assert_ne!(rebuilt.context.model_step_id, original.context.model_step_id);
        assert_ne!(rebuilt.context.context_snapshot_id, original.context.context_snapshot_id);
        assert_eq!(
            rebuilt.context.prompt_bundle_snapshot_id, original.context.prompt_bundle_snapshot_id,
            "same TurnTask must reuse its frozen PBS"
        );
        assert_eq!(
            original.context.snapshot.id, directive.failed_context_snapshot_id,
            "the failed CPS remains immutable and separately addressable"
        );
    }

    fn authorize_addition(
        input: &mut ContextPreparationInput,
        text: &str,
        present: bool,
    ) -> EventId {
        let event_id = EventId::new();
        let content_hash = ContentHash::sha256(text.as_bytes());
        if present {
            input.recent_history_items.push(LogicalConversationItem::User {
                source_event_id: event_id,
                content: text.to_owned(),
                content_hash: content_hash.clone(),
            });
            input
                .section_inputs
                .iter_mut()
                .find(|section| section.kind == ContextSectionKind::RecentHistory)
                .expect("history section")
                .fragments
                .push(ContextFragment {
                    source_ref: aura_context_domain::ContextSourceRef::Event { event_id },
                    source_kind: ModelEgressSourceKind::UserEvent,
                    data_class: NetworkDataClass::UserFileContent,
                    authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
                    content: text.to_owned(),
                });
        }
        let facts = input.tool_continuation_facts.as_mut().expect("tool facts");
        let order = u32::try_from(facts.accepted_additions.len()).expect("order") + 1;
        facts.accepted_additions.push(AcceptedTurnAdditionV1 {
            user_event_id: event_id,
            content_hash: content_hash.clone(),
            session_id: input.request.session_id,
            turn_task_id: input.request.turn_task_id,
            revision: Revision::INITIAL,
            order,
        });
        event_id
    }

    fn compile_prepared(input: ContextPreparationInput) -> PreparedContextDraft {
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        let ContextPreparationCompilationOutcome::Prepared(prepared) =
            compiler.compile(input).expect("authorized addition compiles")
        else {
            panic!("addition fixture must fit");
        };
        *prepared
    }

    #[test]
    fn tool_continuation_accepts_consumed_additions_only_after_the_complete_group() {
        let mut input = tool_continuation_input(false);
        authorize_addition(&mut input, "Record the path.", true);
        authorize_addition(&mut input, "Record the line.", true);
        let prepared = compile_prepared(input);
        let texts = prepared
            .context
            .conversation_items
            .iter()
            .rev()
            .take(2)
            .filter_map(|item| match item {
                LogicalConversationItem::User { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["Record the line.", "Record the path."]);
    }

    #[test]
    fn tool_continuation_rejects_an_addition_inside_the_tool_group() {
        let mut input = tool_continuation_input(false);
        let addition = input.recent_history_items.pop().expect("tool result");
        authorize_addition(&mut input, "Inside the group.", true);
        input.recent_history_items.push(addition);
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        assert!(matches!(
            compiler.compile(input),
            Err(ContextPreparationError::ToolContinuationInvalid)
        ));
    }

    #[test]
    fn tool_continuation_rejects_a_foreign_session_addition() {
        let mut input = tool_continuation_input(false);
        authorize_addition(&mut input, "Foreign.", true);
        input.tool_continuation_facts.as_mut().expect("facts").accepted_additions[0].session_id =
            SessionId::new();
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        assert!(matches!(
            compiler.compile(input),
            Err(ContextPreparationError::ToolContinuationInvalid)
        ));
    }

    #[test]
    fn tool_continuation_accepts_a_compacted_addition_only_with_its_anchor() {
        let mut input = tool_continuation_input(false);
        let event_id = authorize_addition(&mut input, "Compacted addition.", false);
        let hash = aura_context_domain::user_history_anchor_hash(event_id, "Compacted addition.")
            .expect("production history-item anchor");
        input.compacted_source_proofs.push(
            aura_context_domain::prove_user_source(event_id, "Compacted addition.", &hash)
                .expect("proof bound from the stored text"),
        );
        let result = aura_context_domain::ContextReadCompactionResult {
            source_context_snapshot_id: ContextProjectionSnapshotId::new(),
            sequence: 1,
            historical_snapshot: "history".to_owned(),
            active_turn_summary: "active".to_owned(),
            structured_compaction: "structured".to_owned(),
            source_span: aura_context_domain::CompactionSourceSpan {
                start_sequence: SessionSequence::new(1),
                end_sequence: SessionSequence::new(2),
                source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id }],
            },
            anchor_manifest: aura_context_domain::CompactionAnchorManifest {
                anchors: vec![aura_context_domain::CompactionAnchor {
                    anchor: "addition".to_owned(),
                    kind: aura_context_domain::CompactionAnchorKind::Event,
                    source_ref: aura_context_domain::ContextSourceRef::Event { event_id },
                    source_hash: hash,
                    preview: "Compacted addition.".to_owned(),
                }],
                manifest_hash: ContentHash::sha256(b"manifest"),
            },
        };
        input
            .section_inputs
            .iter_mut()
            .find(|section| section.kind == ContextSectionKind::RecentHistory)
            .expect("history section")
            .fragments
            .push(ContextFragment {
                source_ref: aura_context_domain::ContextSourceRef::Compaction {
                    context_snapshot_id: result.source_context_snapshot_id,
                },
                source_kind: ModelEgressSourceKind::Compaction,
                data_class: NetworkDataClass::WorkspaceSource,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: serde_json::to_string(&result).expect("compaction result"),
            });
        let prepared = compile_prepared(input);
        assert!(
            !prepared
                .context
                .conversation_items
                .iter()
                .any(|item| { conversation_source_event_id(item) == event_id })
        );

        let mut missing = tool_continuation_input(false);
        authorize_addition(&mut missing, "Compacted addition.", false);
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        assert!(matches!(
            compiler.compile(missing),
            Err(ContextPreparationError::ToolContinuationInvalid)
        ));
    }

    #[test]
    fn tool_continuation_keeps_a_compacted_addition_after_its_text_is_removed() {
        let mut input = tool_continuation_input(false);
        let first_text = "First consumed addition.";
        let second_text = "Second consumed addition.";
        let first = authorize_addition(&mut input, first_text, true);
        authorize_addition(&mut input, second_text, true);
        compile_prepared(input.clone());
        input.recent_history_items.retain(|item| conversation_source_event_id(item) != first);
        for section in &mut input.section_inputs {
            section.fragments.retain(|fragment| {
                fragment.source_ref
                    != aura_context_domain::ContextSourceRef::Event { event_id: first }
            });
        }
        let hash =
            aura_context_domain::user_history_anchor_hash(first, first_text).expect("anchor");
        input
            .compacted_source_proofs
            .push(aura_context_domain::prove_user_source(first, first_text, &hash).expect("proof"));
        let result = aura_context_domain::ContextReadCompactionResult {
            source_context_snapshot_id: ContextProjectionSnapshotId::new(),
            sequence: 1,
            historical_snapshot: "Earlier input".to_owned(),
            active_turn_summary: "Both additions apply".to_owned(),
            structured_compaction: "summary".to_owned(),
            source_span: aura_context_domain::CompactionSourceSpan {
                start_sequence: SessionSequence::new(5),
                end_sequence: SessionSequence::new(6),
                source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id: first }],
            },
            anchor_manifest: aura_context_domain::CompactionAnchorManifest {
                manifest_hash: ContentHash::sha256(b"manifest"),
                anchors: vec![aura_context_domain::CompactionAnchor {
                    anchor: "a000001".to_owned(),
                    kind: aura_context_domain::CompactionAnchorKind::Event,
                    source_ref: aura_context_domain::ContextSourceRef::Event { event_id: first },
                    source_hash: hash,
                    preview: first_text.to_owned(),
                }],
            },
        };
        input.section_inputs.push(ContextSectionInput {
            kind: ContextSectionKind::HistoricalSnapshotSummary,
            compressibility: ContextCompressibility::CompressibleHistory,
            privacy_class: ContextPrivacyClass::ModelVisible,
            fragments: vec![ContextFragment {
                source_ref: aura_context_domain::ContextSourceRef::Compaction {
                    context_snapshot_id: result.source_context_snapshot_id,
                },
                source_kind: ModelEgressSourceKind::Compaction,
                data_class: NetworkDataClass::WorkspaceSource,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: serde_json::to_string(&result).expect("result"),
            }],
        });
        let prepared = compile_prepared(input);
        let texts = prepared
            .context
            .conversation_items
            .iter()
            .filter_map(|item| match item {
                LogicalConversationItem::User { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(texts.contains(&second_text));
        assert!(!texts.iter().any(|text| text.contains(first_text)));
    }

    #[test]
    fn tool_continuation_follows_a_previous_summary_to_a_compacted_addition() {
        let mut input = tool_continuation_input(false);
        let text = "Compacted addition.";
        let event_id = authorize_addition(&mut input, text, false);
        let anchor_hash =
            aura_context_domain::user_history_anchor_hash(event_id, text).expect("anchor");
        input.compacted_source_proofs.push(
            aura_context_domain::prove_user_source(event_id, text, &anchor_hash).expect("proof"),
        );
        let origin_artifact = ArtifactId::new();
        let origin_id = ContextProjectionSnapshotId::new();
        let origin_anchors = aura_context_domain::CompactionAnchorManifest {
            manifest_hash: ContentHash::sha256(b"origin"),
            anchors: vec![aura_context_domain::CompactionAnchor {
                anchor: "event".to_owned(),
                kind: aura_context_domain::CompactionAnchorKind::Event,
                source_ref: aura_context_domain::ContextSourceRef::Event { event_id },
                source_hash: anchor_hash,
                preview: "preview".to_owned(),
            }],
        };
        let origin = aura_context_domain::ContextReadCompactionResult {
            source_context_snapshot_id: origin_id,
            sequence: 1,
            historical_snapshot: "history".to_owned(),
            active_turn_summary: "active".to_owned(),
            structured_compaction: "structured".to_owned(),
            source_span: aura_context_domain::CompactionSourceSpan {
                start_sequence: SessionSequence::new(1),
                end_sequence: SessionSequence::new(1),
                source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id }],
            },
            anchor_manifest: origin_anchors.clone(),
        };
        let origin_hop = aura_context_domain::CoverageHop::from_record(
            origin_id,
            input.request.session_id,
            true,
            &origin_anchors,
            vec![event_id],
            Some(origin_artifact),
            Some(origin),
        );
        let latest_id = ContextProjectionSnapshotId::new();
        let latest_hash = aura_context_domain::compaction_result_anchor_hash(
            origin_hop.result.as_ref().expect("origin"),
        )
        .expect("previous hash");
        let latest_anchors = aura_context_domain::CompactionAnchorManifest {
            manifest_hash: ContentHash::sha256(b"latest"),
            anchors: vec![aura_context_domain::CompactionAnchor {
                anchor: "previous".to_owned(),
                kind: aura_context_domain::CompactionAnchorKind::Artifact,
                source_ref: aura_context_domain::ContextSourceRef::Artifact {
                    artifact_id: origin_artifact,
                },
                source_hash: latest_hash,
                preview: "previous verified compaction".to_owned(),
            }],
        };
        let latest = aura_context_domain::ContextReadCompactionResult {
            source_context_snapshot_id: latest_id,
            sequence: 2,
            historical_snapshot: "later".to_owned(),
            active_turn_summary: "later".to_owned(),
            structured_compaction: "later".to_owned(),
            source_span: aura_context_domain::CompactionSourceSpan {
                start_sequence: SessionSequence::new(2),
                end_sequence: SessionSequence::new(2),
                source_refs: vec![aura_context_domain::ContextSourceRef::Artifact {
                    artifact_id: origin_artifact,
                }],
            },
            anchor_manifest: latest_anchors.clone(),
        };
        let latest_hop = aura_context_domain::CoverageHop::from_record(
            latest_id,
            input.request.session_id,
            true,
            &latest_anchors,
            vec![event_id],
            Some(ArtifactId::new()),
            Some(latest.clone()),
        );
        input.coverage_hops = vec![latest_hop, origin_hop];
        input.section_inputs.push(ContextSectionInput {
            kind: ContextSectionKind::HistoricalSnapshotSummary,
            compressibility: ContextCompressibility::CompressibleHistory,
            privacy_class: ContextPrivacyClass::ModelVisible,
            fragments: vec![ContextFragment {
                source_ref: aura_context_domain::ContextSourceRef::Compaction {
                    context_snapshot_id: latest_id,
                },
                source_kind: ModelEgressSourceKind::Compaction,
                data_class: NetworkDataClass::WorkspaceSource,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: serde_json::to_string(&latest).expect("result"),
            }],
        });
        compile_prepared(input);
    }

    #[test]
    fn tool_continuation_rejects_a_compacted_addition_anchored_by_its_body_hash() {
        let mut input = tool_continuation_input(false);
        let event_id = authorize_addition(&mut input, "Compacted addition.", false);
        let real = aura_context_domain::user_history_anchor_hash(event_id, "Compacted addition.")
            .expect("production anchor");
        input.compacted_source_proofs.push(
            aura_context_domain::prove_user_source(event_id, "Compacted addition.", &real)
                .expect("proof"),
        );
        let result = aura_context_domain::ContextReadCompactionResult {
            source_context_snapshot_id: ContextProjectionSnapshotId::new(),
            sequence: 1,
            historical_snapshot: "history".to_owned(),
            active_turn_summary: "active".to_owned(),
            structured_compaction: "structured".to_owned(),
            source_span: aura_context_domain::CompactionSourceSpan {
                start_sequence: SessionSequence::new(1),
                end_sequence: SessionSequence::new(2),
                source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id }],
            },
            anchor_manifest: aura_context_domain::CompactionAnchorManifest {
                anchors: vec![aura_context_domain::CompactionAnchor {
                    anchor: "addition".to_owned(),
                    kind: aura_context_domain::CompactionAnchorKind::Event,
                    source_ref: aura_context_domain::ContextSourceRef::Event { event_id },
                    source_hash: ContentHash::sha256(b"Compacted addition."),
                    preview: "Compacted addition.".to_owned(),
                }],
                manifest_hash: ContentHash::sha256(b"manifest"),
            },
        };
        input
            .section_inputs
            .iter_mut()
            .find(|section| section.kind == ContextSectionKind::RecentHistory)
            .expect("history section")
            .fragments
            .push(ContextFragment {
                source_ref: aura_context_domain::ContextSourceRef::Compaction {
                    context_snapshot_id: result.source_context_snapshot_id,
                },
                source_kind: ModelEgressSourceKind::Compaction,
                data_class: NetworkDataClass::WorkspaceSource,
                authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
                content: serde_json::to_string(&result).expect("compaction result"),
            });
        let estimators = ContextEstimatorRegistry::default();
        let compiler = ContextPreparationCompiler::new(&estimators);
        assert!(matches!(
            compiler.compile(input),
            Err(ContextPreparationError::ToolContinuationInvalid)
        ));
    }

    #[test]
    fn core_continuation_accepts_additions_after_the_visible_assistant() {
        let mut input = tool_continuation_input(false);
        let tool_facts = input.tool_continuation_facts.take().expect("tool facts");
        input.recent_history_items.pop();
        let LogicalConversationItem::Assistant { tool_calls, .. } =
            input.recent_history_items.last_mut().expect("assistant")
        else {
            panic!("plain-text history ends with the assistant");
        };
        tool_calls.clear();
        let LogicalConversationItem::User { source_event_id: originating_user_event_id, .. } =
            input.recent_history_items[0]
        else {
            panic!("history starts with the turn root");
        };
        let assistant = tool_facts.history_atomic_group.assistant_response_event_id;
        let previous_model_step_id = tool_facts.history_atomic_group.model_step_id;
        let hash = aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"checkpoint");
        input.trigger = ContextPreparationTriggerV1::CoreContinuation {
            originating_user_event_id,
            previous_model_step_id,
            assistant_response_event_id: assistant,
            checkpoint_hash: hash.clone(),
        };
        input.core_continuation_facts = Some(CoreContinuationFactsV1 {
            session_id: input.request.session_id,
            turn_task_id: input.request.turn_task_id,
            agent_run_id: input.request.agent_run_id,
            previous_model_step_id,
            assistant_response_event_id: assistant,
            checkpoint_hash: hash,
            previous_response_visible: true,
            accepted_additions: vec![AcceptedTurnAdditionV1 {
                user_event_id: EventId::new(),
                content_hash: ContentHash::sha256(b"Seen addition."),
                session_id: input.request.session_id,
                turn_task_id: input.request.turn_task_id,
                revision: Revision::INITIAL,
                order: 1,
            }],
        });
        let addition =
            input.core_continuation_facts.as_ref().expect("core facts").accepted_additions[0]
                .clone();
        input.recent_history_items.push(LogicalConversationItem::User {
            source_event_id: addition.user_event_id,
            content: "Seen addition.".to_owned(),
            content_hash: addition.content_hash.clone(),
        });
        input
            .section_inputs
            .iter_mut()
            .find(|section| section.kind == ContextSectionKind::RecentHistory)
            .expect("history section")
            .fragments
            .push(ContextFragment {
                source_ref: aura_context_domain::ContextSourceRef::Event {
                    event_id: addition.user_event_id,
                },
                source_kind: ModelEgressSourceKind::UserEvent,
                data_class: NetworkDataClass::UserFileContent,
                authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
                content: "Seen addition.".to_owned(),
            });
        let prepared = compile_prepared(input);
        assert!(matches!(
            prepared.context.conversation_items.last(),
            Some(LogicalConversationItem::User { content, .. }) if content == "Seen addition."
        ));
    }
}
