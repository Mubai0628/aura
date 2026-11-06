//! `plugins.*` model tools, for writing a plugin with the user: check a plugin folder in the
//! workspace, install it as a plugin in development, and read what went wrong in a plugin's
//! parts. Installing never trusts a plugin, switches on what it asks for or fills in its
//! settings; the user decides those on the plugin page.
use aura_domain_common::UtcTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsValidateInput {
    /// The plugin's folder, relative to the workspace root.
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsValidateResult {
    pub name: Option<String>,
    pub version: Option<String>,
    /// False when the manifest or layout cannot be used: then it cannot be installed at all. A
    /// part with a problem is not offered; the rest of the plugin still works.
    pub installable: bool,
    /// Each with the part it is in.
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    /// What can be used, by kind (skills, commands, servers, hooks, panels, services).
    pub brings: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsDevInstallInput {
    /// The plugin's folder, relative to the workspace root.
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsDevInstallResult {
    pub installed: bool,
    pub name: Option<String>,
    /// Where the plugin is used from: the folder itself, never a copy.
    pub folder: Option<String>,
    /// `not_trusted` until the user trusts it on the plugin page; `ready` once trusted and on.
    pub state: Option<String>,
    /// Why it was not installed.
    pub problems: Vec<String>,
    /// What happens next, to tell the user.
    pub next: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsLogsInput {
    /// The plugin's name, as its manifest gives it.
    pub name: String,
    /// At most this many entries, newest last (default 50, at most 200).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Which part of a plugin an entry is about.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginLogSource {
    Hook,
    Mcp,
    Panel,
    Service,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginLogEntry {
    pub at: UtcTimestamp,
    pub source: PluginLogSource,
    /// The Hook, server, page or program it came from.
    pub part: String,
    /// What it said; secret values the plugin was given are taken out.
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PluginsLogsResult {
    pub name: String,
    pub entries: Vec<PluginLogEntry>,
    /// Older entries were left out.
    pub truncated: bool,
}
