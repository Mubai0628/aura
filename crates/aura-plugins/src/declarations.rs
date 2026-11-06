//! What a version 2 manifest declares besides the plugin's identity: what the plugin may do with
//! the conversations it is given, its panel, its background service and its own settings. All
//! of it is shown when the plugin is reviewed, and trusting the plugin trusts exactly this.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_TITLE_CHARS: usize = 40;
const MAX_NETWORK: usize = 20;
const MAX_ADDRESS_CHARS: usize = 200;
const MAX_ARGUMENTS: usize = 64;
const MAX_OPTIONS: usize = 20;
const MAX_OPTION_KEY: usize = 40;
const MAX_OPTION_DESCRIPTION: usize = 200;
pub const DEFAULT_PANEL_ENTRY: &str = "panel/index.html";
/// The folder a panel's page and everything it loads come from.
pub const PANEL_FOLDER: &str = "panel";

/// What a plugin's panel and background service may do with the conversations they are given.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
pub enum PluginCapability {
    /// The conversation's title, model, usage, to-do list and context use.
    #[serde(rename = "session.read")]
    SessionRead,
    /// Turns starting and ending, the final reply, approvals waiting, errors and compaction.
    #[serde(rename = "session.events")]
    SessionEvents,
    /// Send a message in the conversation, marked as the plugin's.
    #[serde(rename = "session.send")]
    SessionSend,
    /// Stop the current turn, start a conversation, change the permission mode.
    #[serde(rename = "session.control")]
    SessionControl,
    #[serde(rename = "notify")]
    Notify,
    /// One short item in the status line.
    #[serde(rename = "status")]
    Status,
    /// Run the plugin's own commands.
    #[serde(rename = "commands")]
    Commands,
    /// Read and write the plugin's own settings.
    #[serde(rename = "settings")]
    Settings,
}

impl PluginCapability {
    pub const ALL: [Self; 8] = [
        Self::SessionRead,
        Self::SessionEvents,
        Self::SessionSend,
        Self::SessionControl,
        Self::Notify,
        Self::Status,
        Self::Commands,
        Self::Settings,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SessionRead => "session.read",
            Self::SessionEvents => "session.events",
            Self::SessionSend => "session.send",
            Self::SessionControl => "session.control",
            Self::Notify => "notify",
            Self::Status => "status",
            Self::Commands => "commands",
            Self::Settings => "settings",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|capability| capability.name() == name)
    }
}

/// A page in the right sidebar drawn by the plugin's own files.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPanel {
    pub title: String,
    /// Relative to the plugin folder, inside `panel/`.
    pub entry: String,
    /// The origins (`https://host[:port]`, `wss://…`) the page may connect to, after the user
    /// agreed; none by default.
    pub network: Vec<String>,
}

/// A program Aura keeps running while the plugin is on, talking to it on its own standard input
/// and output.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginService {
    /// The program, with the plugin folder filled in, or a program name found on `PATH`.
    pub command: String,
    pub arguments: Vec<String>,
    /// Where it says it connects. Shown to the user; it runs as the user, so Aura cannot hold
    /// it to these.
    pub network: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginOptionKind {
    #[default]
    Text,
    /// Kept in Windows Credential Manager and given only to the plugin's background service.
    Secret,
}

/// One of the plugin's own settings, which the user fills in on the plugin page.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginOption {
    /// Lowercase letters, digits and `_`, starting with a letter.
    pub key: String,
    pub title: String,
    pub kind: PluginOptionKind,
    pub required: bool,
    pub description: Option<String>,
}

/// Everything a version 2 manifest declares besides the plugin's identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PluginDeclarations {
    pub capabilities: Vec<PluginCapability>,
    pub panel: Option<PluginPanel>,
    pub service: Option<PluginService>,
    pub options: Vec<PluginOption>,
    /// The chat platform accounts the service connects; never without a service.
    pub channels: Vec<crate::PluginChannel>,
}

