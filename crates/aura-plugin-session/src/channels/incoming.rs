//! `message.in`: what a channel received, checked field by field before anything reaches Aura.
//! Ids are kept exactly as the platform wrote them; only the text may span lines.
use aura_agent_domain::{
    PLUGIN_DEDUPE_KEY_MAX_CHARS, PLUGIN_MESSAGE_MAX_CHARS, PluginChat, PluginChatKind,
};
use aura_plugin_host::{ServiceError, shown};
use aura_plugins::{
    PLUGIN_EXTERNAL_ID_MAX_CHARS, PLUGIN_EXTERNAL_NAME_MAX_CHARS, PluginChannel, PluginPlatform,
};
use serde::Deserialize;
use serde_json::Value;

/// A message as the service passed it in, once every field was checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    pub chat: PluginChat,
    pub platform: PluginPlatform,
    pub user_id: String,
    pub user_name: String,
    pub message_id: String,
    pub text: String,
    pub reply_to: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatParams {
    kind: PluginChatKind,
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserParams {
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageParams {
    channel: String,
    chat: ChatParams,
    user: UserParams,
    message_id: String,
    text: String,
    #[serde(default)]
    reply_to: Option<String>,
}

/// One line that is not blank, at most `max` characters.
fn id_ok(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

/// Checks the message against the channels the plugin's trusted manifest declares.
pub fn incoming(params: Value, declared: &[PluginChannel]) -> Result<Incoming, ServiceError> {
    let params: MessageParams = serde_json::from_value(params)
        .map_err(|error| ServiceError::new("protocol.bad_params", format!("参数不对：{error}")))?;
    let channel =
        declared.iter().find(|channel| channel.id == params.channel).ok_or_else(|| {
            ServiceError::new(
                "channel.unknown",
                format!("插件的清单里没有渠道 {}", shown(&params.channel)),
            )
        })?;
    let id_max = PLUGIN_EXTERNAL_ID_MAX_CHARS;
    if !id_ok(&params.chat.id, id_max) {
        return Err(ServiceError::new(
            "channel.chat_invalid",
            format!("chat.id 要是一行不空的文字，最多 {id_max} 个字符"),
        ));
    }
    let name = &params.user.name;
    if !id_ok(&params.user.id, id_max)
        || name.chars().count() > PLUGIN_EXTERNAL_NAME_MAX_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(ServiceError::new(
            "channel.user_invalid",
            format!(
                "user.id 要是一行不空的文字，最多 {id_max} 个字符；user.name 是一行，最多 \
                 {PLUGIN_EXTERNAL_NAME_MAX_CHARS} 个字符"
            ),
        ));
    }
    let key_max = PLUGIN_DEDUPE_KEY_MAX_CHARS;
    if !id_ok(&params.message_id, key_max) {
        return Err(ServiceError::new(
            "channel.message_id_invalid",
            format!("message_id 要是一行不空的文字，最多 {key_max} 个字符"),
        ));
    }
    if params.text.trim().is_empty() || params.text.chars().count() > PLUGIN_MESSAGE_MAX_CHARS {
        return Err(ServiceError::new(
            "channel.text_invalid",
            format!("text 不能为空，最多 {PLUGIN_MESSAGE_MAX_CHARS} 个字符"),
        ));
    }
    if params.reply_to.as_deref().is_some_and(|reply_to| !id_ok(reply_to, key_max)) {
        return Err(ServiceError::new(
            "channel.reply_to_invalid",
            format!("reply_to 要是一行不空的文字，最多 {key_max} 个字符"),
        ));
    }
    Ok(Incoming {
        chat: PluginChat { channel: params.channel, kind: params.chat.kind, id: params.chat.id },
        platform: channel.platform,
        user_id: params.user.id,
        user_name: params.user.name,
        message_id: params.message_id,
        text: params.text,
        reply_to: params.reply_to,
    })
}
