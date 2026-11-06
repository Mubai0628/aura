//! What Aura keeps for a plugin's channels in `plugins\state\<name>.json`, written only by Aura:
//! the conversation each chat is in now, and who wrote lately without being on the allowlist.
//! Both lists keep the newest first and are bounded. A file that cannot be read counts as empty
//! and is replaced at the next change; the plugin never reads or writes it.
use super::runtime::PluginRuntime;
use aura_agent_domain::PluginChat;
use aura_core_base::append_core_log;
use aura_domain_common::{SessionId, UtcTimestamp};
use aura_plugins::{
    PLUGIN_UNKNOWN_USERS_MAX, PluginPackage, PluginRemoteView, PluginSetting, PluginUnknownUser,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The most chats whose conversation is kept; the one used longest ago goes first.
pub const PLUGIN_CHATS_MAX: usize = 500;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChatState {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    chats: Vec<ChatSession>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    unknown: Vec<PluginUnknownUser>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChatSession {
    chat: PluginChat,
    session_id: SessionId,
    used_at: UtcTimestamp,
}

impl PluginRuntime {
    /// Where Aura keeps what it knows of the plugin's chats.
    #[must_use]
    pub fn chat_state_file(&self, name: &str) -> PathBuf {
        self.root().join("state").join(format!("{name}.json"))
    }

    fn read_chats(&self, name: &str) -> ChatState {
        std::fs::read(self.chat_state_file(name))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Changes the plugin's chat state, one change at a time; a failed write is logged and the
    /// state stays as it was.
    fn change_chats(&self, name: &str, change: impl FnOnce(&mut ChatState)) {
        let Ok(_one_at_a_time) = self.changing_chats.lock() else { return };
        let mut state = self.read_chats(name);
        change(&mut state);
        let file = self.chat_state_file(name);
        let next = file.with_extension("json.new");
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&next, serde_json::to_vec(&state).unwrap_or_default()))
            .and_then(|()| std::fs::rename(&next, &file));
        if let Err(error) = written {
            append_core_log(
                "plugins.chat_state_failed",
                serde_json::json!({"file": file.display().to_string(), "error": error.to_string()}),
            );
        }
    }

    /// The conversation the chat is in now, and the others the plugin's chats are in, the one
    /// used last first.
    #[must_use]
    pub fn chat_sessions(
        &self,
        plugin: &str,
        chat: &PluginChat,
    ) -> (Option<SessionId>, Vec<SessionId>) {
        let state = self.read_chats(plugin);
        let current =
            state.chats.iter().find(|kept| kept.chat == *chat).map(|kept| kept.session_id);
        let mut others = Vec::new();
        for kept in &state.chats {
            if Some(kept.session_id) != current && !others.contains(&kept.session_id) {
                others.push(kept.session_id);
            }
        }
        (current, others)
    }

    /// The chat is in `session` now.
    pub fn remember_chat(&self, plugin: &str, chat: &PluginChat, session: SessionId) {
        self.change_chats(plugin, |state| {
            state.chats.retain(|kept| kept.chat != *chat);
            let used_at = UtcTimestamp::now();
            state.chats.insert(0, ChatSession { chat: chat.clone(), session_id: session, used_at });
            state.chats.truncate(PLUGIN_CHATS_MAX);
        });
    }

    /// Someone not on the allowlist wrote: kept once, with the name they have now. True when they
    /// were not listed before.
    pub fn note_unknown(&self, plugin: &str, user: PluginUnknownUser) -> bool {
        let mut new = true;
        self.change_chats(plugin, |state| {
            let listed = state.unknown.len();
            state.unknown.retain(|kept| (&kept.channel, &kept.id) != (&user.channel, &user.id));
            new = state.unknown.len() == listed;
            state.unknown.insert(0, user);
            state.unknown.truncate(PLUGIN_UNKNOWN_USERS_MAX);
        });
        new
    }

    /// What the plugin page shows for a plugin with a background service; someone the user put
    /// on the allowlist since they wrote is no longer listed.
    pub(crate) fn remote_view(
        &self,
        package: &PluginPackage,
        setting: Option<&PluginSetting>,
    ) -> Option<PluginRemoteView> {
        package.manifest.declared.service.as_ref()?;
        let remote = setting.map(|setting| setting.remote.clone()).unwrap_or_default();
        let unknown = self
            .read_chats(&package.manifest.name)
            .unknown
            .into_iter()
            .filter(|user| !remote.allows(&user.channel, &user.id))
            .collect();
        Some(PluginRemoteView {
            channels: package.manifest.declared.channels.clone(),
            scope: remote.scope,
            allowed: remote.allowed,
            remote_approval: remote.remote_approval,
            unknown,
        })
    }
}
