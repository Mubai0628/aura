use crate::{AgentError, AgentRun, SourceMembership, TaskLineage, TaskLineageState, TurnTask};
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "policy", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompletionPolicy {
    BestEffortBalanced,
    StrictCompletionRequired { source_event_ids: Vec<EventId> },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NormalCompletionOutcome {
    FullySatisfied,
    BestEffortBalanced,
    /// The model explicitly ended the task; Core does not certify quality.
    ModelDecision,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "gate", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompletionGate {
    Unreviewed,
    ReviewedNotReady {
        review_id: CompletionReviewId,
        lineage_revision: Revision,
    },
    ReviewedReady {
        review_id: CompletionReviewId,
        lineage_revision: Revision,
        outcome: NormalCompletionOutcome,
    },
    Invalidated {
        reason: CompletionInvalidation,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompletionInvalidation {
    NewUserSourceAttached,
    SteerAttached,
    QueryAnswerAttached,
    RelationChanged,
    CompletionPolicyChanged,
    LineageSwitched,
    SourceReclassified,
    ContextCompacted,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceReadReceipt {
    pub receipt_id: SourceReadReceiptId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub lineage_id: TaskLineageId,
    pub lineage_revision: Revision,
    pub source_event_hashes: Vec<(EventId, ContentHash)>,
    pub created_at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageReadSourcesInput {
    pub lineage_id: TaskLineageId,
    pub expected_lineage_revision: Revision,
    pub source_event_ids: Vec<EventId>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageReadSourcesResult {
    pub lineage_id: TaskLineageId,
    pub lineage_revision: Revision,
    pub messages: Vec<crate::OriginalUserMessage>,
    pub source_read_receipt_id: SourceReadReceiptId,
    pub completion_source_recheck_required: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "assessment", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompletionAssessment {
    FullySatisfied,
    BestEffortBalanced {
        unresolved_intents: Vec<String>,
        limiting_factors: Vec<String>,
        attempted_alternatives: Vec<String>,
        why_further_work_is_not_reasonable: String,
    },
    NotReady {
        remaining_work: Vec<String>,
        next_actions: Vec<String>,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewedIntentResult {
    Satisfied,
    SatisfiedWithinUserRequestedScope,
    DeferredByUser,
    CancelledByUser,
    Unresolved,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewedUserIntent {
    pub description: String,
    pub source_event_ids: Vec<EventId>,
    pub result: ReviewedIntentResult,
    /// Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
    /// commands. An artifact id from a tool result must be one of this run's.
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageReviewCompletionInput {
    pub lineage_id: TaskLineageId,
    pub expected_lineage_revision: Revision,
    pub source_read_receipt_id: Option<SourceReadReceiptId>,
    pub assessment: CompletionAssessment,
    pub reviewed_user_intents: Vec<ReviewedUserIntent>,
    /// Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
    /// commands. An artifact id from a tool result must be one of this run's.
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompletionReview {
    pub review_id: CompletionReviewId,
    pub lineage_id: TaskLineageId,
    pub lineage_revision: Revision,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub input: TaskLineageReviewCompletionInput,
    pub created_at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NormalCompleteInput {
    pub lineage_id: TaskLineageId,
    pub expected_lineage_revision: Revision,
    pub final_report: String,
    /// Default empty.
    #[serde(default)]
    pub disclosed_limitations: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NormalCompletionRecord {
    pub record_id: NormalCompletionRecordId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub lineage_id: TaskLineageId,
    /// Historical review binding; new completions need no review.
    pub review_id: Option<CompletionReviewId>,
    pub outcome: NormalCompletionOutcome,
    pub final_report: String,
    pub disclosed_limitations: Vec<String>,
    pub at: UtcTimestamp,
}
pub struct CompletionContext<'a> {
    pub turn: &'a TurnTask,
    pub run: &'a AgentRun,
    pub model_step_id: ModelStepId,
    pub at: UtcTimestamp,
}

/// The protocol's own, trustworthy end of turn for a Normal response. Only a
/// complete normal end is eligible; truncation, refusal, filtered output and
/// interrupted streams normalize to other reasons and never complete a turn.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NaturalCompletionBasis {
    /// The Provider reported a normal, complete end of turn (`stop`,
    /// `response.completed` without pending calls, or `stop_reason=end_turn`).
    EndTurn,
}

/// A Normal turn ended by the model's own final response instead of an explicit
/// completion tool call. This is a separate record from `NormalCompletionRecord`
/// (the model-visible `normal.complete` result): nothing fakes a tool call, and
/// the real response, its artifact and the end-of-turn basis stay inspectable.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NaturalCompletionRecord {
    pub record_id: NaturalCompletionRecordId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub lineage_id: TaskLineageId,
    /// The ModelStep whose response supplied the final answer.
    pub model_step_id: ModelStepId,
    /// The exact normalized response artifact the final text was taken from.
    pub output_artifact_id: ArtifactId,
    pub basis: NaturalCompletionBasis,
    pub final_report: String,
    pub at: UtcTimestamp,
}

/// Where the final answer of a completed turn really comes from. The desktop
/// projection carries this so one source is rendered once instead of comparing
/// report strings, which never matched a model-rewritten final answer.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentFinalTextSource {
    /// The final area shows this ModelStep's own response text, read through the
    /// existing complete-text route. The same step contributes no process text.
    AssistantStep { model_step_id: ModelStepId },
    /// Completion carried its own report (explicit completion tool calls, Plan
    /// and Goal completion, and every historical turn).
    CompletionReport,
}

fn check_context(lineage: &TaskLineage, ctx: &CompletionContext<'_>) -> Result<(), AgentError> {
    // Plan and Goal are session modes. normal.complete ends this turn only.
    if ctx.turn.state.terminal()
        || ctx.turn.lineage_resolution_required
        || ctx.run.parent_agent_run_id.is_some()
        || ctx.turn.agent_run_id != ctx.run.agent_run_id
        || ctx.run.turn_task_id != ctx.turn.turn_task_id
        || ctx.turn.lineage_id != lineage.lineage_id
        || ctx.turn.session_id != lineage.session_id
        || lineage.state != TaskLineageState::Active
        || lineage.last_active_turn_task_id != ctx.turn.turn_task_id
    {
        return Err(AgentError::IdentityMismatch);
    }
    Ok(())
}
impl TaskLineage {
    pub fn review_completion(
        &mut self,
        input: TaskLineageReviewCompletionInput,
        receipt: Option<&SourceReadReceipt>,
        review_id: CompletionReviewId,
        ctx: &CompletionContext<'_>,
    ) -> Result<CompletionReview, AgentError> {
        check_context(self, ctx)?;
        if input.lineage_id != self.lineage_id || input.expected_lineage_revision != self.revision {
            return Err(AgentError::RevisionConflict);
        }
        let active: BTreeSet<_> = self
            .sources
            .iter()
            .filter(|s| s.membership == SourceMembership::Active)
            .map(|s| s.user_event_id)
            .collect();
        // A voluntary source receipt must be authentic if supplied.
        if let Some(receipt_id) = input.source_read_receipt_id {
            let receipt = receipt.ok_or(AgentError::SourceReadRequired)?;
            if receipt_id != receipt.receipt_id {
                return Err(AgentError::SourceReadRequired);
            }
            if input.source_read_receipt_id != Some(receipt.receipt_id)
                || receipt.session_id != self.session_id
                || receipt.lineage_id != self.lineage_id
                || receipt.lineage_revision != self.revision
                || receipt.turn_task_id != ctx.turn.turn_task_id
                || receipt.agent_run_id != ctx.run.agent_run_id
                || receipt.model_step_id == ctx.model_step_id
                || receipt.created_at > ctx.at
                || receipt.source_event_hashes.len() != active.len()
                || receipt.source_event_hashes.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>()
                    != active
                || receipt.source_event_hashes.iter().any(|(id, hash)| {
                    !self.sources.iter().any(|s| s.user_event_id == *id && s.content_hash == *hash)
                })
            {
                return Err(AgentError::SourceReadRequired);
            }
        }
        for intent in &input.reviewed_user_intents {
            if intent.source_event_ids.iter().any(|id| !active.contains(id)) {
                return Err(AgentError::SourceInvalid);
            }
        }
        // This is a voluntary model assessment, never a Core quality gate.
        let outcome = match &input.assessment {
            CompletionAssessment::FullySatisfied => Some(NormalCompletionOutcome::FullySatisfied),
            CompletionAssessment::BestEffortBalanced { .. } => {
                Some(NormalCompletionOutcome::BestEffortBalanced)
            }
            CompletionAssessment::NotReady { .. } => None,
        };
        self.advance_revision(input.expected_lineage_revision, ctx.at)?;
        self.completion_gate = match outcome {
            Some(outcome) => CompletionGate::ReviewedReady {
                review_id,
                lineage_revision: self.revision,
                outcome,
            },
            None => CompletionGate::ReviewedNotReady { review_id, lineage_revision: self.revision },
        };
        Ok(CompletionReview {
            review_id,
            lineage_id: self.lineage_id,
            lineage_revision: self.revision,
            turn_task_id: ctx.turn.turn_task_id,
            agent_run_id: ctx.run.agent_run_id,
            model_step_id: ctx.model_step_id,
            input,
            created_at: ctx.at,
        })
    }
    pub fn check_normal_completion(
        &self,
        input: &NormalCompleteInput,
        ctx: &CompletionContext<'_>,
    ) -> Result<NormalCompletionOutcome, AgentError> {
        check_context(self, ctx)?;
        if input.lineage_id != self.lineage_id || input.expected_lineage_revision != self.revision {
            return Err(AgentError::RevisionConflict);
        }
        if input.final_report.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        Ok(NormalCompletionOutcome::ModelDecision)
    }
}
