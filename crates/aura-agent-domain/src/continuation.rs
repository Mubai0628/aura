use aura_domain_common::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Why Main started a new Core while Aura stayed open: the Core exited by itself, Main ended it
/// after a silent and idle minute, its channel could not be recovered, or the user asked for
/// 立即重启. Main passes it to the new Core, which then continues the turns the restart cut off.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreRestartCause {
    Crash,
    Unresponsive,
    TransportLost,
    UserRestart,
}

impl CoreRestartCause {
    /// The value of Core's `--continue-after=` argument.
    #[must_use]
    pub fn parse_argument(value: &str) -> Option<Self> {
        match value {
            "crash" => Some(Self::Crash),
            "unresponsive" => Some(Self::Unresponsive),
            "transport_lost" => Some(Self::TransportLost),
            "user_restart" => Some(Self::UserRestart),
            _ => None,
        }
    }
}

/// The turn's "继续" was sent by Aura itself after Core restarted, and why it restarted.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentAutoContinuedView {
    pub cause: CoreRestartCause,
    pub restarted_at: UtcTimestamp,
}

/// The user's 继续 for a turn a restart of Aura cut off: a new turn says "继续" ahead of the
/// queue, which stays paused until the user resumes it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentContinueRequest {
    pub session_id: SessionId,
    /// The cut-off turn the user saw; a later turn makes the request stale.
    pub turn_task_id: TurnTaskId,
}
