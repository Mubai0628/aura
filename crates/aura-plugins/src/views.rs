//! What Settings → 插件 shows and the requests it sends.
use crate::files::PluginFile;
use aura_hooks::HookDefinition;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginLayout {
    /// `aura-plugin.toml`.
    Aura,
    /// `.claude-plugin/plugin.json`; the parts Aura supports are used.
    ClaudeCode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// Imported, not trusted yet: nothing it brings is offered.
    NotTrusted,
    /// Trusted and switched on: what it brings is offered.
    Ready,
    /// Trusted and switched off.
    Disabled,
    /// A file changed after it was trusted: nothing is offered until it is trusted again.
    ContentChanged,
    /// Its manifest or layout cannot be used.
    Invalid,
    /// Its folder is missing or cannot be read.
    Unavailable,
}

/// What each process tree the plugin starts may use (an MCP server instance, one Hook call).
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginLimits {
    pub memory_mb: u32,
    /// Processes alive at once.
    pub processes: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSkillItem {
    /// The Skill folder, relative to the plugin's `skills`.
    pub relative: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCommandItem {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub warnings: Vec<String>,
    /// Why it is not offered.
    pub problem: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginServerKind {
    Stdio,
    Http,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSignIn {
    /// A secret you enter in Aura.
    Secret,
    /// OAuth in the browser.
    Oauth,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginServerItem {
    /// The name inside the plugin.
    pub name: String,
    /// The MCP server it runs as; its tools are `mcp.<id>.<tool>`.
    pub id: String,
    pub display_name: String,
    pub kind: PluginServerKind,
    /// The program (with the plugin folder filled in) or the address.
    pub target: String,
    pub arguments: Vec<String>,
    /// Environment variables whose values you enter in Aura.
    pub secret_names: Vec<String>,
    pub sign_in: Option<PluginSignIn>,
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginHookItem {
    /// The Hook package folder, relative to the plugin's `hooks`.
    pub folder: String,
    pub definition: Option<HookDefinition>,
    pub problems: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCounts {
    pub skills: u32,
    pub commands: u32,
    pub servers: u32,
    pub hooks: u32,
}

/// A newer copy of an installed plugin, imported and waiting to be trusted. The installed copy
/// keeps working until then.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPendingView {
    pub version: Option<String>,
    pub fingerprint: Option<String>,
    pub counts: PluginCounts,
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginView {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub layout: Option<PluginLayout>,
    pub state: PluginState,
    pub enabled: bool,
    /// The installed copy, in Aura's plugin folder.
    pub path: String,
    pub fingerprint: Option<String>,
    pub trusted_fingerprint: Option<String>,
    pub limits: PluginLimits,
    pub counts: PluginCounts,
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    /// Parts of the plugin Aura does not use, each with the reason.
    pub unsupported: Vec<String>,
    pub pending_update: Option<PluginPendingView>,
    pub file_count: u32,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRegistryView {
    pub revision: u64,
    pub plugins: Vec<PluginView>,
    /// Aura's plugin folder.
    pub library: String,
    /// Why nothing any plugin brings is offered (`plugins.json` cannot be used).
    pub issue: Option<String>,
}

/// Everything a plugin brings, for review before it is trusted.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPreview {
    pub plugin: PluginView,
    /// The update waiting to be trusted, rather than the installed copy.
    pub pending: bool,
    /// The fingerprint of the copy shown; trusting names it.
    pub fingerprint: Option<String>,
    pub skills: Vec<PluginSkillItem>,
    pub commands: Vec<PluginCommandItem>,
    pub servers: Vec<PluginServerItem>,
    pub hooks: Vec<PluginHookItem>,
    pub files: Vec<PluginFile>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginListRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRequest {
    pub name: String,
    /// The update waiting to be trusted, rather than the installed copy.
    #[serde(default)]
    pub pending: bool,
}

/// Copies a plugin folder (or unpacks a ZIP file) into Aura's own. A plugin with a name that is
/// already installed becomes that plugin's pending update.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginImportRequest {
    pub path: String,
    pub expected_revision: u64,
}

/// Trusts exactly the content that was shown (its fingerprint); for a pending update, this also
/// replaces the installed copy.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginTrustRequest {
    pub name: String,
    #[serde(default)]
    pub pending: bool,
    pub expected_fingerprint: String,
    pub expected_revision: u64,
}

/// Untrust, uninstall or discard a pending update.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginChangeRequest {
    pub name: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSetEnabledRequest {
    pub name: String,
    pub enabled: bool,
    pub expected_revision: u64,
}

/// Main's own folder dialog for importing a plugin.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSelectDirectoryRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct PluginSelectedDirectory {
    /// Absent when the user cancelled.
    pub path: Option<String>,
}

/// Main's own file dialog for importing a plugin packed as a ZIP file.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSelectArchiveRequest {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct PluginSelectedArchive {
    /// Absent when the user cancelled.
    pub path: Option<String>,
}

/// Main opened an installed plugin's folder in the file manager, or says why it did not.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginDirectoryOpened {
    pub completed: bool,
    pub message: String,
}
