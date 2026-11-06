pub use aura_domain_common::ComputerMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipPhase {
    #[default]
    Closed,
    WaitingForLogin,
    Ready,
    Disconnected,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComputerPipStatus {
    pub phase: PipPhase,
    pub notice: Option<String>,
}
