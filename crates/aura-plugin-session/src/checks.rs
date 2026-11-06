//! Whether a plugin may act, and the message checks made before anything is sent.
use aura_agent_domain::{
    PLUGIN_DEDUPE_KEY_MAX_CHARS, PLUGIN_MESSAGE_MAX_CHARS, PLUGIN_NOTICE_MAX_CHARS,
    PLUGIN_STATUS_MAX_CHARS,
};
use aura_domain_common::{CommandInvocationId, SessionId};
use aura_plugins::{PluginCapability, PluginState};

/// What Core knows of a plugin when it calls.
pub enum Standing<'a> {
    /// Usable, with the capabilities its trusted manifest declares.
    Ready(&'a [PluginCapability]),
    /// Installed, in this state.
    Installed(PluginState),
    Missing,
}

/// The plugin may use `needed` now: it is installed, usable and declared it.
pub fn permitted(
    plugin: &str,
    standing: Standing<'_>,
    needed: PluginCapability,
) -> anyhow::Result<()> {
    let Standing::Ready(declared) = standing else { return Err(unusable(plugin, standing)) };
    anyhow::ensure!(
        declared.contains(&needed),
        "plugin_session.capability_missing: 插件 {plugin} 没有声明 {}，不能{}",
        needed.name(),
        purpose(needed)
    );
    Ok(())
}

/// Why a plugin that was not found usable cannot act now, and where the user changes that.
pub fn unusable(plugin: &str, standing: Standing<'_>) -> anyhow::Error {
    let why = match standing {
        Standing::Missing => {
            return anyhow::anyhow!("plugin_session.plugin_missing: 没有装名为 {plugin} 的插件");
        }
        Standing::Installed(PluginState::NotTrusted) => "还没有信任，在 设置 → 插件 里信任后才能用",
        Standing::Installed(PluginState::ContentChanged) => {
            "的文件在信任后改过，在 设置 → 插件 里重新信任后才能用"
        }
        Standing::Installed(PluginState::Disabled) => "已关闭，在 设置 → 插件 里打开后才能用",
        Standing::Installed(PluginState::Invalid | PluginState::Unavailable) => {
            "现在读不出来，原因见 设置 → 插件"
        }
        // A usable plugin is never reported here; a state read between two changes says so.
        Standing::Ready(_) | Standing::Installed(PluginState::Ready) => "刚刚改过，请再试一次",
    };
    anyhow::anyhow!("plugin_session.not_ready: 插件 {plugin} {why}")
}

fn purpose(capability: PluginCapability) -> &'static str {
    match capability {
        PluginCapability::SessionRead => "读这个会话的状态",
        PluginCapability::SessionEvents => "收到这个会话的事件",
        PluginCapability::SessionSend => "替你在这个会话里发消息",
        PluginCapability::SessionControl => "停止这个会话的回合",
        PluginCapability::Notify => "显示提示",
        PluginCapability::Status => "在状态栏显示",
        PluginCapability::Commands => "运行自己的命令",
        PluginCapability::Settings => "读写自己的设置",
    }
}

/// The command a plugin's message with `key` is sent as: the same plugin, conversation and key
/// always name the same command, so the message sent again is the same message.
pub fn message_command(plugin: &str, session: SessionId, key: &str) -> CommandInvocationId {
    command_of("aura plugin session message command v1", &[plugin, &session.to_string(), key])
}

/// The one command these parts always name, for what is done once however often it is asked.
pub(crate) fn command_of(context: &str, parts: &[&str]) -> CommandInvocationId {
    let mut hash = blake3::Hasher::new_derive_key(context);
    for part in parts {
        hash.update(&(part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash.finalize().as_bytes()[..16]);
    CommandInvocationId::from_bytes(bytes)
}

/// A de-duplication key: a message id from the plugin's channel, on one line.
pub fn checked_key(key: &str) -> anyhow::Result<&str> {
    anyhow::ensure!(
        !key.trim().is_empty()
            && key.chars().count() <= PLUGIN_DEDUPE_KEY_MAX_CHARS
            && !key.chars().any(char::is_control),
        "plugin_session.dedupe_key_invalid: 去重键要是一行不空的文字，最多 \
         {PLUGIN_DEDUPE_KEY_MAX_CHARS} 个字符"
    );
    Ok(key)
}

/// A notice a plugin shows: one line, not blank and short; returned without its edge spaces.
pub fn checked_notice(text: &str) -> anyhow::Result<String> {
    let text = text.trim();
    anyhow::ensure!(
        !text.is_empty() && one_line(text, PLUGIN_NOTICE_MAX_CHARS),
        "plugin_session.notice_invalid: 提示要是一行不空的文字，最多 {PLUGIN_NOTICE_MAX_CHARS} 个字符"
    );
    Ok(text.to_owned())
}

/// A status entry: one short line; none, or a blank one, clears the entry.
pub fn checked_status(text: Option<&str>) -> anyhow::Result<Option<String>> {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else { return Ok(None) };
    anyhow::ensure!(
        one_line(text, PLUGIN_STATUS_MAX_CHARS),
        "plugin_session.status_invalid: 状态栏条目要是一行文字，最多 {PLUGIN_STATUS_MAX_CHARS} 个字符"
    );
    Ok(Some(text.to_owned()))
}

fn one_line(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}

/// A message a plugin sends: not blank, and no longer than a plugin may send at once.
pub fn checked_text(text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !text.trim().is_empty() && text.chars().count() <= PLUGIN_MESSAGE_MAX_CHARS,
        "plugin_session.message_invalid: 消息不能为空，最多 {PLUGIN_MESSAGE_MAX_CHARS} 个字符"
    );
    Ok(())
}
