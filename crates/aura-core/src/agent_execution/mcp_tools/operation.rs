//! The exact operation one MCP tool call becomes: the server source at its trusted
//! configuration, one coarse capability and the server as the resource. Aura does not infer
//! what the tool does; the session mode and the turn answer decide.
use super::super::*;
use super::{runtime, tool_for};
use anyhow::{Result, ensure};
use aura_domain_common::{ContentHash, ContentHashAlgorithm};

impl AgentExecution {
    /// [`AgentExecution::resolve_call`] for an MCP tool: the same identity checks and durable
    /// facts, plus the live server and tool check.
    pub(in crate::agent_execution) fn resolve_mcp_call(
        &self,
        batch: &ToolBatchRequestV1,
        registry: &ToolRegistrySnapshotV1,
        call: &ValidatedToolCallV1,
        snapshot: &PermissionSnapshot,
    ) -> Result<CallContext> {
        let id = &batch.invocation_identity;
        ensure!(
            batch.calls.contains(&call.call)
                && batch.registry_snapshot_ref == registry.snapshot_ref
                && snapshot.session_id == id.session_id,
            "agent.call_identity"
        );
        let runtime = runtime(self)?;
        let store = self.journal.lock()?;
        let db = store.database.connection();
        let run = aura_persistence_v2::agent::turns::load_run(db, id.agent_run_id)?;
        let turn = aura_persistence_v2::agent::turns::load(db, id.turn_task_id)?;
        ensure!(
            run.turn_task_id == turn.turn_task_id
                && turn.agent_run_id == run.agent_run_id
                && turn.session_id == id.session_id
                && turn.workspace_id == snapshot.workspace_id
                && !turn.state.terminal(),
            "agent.live_identity"
        );
        let tool = tool_for(&runtime, db, &call.definition)?;
        drop(store);
        ensure!(
            tool.instance.tool_instance_id == call.binding.tool_instance_id
                && tool.instance.tool_version == call.binding.tool_version
                && tool.instance.source_contract_fingerprint
                    == call.binding.source_contract_fingerprint,
            "mcp.tool_changed: 这个 MCP 工具已更新，请重新发起调用"
        );
        let (purpose, arguments) = match &call.call.input {
            NormalizedToolCallInputStateV1::Ready { purpose, arguments, .. } => {
                (purpose.to_string(), arguments.clone())
            }
            _ => anyhow::bail!("agent.invalid_input"),
        };
        let fingerprint = ContentHash::new(ContentHashAlgorithm::Blake3, &tool.config_fingerprint)
            .map_err(|_| anyhow::anyhow!("mcp.invalid_fingerprint"))?;
        let content = OperationContent {
            identity: InvocationIdentity {
                principal_id: PrincipalId::from_bytes(id.session_id.to_bytes()),
                actor: if run.parent_agent_run_id.is_some() {
                    InvocationActor::ChildAgent
                } else {
                    InvocationActor::ParentAgent
                },
                workspace_id: Some(snapshot.workspace_id),
                session_id: Some(id.session_id),
                turn_task_id: Some(id.turn_task_id),
                agent_run_id: Some(id.agent_run_id),
                parent_agent_run_id: run.parent_agent_run_id,
                correlation_id: CorrelationId::new(),
                permission_snapshot_id: Some(snapshot.id),
            },
            source: SecuritySourceRef::McpServer { server_id: tool.server.id.clone() },
            source_revision_fingerprint: Some(fingerprint.clone()),
            capabilities: vec![ResolvedCapability::ExternalExtensionCall],
            resources: vec![ResourceDescriptor::McpServer {
                server_id: tool.server.id.clone(),
                config_fingerprint: fingerprint,
            }],
            risks: vec![],
            credentials: vec![],
            purpose: Some(purpose),
            network: OperationNetwork::None,
            compatibility: Default::default(),
            action_kind: call.definition.canonical_name.as_str().to_owned(),
            action_revision: call.definition.tool_version.definition_revision,
            action: arguments,
        };
        let operation = SecuredOperation::new(SecurityOperationId::new(), content)?;
        Ok(CallContext {
            batch: batch.clone(),
            registry: registry.clone(),
            validated: call.clone(),
            snapshot: snapshot.clone(),
            operation,
            verified_external_capabilities: vec![],
            verified_external_grants: vec![],
            observed_resources: Arc::new(std::sync::Mutex::new(Vec::new())),
        })
    }
}

/// The scheduler claims of one MCP call: the task state it reads. The server's own effects
/// are outside Aura's resource model, which is why the call never retries on its own.
pub(in crate::agent_execution) fn resources(
    context: &CallContext,
) -> Result<aura_tool_scheduler::ResolvedPackageResourceSetV1> {
    let id = &context.batch.invocation_identity;
    let key = format!("agent-state:{}:{}", id.session_id, id.turn_task_id);
    Ok(aura_tool_scheduler::ResolvedPackageResourceSetV1 {
        scheduler_claims: vec![ResolvedResourceClaimV1 {
            kind: ResolvedResourceKindV1::AgentTaskState,
            access: ResolvedResourceAccessV1::Read,
            canonical_resource_identity: key.clone(),
            canonical_lock_key: key,
            resolution_evidence_hash: aura_domain_common::jcs::canonical_sha256(
                &serde_json::to_value(&context.operation.content.identity)?,
            )?,
            core_derived: true,
        }],
        permission_scopes: vec![],
        risk_flags: vec![],
        resolved_directories: Default::default(),
    })
}
