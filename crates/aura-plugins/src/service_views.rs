//! The plugin page's view of a plugin's background service and own settings, and the two
//! requests only the plugin page makes for them.
use crate::{PluginAllowedUser, PluginChannel, PluginOption, PluginScope};
use aura_agent_domain::PluginChatKind;
use aura_domain_common::UtcTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The most people a plugin's page lists who wrote to its channels and are not on the list.
pub const PLUGIN_UNKNOWN_USERS_MAX: usize = 20;

/// Someone one of the plugin's channels heard from lately who is not on its allowlist. Nothing
/// they sent reached Aura.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginUnknownUser {
    pub channel: String,
    /// The platform's own id of the person.
    pub id: String,
    /// The name the platform gave them; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub chat: PluginChatKind,
    /// When they last wrote.
    pub at: UtcTimestamp,
}

/// What the plugin page shows and changes for a plugin's background service: its channels, the
/// conversations it may act in, who may reach Aura on each channel, whether they may approve
/// remotely, and who wrote lately without being on the list (newest first).
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRemoteView {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<PluginChannel>,
    pub scope: PluginScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<PluginAllowedUser>,
    pub remote_approval: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<PluginUnknownUser>,
}

/// What the plugin's background service is doing now.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PluginServiceStatus {
    /// Its switch on the plugin page is off.
    Off,
    /// On, and it cannot start yet: why.
    Waiting {
        reason: String,
    },
    Starting,
    Running {
        since: UtcTimestamp,
    },
    /// It ended; it starts again in `seconds`.
    Restarting {
        seconds: u32,
        reason: String,
    },
    /// It does not start again until the plugin or its settings change: why.
    Stopped {
        reason: String,
    },
}

/// The plugin's background service on the plugin page.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginServiceView {
    /// Its switch on the plugin page.
    pub on: bool,
    pub status: PluginServiceStatus,
}

/// One of the plugin's own settings with its value.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginOptionValue {
    pub option: PluginOption,
    /// A text setting's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A secret setting has a value kept in the Credential Manager. The value never leaves Core.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub set: bool,
}

/// Saves what the plugin page changed for a plugin. Each part given replaces that part; the
/// others stay as they are.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConfigureRequest {
    pub name: String,
    pub expected_revision: u64,
    /// The background service's switch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<bool>,
    /// Text settings, key → text; an empty text removes the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<BTreeMap<String, String>>,
    /// Secret settings whose values go.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forget_secrets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<PluginScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed: Option<Vec<PluginAllowedUser>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_approval: Option<bool>,
}

/// Sets a secret setting's value. The value arrives only on the secret input frame.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSecretSetRequest {
    pub name: String,
    pub key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSecretSetResult {
    /// `plugins.json`'s revision with the secret's credential named.
    pub revision: u64,
}
