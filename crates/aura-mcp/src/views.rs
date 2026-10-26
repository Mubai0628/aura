//! What Settings → MCP reads and sends. Server IDs are the registry's keys; every change
//! names the registry revision it was made against, and trust is only ever given at the
//! fingerprint the user was shown.
use crate::{McpServerConfig, ResolvedCommand};
use aura_domain_common::UtcTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Why a configured server does or does not offer tools now.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerState {
    /// Trusted and unchanged, but switched off.
    Disabled,
    NotTrusted,
    /// The command, its file, the address or another trusted setting changed since the
    /// user trusted it; it does not run until trusted again.
    ConfigChanged,
    /// The configuration is refused or the command cannot be found.
    Invalid,
    /// Aura is starting it to list its tools.
    Starting,
    Ready,
    /// The last start or tool listing failed; see `last_error`.
    Failed,
    /// Trusted and on, but it signs in with OAuth and is not signed in yet.
    SignInRequired,
}

/// A server's OAuth sign-in as the settings page shows it. Tokens never appear here.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct McpSignInView {
    pub signed_in: bool,
    /// A sign-in waits for the browser.
    pub pending: bool,
    /// The host of the authorization server the browser was sent to.
    pub authorization_server: Option<String>,
    /// Why the last sign-in or refresh did not complete.
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct McpServerView {
    pub config: McpServerConfig,
    /// The plugin that brings it; it is removed with the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    /// A plugin's server the user saved with a configuration of their own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plugin_changed: bool,
    pub state: McpServerState,
    /// What would be trusted now; absent when the command cannot be resolved.
    pub fingerprint: Option<String>,
    pub resolved_command: Option<ResolvedCommand>,
    pub problems: Vec<String>,
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    pub protocol_version: Option<String>,
    pub tool_count: Option<u32>,
    pub discovered_at: Option<UtcTimestamp>,
    pub last_error: Option<String>,
    /// Session instances running now.
    pub running_instances: u32,
    /// Present for a server that signs in with OAuth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<McpSignInView>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct McpRegistryView {
    pub revision: u64,
    pub servers: Vec<McpServerView>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpListRequest {}

/// Check a configuration before saving it: problems, the resolved command, the fingerprint.
/// Nothing is started.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpPreviewRequest {
    pub server: McpServerConfig,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct McpPreview {
    pub fingerprint: Option<String>,
    pub resolved_command: Option<ResolvedCommand>,
    pub problems: Vec<String>,
}

/// Add a server (`replaces` absent) or change one. Trust and the on/off switch are never
/// taken from this request: a new server starts untrusted and off, and a changed one keeps
/// its old trust, which no longer matches when anything that runs has changed.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSaveRequest {
    pub server: McpServerConfig,
    #[serde(default)]
    pub replaces: Option<String>,
    pub expected_revision: u64,
}

/// Trust exactly the shown fingerprint and switch the server on.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpTrustRequest {
    pub id: String,
    pub expected_fingerprint: String,
    pub expected_revision: u64,
}

/// Untrust or remove one server.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerRequest {
    pub id: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSetEnabledRequest {
    pub id: String,
    pub enabled: bool,
    pub expected_revision: u64,
}

/// Start the server now and list its tools again (the settings page's test).
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpRefreshRequest {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpDescribeRequest {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct McpToolView {
    pub canonical_name: String,
    pub wire_name: String,
    pub title: Option<String>,
    pub description: String,
}

/// One server with its tools and the end of its standard error output.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct McpServerDetail {
    pub server: McpServerView,
    pub tools: Vec<McpToolView>,
    pub skipped_tools: Vec<String>,
    pub stderr: Vec<String>,
}

/// Write a secret for one server. The value travels only on the dedicated secret input,
/// never in these parameters. Without `secret_id` a new secret is created; with one, its next
/// value replaces the current one (the server's trust is unchanged).
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSecretSetRequest {
    pub server_id: String,
    #[serde(default)]
    pub secret_id: Option<aura_domain_common::CredentialId>,
}

/// The ID the server's configuration names the secret by; the value is never returned, and
/// an answer with anything more is refused.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSecretSetResult {
    pub secret_id: aura_domain_common::CredentialId,
}

/// Sign in to a trusted OAuth server. Core prepares the sign-in and answers with the
/// authorization address; Main opens it in the system browser (https, or this computer only);
/// the browser's return completes the sign-in in Core.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSignInRequest {
    pub id: String,
}

/// The address the browser opens and its authorization server's host (shown to the user,
/// and as a link to copy when no browser opened).
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSignInBegun {
    pub authorization_url: String,
    pub authorization_server: String,
}

/// Forget a server's sign-in.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSignOutRequest {
    pub id: String,
}
