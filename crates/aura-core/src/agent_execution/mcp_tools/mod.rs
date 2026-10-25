//! MCP tools in the V2 Agent: a trusted server's discovered tools become Registry
//! candidates (`mcp.<server>.<tool>`), exact operations and calls on the session's own
//! instance of the server.
mod candidates;
mod operation;
mod run;
use super::*;
use aura_domain_common::jcs::canonical_sha256;
pub(crate) use candidates::{McpTool, tool_for, usable_tools};
pub(super) use operation::resources;
pub(super) use run::invoke;
use serde_json::{Value, json};

/// The definition and instance records of the MCP tools a snapshot names, stored with it so
/// search, describe and admission read the exact records the snapshot froze.
pub(crate) fn named_by(
    tools: &[McpTool],
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
) -> Vec<(aura_tool_domain::ToolDefinitionRecordV1, aura_tool_domain::ToolInstanceRecordV1)> {
    tools
        .iter()
        .filter(|tool| {
            registry.catalog_entries.iter().any(|entry| {
                entry.tool_id == tool.definition.tool_id
                    && entry.tool_version == tool.definition.tool_version
                    && entry.definition_fingerprint == tool.definition.definition_fingerprint
            })
        })
        .map(|tool| (tool.definition.clone(), tool.instance.clone()))
        .collect()
}

/// The process's MCP servers; absent only in tests that never configured any.
pub(super) fn runtime(service: &AgentExecution) -> anyhow::Result<Arc<crate::mcp::McpRuntime>> {
    service
        .mcp
        .get()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("mcp.unavailable: 这个进程里没有 MCP 服务"))
}

/// Whether an operation is an MCP call in its exact shape.
pub(in crate::agent_execution) fn is_mcp(content: &OperationContent) -> bool {
    aura_security_contract::mcp_server_call(
        &content.source,
        &content.action_kind,
        &content.capabilities,
        &content.resources,
    )
    .is_some()
}

/// The backend an MCP call runs on: Core itself, which talks to the server it started. Its
/// guarantees describe Core's own part of the call, forwarding it to the server the user
/// trusted: Core writes nothing and hands the server no credential. What the server process
/// does as the user is outside them; the approval and the session row say so.
pub(in crate::agent_execution) fn descriptor(runtime: RuntimeInstanceId) -> BackendDescriptor {
    let mut value = super::broker::descriptor(runtime);
    value.id = BackendId("agent-mcp-v1".into());
    value.capabilities = [ResolvedCapability::ExternalExtensionCall].into();
    value.guarantees = [
        aura_security_contract::EnforcementGuarantee::AuthorizedWritesOnly,
        aura_security_contract::EnforcementGuarantee::CredentialIsolation,
    ]
    .into();
    value.protocol_fingerprint = canonical_sha256(&"aura.agent-mcp.v1").expect("literal");
    value
}

/// The preparation facts: the server configuration and the tool contract this call runs.
/// A change between preparation and dispatch refuses the call.
pub(in crate::agent_execution) fn inspect(
    service: &AgentExecution,
    call: &CallContext,
) -> anyhow::Result<Value> {
    let runtime = runtime(service)?;
    let store = service.journal.lock()?;
    let tool = tool_for(&runtime, store.database.connection(), &call.validated.definition)?;
    Ok(json!({
        "server_id": tool.server.id,
        "config_fingerprint": tool.config_fingerprint,
        "tool": tool.tool.wire_name,
        "tool_fingerprint": tool.tool.fingerprint,
    }))
}

/// Whether the user already denied this server in this turn (P04: a ReadOnly denial covers
/// the server for the rest of the turn; it is not asked again).
pub(in crate::agent_execution) fn denied_for_turn(
    service: &AgentExecution,
    operation: &SecuredOperation,
) -> anyhow::Result<bool> {
    let c = &operation.content;
    let (Some((server_id, _)), Some(run), Some(turn)) = (
        aura_security_contract::mcp_server_call(
            &c.source,
            &c.action_kind,
            &c.capabilities,
            &c.resources,
        ),
        c.identity.agent_run_id,
        c.identity.turn_task_id,
    ) else {
        return Ok(false);
    };
    let store = service.journal.lock()?;
    Ok(aura_persistence_v2::mcp::denied_in_turn(store.database.connection(), run, turn, server_id)?)
}
