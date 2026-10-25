//! Structured, countable tool failures and turn-level protocol guards.
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CORRECTION_RULE_SCHEMA_V1: &str = "aura.tool.schema.constraint.v1";
pub const CORRECTION_RULE_DISPOSITION_V1: &str = "aura.task.disposition.constraint.v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnGuardStopReason {
    ProtocolNotCompleted,
    DeterministicCorrectionExhausted,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeterministicFailure {
    pub rule_id: String,
    pub rule_version: u32,
    pub field: String,
    pub constraint: String,
    pub current_value: String,
    pub correction: String,
    pub executed: bool,
    pub effect: String,
}

impl DeterministicFailure {
    #[must_use]
    pub fn target_key(&self) -> String {
        format!("{}:{}:{}:{}", self.rule_id, self.rule_version, self.field, self.constraint)
    }
    #[must_use]
    pub fn schema_constraint(
        field: impl Into<String>,
        constraint: impl Into<String>,
        current_value: impl Into<String>,
        correction: impl Into<String>,
    ) -> Self {
        Self {
            rule_id: CORRECTION_RULE_SCHEMA_V1.into(),
            rule_version: 1,
            field: field.into(),
            constraint: constraint.into(),
            current_value: current_value.into(),
            correction: correction.into(),
            executed: false,
            effect: "No task state was changed.".into(),
        }
    }
    #[must_use]
    pub fn disposition(
        field: impl Into<String>,
        constraint: impl Into<String>,
        current_value: impl Into<String>,
        correction: impl Into<String>,
    ) -> Self {
        Self {
            rule_id: CORRECTION_RULE_DISPOSITION_V1.into(),
            rule_version: 1,
            field: field.into(),
            constraint: constraint.into(),
            current_value: current_value.into(),
            correction: correction.into(),
            executed: false,
            effect: "No task state was changed.".into(),
        }
    }
    #[must_use]
    pub fn model_message(&self) -> String {
        format!(
            "Field {} failed constraint {}. Current value: {}. {}. Executed: {}. {}. retryable:false means the identical arguments will fail; corrected arguments may be submitted.",
            self.field,
            self.constraint,
            self.current_value,
            self.correction,
            self.executed,
            self.effect
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorrectionSequence {
    pub target_key: String,
    pub rule_id: String,
    pub rule_version: u32,
    pub field: String,
    pub feedback_delivered_to_step: Option<ModelStepId>,
    pub remaining_attempts: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TurnGuardState {
    pub turn_task_id: TurnTaskId,
    pub revision: Revision,
    pub completion_reminder_sent: bool,
    pub stopped: Option<TurnGuardStopReason>,
    pub corrections: Vec<CorrectionSequence>,
}

impl TurnGuardState {
    #[must_use]
    pub fn fresh(turn_task_id: TurnTaskId) -> Self {
        Self {
            turn_task_id,
            revision: Revision::INITIAL,
            completion_reminder_sent: false,
            stopped: None,
            corrections: vec![],
        }
    }
}
