use aura_domain_common::SessionId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionPolicyRequest {
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionPolicy {
    pub session_id: SessionId,
    /// A behaviour preference, never a grant of file, tool or model permissions.
    pub allow_proactive_temporary_agents: bool,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionPolicySetRequest {
    pub session_id: SessionId,
    pub expected_revision: u64,
    pub allow_proactive_temporary_agents: bool,
}
