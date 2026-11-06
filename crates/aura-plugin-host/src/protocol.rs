//! The protocol between Aura and a plugin's background service, version 1: one JSON object per
//! line on the service's own standard input and output. Nothing in it belongs to a platform;
//! a chat platform is the plugin's own adapter on the far side.
use aura_agent_domain::PluginChatKind;
use aura_plugins::PluginChannel;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 1;
/// The longest line either side writes, without its line break.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
pub const MAX_REQUEST_ID_CHARS: usize = 64;
const MAX_METHOD_CHARS: usize = 64;
const MAX_LOG_CHARS: usize = 4_000;
/// The longest message a channel may report taking, in characters.
pub const MAX_CHANNEL_TEXT_CHARS: u32 = 100_000;

/// The service's own name for one of its requests: text or a whole number.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    Text(String),
}

/// What a channel can do, as the service reports once it is connected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelAbilities {
    pub id: String,
    /// The longest message the platform takes at once, in characters.
    pub max_text_chars: u32,
    #[serde(default)]
    pub markdown: bool,
    #[serde(default)]
    pub buttons: bool,
    #[serde(default)]
    pub command_menu: bool,
    #[serde(default)]
    pub groups: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    #[default]
    Info,
    Warning,
    Error,
}

impl LogLevel {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// What a service wrote on one line.
#[derive(Clone, Debug, PartialEq)]
pub enum FromService {
    Ready { protocol: u64, channels: Vec<ChannelAbilities> },
    Request { id: RequestId, method: String, params: Value },
    Log { level: LogLevel, text: String },
}

/// Why a line could not be read, as the service is told: a stable code and what was wrong.
/// `id` is set when the line was a request whose `id` could still be read, so the answer goes
/// to that request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineProblem {
    pub code: &'static str,
    pub message: String,
    pub id: Option<RequestId>,
}

impl LineProblem {
    fn new(code: &'static str, message: String) -> Self {
        Self { code, message, id: None }
    }
}

/// A failure answered for a request, or reported on its own.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceError {
    pub code: String,
    pub message: String,
}

impl ServiceError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_owned(), message: message.into() }
    }

    /// An error of Aura's own (`code: message`), split into its code and its message.
    #[must_use]
    pub fn from_reason(reason: &str) -> Self {
        match reason.split_once(": ") {
            Some((code, message))
                if !code.is_empty()
                    && code.bytes().all(|b| b.is_ascii_alphanumeric() || b"._".contains(&b)) =>
            {
                Self::new(code, message)
            }
            _ => Self::new("aura.failed", reason),
        }
    }
}

/// What Aura writes to a service.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToService {
    /// The first line, before anything else: who the service is and what the user set.
    Hello {
        protocol: u32,
        plugin: String,
        channels: Vec<PluginChannel>,
        /// The plugin's text settings; secret ones are in the environment.
        settings: BTreeMap<String, String>,
    },
    Response {
        id: RequestId,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<ServiceError>,
    },
    /// A problem with a line that cannot be answered as a request.
    Error { code: String, message: String },
    /// Aura stops the service: it should end within a few seconds.
    Shutdown,
    /// What Aura answers a message one of its channels passed in, to send back in that chat.
    Reply {
        channel: String,
        chat: ChatAddress,
        /// The platform's own id of the message this answers.
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        text: String,
        format: ReplyFormat,
        /// Which piece this is, from 1, when the reply was split to fit the channel.
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        parts: Option<u32>,
    },
    /// The whole command menu of a channel that reported `command_menu`, for the platform to
    /// show in place of the one before.
    Commands { channel: String, commands: Vec<MenuCommand> },
    /// An approval handed to a private chat for its person to decide, with the whole `text` to
    /// send where the platform shows no buttons.
    Approval { channel: String, chat: ChatAddress, approval: ApprovalOffer, text: String },
}

/// What an approval handed to a chat asks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ApprovalOffer {
    /// The number the chat names it by.
    pub id: String,
    /// What the call does, as the conversation names it.
    pub title: String,
    /// The call's explanation and what it acts on, cut short; may be empty.
    pub detail: String,
    /// What the chat may answer: `approve`, `approve_session` and `deny`.
    pub decisions: Vec<String>,
}

/// One command of a channel's menu.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MenuCommand {
    pub name: String,
    pub title: String,
    pub description: String,
    /// What the text after the command means; none for a command that takes no text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}

