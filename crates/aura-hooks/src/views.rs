//! What the settings page and the session view read and send. Paths are the registry's
//! keys; every change names the registry revision it was made against.
use crate::{HookDefinition, HookEvent, HookPackageFile};
use aura_domain_common::{HookInvocationId, SessionId, ToolCallId, TurnTaskId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Why a registered Hook does or does not run now.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEntryState {
    /// Trusted, enabled and unchanged: it runs on its events.
    Ready,
    Disabled,
    NotTrusted,
    /// A file changed since the user trusted it; it does not run until trusted again.
    ContentChanged,
    /// The package cannot be read or its manifest is refused.
    Invalid,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookEntryView {
    pub path: String,
    /// The plugin that brings it; it is trusted, switched and removed with the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    pub enabled: bool,
    pub state: HookEntryState,
    pub definition: Option<HookDefinition>,
    pub fingerprint: Option<String>,
    pub trusted_fingerprint: Option<String>,
    pub problems: Vec<String>,
}

/// The registry in execution order.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookRegistryView {
    pub revision: u64,
    pub entries: Vec<HookEntryView>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookListRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookPreviewRequest {
    pub path: String,
}

/// Everything the user sees before trusting a package.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookPreview {
    pub path: String,
    pub definition: Option<HookDefinition>,
    pub files: Vec<HookPackageFile>,
    pub fingerprint: Option<String>,
    pub problems: Vec<String>,
    /// Already in the registry (trusting again replaces its fingerprint).
    pub registered: bool,
}

/// Trust exactly the previewed content; adds the Hook when it is not registered yet.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookTrustRequest {
    pub path: String,
    pub expected_fingerprint: String,
    pub enabled: bool,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookSetEnabledRequest {
    pub path: String,
    pub enabled: bool,
    pub expected_revision: u64,
}

/// Untrust or remove one Hook. Removing only drops the registry entry; the folder stays.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookPathRequest {
    pub path: String,
    pub expected_revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookMoveDirection {
    Up,
    Down,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookMoveRequest {
    pub path: String,
    pub direction: HookMoveDirection,
    pub expected_revision: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    Continued,
    Transformed,
    Blocked,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFailureKind {
    /// The content no longer matches the trusted fingerprint.
    ContentChanged,
    NotStarted,
    TimedOut,
    OutputLimit,
    ExitCode,
    InvalidResult,
    Cancelled,
}

/// What the decision did: the action went on, was stopped, or only later Hooks were.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEffect {
    None,
    Applied,
    ActionBlocked,
    LaterHooksStopped,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookInvocationView {
    pub invocation_id: HookInvocationId,
    pub path: String,
    pub hook_name: String,
    pub hook_version: String,
    pub fingerprint: String,
    pub event: HookEvent,
    pub session_id: Option<SessionId>,
    pub turn_task_id: Option<TurnTaskId>,
    pub tool_call_id: Option<ToolCallId>,
    pub started_at: UtcTimestamp,
    pub duration_ms: u64,
    pub outcome: HookOutcome,
    pub failure: Option<HookFailureKind>,
    pub effect: HookEffect,
    /// The block reason, or what went wrong.
    pub reason: Option<String>,
    /// A short description of the transform.
    pub summary: Option<String>,
    /// The start of stderr, for the user's own debugging.
    pub stderr: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookInvocationListRequest {
    /// One Hook's calls, or every Hook's when absent.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub turn_task_id: Option<TurnTaskId>,
    pub limit: u32,
}

/// Newest first.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookInvocationList {
    pub invocations: Vec<HookInvocationView>,
    /// Every call of the turn the request names, however many the page holds.
    pub statistics: Option<HookTurnStatistics>,
}

/// A turn's Hook calls, as the session view shows them when the turn ends.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookTurnStatistics {
    pub calls: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub blocked: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookSelectDirectoryRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookSelectedDirectory {
    pub path: Option<String>,
}