impl PluginDeclarations {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanelFile {
    title: String,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    network: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceFile {
    command: String,
    #[serde(default)]
    arguments: Vec<String>,
    #[serde(default)]
    network: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OptionFile {
    key: String,
    title: String,
    #[serde(default)]
    kind: PluginOptionKind,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    description: Option<String>,
}

/// The new parts of a manifest as written, before they are checked.
#[derive(Default)]
pub(crate) struct DeclaredFiles {
    pub capabilities: Option<Vec<String>>,
    pub panel: Option<PanelFile>,
    pub service: Option<ServiceFile>,
    pub settings: Option<Vec<OptionFile>>,
    pub channels: Option<Vec<crate::channels::ChannelFile>>,
}

impl DeclaredFiles {
    pub fn any(&self) -> bool {
        self.capabilities.is_some()
            || self.panel.is_some()
            || self.service.is_some()
            || self.settings.is_some()
            || self.channels.is_some()
    }
}

/// Checks what the manifest declares; `fill` puts the plugin folder in place of
/// `${PLUGIN_ROOT}`.
pub(crate) fn check(
    files: DeclaredFiles,
    fill: &dyn Fn(&str) -> String,
    problems: &mut Vec<String>,
) -> PluginDeclarations {
    let mut capabilities = Vec::new();
    for name in files.capabilities.unwrap_or_default() {
        match PluginCapability::parse(name.trim()) {
            Some(capability) if !capabilities.contains(&capability) => {
                capabilities.push(capability)
            }
            Some(_) => {}
            None => problems.push(format!(
                "capabilities 里的 {} 不认识；能用的有：{}",
                crate::manifest::shown(&name),
                PluginCapability::ALL.map(PluginCapability::name).join("、")
            )),
        }
    }
    capabilities.sort();
    let service = files.service.map(|service| check_service(service, fill, problems));
    let channels = files.channels.unwrap_or_default();
    PluginDeclarations {
        capabilities,
        panel: files.panel.map(|panel| check_panel(panel, problems)),
        channels: crate::channels::check_channels(channels, service.is_some(), problems),
        service,
        options: check_options(files.settings.unwrap_or_default(), problems),
    }
}

pub(crate) fn title(field: &str, text: &str, problems: &mut Vec<String>) -> String {
    let text = text.trim().to_owned();
    if text.is_empty() {
        problems.push(format!("{field} 不能为空"));
    } else if text.chars().any(char::is_control) {
        problems.push(format!("{field} 必须是一行文字"));
    } else if text.chars().count() > MAX_TITLE_CHARS {
        problems.push(format!("{field} 不能超过 {MAX_TITLE_CHARS} 个字符"));
    }
    text
}

fn check_panel(panel: PanelFile, problems: &mut Vec<String>) -> PluginPanel {
    let entry = panel.entry.unwrap_or_else(|| DEFAULT_PANEL_ENTRY.to_owned());
    let inside = entry.strip_prefix("panel/").is_some_and(|rest| {
        !rest.is_empty()
            && rest.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
    });
    if !inside || entry.contains('\\') || !entry.to_ascii_lowercase().ends_with(".html") {
        problems.push(format!(
            "panel.entry（{}）必须是 panel/ 文件夹里的 .html 文件，用 / 分隔",
            crate::manifest::shown(&entry)
        ));
    }
    if panel.network.len() > MAX_NETWORK {
        problems.push(format!("panel.network 最多 {MAX_NETWORK} 个地址"));
    }
    for origin in &panel.network {
        if !origin_ok(origin) {
            problems.push(format!(
                "panel.network 里的 {} 必须是 https:// 或 wss:// 加主机名（可带端口），不带路径和 *",
                crate::manifest::shown(origin)
            ));
        }
    }
    PluginPanel {
        title: title("panel.title", &panel.title, problems),
        entry,
        network: panel.network,
    }
}

/// `https://host[:port]` or `wss://host[:port]`, with nothing after but an optional `/`.
fn origin_ok(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("https://").or_else(|| origin.strip_prefix("wss://"))
    else {
        return false;
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = rest.split_once(':').map_or((rest, None), |(h, p)| (h, Some(p)));
    !host.is_empty()
        && origin.len() <= MAX_ADDRESS_CHARS
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && port.is_none_or(|port| port.parse::<u16>().is_ok_and(|port| port > 0))
}

fn check_service(
    service: ServiceFile,
    fill: &dyn Fn(&str) -> String,
    problems: &mut Vec<String>,
) -> PluginService {
    let command = fill(service.command.trim());
    if command.is_empty() || command.chars().any(char::is_control) {
        problems.push("service.command 必须是一行、不为空的程序".into());
    }
    if command.contains("${") || service.arguments.iter().any(|a| fill(a).contains("${")) {
        problems.push("service 里只有 ${PLUGIN_ROOT} 会展开；Aura 不把环境变量的值写进配置".into());
    }
    if service.arguments.len() > MAX_ARGUMENTS {
        problems.push(format!("service.arguments 最多 {MAX_ARGUMENTS} 个"));
    }
    if service.network.len() > MAX_NETWORK {
        problems.push(format!("service.network 最多 {MAX_NETWORK} 个地址"));
    }
    for address in &service.network {
        if address.trim().is_empty()
            || address.chars().count() > MAX_ADDRESS_CHARS
            || address.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            problems.push(format!(
                "service.network 里的 {} 必须是一个不带空格的主机名或网址",
                crate::manifest::shown(address)
            ));
        }
    }
    PluginService {
        command,
        arguments: service.arguments.iter().map(|argument| fill(argument)).collect(),
        network: service.network,
    }
}

/// Lowercase letters, digits and `_`, starting with a letter, at most 40 characters.
pub(crate) fn option_key_ok(key: &str) -> bool {
    (1..=MAX_OPTION_KEY).contains(&key.len())
        && key.starts_with(|c: char| c.is_ascii_lowercase())
        && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn check_options(files: Vec<OptionFile>, problems: &mut Vec<String>) -> Vec<PluginOption> {
    if files.len() > MAX_OPTIONS {
        problems.push(format!("settings 最多 {MAX_OPTIONS} 项"));
    }
    let mut keys = std::collections::BTreeSet::new();
    files
        .into_iter()
        .map(|file| {
            let key = file.key.trim().to_owned();
            if !option_key_ok(&key) {
                problems.push(format!(
                    "settings.key（{}）只能用小写字母、数字和 _，以字母开头，最多 {MAX_OPTION_KEY} 个字符",
                    crate::manifest::shown(&key)
                ));
            } else if !keys.insert(key.clone()) {
                problems.push(format!("settings 里的 {key} 出现了两次"));
            }
            let description = crate::manifest::blank(file.description);
            if description.as_ref().is_some_and(|text| {
                text.chars().count() > MAX_OPTION_DESCRIPTION || text.chars().any(char::is_control)
            }) {
                problems.push(format!(
                    "settings 里 {key} 的 description 必须是一行，不超过 {MAX_OPTION_DESCRIPTION} 个字符"
                ));
            }
            PluginOption {
                title: title(&format!("settings 里 {key} 的 title"), &file.title, problems),
                key,
                kind: file.kind,
                required: file.required,
                description,
            }
        })
        .collect()
}
