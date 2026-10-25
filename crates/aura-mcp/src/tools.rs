//! A server's tools as Aura tools: `mcp.<server-id>.<name>`, with bounded descriptions and
//! object input schemas. The server's own name is kept for the call; the Aura name is only
//! for the registry, so two server names that normalize alike get distinct Aura names.
use crate::McpTool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_TOOLS_PER_SERVER: usize = 128;
pub const MAX_TOOL_SCHEMA_BYTES: usize = 64 * 1024;
pub const MAX_TOOL_DESCRIPTION_CHARS: usize = 2048;
const MAX_WIRE_NAME_BYTES: usize = 128;
const SEGMENT_BYTES: usize = 56;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct NormalizedTool {
    pub canonical_name: String,
    /// The server's name for the tool, sent in `tools/call`.
    pub wire_name: String,
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
    /// What a frozen definition binds: name, description and schema as the server gave them.
    pub fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ToolCatalog {
    pub server_id: String,
    pub tools: Vec<NormalizedTool>,
    /// Tools that were left out, with the reason.
    pub skipped: Vec<String>,
    pub fingerprint: String,
}

fn segment(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !out.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        out.insert(0, 't');
    }
    out.truncate(SEGMENT_BYTES);
    out
}

fn description(tool: &McpTool) -> String {
    tool.description
        .as_deref()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(MAX_TOOL_DESCRIPTION_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The schema in Aura's dialect, within the size limit.
fn usable_schema(schema: &Value) -> Result<Value, String> {
    if !schema.is_object() {
        return Err("inputSchema 不是对象".into());
    }
    let converted = crate::schema::input_schema(schema)?;
    let size = converted.to_string().len();
    if size > MAX_TOOL_SCHEMA_BYTES {
        return Err(format!("inputSchema 有 {size} 字节，超过 {MAX_TOOL_SCHEMA_BYTES}"));
    }
    Ok(converted)
}

fn hash(value: &Value) -> String {
    blake3::hash(value.to_string().as_bytes()).to_hex().to_string()
}

/// The server's tools in its order. Tools beyond the limit, without a usable schema or with
/// an unusable name are skipped with the reason; the rest stay usable.
#[must_use]
pub fn normalize_tools(server_id: &str, tools: Vec<McpTool>) -> ToolCatalog {
    let mut used = std::collections::BTreeSet::new();
    let mut wire_names = std::collections::BTreeSet::new();
    let mut normalized = Vec::new();
    let mut skipped = Vec::new();
    for tool in tools {
        if normalized.len() >= MAX_TOOLS_PER_SERVER {
            skipped
                .push(format!("{}：超过每个服务器 {MAX_TOOLS_PER_SERVER} 个工具的上限", tool.name));
            continue;
        }
        if tool.name.is_empty()
            || tool.name.len() > MAX_WIRE_NAME_BYTES
            || tool.name.chars().any(char::is_control)
        {
            skipped.push(format!("{}：名字为空、过长或含控制字符", tool.name.escape_debug()));
            continue;
        }
        if !wire_names.insert(tool.name.clone()) {
            skipped.push(format!("{}：服务器列出了两个同名工具", tool.name));
            continue;
        }
        let input_schema = match usable_schema(&tool.input_schema) {
            Ok(schema) => schema,
            Err(problem) => {
                skipped.push(format!("{}：{problem}", tool.name));
                continue;
            }
        };
        let base = segment(&tool.name);
        let mut name = base.clone();
        let mut counter = 2;
        while !used.insert(name.clone()) {
            name = format!("{base}_{counter}");
            counter += 1;
        }
        let description = description(&tool);
        let fingerprint = hash(&json!({
            "wire_name": tool.name,
            "title": tool.title,
            "description": description,
            "input_schema": input_schema,
        }));
        normalized.push(NormalizedTool {
            canonical_name: format!("mcp.{server_id}.{name}"),
            wire_name: tool.name,
            title: tool.title,
            description,
            input_schema,
            fingerprint,
        });
    }
    let fingerprint = hash(&json!({
        "server_id": server_id,
        "tools": normalized.iter().map(|tool| (&tool.canonical_name, &tool.fingerprint)).collect::<Vec<_>>(),
    }));
    ToolCatalog { server_id: server_id.to_owned(), tools: normalized, skipped, fingerprint }
}
