//! Agent-facing correction state. Raw Provider evidence stays behind the Model Store.
use crate::ContextWindowCorrectionView;
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextWindowState {
    Awaiting,
    Preparing,
    RequestRunning,
    Applied,
    Cleared,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentContextWindowCorrection {
    pub correction: ContextWindowCorrectionView,
    pub revision: Revision,
    pub state: AgentContextWindowState,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentContextWindowAction {
    Recheck,
    TrySmallerWindows,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentContextWindowResolveRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub source_model_step_id: ModelStepId,
    pub expected_revision: Revision,
    pub action: AgentContextWindowAction,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn correction_entry_cannot_supply_configuration_mode_or_execution_authority() {
        let request = AgentContextWindowResolveRequest {
            session_id: SessionId::new(),
            agent_run_id: AgentRunId::new(),
            source_model_step_id: ModelStepId::new(),
            expected_revision: Revision::INITIAL,
            action: AgentContextWindowAction::TrySmallerWindows,
        };
        let value = serde_json::to_value(request).expect("request");
        assert!(serde_json::from_value::<AgentContextWindowResolveRequest>(value.clone()).is_ok());
        for field in
            ["model_profile_id", "permission", "mode", "execution_snapshot_id", "window_tokens"]
        {
            let mut forged = value.clone();
            forged[field] = serde_json::json!("untrusted");
            assert!(
                serde_json::from_value::<AgentContextWindowResolveRequest>(forged).is_err(),
                "{field}"
            );
        }
        let mut forged = value;
        forged["action"] = serde_json::json!("replay");
        assert!(serde_json::from_value::<AgentContextWindowResolveRequest>(forged).is_err());
    }
}
