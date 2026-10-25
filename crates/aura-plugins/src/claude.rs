//! A Claude Code plugin's `.claude-plugin/plugin.json`: its name and what to show, where its
//! commands and MCP servers are, and which of its parts Aura does not use (each named with the
//! reason, never dropped silently).
use crate::files::Walked;
use crate::manifest::{CLAUDE_MANIFEST, MAX_MANIFEST_BYTES, PluginManifest, blank, check, tidy};
use crate::servers::CLAUDE_SERVERS_FILE;
use crate::views::PluginLimits;
use serde_json::Value;

pub(crate) struct ClaudePlugin {
    pub manifest: PluginManifest,
    /// Folders (or `.md` files) holding commands, relative to the root.
    pub command_sources: Vec<String>,
    pub servers: ClaudeServers,
    pub unsupported: Vec<String>,
}

pub(crate) enum ClaudeServers {
    None,
    /// `mcpServers` written in `plugin.json`.
    Inline(Value),
    /// A JSON file holding them, relative to the root.
    File(String),
}

/// A path the manifest names: relative, inside the plugin, `/`-separated.
fn relative(path: &str) -> Option<String> {
    let normalized = path.trim().replace('\\', "/");
    let trimmed = normalized.trim_start_matches("./").trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    (!parts.is_empty()
        && !normalized.starts_with('/')
        && !normalized.contains(':')
        && parts.iter().all(|p| *p != ".." && !p.chars().any(char::is_control)))
    .then(|| parts.join("/"))
}

fn text(value: Option<&Value>) -> Option<String> {
    blank(value.and_then(Value::as_str).map(str::to_owned))
}

fn paths(value: &Value, field: &str, problems: &mut Vec<String>) -> Vec<String> {
    let named: Vec<&str> = match value {
        Value::String(path) => vec![path.as_str()],
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => {
            problems.push(format!("{field} 必须是路径或路径的列表"));
            return Vec::new();
        }
    };
    named
        .into_iter()
        .filter_map(|path| {
            let found = relative(path);
            if found.is_none() {
                problems.push(format!("{field} 里的 {path} 不是插件内的相对路径"));
            }
            found
        })
        .collect()
}

pub(crate) fn parse_plugin_json(
    text_in: &str,
    walked: &Walked,
) -> Result<ClaudePlugin, Vec<String>> {
    if text_in.len() > MAX_MANIFEST_BYTES {
        return Err(vec![format!("{CLAUDE_MANIFEST} 超过 {MAX_MANIFEST_BYTES} 字节")]);
    }
    let value: Value = serde_json::from_str(text_in)
        .map_err(|error| vec![format!("{CLAUDE_MANIFEST} 无法解析：{error}")])?;
    let Value::Object(fields) = &value else {
        return Err(vec![format!("{CLAUDE_MANIFEST} 必须是一个对象")]);
    };
    let mut problems = Vec::new();
    let author = match fields.get("author") {
        Some(Value::Object(author)) => {
            text(author.get("name")).or_else(|| text(author.get("email")))
        }
        other => text(other),
    };
    let manifest = tidy(PluginManifest {
        name: text(fields.get("name")).unwrap_or_default(),
        version: text(fields.get("version")).unwrap_or_else(|| "未标明".to_owned()),
        description: text(fields.get("description")).unwrap_or_default(),
        author,
        homepage: text(fields.get("homepage")),
        limits: PluginLimits::default(),
    });
    problems.extend(check(&manifest));
    let mut command_sources = vec!["commands".to_owned()];
    if let Some(extra) = fields.get("commands") {
        command_sources.extend(paths(extra, "commands", &mut problems));
    }
    let servers = match fields.get("mcpServers") {
        Some(Value::String(path)) => match relative(path) {
            Some(path) => ClaudeServers::File(path),
            None => {
                problems.push(format!("mcpServers 的 {path} 不是插件内的相对路径"));
                ClaudeServers::None
            }
        },
        Some(inline @ Value::Object(_)) => ClaudeServers::Inline(inline.clone()),
        Some(_) => {
            problems.push("mcpServers 必须是路径或对象".into());
            ClaudeServers::None
        }
        None if walked.has(CLAUDE_SERVERS_FILE) => ClaudeServers::File(CLAUDE_SERVERS_FILE.into()),
        None => ClaudeServers::None,
    };
    let mut unsupported = Vec::new();
    if fields.contains_key("hooks") || walked.has("hooks/hooks.json") {
        unsupported.push("Hook（hooks）：协议与 Aura 的 Hook 不同，没有使用".to_owned());
    }
    if fields.contains_key("agents") || walked.has_folder("agents") {
        unsupported.push("子 Agent（agents）：Aura 现在没有子 Agent".to_owned());
    }
    if fields.contains_key("outputStyles") || walked.has_folder("output-styles") {
        unsupported.push("输出样式（outputStyles）：Aura 不支持".to_owned());
    }
    if fields.contains_key("lspServers") || walked.has(".lsp.json") {
        unsupported.push("语言服务器（lspServers）：Aura 不支持".to_owned());
    }
    if problems.is_empty() {
        Ok(ClaudePlugin { manifest, command_sources, servers, unsupported })
    } else {
        Err(problems)
    }
}
