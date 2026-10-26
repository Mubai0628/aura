//! `mcp.json`: the servers the user configured. A server is trusted at the exact fingerprint
//! of what Aura would run or reach; changing any of it needs a new trust. Secrets never live
//! here: names that look like one are refused with the reason.
mod validate;

use aura_domain_common::CredentialId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_SERVERS: usize = 32;
const MAX_ARGUMENTS: usize = 64;
const MAX_ENVIRONMENT: usize = 64;
const MAX_SECRET_ENVIRONMENT: usize = 8;
const MAX_HEADERS: usize = 32;
const MAX_SCOPES: usize = 16;
/// Headers Aura sets itself; neither a configured header nor a secret's header may be one.
const MANAGED_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "content-type",
    "accept",
    "mcp-session-id",
    "mcp-protocol-version",
    "last-event-id",
    "transfer-encoding",
];
const MAX_VALUE_BYTES: usize = 4096;
/// Server IDs that would collide with Aura's own `mcp.*` tools.
const RESERVED_IDS: [&str; 5] = ["aura", "core", "prompts", "resources", "servers"];

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpEnvironmentEntry {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpHeader {
    pub name: String,
    pub value: String,
}

/// An environment variable whose value is a secret Aura keeps (never in this file).
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSecretEnvironment {
    pub name: String,
    pub secret_id: CredentialId,
}

/// How Aura signs in to an HTTP server. The secret or the tokens are never in this file.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpHttpAuth {
    /// A secret the user entered in Settings, sent as `Authorization: Bearer <secret>`, or as
    /// the whole value of `header` when one is named.
    Secret {
        secret_id: CredentialId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        header: Option<String>,
    },
    /// OAuth sign-in in the browser (PKCE); Aura keeps the tokens. Without a client ID Aura
    /// registers itself with the authorization server.
    #[serde(rename = "oauth")]
    OAuth {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_id: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
    },
}

/// How Aura reaches the server.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpTransport {
    /// A program on this computer, run as the user in a Job; JSON-RPC over stdin/stdout.
    Stdio {
        command: String,
        #[serde(default)]
        arguments: Vec<String>,
        #[serde(default)]
        working_directory: Option<String>,
        #[serde(default)]
        environment: Vec<McpEnvironmentEntry>,
        /// Variables whose values are secrets Aura keeps; their names may look like secrets.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        secret_environment: Vec<McpSecretEnvironment>,
    },
    /// A Streamable HTTP endpoint (POST only).
    Http {
        url: String,
        #[serde(default)]
        headers: Vec<McpHeader>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth: Option<McpHttpAuth>,
    },
}

impl McpTransport {
    /// The secrets this configuration uses, by ID.
    #[must_use]
    pub fn secret_ids(&self) -> Vec<CredentialId> {
        match self {
            Self::Stdio { secret_environment, .. } => {
                secret_environment.iter().map(|entry| entry.secret_id).collect()
            }
            Self::Http { auth: Some(McpHttpAuth::Secret { secret_id, .. }), .. } => {
                vec![*secret_id]
            }
            Self::Http { .. } => Vec::new(),
        }
    }
}

const fn default_startup_timeout() -> u64 {
    30_000
}
const fn default_call_timeout() -> u64 {
    120_000
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    /// Stable: it names the tools (`mcp.<id>.<tool>`); the display name may change freely.
    pub id: String,
    pub display_name: String,
    pub transport: McpTransport,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trusted_fingerprint: Option<String>,
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_ms: u64,
    #[serde(default = "default_call_timeout")]
    pub call_timeout_ms: u64,
    /// Saved for multi-agent; no sub-agent exists yet.
    // UNWIRED: no child agent exists to read it.
    #[serde(default)]
    pub available_for_subagents: bool,
}

/// The program a stdio command resolved to when the user previewed or trusted it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct ResolvedCommand {
    /// Absolute path of the executable or `.cmd`/`.bat` script.
    pub path: String,
    pub sha256: String,
}

/// A name that looks like it carries a secret. Such values belong in a credential, never in
/// `mcp.json`, and a server process does not inherit them from the user's environment either.
#[must_use]
pub fn secret_like(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("AURA_")
        || matches!(upper.as_str(), "AUTHORIZATION" | "PROXY-AUTHORIZATION" | "COOKIE")
        || upper.split(['_', '-']).any(|part| {
            matches!(
                part,
                "TOKEN" | "KEY" | "SECRET" | "PASSWORD" | "CREDENTIAL" | "APIKEY" | "AUTH"
            )
        })
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn plain(value: &str, limit: usize) -> bool {
    value.len() <= limit && !value.chars().any(char::is_control)
}

fn loopback(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(address) => address.is_loopback(),
        url::Host::Ipv6(address) => address.is_loopback(),
    }
}

/// Every problem with one server, in the user's words; empty means valid.
#[must_use]
pub fn validate_server(server: &McpServerConfig) -> Vec<String> {
    let mut problems = Vec::new();
    if !valid_id(&server.id) {
        problems.push("id 只能用小写字母、数字和 -，以字母开头，最长 32 个字符".into());
    } else if RESERVED_IDS.contains(&server.id.as_str()) {
        problems.push(format!("id {} 是 Aura 保留的名字", server.id));
    }
    if server.display_name.trim().is_empty() || !plain(&server.display_name, 64) {
        problems.push("显示名不能为空，最长 64 字节".into());
    }
    validate::transport(&server.transport, &mut problems);
    if !(1_000..=120_000).contains(&server.startup_timeout_ms) {
        problems.push("startup_timeout_ms 必须在 1000 到 120000 之间".into());
    }
    if !(1_000..=600_000).contains(&server.call_timeout_ms) {
        problems.push("call_timeout_ms 必须在 1000 到 600000 之间".into());
    }
    problems
}

/// The whole list: each server, unique IDs and the server limit.
#[must_use]
pub fn validate_servers(servers: &[McpServerConfig]) -> Vec<String> {
    let mut problems = Vec::new();
    if servers.len() > MAX_SERVERS {
        problems.push(format!("最多 {MAX_SERVERS} 个服务器"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for server in servers {
        problems.extend(validate_server(server).into_iter().map(|p| format!("{}：{p}", server.id)));
        if !seen.insert(server.id.as_str()) {
            problems.push(format!("服务器 id {} 重复", server.id));
        }
    }
    problems
}

/// What the user trusts: the transport exactly as configured plus, for a program, the file it
/// resolved to and that file's content. Names, timeouts and switches are not part of it.
#[must_use]
pub fn config_fingerprint(transport: &McpTransport, resolved: Option<&ResolvedCommand>) -> String {
    let canonical = serde_json::json!({
        "fingerprint_version": 1,
        "transport": transport,
        "resolved_command": resolved,
    });
    blake3::hash(canonical.to_string().as_bytes()).to_hex().to_string()
}
