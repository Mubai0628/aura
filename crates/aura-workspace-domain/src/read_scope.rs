use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A verified range excluded from an Agent's traversal. It is not an empty match.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FsSkippedScope {
    pub canonical_path: String,
    pub reason: FsSkippedScopeReason,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FsSkippedScopeReason {
    ProtectedScope,
    ControlAuthority,
    TargetUnavailable,
}
