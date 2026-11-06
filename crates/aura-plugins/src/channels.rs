//! The chat platform accounts a plugin's background service connects, one channel each. Aura
//! treats every channel the same way; what is particular to a platform lives in the plugin's own
//! adapter.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_CHANNELS: usize = 10;
const MAX_CHANNEL_ID: usize = 40;

/// The kind of chat platform a channel is on. Only shown to the user and given to the service;
/// Aura's rules are the same for all.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PluginPlatform {
    Qq,
    Feishu,
    Wechat,
    Wecom,
    Telegram,
    Dingtalk,
    Slack,
    Discord,
    /// The OneBot v11 protocol, which many QQ frameworks speak.
    Onebot,
    /// The Satori protocol, one interface to several platforms.
    Satori,
    Other,
}

impl PluginPlatform {
    pub const ALL: [Self; 11] = [
        Self::Qq,
        Self::Feishu,
        Self::Wechat,
        Self::Wecom,
        Self::Telegram,
        Self::Dingtalk,
        Self::Slack,
        Self::Discord,
        Self::Onebot,
        Self::Satori,
        Self::Other,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Qq => "qq",
            Self::Feishu => "feishu",
            Self::Wechat => "wechat",
            Self::Wecom => "wecom",
            Self::Telegram => "telegram",
            Self::Dingtalk => "dingtalk",
            Self::Slack => "slack",
            Self::Discord => "discord",
            Self::Onebot => "onebot",
            Self::Satori => "satori",
            Self::Other => "other",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|platform| platform.name() == name)
    }
}

/// One chat platform account (a bot) the plugin's background service connects.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginChannel {
    /// Lowercase letters, digits, `-` and `_`; the name the service and the allowlist use.
    pub id: String,
    pub platform: PluginPlatform,
    pub title: String,
}

/// Lowercase letters, digits, `-` and `_`, at most 40 characters.
#[must_use]
pub fn valid_channel_id(id: &str) -> bool {
    (1..=MAX_CHANNEL_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChannelFile {
    id: String,
    platform: String,
    title: String,
}

/// Checks the declared channels; one that cannot be read is left out, with its reason.
pub(crate) fn check_channels(
    files: Vec<ChannelFile>,
    has_service: bool,
    problems: &mut Vec<String>,
) -> Vec<PluginChannel> {
    if !files.is_empty() && !has_service {
        problems.push("写了 channels 就要有 [service]：渠道由后台服务连上".into());
    }
    if files.len() > MAX_CHANNELS {
        problems.push(format!("channels 最多 {MAX_CHANNELS} 个"));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut channels = Vec::new();
    for file in files {
        let id = file.id.trim().to_owned();
        if !valid_channel_id(&id) {
            problems.push(format!(
                "channels 的 id（{}）只能用小写字母、数字、- 和 _，最多 {MAX_CHANNEL_ID} 个字符",
                crate::manifest::shown(&id)
            ));
            continue;
        }
        if !ids.insert(id.clone()) {
            problems.push(format!("channels 里的 {id} 出现了两次"));
            continue;
        }
        let title = crate::declarations::title(
            &format!("channels 里 {id} 的 title"),
            &file.title,
            problems,
        );
        match PluginPlatform::parse(file.platform.trim()) {
            Some(platform) => channels.push(PluginChannel { id, platform, title }),
            None => problems.push(format!(
                "channels 里 {id} 的 platform（{}）不认识；能用的有：{}",
                crate::manifest::shown(&file.platform),
                PluginPlatform::ALL.map(PluginPlatform::name).join("、")
            )),
        }
    }
    channels
}
