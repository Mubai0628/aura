//! What the user fills in and chooses for a plugin on the plugin page, kept in `plugins.json`:
//! the values of the plugin's own settings, and for its background service the switch, the
//! conversations it may act in, the people on each channel who may reach Aura and whether they
//! may approve remotely. Only the plugin page changes these, except that a plugin that declares
//! `settings` may set its own text values the same way.
use aura_domain_common::{CredentialId, SessionId, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PLUGIN_TEXT_VALUE_MAX_CHARS: usize = 2_000;
pub const PLUGIN_EXTERNAL_ID_MAX_CHARS: usize = 128;
pub const PLUGIN_EXTERNAL_NAME_MAX_CHARS: usize = 64;
pub const PLUGIN_ALLOWED_MAX: usize = 200;

/// The values of the plugin's own settings (`[[settings]]` in its manifest).
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginValues {
    /// Setting key → its text.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub text: BTreeMap<String, String>,
    /// Setting key → the credential its secret value is kept in. Kept as a list with the key as
    /// data: a key may be `token`, a name no config file may use for a field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "secret_list")]
    #[schemars(with = "Vec<PluginValueSecret>")]
    pub secrets: BTreeMap<String, CredentialId>,
}

impl PluginValues {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.secrets.is_empty()
    }
}

/// One secret setting's credential in `plugins.json`.
#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct PluginValueSecret {
    key: String,
    secret_id: CredentialId,
}

mod secret_list {
    use super::PluginValueSecret;
    use aura_domain_common::CredentialId;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(
        secrets: &BTreeMap<String, CredentialId>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            secrets.iter().map(|(key, id)| PluginValueSecret { key: key.clone(), secret_id: *id }),
        )
    }

    /// The list, or the map from setting key to credential written before it.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        List(Vec<PluginValueSecret>),
        Map(BTreeMap<String, CredentialId>),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<String, CredentialId>, D::Error> {
        let list = match Stored::deserialize(deserializer)? {
            Stored::Map(map) => return Ok(map),
            Stored::List(list) => list,
        };
        let mut secrets = BTreeMap::new();
        for entry in list {
            if secrets.insert(entry.key.clone(), entry.secret_id).is_some() {
                return Err(D::Error::custom(format!("保密设置 {} 出现了两次", entry.key)));
            }
        }
        Ok(secrets)
    }
}

/// The conversations the plugin's background service may act in: every conversation of these
/// workspaces, and these conversations. Empty: none.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginScope {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<SessionId>,
}

impl PluginScope {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty() && self.sessions.is_empty()
    }
}

/// Someone on a chat platform the user lets reach Aura through one of the plugin's channels.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginAllowedUser {
    pub channel: String,
    /// The platform's own id of the user.
    pub id: String,
    /// The user's own note of who this is; may be empty.
    #[serde(default)]
    pub name: String,
}

/// The background service's switch and what it may do. All empty or off until the user changes
/// them on the plugin page.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginRemote {
    /// The service runs only while this is on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub service: bool,
    #[serde(default, skip_serializing_if = "PluginScope::is_empty")]
    pub scope: PluginScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<PluginAllowedUser>,
    /// People on the allowlist may approve from a private chat, under the remote approval rules.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remote_approval: bool,
}

impl PluginRemote {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether `user` on `channel` is on the allowlist.
    #[must_use]
    pub fn allows(&self, channel: &str, user: &str) -> bool {
        self.allowed.iter().any(|allowed| allowed.channel == channel && allowed.id == user)
    }
}

fn one_line(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

/// Checks the values' shape; `credentials` collects every credential the list names. Whether a
/// key is one the manifest declares is checked where the manifest is read, since it can change.
pub(crate) fn check_values(
    name: &str,
    values: &PluginValues,
    credentials: &mut BTreeSet<CredentialId>,
) -> Result<(), String> {
    for key in values.text.keys().chain(values.secrets.keys()) {
        if !crate::declarations::option_key_ok(key) {
            return Err(format!("{name} 的设置 {} 的键不合规则", crate::manifest::shown(key)));
        }
    }
    for (key, text) in &values.text {
        if !one_line(text, PLUGIN_TEXT_VALUE_MAX_CHARS) {
            return Err(format!(
                "{name} 的设置 {key} 要是一行文字，最多 {PLUGIN_TEXT_VALUE_MAX_CHARS} 个字符"
            ));
        }
        if values.secrets.contains_key(key) {
            return Err(format!("{name} 的设置 {key} 既有文字又有保密值"));
        }
    }
    for id in values.secrets.values() {
        if !credentials.insert(*id) {
            return Err(format!("{name} 的密钥 {id} 被用了两次"));
        }
    }
    Ok(())
}

pub(crate) fn check_remote(name: &str, remote: &PluginRemote) -> Result<(), String> {
    let workspaces: BTreeSet<_> = remote.scope.workspaces.iter().collect();
    if workspaces.len() != remote.scope.workspaces.len() {
        return Err(format!("{name} 的会话范围里有重复的工作区"));
    }
    let sessions: BTreeSet<_> = remote.scope.sessions.iter().collect();
    if sessions.len() != remote.scope.sessions.len() {
        return Err(format!("{name} 的会话范围里有重复的会话"));
    }
    if remote.allowed.len() > PLUGIN_ALLOWED_MAX {
        return Err(format!("{name} 的白名单最多 {PLUGIN_ALLOWED_MAX} 人"));
    }
    let mut people = BTreeSet::new();
    for allowed in &remote.allowed {
        let id = allowed.id.trim();
        if !crate::valid_channel_id(&allowed.channel) {
            return Err(format!(
                "{name} 的白名单里的渠道 {} 不合规则",
                crate::manifest::shown(&allowed.channel)
            ));
        }
        if id.is_empty() || !one_line(id, PLUGIN_EXTERNAL_ID_MAX_CHARS) {
            return Err(format!(
                "{name} 的白名单里的用户编号要是一行不空的文字，最多 \
                 {PLUGIN_EXTERNAL_ID_MAX_CHARS} 个字符"
            ));
        }
        if !one_line(&allowed.name, PLUGIN_EXTERNAL_NAME_MAX_CHARS) {
            return Err(format!(
                "{name} 的白名单里的名字要是一行，最多 {PLUGIN_EXTERNAL_NAME_MAX_CHARS} 个字符"
            ));
        }
        if !people.insert((allowed.channel.as_str(), allowed.id.as_str())) {
            return Err(format!(
                "{name} 的白名单里 {} 的 {} 出现了两次",
                allowed.channel,
                crate::manifest::shown(&allowed.id)
            ));
        }
    }
    Ok(())
}
