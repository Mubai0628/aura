use aura_domain_common::{SessionId, ToolCallId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedProcessRequestV1 {
    pub session_id: SessionId,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedProcessStopRequestV1 {
    pub session_id: SessionId,
    pub tool_call_id: ToolCallId,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedProcessKindV1 {
    Shell,
    Terminal,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedProcessViewV1 {
    pub kind: ManagedProcessKindV1,
    pub command: String,
    pub started_at: UtcTimestamp,
    pub tool_call_id: ToolCallId,
    pub terminal_id: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedProcessListV1 {
    pub processes: Vec<ManagedProcessViewV1>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedProcessStopResultV1 {
    pub stopped: bool,
}
