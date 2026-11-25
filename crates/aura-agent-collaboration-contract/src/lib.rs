//! Collaboration identities are existing conversations, not another conversation registry.
mod graph;
pub use graph::*;
mod peer_management;
pub use peer_management::*;
mod peers;
pub use peers::*;
mod policy;
use aura_domain_common::{
    AgentRunId, ModelProfileId, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
pub use policy::{AgentSessionPolicy, AgentSessionPolicyRequest, AgentSessionPolicySetRequest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_DESCRIPTION_CHARS: usize = 1200;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentDescriptionSource {
    #[default]
    None,
    User,
    Model,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileRequest {
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileSetRequest {
    pub session_id: SessionId,
    pub expected_revision: u64,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentProfilePreviewRequest {
    pub session_id: SessionId,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentProfilePreview {
    pub base_revision: u64,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    /// The primary session registry owns this name; editing a description cannot rename it.
    pub name: String,
    pub description: String,
    pub description_source: AgentDescriptionSource,
    pub revision: u64,
    pub updated_at: Option<UtcTimestamp>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usage_complete: bool,
    pub generation: Option<AgentProfileGeneration>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileGenerationState {
    Running,
    Completed,
    Failed,
    Interrupted,
    Superseded,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentProfileGeneration {
    pub source_run_id: AgentRunId,
    pub source_turn_id: TurnTaskId,
    pub model_profile_id: Option<ModelProfileId>,
    pub state: AgentProfileGenerationState,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usage_complete: bool,
    /// A bounded, safe product diagnostic, never the provider's raw response or credentials.
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<AgentProfilePreview>,
}

/// Text only: a model never supplies session identity, authorship, permissions or revisions.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeneratedAgentDescription {
    /// None means there was no material change.
    pub description: Option<String>,
}
