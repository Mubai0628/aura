//! Authenticated desktop terminal protocol. Model terminal tools cannot invoke these routes.
use crate::ManagedShell;
use aura_domain_common::{RuntimeInstanceId, SessionId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalEndReason {
    Exited,
    UserClosed,
    AgentClosed,
    CoreStopped,
    SessionDeleted,
    OwnerDropped,
    OutputFailed,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TerminalEnd {
    pub reason: TerminalEndReason,
    pub exit_code: Option<u32>,
    pub stop_confirmed: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserTerminalOpenRequestV1 {
    pub session_id: SessionId,
    pub shell: ManagedShell,
    pub columns: u16,
    pub rows: u16,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalSessionRequestV1 {
    pub session_id: SessionId,
    #[serde(default)]
    pub cursor: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalRequestV1 {
    pub session_id: SessionId,
    pub terminal_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserTerminalWriteRequestV1 {
    pub session_id: SessionId,
    pub terminal_id: String,
    pub input: String,
    pub interrupt: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalResizeRequestV1 {
    pub session_id: SessionId,
    pub terminal_id: String,
    pub columns: u16,
    pub rows: u16,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalSubscribeRequestV1 {
    pub session_id: SessionId,
    pub terminal_id: String,
    pub offset: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalViewV1 {
    pub session_id: SessionId,
    pub terminal_id: String,
    pub shell: ManagedShell,
    pub process_id: u32,
    pub started_at: UtcTimestamp,
    pub end: Option<TerminalEnd>,
    pub owner: TerminalOwnerV1,
    pub controller: TerminalControllerV1,
    pub title: String,
    pub tool_call_id: Option<aura_domain_common::ToolCallId>,
    pub revision: aura_domain_common::Revision,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOwnerV1 {
    User,
    Agent,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalControllerV1 {
    User,
    Agent,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalControlRequestV1 {
    pub session_id: SessionId,
    pub terminal_id: String,
    pub controller: TerminalControllerV1,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalListV1 {
    pub terminals: Vec<TerminalViewV1>,
    pub next_cursor: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalOutputBatchV1 {
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    pub terminal_id: String,
    pub start_offset: u64,
    pub next_offset: u64,
    #[schemars(
        length(max = 87384),
        regex(pattern = "^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$")
    )]
    pub data_base64: String,
    pub truncated: bool,
    pub end: Option<TerminalEnd>,
}
