//! Terminal input metadata contains no secret value, digest or credential reference.
use aura_domain_common::{RuntimeInstanceId, SessionId, ToolCallId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTerminalSensitiveInput {
    pub terminal_id: String,
    #[schemars(length(min = 1, max = 2000))]
    pub prompt: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSensitiveStatusV1 {
    Completed,
    Cancelled,
    Failed,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSensitiveResultV1 {
    pub status: TerminalSensitiveStatusV1,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSensitiveRequestV1 {
    pub session_id: SessionId,
    pub terminal_id: String,
    pub tool_call_id: ToolCallId,
    pub prompt: String,
    pub expires_at: UtcTimestamp,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSensitiveReplyV1 {
    pub session_id: SessionId,
    pub tool_call_id: ToolCallId,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSensitivePendingV1 {
    pub session_id: SessionId,
    pub requests: Vec<TerminalSensitiveRequestV1>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalSensitiveChangedV1 {
    pub runtime_instance_id: RuntimeInstanceId,
    pub pending: TerminalSensitivePendingV1,
}
