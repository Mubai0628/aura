//! A trusted server's discovered tools as V2 Registry candidates. Every value is derived from
//! the server's current trusted configuration, its stored catalog and the revision assigned to
//! exactly those inputs; the same inputs always derive the same definition, so a frozen call
//! can be checked against the live server.
use anyhow::{Result, ensure};
use aura_core_base::append_core_log;
use aura_domain_common::{Revision, jcs::canonical_sha256};
use aura_extension_service::mcp::{McpRuntime, identity};
use aura_mcp::{McpServerConfig, NormalizedTool};
use aura_persistence_v2::mcp::ToolRevision;
use aura_security_contract::ResolvedCapability;
use aura_tool_contract::{
    NormalizedMcpToolContractFingerprintV1, ToolAudienceV1, ToolCanonicalName, ToolInputSchemaV1,
    ToolResultContractV1, ToolRuntimeDescriptorV1, ToolSourceContractFingerprintV1,
    ToolSourceKindV1, ToolVersion,
};
use aura_tool_domain::{
    ToolDefinitionRecordV1, ToolExecutionSemanticsV1, ToolInstanceOverridesV1,
    ToolInstanceRecordV1, ToolRuntimeHealthV1,
};
use aura_tool_registry::{RegistryCandidateV1, ToolSourceEligibilityEvidenceV1};
use std::sync::Arc;

const SUMMARY_CHARS: usize = 200;

/// One tool of a trusted, enabled server whose catalog was listed with its current
/// configuration.
#[derive(Clone, Debug)]
pub struct McpTool {
    pub server: McpServerConfig,
    pub config_fingerprint: String,
    pub tool: NormalizedTool,
    pub definition: ToolDefinitionRecordV1,
    pub instance: ToolInstanceRecordV1,
}

impl McpTool {
    pub fn candidate(&self) -> RegistryCandidateV1 {
        RegistryCandidateV1 {
            definition: self.definition.clone(),
            instance: self.instance.clone(),
            source_evidence: ToolSourceEligibilityEvidenceV1::ExternalSource {
                source_enabled: true,
                source_trusted: true,
                source_runtime_healthy: true,
            },
            active_for_unpinned: true,
        }
    }
}

fn model_text(server: &McpServerConfig, tool: &NormalizedTool) -> (String, String, String) {
    let title =
        tool.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or(tool.wire_name.clone());
    let display = format!("{} · {}", server.display_name, title);
    let description = if tool.description.trim().is_empty() {
        format!("MCP tool {} of the server {}.", tool.wire_name, server.id)
    } else {
        tool.description.clone()
    };
    let summary =
        description.lines().next().unwrap_or_default().chars().take(SUMMARY_CHARS).collect();
    (display, description, summary)
}

fn derive(
    server: &McpServerConfig,
    config_fingerprint: &str,
    tool: &NormalizedTool,
    revision: ToolRevision,
) -> Result<McpTool> {
    ensure!(
        revision.revision > 0,
        "mcp.tool_unrevised: 这个 MCP 工具的版本记录还没有建好，暂时不能调用，调用没有发出。稍后用 tools.search 重新找到它再调用。"
    );
    let created_at = revision.created_at;
    let tool_id = identity::tool_id(&server.id, &tool.wire_name);
    let version = ToolVersion::new(None, Revision::new(revision.revision))?;
    let contract = canonical_sha256(&serde_json::json!({
        "tool_fingerprint": tool.fingerprint,
        "config_fingerprint": config_fingerprint,
    }))?;
    let (display, description, summary) = model_text(server, tool);
    let definition = ToolDefinitionRecordV1::new(
        tool_id,
        version.clone(),
        None,
        None,
        None,
        ToolCanonicalName::new(tool.canonical_name.clone())?,
        display,
        description,
        summary,
        ToolInputSchemaV1::new(tool.input_schema.clone())?,
        false,
        ToolResultContractV1::Utf8Text,
        ToolRuntimeDescriptorV1::ExternalSource {
            source_contract_key: format!("mcp:{}:{}", server.id, tool.wire_name),
        },
        vec![ResolvedCapability::ExternalExtensionCall],
        vec![],
        // A sub-agent has the user's servers as the main agent does. Any change to what this
        // derives from the same inputs needs the next `identity::DERIVATION`.
        ToolAudienceV1::SubagentConfigurable,
        ToolSourceKindV1::Mcp,
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed: false,
            repeat_is_idempotent: false,
            may_create_persistent_effects: true,
        },
        contract.clone(),
        Revision::INITIAL,
        created_at,
    )?;
    let instance = ToolInstanceRecordV1 {
        tool_instance_id: identity::instance_id(
            &server.id,
            &tool.wire_name,
            revision.revision,
            config_fingerprint,
        ),
        tool_id,
        tool_version: version,
        source_kind: ToolSourceKindV1::Mcp,
        source_contract_fingerprint: ToolSourceContractFingerprintV1::Mcp {
            normalized_mcp_tool_contract_fingerprint: NormalizedMcpToolContractFingerprintV1::new(
                contract,
            ),
        },
        enabled: true,
        runtime_health: ToolRuntimeHealthV1::Healthy,
        overrides: ToolInstanceOverridesV1 {
            timeout_ms: Some(server.call_timeout_ms),
            max_retries: None,
            max_concurrent_attempts: None,
            subagent_enabled: None,
        },
        entity_revision: Revision::INITIAL,
        created_at,
        updated_at: created_at,
    };
    Ok(McpTool {
        server: server.clone(),
        config_fingerprint: config_fingerprint.to_owned(),
        tool: tool.clone(),
        definition,
        instance,
    })
}