/// A chat on a channel's platform, as the service names it in `message.in`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChatAddress {
    pub kind: PluginChatKind,
    pub id: String,
}

/// How a reply's text is written: Markdown the channel may show as it can, or plain text.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyFormat {
    Markdown,
    Text,
}

impl ToService {
    #[must_use]
    pub fn answer(id: RequestId, outcome: Result<Value, ServiceError>) -> Self {
        match outcome {
            Ok(result) => Self::Response { id, result: Some(result), error: None },
            Err(error) => Self::Response { id, result: None, error: Some(error) },
        }
    }

    /// The line, with its line break.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut line =
            serde_json::to_vec(self).unwrap_or_else(|_| b"{\"type\":\"shutdown\"}".into());
        line.push(b'\n');
        line
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyLine {
    protocol: u64,
    #[serde(default)]
    channels: Vec<ChannelAbilities>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestLine {
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogLine {
    #[serde(default)]
    level: LogLevel,
    text: String,
}

fn request_id(value: &Value) -> Result<RequestId, String> {
    match value {
        Value::String(text)
            if !text.is_empty()
                && text.chars().count() <= MAX_REQUEST_ID_CHARS
                && !text.chars().any(char::is_control) =>
        {
            Ok(RequestId::Text(text.clone()))
        }
        Value::Number(number) => number
            .as_i64()
            .map(RequestId::Number)
            .ok_or_else(|| "id 要是整数，或不超过 64 个字符的一行文字".to_owned()),
        _ => Err(format!("id 要是整数，或不超过 {MAX_REQUEST_ID_CHARS} 个字符的一行文字")),
    }
}

fn fields<T: serde::de::DeserializeOwned>(
    kind: &str,
    rest: Map<String, Value>,
) -> Result<T, String> {
    serde_json::from_value(Value::Object(rest))
        .map_err(|error| format!("{kind} 的字段不对：{error}"))
}

/// Reads one line the service wrote (without its line break).
pub fn parse_line(line: &[u8]) -> Result<FromService, LineProblem> {
    let value: Value = serde_json::from_slice(line).map_err(|error| {
        LineProblem::new("protocol.not_json", format!("这一行不是 JSON：{error}"))
    })?;
    let Value::Object(mut object) = value else {
        return Err(LineProblem::new("protocol.not_json", "每一行要是一个 JSON 对象".into()));
    };
    let kind = match object.remove("type") {
        Some(Value::String(kind)) => kind,
        _ => return Err(LineProblem::new("protocol.bad_message", "缺少文字的 type".into())),
    };
    let bad = |message: String| LineProblem::new("protocol.bad_message", message);
    match kind.as_str() {
        "ready" => {
            let ready: ReadyLine = fields("ready", object).map_err(bad)?;
            Ok(FromService::Ready { protocol: ready.protocol, channels: ready.channels })
        }
        "request" => {
            let id =
                request_id(object.remove("id").as_ref().unwrap_or(&Value::Null)).map_err(bad)?;
            let request: RequestLine = fields("request", object).map_err(|message| {
                LineProblem { code: "protocol.bad_message", message, id: Some(id.clone()) }
            })?;
            let method_ok = !request.method.is_empty()
                && request.method.chars().count() <= MAX_METHOD_CHARS
                && request.method.bytes().all(|b| b.is_ascii_lowercase() || b"._".contains(&b));
            let params = request.params.unwrap_or_else(|| Value::Object(Map::new()));
            if !method_ok || !params.is_object() {
                return Err(LineProblem {
                    code: "protocol.bad_message",
                    message: "method 要是小写字母、. 和 _，params 要是一个 JSON 对象".into(),
                    id: Some(id),
                });
            }
            Ok(FromService::Request { id, method: request.method, params })
        }
        "log" => {
            let log: LogLine = fields("log", object).map_err(bad)?;
            Ok(FromService::Log {
                level: log.level,
                text: log.text.chars().take(MAX_LOG_CHARS).collect(),
            })
        }
        other => Err(LineProblem::new(
            "protocol.unknown_type",
            format!("不认识的 type：{}；能用的有 ready、request、log", shown(other)),
        )),
    }
}

/// A value from the service as it is shown back to it: on one line and short.
/// Text a service sent, as one short line for a message.
#[must_use]
pub fn shown(text: &str) -> String {
    let line: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut short: String = line.chars().take(40).collect();
    if line.chars().count() > 40 {
        short.push('…');
    }
    short
}
