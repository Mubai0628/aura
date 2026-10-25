use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionCompatibility {
    NativeWindowsToolchain,
    LongRunningProcess,
    InteractiveTerminal,
    ProcessTreeCancellation,
}

/// Broad process networking is never represented as exact-origin HTTP access.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationNetwork {
    None,
    ExactOrigins,
    BroadProcess,
    // A page in Aura's built-in browser. Its own requests are held to the browser's site
    // policy (the block list, ReadOnly navigation, no local network from a public page), not to
    // exact origins.
    BrowserSitePolicy,
}