/// The revision assigned to exactly these inputs, if any.
fn revision(
    db: &aura_persistence_core::Connection,
    server: &McpServerConfig,
    fingerprint: &str,
    tool: &NormalizedTool,
) -> Result<Option<ToolRevision>> {
    let key = identity::definition_key(server, fingerprint, tool)?;
    let id = identity::tool_id(&server.id, &tool.wire_name).to_string();
    Ok(aura_persistence_v2::mcp::revision_of(db, &id, &key)?)
}

/// Every tool of every usable server: trusted, enabled, unchanged since the trust, and listed
/// with that configuration. One server whose facts no longer derive is left out with a log
/// line; the others and the built-in tools stay. A tool without a revision for the server's
/// current settings is left out until its revision is assigned in the background.
/// `db` is the caller's connection: the catalogs are read inside the caller's own store access.
pub fn usable_tools(
    runtime: &Arc<McpRuntime>,
    db: &aura_persistence_core::Connection,
) -> Vec<McpTool> {
    let registry = runtime.configured();
    let mut tools = Vec::new();
    for server in &registry.servers {
        let Ok(checked) = runtime.usable_in(server, db) else { continue };
        let Some(fingerprint) = checked.fingerprint else { continue };
        let Ok(Some(catalog)) = aura_persistence_v2::mcp::load(db, &server.id) else { continue };
        if catalog.config_fingerprint != fingerprint {
            continue;
        }
        for tool in &catalog.catalog.tools {
            let derived = match revision(db, server, &fingerprint, tool) {
                Ok(Some(revision)) => derive(server, &fingerprint, tool, revision),
                Ok(None) => {
                    runtime.revise_later(server.clone());
                    continue;
                }
                Err(error) => Err(error),
            };
            match derived {
                Ok(derived) => tools.push(derived),
                Err(error) => append_core_log(
                    "mcp.tool_left_out",
                    serde_json::json!({"server": server.id, "tool": tool.wire_name, "reason": error.to_string()}),
                ),
            }
        }
    }
    tools
}

/// The live tool a frozen definition names, only while its server is still usable and
/// derives exactly this definition. Anything else is a refusal the model can act on.
pub fn tool_for(
    runtime: &Arc<McpRuntime>,
    db: &aura_persistence_core::Connection,
    definition: &ToolDefinitionRecordV1,
) -> Result<McpTool> {
    let tool = usable_tools(runtime, db)
        .into_iter()
        .find(|tool| tool.definition.tool_id == definition.tool_id)
        .ok_or_else(|| anyhow::anyhow!(TOOL_UNAVAILABLE))?;
    ensure!(tool.definition == *definition, TOOL_CHANGED);
    Ok(tool)
}

const TOOL_UNAVAILABLE: &str = "mcp.tool_unavailable: 这个 MCP 工具现在不可用（服务器已停用或不再受信任、配置改了，或工具已被移除），调用没有发给服务器。请用 tools.search 找现在可用的工具。";

/// Also the refusal when the instance the call was bound to is no longer the live one.
pub(super) const TOOL_CHANGED: &str = "mcp.tool_changed: 这个 MCP 工具在调用准备好之后更新了，调用没有发给服务器。请用 tools.load 重新载入它再调用。";
