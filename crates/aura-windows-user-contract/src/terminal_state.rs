//! P2 ownership/lifecycle notification from the authenticated Core generation.
use crate::TerminalViewV1;
use aura_domain_common::RuntimeInstanceId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalStateChangedV1 {
    pub runtime_instance_id: RuntimeInstanceId,
    pub view: TerminalViewV1,
}
