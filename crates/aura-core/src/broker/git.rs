//! Authenticated Main reads share the native adapter, scope and physical repository identity.
use crate::{JsonRpcRequest, RuntimeServices, RuntimeState};
use aura_execution_contract::{BrokerEvidence, SideEffectStateV1};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::{ResolvedCapability, ResourceDescriptor};
use aura_workspace_domain::GitWorkspaceReadRequestV1;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, atomic::AtomicBool};

pub(crate) async fn dispatch_git(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
    cancel: Arc<AtomicBool>,
) -> Result<Value, IpcErrorEnvelope> {
    let input: GitWorkspaceReadRequestV1 = serde_json::from_value(request.params.clone())
        .map_err(|error| crate::git::envelope(request.id, error.into()))?;
    let operation = super::operation(
        &request.method,
        &request.params,
        request.id,
        vec![ResolvedCapability::GitRead],
        vec![ResourceDescriptor::GitRepository { workspace_id: input.workspace_id }],
    )
    .map_err(|error| crate::git::envelope(request.id, error))?;
    let persistence = services.persistence.clone();
    let workspace = services
        .p03()
        .map_err(|_| crate::MissingCapability::P03WorkspaceFiles.envelope(request.id))?
        .workspace
        .clone();
    let prepared = Arc::new(Mutex::new(None));
    let slot = prepared.clone();
    let inspect: super::OwnedInspect = Arc::new(move || {
        anyhow::ensure!(
            persistence
                .workspace_session_projection()?
                .sessions
                .iter()
                .any(|session| session.session_id == input.session_id
                    && session.workspace_id == input.workspace_id),
            "git.session_scope_mismatch: 此会话不属于请求的工作区，请重新选择当前会话"
        );
        let resolver = workspace.resolver(input.workspace_id)?;
        resolver.verify_root_identity()?;
        let root = resolver.resolve_existing(&aura_domain_common::WorkspacePath::parse(
            aura_domain_common::WorkspacePath::ROOT,
        )?)?;
        let reader = crate::git::GitReader::open(&root.absolute_path, cancel.clone())?;
        let facts = reader.facts()?;
        *slot.lock().map_err(|_| anyhow::anyhow!("git.preparation_lock"))? = Some(reader);
        Ok(facts)
    });
    let method = request.method.clone();
    let evidence: super::OwnedEvidence<Value> = Arc::new(move |dispatch, result| {
        Ok(BrokerEvidence {
            action_kind: method.clone(),
            request_fingerprint: dispatch.operation.fingerprint.0.clone(),
            before: dispatch.plan.prepared.preparation.clone(),
            after: result
                .as_ref()
                .ok()
                .map(aura_domain_common::jcs::canonical_sha256)
                .transpose()?
                .map_or(Value::Null, |hash| json!({"result":hash})),
            domain_journal: json!({"native_git":true,"read_only":true,"index_refresh":false}),
            side_effect: SideEffectStateV1::NoEffect,
        })
    });
    let correlation = request.id;
    super::run(
        state,
        &services.broker,
        operation,
        request.id.to_string(),
        request.command_invocation_id,
        inspect,
        evidence,
        move || async move {
            let reader = prepared
                .lock()
                .map_err(|_| super::error_envelope(correlation, "git.preparation_lock"))?
                .take()
                .ok_or_else(|| super::error_envelope(correlation, "git.preparation_missing"))?;
            tokio::task::spawn_blocking(move || {
                reader.read(&input.query).and_then(|value| Ok(serde_json::to_value(value)?))
            })
            .await
            .map_err(|error| super::error_envelope(correlation, error))?
            .map_err(|error| crate::git::envelope(correlation, error))
        },
    )
    .await
}

pub(crate) fn descriptor(
    runtime: aura_domain_common::RuntimeInstanceId,
) -> aura_execution_contract::BackendDescriptor {
    let mut descriptor = super::descriptor(runtime);
    descriptor.id = aura_execution_contract::BackendId("native-git-broker-v1".into());
    descriptor.capabilities = [ResolvedCapability::GitRead].into();
    // AuthorizedWritesOnly + ReadOnlyUserFiles: duplicated Core token at Low IL;
    // worktree/git-dir/common-dir must be above Low and enforce no-write-up.
    // ChildProcessControl: creation-time membership in a kill-on-close Job with
    // ActiveProcessLimit=1. BoundedOutput: the adapter's combined stream quota.
    // AuthorizedReadsOnly + CredentialIsolation are construction guarantees, as
    // with the retired Core reader: Core fixes the read argv after repository
    // identification/authorization, protocols are disabled and no child (including
    // helpers, hooks, textconv or filters) can start. No credential is materialized.
    descriptor.guarantees = [
        aura_security_contract::EnforcementGuarantee::AuthorizedReadsOnly,
        aura_security_contract::EnforcementGuarantee::AuthorizedWritesOnly,
        aura_security_contract::EnforcementGuarantee::ReadOnlyUserFiles,
        aura_security_contract::EnforcementGuarantee::CredentialIsolation,
        aura_security_contract::EnforcementGuarantee::ChildProcessControl,
        aura_security_contract::EnforcementGuarantee::BoundedOutput,
    ]
    .into();
    descriptor.protocol_fingerprint =
        aura_domain_common::jcs::canonical_sha256(&"aura.native-git-broker.v1").expect("literal");
    descriptor
}
