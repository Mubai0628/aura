//! The MCP subset Aura speaks: initialize, tools/list, tools/call, cancellation and ping.
//! Aura declares no client capability (no roots, sampling or elicitation), so a server has
//! nothing to ask of it beyond ping.
use serde_json::{Value, json};

/// Versions Aura accepts from a server. The request names the newest one Aura was built and
/// tested against; a server answering with another listed version is accepted.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"];
pub const REQUESTED_PROTOCOL_VERSION: &str = "2025-11-25";
const MAX_INSTRUCTIONS_CHARS: usize = 8_000;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProtocolError {
    #[error("服务器使用的协议版本 {0} 不受支持")]
    Version(String),
    #[error("服务器的回应缺少 {0}")]
    Missing(&'static str),
    #[error("服务器的回应格式不对：{0}")]
    Shape(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub server_info: ServerInfo,
    pub tools_supported: bool,
    pub tools_list_changed: bool,
    pub instructions: Option<String>,
}

#[must_use]
pub fn initialize_params(client_version: &str) -> Value {
    json!({
        "protocolVersion": REQUESTED_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": "Aura", "version": client_version},
    })
}

fn text(value: &Value, key: &str, limit: usize) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(|s| s.chars().take(limit).collect())
}

pub fn parse_initialize_result(value: &Value) -> Result<InitializeResult, ProtocolError> {
    let version = value
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or(ProtocolError::Missing("protocolVersion"))?;
    if !PROTOCOL_VERSIONS.contains(&version) {
        return Err(ProtocolError::Version(version.chars().take(64).collect()));
    }
    let info = value.get("serverInfo").cloned().unwrap_or(Value::Null);
    let tools = value.get("capabilities").and_then(|c| c.get("tools"));
    Ok(InitializeResult {
        protocol_version: version.to_owned(),
        server_info: ServerInfo {
            name: text(&info, "name", 128).unwrap_or_default(),
            version: text(&info, "version", 64).unwrap_or_default(),
            title: text(&info, "title", 128),
        },
        tools_supported: tools.is_some(),
        tools_list_changed: tools
            .and_then(|t| t.get("listChanged"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        instructions: text(value, "instructions", MAX_INSTRUCTIONS_CHARS),
    })
}

/// One tool exactly as the server described it.
#[derive(Clone, Debug, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolsPage {
    pub tools: Vec<McpTool>,
    pub next_cursor: Option<String>,
}

pub fn parse_tools_page(value: &Value) -> Result<ToolsPage, ProtocolError> {
    let tools =
        value.get("tools").and_then(Value::as_array).ok_or(ProtocolError::Missing("tools"))?;
    let mut parsed = Vec::with_capacity(tools.len());
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .ok_or(ProtocolError::Shape("一个工具没有 name".into()))?;
        parsed.push(McpTool {
            name: name.to_owned(),
            title: text(tool, "title", 256),
            description: tool.get("description").and_then(Value::as_str).map(str::to_owned),
            input_schema: tool.get("inputSchema").cloned().unwrap_or(Value::Null),
        });
    }
    Ok(ToolsPage {
        tools: parsed,
        next_cursor: value
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned),
    })
}

/// One block of a tool result. Binary data is not kept: only what it was and how big.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentBlock {
    Text(String),
    Image { mime_type: String, bytes: usize },
    Audio { mime_type: String, bytes: usize },
    ResourceLink { uri: String, name: Option<String> },
    Resource { uri: String, text: Option<String> },
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    pub structured_content: Option<Value>,
    pub is_error: bool,
}

fn decoded_len(data: Option<&str>) -> usize {
    data.map_or(0, |data| data.len() / 4 * 3)
}

pub fn parse_call_result(value: &Value) -> Result<CallToolResult, ProtocolError> {
    let content = match value.get("content") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .map(|block| {
                let kind = block.get("type").and_then(Value::as_str).unwrap_or_default();
                let mime = || text(block, "mimeType", 128).unwrap_or_default();
                match kind {
                    "text" => ContentBlock::Text(
                        block.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    ),
                    "image" => ContentBlock::Image {
                        mime_type: mime(),
                        bytes: decoded_len(block.get("data").and_then(Value::as_str)),
                    },
                    "audio" => ContentBlock::Audio {
                        mime_type: mime(),
                        bytes: decoded_len(block.get("data").and_then(Value::as_str)),
                    },
                    "resource_link" => ContentBlock::ResourceLink {
                        uri: text(block, "uri", 2048).unwrap_or_default(),
                        name: text(block, "name", 256),
                    },
                    "resource" => {
                        let resource = block.get("resource").cloned().unwrap_or(Value::Null);
                        ContentBlock::Resource {
                            uri: text(&resource, "uri", 2048).unwrap_or_default(),
                            text: resource.get("text").and_then(Value::as_str).map(str::to_owned),
                        }
                    }
                    other => ContentBlock::Other(other.chars().take(64).collect()),
                }
            })
            .collect(),
        Some(_) => return Err(ProtocolError::Shape("content 不是数组".into())),
    };
    Ok(CallToolResult {
        content,
        structured_content: value.get("structuredContent").cloned().filter(|v| !v.is_null()),
        is_error: value.get("isError").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// What the model reads of a result, at most `limit` bytes. Text is kept; anything binary is
/// described, never decoded or forwarded.
#[must_use]
pub fn model_text(result: &CallToolResult, limit: usize) -> (String, bool) {
    let mut parts = Vec::new();
    for block in &result.content {
        parts.push(match block {
            ContentBlock::Text(text) => text.clone(),
            ContentBlock::Image { mime_type, bytes } => {
                format!("[图片 {mime_type}，约 {bytes} 字节，未发给模型]")
            }
            ContentBlock::Audio { mime_type, bytes } => {
                format!("[音频 {mime_type}，约 {bytes} 字节，未发给模型]")
            }
            ContentBlock::ResourceLink { uri, name } => match name {
                Some(name) => format!("[资源链接 {name}：{uri}]"),
                None => format!("[资源链接 {uri}]"),
            },
            ContentBlock::Resource { uri, text: Some(text) } => format!("[资源 {uri}]\n{text}"),
            ContentBlock::Resource { uri, text: None } => {
                format!("[资源 {uri}（二进制，未发给模型）]")
            }
            ContentBlock::Other(kind) => format!("[不支持的内容类型 {kind}]"),
        });
    }
    if parts.is_empty()
        && let Some(structured) = &result.structured_content
    {
        parts.push(structured.to_string());
    }
    let joined = parts.join("\n");
    if joined.len() <= limit {
        return (joined, false);
    }
    let mut end = limit;
    while !joined.is_char_boundary(end) {
        end -= 1;
    }
    (joined[..end].to_owned(), true)
}

/// `notifications/cancelled` for one request Aura sent.
#[must_use]
pub fn cancelled_params(request_id: &crate::RequestId, reason: &str) -> Value {
    json!({"requestId": request_id, "reason": reason})
}
