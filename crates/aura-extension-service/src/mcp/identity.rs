//! Stable identities of a server's tools. The tool ID depends only on the server ID and the
//! server's own tool name, so a session that loaded the tool keeps it when its contract
//! changes; the definition revision and the instance identity change with the contract.
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{ToolId, ToolInstanceId};
use aura_mcp::{McpServerConfig, NormalizedTool};

fn derive(material: &str) -> [u8; 16] {
    let digest = CanonicalSha256HashV1::sha256(material.as_bytes());
    let hex = digest.hex();
    let mut bytes = [0_u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap_or(0);
    }
    bytes
}

pub fn tool_id(server_id: &str, wire_name: &str) -> ToolId {
    ToolId::from_bytes(derive(&format!("aura.mcp-tool.tool-id.v1\0{server_id}\0{wire_name}")))
}

/// Everything a tool's derived definition and instance depend on besides its ID: the
/// server's listing of the tool, the configuration the user trusted, and the server settings
/// the definition shows (its display name and call timeout). A revision is assigned to
/// exactly one key, so a renamed server or a new timeout is a new revision, never the same
/// revision with other content.
pub fn definition_key(
    server: &McpServerConfig,
    config_fingerprint: &str,
    tool: &NormalizedTool,
) -> Result<String, aura_domain_common::jcs::CanonicalJsonError> {
    aura_domain_common::jcs::canonical_sha256(&serde_json::json!({
        "tool": tool,
        "config_fingerprint": config_fingerprint,
        "display_name": server.display_name,
        "call_timeout_ms": server.call_timeout_ms,
    }))
    .map(|hash| hash.hex().to_owned())
}

pub fn instance_id(
    server_id: &str,
    wire_name: &str,
    revision: u64,
    config_fingerprint: &str,
) -> ToolInstanceId {
    ToolInstanceId::from_bytes(derive(&format!(
        "aura.mcp-tool.instance-id.v1\0{server_id}\0{wire_name}\0{revision}\0{config_fingerprint}"
    )))
}
