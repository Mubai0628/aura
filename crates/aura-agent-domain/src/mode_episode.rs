//! Session-level Plan/Goal cycle. A new turn inherits the mode; it does not
//! reactivate an interrupted lineage.
use crate::{AgentTaskMode, GoalBudget, GoalState};
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModeEpisodeEndReason {
    PlanCompleted,
    GoalCompleted,
    UserTerminated,
    Skipped,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionModeEpisode {
    pub session_id: SessionId,
    pub revision: Revision,
    pub mode: AgentTaskMode,
    pub paused: bool,
    pub goal_budget: Option<GoalBudget>,
    pub inherited_goal: Option<GoalState>,
    pub started_turn_task_id: Option<TurnTaskId>,
    pub ended_reason: Option<ModeEpisodeEndReason>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    /// An interrupted or paused special mode is waiting for an explicit decision.
    #[serde(default)]
    pub disposition_required: bool,
}

impl SessionModeEpisode {
    #[must_use]
    pub fn active_mode(&self) -> AgentTaskMode {
        if self.ended_reason.is_some() { AgentTaskMode::Normal } else { self.mode }
    }
    #[must_use]
    pub fn is_active_special(&self) -> bool {
        self.ended_reason.is_none() && self.mode != AgentTaskMode::Normal
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentModeEpisodeView {
    pub session_id: SessionId,
    pub revision: Revision,
    pub mode: AgentTaskMode,
    pub paused: bool,
    pub goal_budget: Option<GoalBudget>,
    pub inherited_goal: Option<GoalState>,
    #[serde(default)]
    pub disposition_required: bool,
    #[serde(default)]
    pub end_reason: Option<ModeEpisodeEndReason>,
}

impl From<&SessionModeEpisode> for AgentModeEpisodeView {
    fn from(value: &SessionModeEpisode) -> Self {
        Self {
            session_id: value.session_id,
            revision: value.revision,
            mode: value.active_mode(),
            paused: value.ended_reason.is_none() && value.paused,
            goal_budget: value.goal_budget.clone(),
            inherited_goal: value.inherited_goal.clone(),
            disposition_required: value.ended_reason.is_none() && value.disposition_required,
            end_reason: value.ended_reason,
        }
    }
}

impl Default for AgentModeEpisodeView {
    fn default() -> Self {
        Self {
            session_id: SessionId::from_bytes([0; 16]),
            revision: Revision::INITIAL,
            mode: AgentTaskMode::Normal,
            paused: false,
            goal_budget: None,
            inherited_goal: None,
            disposition_required: false,
            end_reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModeDispositionDecision {
    Continue,
    Skip,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModeDispositionInput {
    pub decision: ModeDispositionDecision,
    pub reason: String,
}

/// Model steps taken while judging continue/skip are excluded from Goal consumption.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModeJudgmentRecord {
    pub episode_created_at: UtcTimestamp,
    pub decision: ModeDispositionDecision,
    pub excluded_model_step_ids: Vec<ModelStepId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentModeControlAction {
    Terminate,
    PauseGoal,
    EditGoalBudget { goal_budget: GoalBudget },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentModeControlRequest {
    pub session_id: SessionId,
    pub expected_revision: Revision,
    pub action: AgentModeControlAction,
}
