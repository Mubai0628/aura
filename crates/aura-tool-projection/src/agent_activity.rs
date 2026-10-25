//! Read-only activity over the current Tool store; the original result stays immutable.
use super::*;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentToolActivityRequest {
    pub session_id: SessionId,
    pub before_call_id: Option<ToolCallId>,
    pub limit: u32,
    /// Exact visible identities, at most 100. Mutually exclusive with a cursor.
    #[serde(default)]
    pub call_ids: Option<Vec<ToolCallId>>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentToolActivityEntry {
    pub summary: ToolCallActivitySummaryView,
    pub model_step_id: ModelStepId,
    pub display: AgentToolDisplay,
    pub shell_query_available: bool,
    pub permission_mode: Option<aura_security_contract::PermissionMode>,
    pub permission_revision: Option<u64>,
}

/// Bounded, deterministic display derived from execution facts, never model prose.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentToolDisplay {
    pub title: String,
    pub target: Option<String>,
    pub summary: String,
    pub details: Vec<String>,
    pub truncated: bool,
    pub internal: bool,
    /// The screenshot the call took, read with `browser.screenshot.read`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<aura_domain_common::ArtifactId>,
    /// What the user can do from the row about why the call did not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<AgentToolOffer>,
}

/// A choice only the
/// user can make, offered on the row of a call it stopped.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolOffer {
    /// The call needs 设置 → 网络与浏览器 → 允许模型调试网页, which is off.
    EnableBrowserDebugging,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentToolActivityPage {
    pub session_id: SessionId,
    pub calls: Vec<AgentToolActivityEntry>,
    pub next_before_call_id: Option<ToolCallId>,
}
