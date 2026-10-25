use crate::*;
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskLineageReconcileDecision {
    ContinueExisting,
    ClarifyExisting,
    ModifyExisting,
    ExpandExisting,
    NarrowExisting,
    DeferPart,
    CancelPart,
    Mixed,
    /// Historical persisted value. New execution rejects reactivation.
    SuspendAndStartNew,
    /// Historical persisted value. New execution never reactivates the target.
    ResumeSuspended {
        lineage_id: TaskLineageId,
    },
    CancelExisting,
    ExpirePrior {
        lineage_id: TaskLineageId,
    },
    ReferencePrior {
        lineage_id: TaskLineageId,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskLineageReconcileToolDecision {
    ContinueExisting,
    ClarifyExisting,
    ModifyExisting,
    ExpandExisting,
    NarrowExisting,
    DeferPart,
    CancelPart,
    Mixed,
    CancelExisting,
    ExpirePrior { lineage_id: TaskLineageId },
    ReferencePrior { lineage_id: TaskLineageId },
}
impl From<TaskLineageReconcileToolDecision> for TaskLineageReconcileDecision {
    fn from(value: TaskLineageReconcileToolDecision) -> Self {
        match value {
            TaskLineageReconcileToolDecision::ContinueExisting => Self::ContinueExisting,
            TaskLineageReconcileToolDecision::ClarifyExisting => Self::ClarifyExisting,
            TaskLineageReconcileToolDecision::ModifyExisting => Self::ModifyExisting,
            TaskLineageReconcileToolDecision::ExpandExisting => Self::ExpandExisting,
            TaskLineageReconcileToolDecision::NarrowExisting => Self::NarrowExisting,
            TaskLineageReconcileToolDecision::DeferPart => Self::DeferPart,
            TaskLineageReconcileToolDecision::CancelPart => Self::CancelPart,
            TaskLineageReconcileToolDecision::Mixed => Self::Mixed,
            TaskLineageReconcileToolDecision::CancelExisting => Self::CancelExisting,
            TaskLineageReconcileToolDecision::ExpirePrior { lineage_id } => {
                Self::ExpirePrior { lineage_id }
            }
            TaskLineageReconcileToolDecision::ReferencePrior { lineage_id } => {
                Self::ReferencePrior { lineage_id }
            }
        }
    }
}
/// Model-facing reconcile arguments. Session, current user event, and lineage
/// revision are bound by Core from the live call.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageReconcileToolInput {
    pub decision: TaskLineageReconcileToolDecision,
    #[serde(default)]
    pub relations: Vec<TaskLineageRelationInput>,
    pub completion_policy_update: Option<CompletionPolicyUpdate>,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageRelationInput {
    pub earlier_user_event_id: Option<EventId>,
    pub later_user_event_id: EventId,
    pub kind: TaskLineageRelationKind,
    pub earlier_excerpt: Option<UserMessageExcerptSelector>,
    pub later_excerpt: Option<UserMessageExcerptSelector>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompletionPolicyUpdate {
    pub policy: CompletionPolicy,
    /// Required also when relaxing Strict. The referenced original user message
    /// and exact excerpt remain audit evidence; the model cannot erase them.
    pub authority_event_id: EventId,
    pub authority_excerpt: UserMessageExcerptSelector,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageReconcileInput {
    pub current_user_event_id: EventId,
    pub active_lineage_id: Option<TaskLineageId>,
    pub expected_lineage_revision: Option<Revision>,
    pub decision: TaskLineageReconcileDecision,
    pub relations: Vec<TaskLineageRelationInput>,
    pub completion_policy_update: Option<CompletionPolicyUpdate>,
    pub reason: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OpenTaskLineageSummary {
    pub lineage_id: TaskLineageId,
    pub state: TaskLineageState,
    pub root_user_event_id: EventId,
    pub root_preview: String,
    pub source_count: u32,
    pub revision: Revision,
    pub last_active_at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageListOpenResult {
    pub active_lineage_id: Option<TaskLineageId>,
    pub lineages: Vec<OpenTaskLineageSummary>,
}

impl TaskLineage {
    pub fn from_root(
        id: TaskLineageId,
        user: &OriginalUserMessage,
        turn: TurnTaskId,
    ) -> Result<Self, AgentError> {
        user.validate()?;
        let source = TaskLineageSource {
            user_event_id: user.event_id,
            source_sequence: 1,
            content_hash: user.content_hash.clone(),
            immutable_preview: source_preview(&user.text),
            related_ranges: vec![],
            membership: SourceMembership::Active,
            attached_by_turn_task_id: turn,
            attached_at: user.at,
        };
        Ok(Self {
            lineage_id: id,
            session_id: user.session_id,
            root_user_event_id: user.event_id,
            state: TaskLineageState::Active,
            revision: Revision::INITIAL,
            completion_policy: CompletionPolicy::BestEffortBalanced,
            completion_gate: CompletionGate::Unreviewed,
            derived_from_lineage_id: None,
            created_by_turn_task_id: turn,
            last_active_turn_task_id: turn,
            sources: vec![source],
            relations: vec![TaskLineageRelation {
                relation_id: TaskLineageRelationId::new(),
                earlier_user_event_id: None,
                later_user_event_id: user.event_id,
                kind: TaskLineageRelationKind::Root,
                earlier_excerpt: None,
                later_excerpt: None,
                created_by_turn_task_id: turn,
                created_at: user.at,
            }],
            source_recheck: SourceRecheckFacts::default(),
            created_at: user.at,
            updated_at: user.at,
            completed_at: None,
        })
    }
    /// Authenticated User Events only. Validate on a candidate first so invalid
    /// excerpts or duplicate identities never partially mutate the aggregate.
    pub fn attach_source(
        &mut self,
        user: &OriginalUserMessage,
        turn: TurnTaskId,
        related_ranges: Vec<UserMessageTextRange>,
        expected: Revision,
        at: UtcTimestamp,
    ) -> Result<(), AgentError> {
        user.validate()?;
        if self.session_id != user.session_id
            || self.state != TaskLineageState::Active
            || self.sources.iter().any(|s| s.user_event_id == user.event_id)
        {
            return Err(AgentError::SourceInvalid);
        }
        let mut selected = Vec::new();
        let mut previous_end = 0;
        for range in &related_ranges {
            let start =
                usize::try_from(range.start_utf8_byte).map_err(|_| AgentError::SourceInvalid)?;
            let end =
                usize::try_from(range.end_utf8_byte).map_err(|_| AgentError::SourceInvalid)?;
            if start < previous_end || start >= end {
                return Err(AgentError::SourceInvalid);
            }
            selected.push(user.text.get(start..end).ok_or(AgentError::SourceInvalid)?);
            previous_end = end;
        }
        let sequence = u32::try_from(self.sources.len())
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(AgentError::SourceInvalid)?;
        let source = TaskLineageSource {
            user_event_id: user.event_id,
            source_sequence: sequence,
            content_hash: user.content_hash.clone(),
            immutable_preview: source_preview(&if selected.is_empty() {
                user.text.clone()
            } else {
                selected.join("\n")
            }),
            related_ranges,
            membership: SourceMembership::Active,
            attached_by_turn_task_id: turn,
            attached_at: at,
        };
        self.advance_revision(expected, at)?;
        self.sources.push(source);
        self.source_recheck.crossed_turn |= self.last_active_turn_task_id != turn;
        self.last_active_turn_task_id = turn;
        let reason = match user.origin {
            UserMessageOrigin::Steer => {
                self.source_recheck.used_steer = true;
                CompletionInvalidation::SteerAttached
            }
            UserMessageOrigin::QueryAnswer { .. } => {
                self.source_recheck.used_query = true;
                CompletionInvalidation::QueryAnswerAttached
            }
            UserMessageOrigin::UserInput => CompletionInvalidation::NewUserSourceAttached,
        };
        self.completion_gate = CompletionGate::Invalidated { reason };
        Ok(())
    }
    pub fn source_read_receipt(
        &self,
        messages: &[OriginalUserMessage],
        turn: &TurnTask,
        model_step_id: ModelStepId,
        id: SourceReadReceiptId,
        at: UtcTimestamp,
    ) -> Result<SourceReadReceipt, AgentError> {
        if turn.session_id != self.session_id || turn.state.terminal() {
            return Err(AgentError::IdentityMismatch);
        }
        let mut hashes = Vec::new();
        for message in messages {
            message.validate()?;
            let source = self
                .sources
                .iter()
                .find(|s| {
                    s.user_event_id == message.event_id && s.membership == SourceMembership::Active
                })
                .ok_or(AgentError::SourceInvalid)?;
            if message.session_id != self.session_id
                || message.content_hash != source.content_hash
                || hashes.iter().any(|(id, _)| *id == message.event_id)
            {
                return Err(AgentError::SourceInvalid);
            }
            hashes.push((message.event_id, message.content_hash.clone()));
        }
        Ok(SourceReadReceipt {
            receipt_id: id,
            session_id: self.session_id,
            turn_task_id: turn.turn_task_id,
            agent_run_id: turn.agent_run_id,
            model_step_id,
            lineage_id: self.lineage_id,
            lineage_revision: self.revision,
            source_event_hashes: hashes,
            created_at: at,
        })
    }
}
