use crate::{AgentError, CompletionGate, CompletionPolicy};
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Only an authenticated Core user-entry transaction may append this event.
/// Model continuation is a different type and cannot construct a user source.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OriginalUserMessage {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub text: String,
    pub content_hash: ContentHash,
    pub origin: UserMessageOrigin,
    pub at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "origin", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserMessageOrigin {
    UserInput,
    Steer,
    QueryAnswer { query_id: UserQueryId },
}
impl OriginalUserMessage {
    pub fn new(
        event_id: EventId,
        session_id: SessionId,
        text: String,
        origin: UserMessageOrigin,
        at: UtcTimestamp,
    ) -> Result<Self, AgentError> {
        if text.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        Ok(Self {
            event_id,
            session_id,
            content_hash: ContentHash::blake3(text.as_bytes()),
            text,
            origin,
            at,
        })
    }
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.text.trim().is_empty()
            || self.content_hash != ContentHash::blake3(self.text.as_bytes())
        {
            Err(AgentError::SourceInvalid)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLineageState {
    Active,
    SuspendedByTopicSwitch,
    InterruptedResumable,
    /// Model-marked background. Not user cancellation, not completion, and not
    /// eligible for reactivation.
    Expired,
    Completed,
    CancelledByUser,
}
impl TaskLineageState {
    #[must_use]
    pub fn is_closed(self) -> bool {
        matches!(self, Self::Completed | Self::CancelledByUser | Self::Expired)
    }
    #[must_use]
    pub fn may_activate(self) -> bool {
        self == Self::Active
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "membership", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceMembership {
    Active,
    ReclassifiedToLineage { target_lineage_id: TaskLineageId, decision_event_id: EventId },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserMessageTextRange {
    pub start_utf8_byte: u64,
    pub end_utf8_byte: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserMessageExcerptSelector {
    pub exact_text: String,
    pub occurrence: u32,
}
impl UserMessageExcerptSelector {
    pub fn resolve(&self, text: &str) -> Result<UserMessageTextRange, AgentError> {
        if self.exact_text.is_empty() {
            return Err(AgentError::SourceInvalid);
        }
        let (start, found) = text
            .match_indices(&self.exact_text)
            .nth(self.occurrence as usize)
            .ok_or(AgentError::SourceInvalid)?;
        Ok(UserMessageTextRange {
            start_utf8_byte: start as u64,
            end_utf8_byte: (start + found.len()) as u64,
        })
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageSource {
    pub user_event_id: EventId,
    pub source_sequence: u32,
    pub content_hash: ContentHash,
    pub immutable_preview: String,
    pub related_ranges: Vec<UserMessageTextRange>,
    pub membership: SourceMembership,
    pub attached_by_turn_task_id: TurnTaskId,
    pub attached_at: UtcTimestamp,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLineageRelationKind {
    Root,
    Continues,
    AddsRequirement,
    AddsConstraint,
    Clarifies,
    Corrects,
    Narrows,
    Defers,
    Expands,
    CancelsPart,
    CancelsTask,
    Mixed,
    ReclassifiesUnrelatedPart,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineageRelation {
    pub relation_id: TaskLineageRelationId,
    pub earlier_user_event_id: Option<EventId>,
    pub later_user_event_id: EventId,
    pub kind: TaskLineageRelationKind,
    pub earlier_excerpt: Option<UserMessageTextRange>,
    pub later_excerpt: Option<UserMessageTextRange>,
    pub created_by_turn_task_id: TurnTaskId,
    pub created_at: UtcTimestamp,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRecheckFacts {
    pub compacted: bool,
    pub used_steer: bool,
    pub used_query: bool,
    pub crossed_turn: bool,
    pub used_child_agent: bool,
    pub source_not_in_context: bool,
    pub resumed: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskLineage {
    pub lineage_id: TaskLineageId,
    pub session_id: SessionId,
    pub root_user_event_id: EventId,
    pub state: TaskLineageState,
    pub revision: Revision,
    pub completion_policy: CompletionPolicy,
    pub completion_gate: CompletionGate,
    pub derived_from_lineage_id: Option<TaskLineageId>,
    pub created_by_turn_task_id: TurnTaskId,
    pub last_active_turn_task_id: TurnTaskId,
    pub sources: Vec<TaskLineageSource>,
    pub relations: Vec<TaskLineageRelation>,
    pub source_recheck: SourceRecheckFacts,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub completed_at: Option<UtcTimestamp>,
}
impl TaskLineage {
    pub fn source_recheck_required(&self) -> bool {
        let f = &self.source_recheck;
        self.sources.iter().filter(|s| s.membership == SourceMembership::Active).count() > 1
            || f.compacted
            || f.used_steer
            || f.used_query
            || f.crossed_turn
            || f.used_child_agent
            || f.source_not_in_context
            || f.resumed
            || self.relations.iter().any(|r| {
                matches!(
                    r.kind,
                    TaskLineageRelationKind::Corrects
                        | TaskLineageRelationKind::Narrows
                        | TaskLineageRelationKind::Defers
                        | TaskLineageRelationKind::CancelsPart
                        | TaskLineageRelationKind::Mixed
                )
            })
    }
    pub fn advance_revision(
        &mut self,
        expected: Revision,
        at: UtcTimestamp,
    ) -> Result<(), AgentError> {
        if self.revision != expected || at < self.updated_at {
            return Err(AgentError::RevisionConflict);
        }
        self.revision =
            Revision::new(expected.value().checked_add(1).ok_or(AgentError::RevisionConflict)?);
        self.updated_at = at;
        Ok(())
    }
}
pub fn source_preview(text: &str) -> String {
    let chars: Vec<_> = text.chars().collect();
    if chars.len() <= 768 {
        return text.into();
    }
    format!(
        "{}[...... original user message preview pruned ......]{}",
        chars[..512].iter().collect::<String>(),
        chars[chars.len() - 256..].iter().collect::<String>()
    )
}
