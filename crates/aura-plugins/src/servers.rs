//! A plugin's MCP servers. They are declared without trust, switches or secret IDs (those
//! belong to the installation), with `${PLUGIN_ROOT}` (in a Claude Code plugin,
//! `${CLAUDE_PLUGIN_ROOT}`) standing for the plugin folder. Each runs as the MCP server
//! `<plugin>-<name>` (or `<plugin>` when the two are the same) and is checked by the same rules
//! as a server the user adds. Aura never writes the value of an environment variable into a
//! configuration: a value that refers to one becomes a secret the user enters in Aura.
use aura_domain_common::CredentialId;
use aura_mcp::{
    McpEnvironmentEntry, McpHeader, McpHttpAuth, McpSecretEnvironment, McpServerConfig,
    McpTransport, validate_server,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::path::Path;

pub const PLUGIN_ROOT_VARIABLE: &str = "${PLUGIN_ROOT}";
pub(crate) const CLAUDE_ROOT_VARIABLE: &str = "${CLAUDE_PLUGIN_ROOT}";
pub(crate) const SERVERS_FILE: &str = "mcp.json";
pub(crate) const CLAUDE_SERVERS_FILE: &str = ".mcp.json";
/// The key of the HTTP sign-in secret among a server's secrets (the others are named by their
/// environment variable).
pub const HTTP_SECRET_KEY: &str = "http";
const MAX_PLUGIN_SERVERS: usize = 8;
const DEFAULT_STARTUP_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CALL_TIMEOUT_MS: u64 = 120_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginAuth {
    /// A secret the user enters, sent as `Authorization: Bearer <secret>` or as the whole value of
    /// `header`.
    Secret {
        header: Option<String>,
    },
    OAuth {
        client_id: Option<String>,
        scopes: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PluginTransport {
    Stdio {
        command: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        environment: Vec<(String, String)>,
        /// Variables whose values the user enters in Aura.
        secret_environment: Vec<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        auth: Option<PluginAuth>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginServer {
    /// The name inside the plugin.
    pub name: String,
    /// The MCP server ID it runs as.
    pub id: String,
    pub display_name: String,
    /// With the plugin folder filled in.
    pub transport: PluginTransport,
    pub startup_timeout_ms: u64,
    pub call_timeout_ms: u64,
    /// Why it cannot run as declared.
    pub problems: Vec<String>,
}

impl PluginServer {
    /// The keys of the secrets the user enters for this server: environment variable names, and
    /// [`HTTP_SECRET_KEY`] for an HTTP sign-in secret.
    #[must_use]
    pub fn secret_keys(&self) -> Vec<String> {
        match &self.transport {
            PluginTransport::Stdio { secret_environment, .. } => secret_environment.clone(),
            PluginTransport::Http { auth: Some(PluginAuth::Secret { .. }), .. } => {
                vec![HTTP_SECRET_KEY.to_owned()]
            }
            PluginTransport::Http { .. } => Vec::new(),
        }
    }

    /// The MCP configuration it runs as; `secret` gives the installation's credential for each
    /// secret key. Trust and the switch are the installation's and are filled in by the caller.
    pub fn config(&self, secret: &dyn Fn(&str) -> CredentialId) -> McpServerConfig {
        let transport = match &self.transport {
            PluginTransport::Stdio {
                command,
                arguments,
                working_directory,
                environment,
                secret_environment,
            } => McpTransport::Stdio {
                command: command.clone(),
                arguments: arguments.clone(),
                working_directory: working_directory.clone(),
                environment: environment
                    .iter()
                    .map(|(name, value)| McpEnvironmentEntry {
                        name: name.clone(),
                        value: value.clone(),
                    })
                    .collect(),
                secret_environment: secret_environment
                    .iter()
                    .map(|name| McpSecretEnvironment {
                        name: name.clone(),
                        secret_id: secret(name),
                    })
                    .collect(),
            },
            PluginTransport::Http { url, headers, auth } => McpTransport::Http {
                url: url.clone(),
                headers: headers
                    .iter()
                    .map(|(name, value)| McpHeader { name: name.clone(), value: value.clone() })
                    .collect(),
                auth: auth.as_ref().map(|auth| match auth {
                    PluginAuth::Secret { header } => McpHttpAuth::Secret {
                        secret_id: secret(HTTP_SECRET_KEY),
                        header: header.clone(),
                    },
                    PluginAuth::OAuth { client_id, scopes } => {
                        McpHttpAuth::OAuth { client_id: client_id.clone(), scopes: scopes.clone() }
                    }
                }),
            },
        };
        McpServerConfig {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            transport,
            enabled: false,
            trusted_fingerprint: None,
            startup_timeout_ms: self.startup_timeout_ms,
            call_timeout_ms: self.call_timeout_ms,
            available_for_subagents: false,
        }
    }
}

/// The MCP server ID of `name` in `plugin`.
#[must_use]
pub fn server_id(plugin: &str, name: &str) -> String {
    let normalized: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if normalized == plugin || normalized.is_empty() {
        plugin.to_owned()
    } else {
        format!("{plugin}-{normalized}")
    }
}

/// Puts the plugin folder in place of `variable`; a value that starts with it gets Windows
/// separators.
fn fill(value: &str, variable: &str, root: &str) -> String {
    let filled = value.replace(variable, root);
    match filled.strip_prefix(root) {
        Some(rest) if value.starts_with(variable) => format!("{root}{}", rest.replace('/', "\\")),
        _ => filled,
    }
}

/// `${NAME}` or `${NAME:-default}`: the name of the variable the whole value refers to.
fn whole_reference(value: &str) -> Option<&str> {
    let inner = value.trim().strip_prefix("${")?.strip_suffix('}')?;
    let name = inner.split(":-").next().unwrap_or(inner);
    (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then_some(name)
}

fn reference_problem(field: &str, value: &str, problems: &mut Vec<String>) {
    if let Some(start) = value.find("${") {
        let reference: String = value[start..].chars().take_while(|c| *c != '}').collect();
        problems
            .push(format!("{field} 里的 {reference}}} 不会展开；Aura 不把环境变量的值写进配置"));
    }
}

/// Checks a declared server by the rules for one the user adds.
fn finish(mut server: PluginServer) -> PluginServer {
    let mut placeholders = std::collections::BTreeMap::new();
    for key in server.secret_keys() {
        placeholders.insert(key, CredentialId::new());
    }
    let config =
        server.config(&|key| placeholders.get(key).copied().unwrap_or_else(CredentialId::new));
    server.problems.extend(validate_server(&config));
    server
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuraServersFile {
    servers: Vec<AuraServer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuraServer {
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    transport: AuraTransport,
    #[serde(default)]
    startup_timeout_ms: Option<u64>,
    #[serde(default)]
    call_timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum AuraTransport {
    Stdio {
        command: String,
        #[serde(default)]
        arguments: Vec<String>,
        #[serde(default)]
        working_directory: Option<String>,
        #[serde(default)]
        environment: Vec<NameValue>,
        #[serde(default)]
        secret_environment: Vec<String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: Vec<NameValue>,
        #[serde(default)]
        auth: Option<AuraAuth>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameValue {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum AuraAuth {
    Secret {
        #[serde(default)]
        header: Option<String>,
    },
    #[serde(rename = "oauth")]
    OAuth {
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        scopes: Vec<String>,
    },
}

/// The servers in an Aura plugin's `mcp.json`.
pub(crate) fn aura_servers(
    text: &str,
    plugin: &str,
    root: &Path,
) -> Result<Vec<PluginServer>, String> {
    let file: AuraServersFile =
        serde_json::from_str(text).map_err(|error| format!("{SERVERS_FILE} 无法解析：{error}"))?;
    if file.servers.len() > MAX_PLUGIN_SERVERS {
        return Err(format!("{SERVERS_FILE} 最多 {MAX_PLUGIN_SERVERS} 个服务器"));
    }
    let root = root.display().to_string();
    let filled = |value: &str| fill(value, PLUGIN_ROOT_VARIABLE, &root);
    let servers = file.servers.into_iter().map(|declared| {
        let mut problems = Vec::new();
        let transport = match declared.transport {
            AuraTransport::Stdio {
                command,
                arguments,
                working_directory,
                environment,
                secret_environment,
            } => {
                let command = filled(&command);
                let arguments: Vec<String> = arguments.iter().map(|a| filled(a)).collect();
                let working_directory = working_directory.as_deref().map(filled);
                let environment: Vec<(String, String)> =
                    environment.into_iter().map(|e| (e.name, filled(&e.value))).collect();
                reference_problem("command", &command, &mut problems);
                for argument in &arguments {
                    reference_problem("arguments", argument, &mut problems);
                }
                for (name, value) in &environment {
                    reference_problem(&format!("环境变量 {name}"), value, &mut problems);
                }
                PluginTransport::Stdio {
                    command,
                    arguments,
                    working_directory,
                    environment,
                    secret_environment,
                }
            }
            AuraTransport::Http { url, headers, auth } => {
                reference_problem("url", &url, &mut problems);
                PluginTransport::Http {
                    url,
                    headers: headers.into_iter().map(|h| (h.name, h.value)).collect(),
                    auth: auth.map(|auth| match auth {
                        AuraAuth::Secret { header } => PluginAuth::Secret { header },
                        AuraAuth::OAuth { client_id, scopes } => {
                            PluginAuth::OAuth { client_id, scopes }
                        }
                    }),
                }
            }
        };
        finish(PluginServer {
            id: server_id(plugin, &declared.name),
            display_name: declared.display_name.unwrap_or_else(|| declared.name.clone()),
            name: declared.name,
            transport,
            startup_timeout_ms: declared.startup_timeout_ms.unwrap_or(DEFAULT_STARTUP_TIMEOUT_MS),
            call_timeout_ms: declared.call_timeout_ms.unwrap_or(DEFAULT_CALL_TIMEOUT_MS),
            problems,
        })
    });
    Ok(unique(servers.collect()))
}

/// The servers of a Claude Code plugin: the `mcpServers` object (or a file holding one).
/// Server kinds Aura cannot reach are named in `unsupported`.
pub(crate) fn claude_servers(
    value: &Value,
    plugin: &str,
    root: &Path,
    unsupported: &mut Vec<String>,
) -> Result<Vec<PluginServer>, String> {
    let map = value.get("mcpServers").unwrap_or(value);
    let Value::Object(map) = map else {
        return Err("mcpServers 必须是一个对象".into());
    };
    if map.len() > MAX_PLUGIN_SERVERS {
        return Err(format!("最多 {MAX_PLUGIN_SERVERS} 个 MCP 服务器"));
    }
    let root = root.display().to_string();
    let mut servers = Vec::new();
    for (name, entry) in map {
        let Value::Object(entry) = entry else {
            return Err(format!("MCP 服务器 {name} 的配置必须是一个对象"));
        };
        let kind = entry.get("type").and_then(Value::as_str);
        let server = match (kind, entry.contains_key("url")) {
            (Some("stdio"), _) | (None, false) => claude_stdio(name, entry, plugin, &root),
            (Some("http" | "streamable-http"), _) | (None, true) => {
                claude_http(name, entry, plugin)
            }
            (Some(other), _) => {
                unsupported.push(format!(
                    "MCP 服务器 {name}：{other} 方式 Aura 不支持（只支持本机程序与 Streamable HTTP）"
                ));
                continue;
            }
        };
        servers.push(finish(server));
    }
    Ok(unique(servers))
}

fn strings(entry: &Map<String, Value>, key: &str, problems: &mut Vec<String>) -> Vec<String> {
    match entry.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                let text = item.as_str().map(str::to_owned);
                if text.is_none() {
                    problems.push(format!("{key} 里只能是文字"));
                }
                text
            })
            .collect(),
        Some(_) => {
            problems.push(format!("{key} 必须是文字的列表"));
            Vec::new()
        }
    }
}

fn pairs(
    entry: &Map<String, Value>,
    key: &str,
    problems: &mut Vec<String>,
) -> Vec<(String, String)> {
    match entry.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(map)) => map
            .iter()
            .filter_map(|(name, value)| {
                let text = value.as_str().map(|text| (name.clone(), text.to_owned()));
                if text.is_none() {
                    problems.push(format!("{key} 的 {name} 必须是文字"));
                }
                text
            })
            .collect(),
        Some(_) => {
            problems.push(format!("{key} 必须是一个对象"));
            Vec::new()
        }
    }
}

fn claude_stdio(name: &str, entry: &Map<String, Value>, plugin: &str, root: &str) -> PluginServer {
    let mut problems = Vec::new();
    let filled = |value: &str| fill(value, CLAUDE_ROOT_VARIABLE, root);
    let command = entry.get("command").and_then(Value::as_str).map(filled).unwrap_or_default();
    let arguments: Vec<String> =
        strings(entry, "args", &mut problems).iter().map(|a| filled(a)).collect();
    let working_directory = entry.get("cwd").and_then(Value::as_str).map(filled);
    reference_problem("command", &command, &mut problems);
    for argument in &arguments {
        reference_problem("args", argument, &mut problems);
    }
    let (mut environment, mut secret_environment) = (Vec::new(), Vec::new());
    for (key, value) in pairs(entry, "env", &mut problems) {
        let value = filled(&value);
        if whole_reference(&value).is_some() {
            secret_environment.push(key);
        } else {
            reference_problem(&format!("环境变量 {key}"), &value, &mut problems);
            environment.push((key, value));
        }
    }
    PluginServer {
        id: server_id(plugin, name),
        display_name: name.to_owned(),
        name: name.to_owned(),
        transport: PluginTransport::Stdio {
            command,
            arguments,
            working_directory,
            environment,
            secret_environment,
        },
        startup_timeout_ms: DEFAULT_STARTUP_TIMEOUT_MS,
        call_timeout_ms: DEFAULT_CALL_TIMEOUT_MS,
        problems,
    }
}

fn claude_http(name: &str, entry: &Map<String, Value>, plugin: &str) -> PluginServer {
    let mut problems = Vec::new();
    let url = entry.get("url").and_then(Value::as_str).unwrap_or_default().to_owned();
    reference_problem("url", &url, &mut problems);
    let mut headers = Vec::new();
    let mut auth = None;
    for (header, value) in pairs(entry, "headers", &mut problems) {
        // `Authorization: Bearer ${TOKEN}` is Aura's default way to send a secret; any other
        // header whose whole value is a reference carries the secret as that value.
        let bearer = header.eq_ignore_ascii_case("authorization")
            && value
                .trim()
                .strip_prefix("Bearer ")
                .is_some_and(|rest| whole_reference(rest.trim()).is_some());
        if !bearer && whole_reference(&value).is_none() {
            reference_problem(&format!("请求头 {header}"), &value, &mut problems);
            headers.push((header, value));
        } else if auth.is_some() {
            problems.push("只能有一个带密钥的请求头".into());
        } else {
            auth = Some(PluginAuth::Secret { header: (!bearer).then_some(header) });
        }
    }
    PluginServer {
        id: server_id(plugin, name),
        display_name: name.to_owned(),
        name: name.to_owned(),
        transport: PluginTransport::Http { url, headers, auth },
        startup_timeout_ms: DEFAULT_STARTUP_TIMEOUT_MS,
        call_timeout_ms: DEFAULT_CALL_TIMEOUT_MS,
        problems,
    }
}

/// Two servers that would run under the same ID: the later one says so.
fn unique(mut servers: Vec<PluginServer>) -> Vec<PluginServer> {
    let mut seen = std::collections::BTreeSet::new();
    for server in &mut servers {
        if !seen.insert(server.id.clone()) {
            server.problems.push(format!("与同一插件里的另一个服务器都叫 {}", server.id));
        }
    }
    servers
}
