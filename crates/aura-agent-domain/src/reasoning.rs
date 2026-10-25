//! The thinking level the user chose in
//! the composer for the turn a message starts. Each conversation remembers its own choice per
//! model and sends it with the message; with none the turn takes the model's default.
//!
//! The same seven levels as the model domain's `ReasoningEffort`, which this kernel crate cannot
//! name (it depends on `aura-domain-common` alone); Core converts one into the other.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TurnReasoningEffort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl TurnReasoningEffort {
    pub const ALL: [Self; 7] =
        [Self::Off, Self::Minimal, Self::Low, Self::Medium, Self::High, Self::Xhigh, Self::Max];
}
