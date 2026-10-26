//! What `plugins.json` keeps for each installed plugin: the switch, the content the user
//! trusted and each part's own fingerprint at that moment, the credentials Aura keeps for its
//! servers' secrets, and a newer copy waiting to be trusted. What a plugin brings is read from
//! its folder; nothing here describes it.
use crate::manifest::valid_plugin_name;
use aura_domain_common::CredentialId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Each part's own fingerprint when the plugin was trusted. A part is offered only while it is
/// still exactly that, so a change after the plugin's own check is never used.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginTrustedParts {
    /// Skill folder (relative to `skills`) → the Skill's fingerprint.
    #[serde(default)]
    pub skills: BTreeMap<String, String>,
    /// Command name → the command's revision.
    #[serde(default)]
    pub commands: BTreeMap<String, String>,
    /// Hook folder (under `hooks`) → the Hook package's fingerprint.
    #[serde(default)]
    pub hooks: BTreeMap<String, String>,
    /// MCP server ID → the configuration fingerprint it was trusted at (the program it resolves
    /// to and that program's content). A server whose check failed at trust time is absent and
    /// is trusted on its own later.
    #[serde(default)]
    pub servers: BTreeMap<String, String>,
}

/// Parts of each kind, by the name `trusted_parts` uses for them.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginParts {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub skills: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub commands: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hooks: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub servers: BTreeSet<String>,
}

impl PluginParts {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
            && self.commands.is_empty()
            && self.hooks.is_empty()
            && self.servers.is_empty()
    }
}

/// What the user changed in what the plugin brings: parts switched off, and MCP servers saved
/// with a configuration of the user's own. Kept across the plugin's updates for the parts the
/// new version still has, and removed with the plugin.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginChanges {
    #[serde(default, skip_serializing_if = "PluginParts::is_empty")]
    pub switched_off: PluginParts,
    /// MCP server ID → the configuration the user saved in place of the plugin's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub servers: BTreeMap<String, aura_mcp::McpServerConfig>,
}

impl PluginChanges {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.switched_off.is_empty() && self.servers.is_empty()
    }
}

/// A credential Aura keeps for one of the plugin's server secrets.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSecret {
    /// The MCP server ID.
    pub server: String,
    /// The environment variable, or `http` for an HTTP sign-in secret.
    pub key: String,
    pub secret_id: CredentialId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPendingSetting {
    /// Where the newer copy was imported from, as the user chose it.
    #[serde(default)]
    pub imported_from: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSetting {
    pub name: String,
    pub enabled: bool,
    #[serde(default)]
    pub trusted_fingerprint: Option<String>,
    #[serde(default)]
    pub trusted_parts: PluginTrustedParts,
    #[serde(default)]
    pub secrets: Vec<PluginSecret>,
    #[serde(default)]
    pub imported_from: Option<String>,
    #[serde(default)]
    pub pending: Option<PluginPendingSetting>,
    /// Written only once the user changed something, so an untouched entry reads the same in
    /// earlier versions.
    #[serde(default, skip_serializing_if = "PluginChanges::is_empty")]
    pub changes: PluginChanges,
}

impl PluginSetting {
    /// A newly imported plugin: switched on, not trusted.
    #[must_use]
    pub fn imported(name: &str, imported_from: Option<String>) -> Self {
        Self {
            name: name.to_owned(),
            enabled: true,
            trusted_fingerprint: None,
            trusted_parts: PluginTrustedParts::default(),
            secrets: Vec::new(),
            imported_from,
            pending: None,
            changes: PluginChanges::default(),
        }
    }

    /// The credential kept for a server's secret, if there is one.
    #[must_use]
    pub fn secret(&self, server: &str, key: &str) -> Option<CredentialId> {
        self.secrets.iter().find(|s| s.server == server && s.key == key).map(|s| s.secret_id)
    }
}

fn fingerprint_ok(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Checks the list as a whole; the reason names the plugin.
pub fn check_settings(plugins: &[PluginSetting]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut credentials = BTreeSet::new();
    for plugin in plugins {
        let name = &plugin.name;
        if !valid_plugin_name(name) {
            return Err(format!("插件名 {name} 不合规则"));
        }
        if !names.insert(name.as_str()) {
            return Err(format!("{name} 出现了两次"));
        }
        let parts = &plugin.trusted_parts;
        let fingerprints = plugin
            .trusted_fingerprint
            .iter()
            .chain(parts.skills.values())
            .chain(parts.hooks.values())
            .chain(parts.servers.values());
        if fingerprints.into_iter().any(|f| !fingerprint_ok(f)) {
            return Err(format!("{name} 的指纹必须是 64 位十六进制"));
        }
        if plugin.trusted_fingerprint.is_none() && *parts != PluginTrustedParts::default() {
            return Err(format!("{name} 没有信任，却记着信任时的内容"));
        }
        for (id, server) in &plugin.changes.servers {
            if server.id != *id {
                return Err(format!("{name} 改过的服务器 {id} 与它记着的 id 不一致"));
            }
            let problems = aura_mcp::validate_server(server);
            if !problems.is_empty() {
                return Err(format!("{name} 改过的服务器 {id}：{}", problems.join("；")));
            }
        }
        let mut keys = BTreeSet::new();
        for secret in &plugin.secrets {
            if !keys.insert((secret.server.as_str(), secret.key.as_str())) {
                return Err(format!("{name} 的 {} {} 有两个密钥", secret.server, secret.key));
            }
            if !credentials.insert(secret.secret_id) {
                return Err(format!("密钥 {} 被用了两次", secret.secret_id));
            }
        }
    }
    Ok(())
}
