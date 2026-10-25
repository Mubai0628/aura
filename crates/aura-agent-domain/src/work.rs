use crate::AgentError;
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
    Cancelled,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TodoItem {
    pub id: TodoItemId,
    pub title: String,
    pub status: TodoStatus,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TodoState {
    pub turn_task_id: TurnTaskId,
    pub revision: Revision,
    pub items: Vec<TodoItem>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TodoItemDraft {
    pub title: String,
    /// Default pending.
    #[serde(default = "pending")]
    pub status: TodoStatus,
}
fn pending() -> TodoStatus {
    TodoStatus::Pending
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TodoReplaceInput {
    pub expected_revision: Revision,
    pub items: Vec<TodoItemDraft>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TodoUpdateInput {
    pub expected_revision: Revision,
    pub item_id: TodoItemId,
    pub title: Option<String>,
    pub status: TodoStatus,
}
impl TodoState {
    pub fn unfinished(&self) -> bool {
        self.items.iter().any(|i| {
            matches!(i.status, TodoStatus::Pending | TodoStatus::InProgress | TodoStatus::Blocked)
        })
    }
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.revision.value() == 0
            || self.items.iter().any(|i| i.title.trim().is_empty())
            || self.items.iter().map(|i| i.id).collect::<BTreeSet<_>>().len() != self.items.len()
        {
            Err(AgentError::InvalidInput)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "execution", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanStepExecution {
    Serial,
    ParallelGroup { group: String },
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub id: String,
    pub description: String,
    /// Default empty.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Default serial.
    #[serde(default = "serial")]
    pub execution: PlanStepExecution,
    #[serde(default)]
    pub validation: String,
}
fn serial() -> PlanStepExecution {
    PlanStepExecution::Serial
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanCompleteInput {
    pub expected_turn_revision: Revision,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub current_project_state: String,
    #[serde(default)]
    pub steps: Vec<PlanStep>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanRecord {
    pub turn_task_id: TurnTaskId,
    pub revision: Revision,
    /// Loaded verbatim from User Events by Core, never accepted in Tool input.
    pub original_user_goal: String,
    pub plan: PlanCompleteInput,
    pub at: UtcTimestamp,
}
impl PlanCompleteInput {
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.expected_turn_revision.value() == 0 {
            return Err(AgentError::InvalidInput);
        }
        let graph: BTreeMap<_, _> = self.steps.iter().map(|s| (s.id.as_str(), s)).collect();
        if graph.len() != self.steps.len() {
            return Err(AgentError::InvalidInput);
        }
        for step in &self.steps {
            if step.id.trim().is_empty()
                || step.description.trim().is_empty()
                || step.depends_on.iter().any(|d| !graph.contains_key(d.as_str()) || d == &step.id)
                || step.depends_on.iter().collect::<BTreeSet<_>>().len() != step.depends_on.len()
                || matches!(&step.execution, PlanStepExecution::ParallelGroup { group } if group.trim().is_empty())
            {
                return Err(AgentError::InvalidInput);
            }
        }
        let mut remaining: BTreeSet<_> = graph.keys().copied().collect();
        while !remaining.is_empty() {
            let next = remaining
                .iter()
                .find(|id| graph[**id].depends_on.iter().all(|d| !remaining.contains(d.as_str())))
                .copied()
                .ok_or(AgentError::InvalidInput)?;
            remaining.remove(next);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalBudget {
    pub max_tokens: Option<u64>,
    pub max_turns: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalState {
    pub turn_task_id: TurnTaskId,
    pub revision: Revision,
    pub budget: GoalBudget,
    pub consumed_tokens: u64,
    pub consumed_turns: u64,
    pub latest_check_model_step: Option<ModelStepId>,
    pub completed: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalCheckInput {
    pub expected_goal_revision: Revision,
    pub assessment: String,
    /// Default empty.
    #[serde(default)]
    pub remaining_work: Vec<String>,
    /// Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
    /// commands. An artifact id from a tool result must be one of this run's.
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalCompleteInput {
    pub expected_goal_revision: Revision,
    pub final_report: String,
    #[serde(default)]
    pub verification: String,
    /// Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
    /// commands. An artifact id from a tool result must be one of this run's.
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}
impl GoalState {
    pub fn budget_exhausted(&self) -> bool {
        self.budget.max_tokens.is_some_and(|v| self.consumed_tokens >= v)
            || self.budget.max_turns.is_some_and(|v| self.consumed_turns >= v)
    }
    pub fn check_completion(&self, input: &GoalCompleteInput) -> Result<(), AgentError> {
        if self.revision != input.expected_goal_revision {
            return Err(AgentError::RevisionConflict);
        }
        if self.completed {
            return Err(AgentError::InvalidTransition);
        }
        if input.final_report.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QueryChoice {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryAnswerType {
    Text,
    SingleChoice { options: Vec<QueryChoice> },
    MultiChoice { options: Vec<QueryChoice> },
    Confirm,
    FileOrDirectory,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QueryQuestion {
    pub id: String,
    pub text: String,
    pub answer_type: QueryAnswerType,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QueryUserQuestionInput {
    pub questions: Vec<QueryQuestion>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "answer", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryAnswerValue {
    Text {
        text: String,
    },
    Choices {
        ids: Vec<String>,
    },
    Confirm {
        accepted: bool,
    },
    FileOrDirectory {
        file_reference_id: FileReferenceId,
    },
    /// The user's own words, which every question accepts.
    Custom {
        text: String,
    },
    /// The user chose not to answer. It covers the whole question.
    Declined,
    /// Nobody touched the question for its idle time and its countdown; the user may be busy or
    /// away. It is not an answer, and it covers the whole question.
    NoResponse,
}

/// The longest custom reply, in characters.
pub const MAX_CUSTOM_REPLY_CHARS: usize = 4000;
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QueryAnswer {
    pub question_id: String,
    pub value: QueryAnswerValue,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserQueryState {
    Pending,
    Answered { user_event_id: EventId, answers: Vec<QueryAnswer> },
    Cancelled,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserQuery {
    pub query_id: UserQueryId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub tool_call_id: ToolCallId,
    pub revision: Revision,
    pub input: QueryUserQuestionInput,
    pub state: UserQueryState,
    pub created_at: UtcTimestamp,
}
impl QueryUserQuestionInput {
    pub fn validate(&self) -> Result<(), AgentError> {
        if self.questions.is_empty()
            || self.questions.iter().map(|q| &q.id).collect::<BTreeSet<_>>().len()
                != self.questions.len()
        {
            return Err(AgentError::InvalidInput);
        }
        for q in &self.questions {
            if q.id.trim().is_empty() || q.text.trim().is_empty() {
                return Err(AgentError::InvalidInput);
            }
            if let QueryAnswerType::SingleChoice { options }
            | QueryAnswerType::MultiChoice { options } = &q.answer_type
                && (options.len() < 2
                    || options.iter().map(|o| &o.id).collect::<BTreeSet<_>>().len()
                        != options.len()
                    || options.iter().any(|o| o.id.trim().is_empty() || o.label.trim().is_empty()))
            {
                return Err(AgentError::InvalidInput);
            }
        }
        Ok(())
    }
    pub fn validate_answers(&self, answers: &[QueryAnswer]) -> Result<(), AgentError> {
        self.validate()?;
        if answers.len() != self.questions.len()
            || answers.iter().map(|a| &a.question_id).collect::<BTreeSet<_>>().len()
                != answers.len()
        {
            return Err(AgentError::InvalidInput);
        }
        // Declining and "no response" are about the question as a whole, never one part of it.
        for whole in [QueryAnswerValue::Declined, QueryAnswerValue::NoResponse] {
            let count = answers.iter().filter(|answer| answer.value == whole).count();
            if count != 0 && count != answers.len() {
                return Err(AgentError::InvalidInput);
            }
        }
        for q in &self.questions {
            let value = &answers
                .iter()
                .find(|a| a.question_id == q.id)
                .ok_or(AgentError::InvalidInput)?
                .value;
            let valid = match (&q.answer_type, value) {
                (_, QueryAnswerValue::Custom { text }) => {
                    !text.trim().is_empty() && text.chars().count() <= MAX_CUSTOM_REPLY_CHARS
                }
                (_, QueryAnswerValue::Declined | QueryAnswerValue::NoResponse) => true,
                (QueryAnswerType::Text, QueryAnswerValue::Text { text }) => !text.trim().is_empty(),
                (QueryAnswerType::Confirm, QueryAnswerValue::Confirm { .. })
                | (QueryAnswerType::FileOrDirectory, QueryAnswerValue::FileOrDirectory { .. }) => {
                    true
                }
                (QueryAnswerType::SingleChoice { options }, QueryAnswerValue::Choices { ids }) => {
                    ids.len() == 1 && options.iter().any(|o| o.id == ids[0])
                }
                (QueryAnswerType::MultiChoice { options }, QueryAnswerValue::Choices { ids }) => {
                    ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
                        && ids.iter().all(|id| options.iter().any(|o| o.id == *id))
                }
                _ => false,
            };
            if !valid {
                return Err(AgentError::InvalidInput);
            }
        }
        Ok(())
    }
}
