//! Authenticated user entry DTOs. No caller-controlled Agent identity, network
//! channel, authorization grant, or Tool result is accepted here.
use crate::*;
use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentStartRequest {
    pub session_id: SessionId,
    pub model_profile_id: ModelProfileId,
    pub text: String,
    pub mode: AgentTaskMode,
    pub goal_budget: Option<GoalBudget>,
    /// User preview of NativeApproved. This is never an approval or a silent
    /// fallback; policy still Asks AllowOnce/AllowSession before dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_backend_id: Option<String>,
    /// Skills the user chose for this turn in the composer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_ids: Vec<SkillInstanceId>,
    /// The thinking level the conversation uses for this turn; none is the model's
    /// default. Kept with the run's configuration, so every step of the turn uses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<TurnReasoningEffort>,
    /// Pictures and files added to the message in the composer. The run's configuration
    /// keeps their digests ([`AgentStartRequest::receipt`]); the message keeps them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AgentAttachmentDraft>,
}
impl AgentStartRequest {
    /// The request as a repeated command is compared and kept: digests in place of what its
    /// pictures and text files hold.
    #[must_use]
    pub fn receipt(&self) -> Self {
        Self {
            attachments: self.attachments.iter().map(AgentAttachmentDraft::receipt).collect(),
            ..self.clone()
        }
    }
}
/// `/init`: start a workspace initialization task in an idle session. `text` is the
/// user's optional note after the command; the task uses the session's permission mode.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentInitStartRequest {
    pub session_id: SessionId,
    pub model_profile_id: ModelProfileId,
    #[serde(default)]
    pub text: String,
}
/// `/compact` in an idle session. `focus` is the user's optional note after the command:
/// what the summary should keep. Core compacts before the turn's first step.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentCompactionStartRequest {
    pub session_id: SessionId,
    pub model_profile_id: ModelProfileId,
    #[serde(default)]
    pub focus: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentRunRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentQueryAnswerRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub query_id: UserQueryId,
    pub expected_revision: Revision,
    pub answers: Vec<QueryAnswer>,
}
/// The user touched the waiting question (clicked it, chose, typed), so
/// its countdown starts again.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentQueryTouchRequest {
    pub session_id: SessionId,
    pub agent_run_id: AgentRunId,
    pub query_id: UserQueryId,
}
/// When the touched question now ends as "no response".
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentQueryTouchView {
    pub query_id: UserQueryId,
    pub deadline_at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentRunProjection {
    pub turn: TurnTask,
    pub run: AgentRun,
    pub model_steps: Vec<ModelStep>,
    pub pending_query: Option<UserQuery>,
    /// When the waiting question ends as "no response" unless the user touches it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_query_deadline_at: Option<UtcTimestamp>,
    pub cancellation_requested: bool,
    pub pending_approval_ids: Vec<ApprovalRequestId>,
    pub recovery_episodes: Vec<RecoveryEpisode>,
}
