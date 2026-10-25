//! Non-secret opening metadata. A renderer token never grants authority to cancel a Core request.
use crate::UserTerminalOpenRequestV1;
use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId, ToolWireVersionV1};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOpeningPhaseV1 {
    WaitingRuntime,
    WaitingLaunch,
    Launching,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalOpeningStartV1 {
    pub contract_version: ToolWireVersionV1,
    #[schemars(
        length(min = 36, max = 36),
        regex(pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    )]
    pub opening_id: String,
    pub request: UserTerminalOpenRequestV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalOpeningRefV1 {
    pub contract_version: ToolWireVersionV1,
    #[schemars(
        length(min = 36, max = 36),
        regex(pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    )]
    pub opening_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalOpeningProgressV1 {
    pub contract_version: ToolWireVersionV1,
    pub runtime_instance_id: RuntimeInstanceId,
    pub request_id: CorrelationId,
    pub session_id: SessionId,
    pub phase: TerminalOpeningPhaseV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalOpeningChangeV1 {
    pub contract_version: ToolWireVersionV1,
    #[schemars(
        length(min = 36, max = 36),
        regex(pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    )]
    pub opening_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    pub phase: TerminalOpeningPhaseV1,
}
