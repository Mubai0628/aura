//! The MCP servers in `mcp.json`, in the user's order. Trust is the fingerprint of what
//! Aura would run or reach; no secret, tool list or per-tool switch is ever stored here.
use crate::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, SettingsFacade, SettingsFacadeError,
};
use aura_mcp::{McpServerConfig, validate_servers};
use serde_json::Value;
use std::collections::BTreeMap;

const SERVERS: &str = "servers";

/// The servers in force. When `mcp.json` cannot be used, no server is reached and `issue`
/// says why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpRegistry {
    pub servers: Vec<McpServerConfig>,
    pub revision: u64,
    pub issue: Option<String>,
}

pub(crate) fn validate_mcp_document(
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    servers(&document.values).map(|_| ())
}

fn invalid(message: impl Into<String>) -> ConfigValidationError {
    ConfigValidationError::Semantic(format!("mcp.json: {}", message.into()))
}

fn servers(
    values: &BTreeMap<String, Value>,
) -> Result<Vec<McpServerConfig>, ConfigValidationError> {
    if let Some(field) = values.keys().find(|key| key.as_str() != SERVERS) {
        return Err(ConfigValidationError::UnknownModuleField {
            module: ConfigModule::Mcp,
            field: field.clone(),
        });
    }
    let Some(items) = values.get(SERVERS) else { return Ok(Vec::new()) };
    let servers: Vec<McpServerConfig> = serde_json::from_value(items.clone())
        .map_err(|error| invalid(format!("servers 格式不对：{error}")))?;
    let problems = validate_servers(&servers);
    if !problems.is_empty() {
        return Err(invalid(problems.join("；")));
    }
    if let Some(server) = servers.iter().find(|server| {
        server
            .trusted_fingerprint
            .as_ref()
            .is_some_and(|f| f.len() != 64 || !f.bytes().all(|b| b.is_ascii_hexdigit()))
    }) {
        return Err(invalid(format!("{} 的 trusted_fingerprint 必须是 64 位十六进制", server.id)));
    }
    Ok(servers)
}

impl SettingsFacade {
    /// The registry in force. When the domain is not healthy, or the served document is a
    /// last-known-good fallback rather than the file on disk, no server is reached: an edit
    /// that failed may have been the user removing or changing one.
    pub fn mcp_registry(&self) -> McpRegistry {
        let module = ConfigModule::Mcp;
        let snapshot = match self.snapshot(module) {
            Ok(snapshot)
                if self.health(module).state == crate::DomainHealthState::Ready
                    && crate::memory::current_input_matches(self, module, &snapshot) =>
            {
                snapshot
            }
            _ => {
                return McpRegistry {
                    issue: Some(
                        "mcp.json 当前无法使用，所有 MCP 服务器都不连接。请在设置里重试、恢复上一版有效配置或重置。".into(),
                    ),
                    ..McpRegistry::default()
                };
            }
        };
        let revision = snapshot.document.revision;
        match servers(&snapshot.document.values) {
            Ok(servers) => McpRegistry { servers, revision, issue: None },
            Err(error) => McpRegistry {
                servers: Vec::new(),
                revision,
                issue: Some(format!("{error}；所有 MCP 服务器都不连接")),
            },
        }
    }

    /// Replaces the servers at `expected_revision`. A concurrent change is a conflict for the
    /// caller to reread, never a silent overwrite.
    pub fn replace_mcp_registry(
        &self,
        expected_revision: u64,
        servers: &[McpServerConfig],
    ) -> Result<u64, SettingsFacadeError> {
        let items = servers
            .iter()
            .map(|server| serde_json::to_value(server).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        let snapshot = self.update(ConfigModule::Mcp, expected_revision, |values| {
            if items.is_empty() {
                values.remove(SERVERS);
            } else {
                values.insert(SERVERS.to_owned(), Value::Array(items));
            }
        })?;
        Ok(snapshot.document.revision)
    }
}
