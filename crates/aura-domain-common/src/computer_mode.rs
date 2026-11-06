use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Shared here because the turn records of the agent domain and the desktop domain's routing both
// carry it, and neither domain may depend on the other.
/// The desktop selected by the user for one durable turn. It is never a fallback policy.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerMode {
    #[default]
    Foreground,
    ChildSession,
}
impl ComputerMode {
    pub fn is_foreground(&self) -> bool {
        *self == Self::Foreground
    }
}
