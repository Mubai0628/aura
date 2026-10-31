//! The exact operation one package tool call becomes. Capabilities are the shape the
//! permission exception names, not the manifest's self-declared list: an HTTP tool
//! is ordinary exact-origin network, a program is a Native run (plus its network).
use super::super::*;
use super::{InstalledPackageTool, installed_tool_for};
use anyhow::{Result, ensure};
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_package_service::package_http;
use aura_security_contract::PackageRuntimeClass;

impl AgentExecution {
    /// [`AgentExecution::resolve_call`] for a package tool. Same identity checks and
    /// the same durable facts, plus the live install check.
    pub(in crate::agent_execution) fn resolve_package_call(
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
        let tool = installed_tool_for(db, &call.definition)?;
        drop(store);
        ensure!(
            tool.instance.tool_instance_id == call.binding.tool_instance_id
                && tool.instance.tool_version == call.binding.tool_version
                && tool.instance.source_contract_fingerprint
                    == call.binding.source_contract_fingerprint,
            "package.version_changed: 这个工具包已更新，请重新发起调用"
        );
        let (purpose, arguments) = match &call.call.input {
            NormalizedToolCallInputStateV1::Ready { purpose, arguments, .. } => {
                (purpose.to_string(), arguments.clone())
            }
            _ => anyhow::bail!("agent.invalid_input"),
        };
        let source =
            aura_tool_scheduler::security_source_for_admitted_tool_call_v1(call, registry)?;
        let mut content = OperationContent {
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
            source: source.security_source_ref(),
            source_revision_fingerprint: Some(ContentHash::new(
                ContentHashAlgorithm::Sha256,
                call.definition.definition_fingerprint.hex().to_owned(),
            )?),
            capabilities: vec![],
            resources: vec![],
            risks: vec![],
            credentials: vec![],
            purpose: Some(purpose),
            network: OperationNetwork::None,
            compatibility: Default::default(),
            action_kind: call.definition.canonical_name.as_str().to_owned(),
            action_revision: call.definition.tool_version.definition_revision,
            action: arguments.clone(),
        };
        let package = |runtime| ResourceDescriptor::ToolPackage {
            package_id: tool.package_id,
            package_fingerprint: tool.version.package_fingerprint.0.clone(),
            runtime,
        };
        if tool.http() {
            let request = package_http::build(
                &tool.tool,
                &arguments,
                tool.settings.as_ref(),
                content.identity.clone(),
            )?;
            let credential = self.package_credential(&tool, &request)?;
            content.capabilities = vec![ResolvedCapability::OrdinaryNetwork];
            content.network = OperationNetwork::ExactOrigins;
            content.resources = vec![request.origin.clone(), package(PackageRuntimeClass::Http)];
            content.credentials = credential
                .as_ref()
                .map(|(_, reference)| vec![reference.clone()])
                .unwrap_or_default();
            content.action = super::http::action(
                &arguments,
                &request,
                credential.as_ref().map(|(usage, _)| usage),
            )?;
        } else {
            content.capabilities = vec![ResolvedCapability::ProcessExecute];
            if super::program::declares_network(&tool.tool) {
                content.capabilities.push(ResolvedCapability::OrdinaryNetwork);
                content.network = OperationNetwork::BroadProcess;
            }
            content.resources = vec![
                ResourceDescriptor::WorkspaceRoot { workspace_id: snapshot.workspace_id },
                package(PackageRuntimeClass::Program),
            ];
            content.compatibility = [ExecutionCompatibility::ProcessTreeCancellation].into();
        }
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

    /// The bound credential for an HTTP package tool, checked now and again at use:
    /// configured, the bound revision, and allowed websites containing the exact origin.
    fn package_credential(
        &self,
        tool: &InstalledPackageTool,
        request: &package_http::PackageHttpRequest,
    ) -> Result<Option<(ToolCredentialUseV1Ref, CredentialUseRef)>> {
        let Some(slot) = &tool.credential_slot else { return Ok(None) };
        let target = super::http::credential_target(tool, slot)?;
        let record =
            super::http::current_credential(&self.persistence, tool, &target, &request.origin)?;
        let binding =
            tool.credential.as_ref().ok_or_else(|| anyhow::anyhow!("http.credential_unbound"))?;
        let usage = aura_permission_domain::ToolCredentialUseV1 {
            credential_id: record.credential_id,
            credential_revision: record.credential_revision,
            slot_key: slot.clone(),
            placement: super::http::usage_placement(&target.placement)?,
        };
        let reference = CredentialUseRef {
            credential_id: record.credential_id,
            credential_revision: record.credential_revision,
            authority: SecuritySourceRef::SystemComponent {
                component_id: "tool-credential-authority-v1".into(),
            },
            use_fingerprint: aura_domain_common::jcs::canonical_sha256(&(
                &record, &target, binding,
            ))?,
        };
        Ok(Some((usage, reference)))
    }
}

type ToolCredentialUseV1Ref = aura_permission_domain::ToolCredentialUseV1;

/// The scheduler claims of one package call: task state read, and for a program the
/// whole workspace it may reach, which is the honest claim for unknown writes.
pub(in crate::agent_execution) fn resources(
    service: &AgentExecution,
    context: &CallContext,
) -> Result<aura_tool_scheduler::ResolvedPackageResourceSetV1> {
    let id = &context.batch.invocation_identity;
    let write = context.validated.definition.execution_semantics.may_create_persistent_effects;
    let evidence = |value: serde_json::Value| -> Result<CanonicalSha256HashV1> {
        Ok(aura_domain_common::jcs::canonical_sha256(&value)?)
    };
    let mut claims = vec![ResolvedResourceClaimV1 {
        kind: ResolvedResourceKindV1::AgentTaskState,
        access: ResolvedResourceAccessV1::Read,
        canonical_resource_identity: format!("agent-state:{}:{}", id.session_id, id.turn_task_id),
        canonical_lock_key: format!("agent-state:{}:{}", id.session_id, id.turn_task_id),
        resolution_evidence_hash: evidence(serde_json::to_value(
            &context.operation.content.identity,
        )?)?,
        core_derived: true,
    }];
    if context.operation.content.capabilities.contains(&ResolvedCapability::ProcessExecute) {
        let root = service.workspace.resolver(context.snapshot.workspace_id)?;
        let key = format!("agent-workspace:{}", context.snapshot.workspace_id);
        claims.push(ResolvedResourceClaimV1 {
            kind: ResolvedResourceKindV1::WorkspacePath,
            access: if write {
                ResolvedResourceAccessV1::Write
            } else {
                ResolvedResourceAccessV1::Read
            },
            canonical_resource_identity: key.clone(),
            canonical_lock_key: key,
            resolution_evidence_hash: evidence(serde_json::to_value(root.root_identity())?)?,
            core_derived: true,
        });
    }
    claims.sort_by(|a, b| a.canonical_lock_key.cmp(&b.canonical_lock_key));
    Ok(aura_tool_scheduler::ResolvedPackageResourceSetV1 {
        scheduler_claims: claims,
        permission_scopes: vec![],
        risk_flags: vec![],
        resolved_directories: Default::default(),
    })
}
