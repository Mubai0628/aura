//! Local preflight diagnostics. No Provider raw evidence or execution authority.
use crate::{ContextBudgetSnapshot, ContextRemediationPlan};
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextRemediationState {
    Awaiting,
    Rechecking,
    Cleared,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentContextRemediation {
    pub model_step_id: ModelStepId,
    pub revision: Revision,
    pub plan: ContextRemediationPlan,
    pub budget: ContextBudgetSnapshot,
    pub state: AgentContextRemediationState,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentContextRemediationView {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub remediation: Option<AgentContextRemediation>,
    pub provider_correction: Option<aura_model_domain::AgentContextWindowCorrection>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentContextRecheckRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub source_model_step_id: ModelStepId,
    pub expected_revision: Revision,
}
