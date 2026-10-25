use crate::{JsonRpcRequest, RuntimeServices, RuntimeState, p07_commands::ToolMutationDispatchV1};
use aura_domain_common::{SessionId, WorkspaceId};
use aura_execution_contract::{BrokerEvidence, SideEffectStateV1};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::{ResolvedCapability, ResourceDescriptor};
use serde_json::{Value, json};
use std::future::Future;
use std::sync::Arc;

#[cfg(test)]
mod tests;

fn configuration_scope(params: &Value) -> anyhow::Result<(Option<WorkspaceId>, Option<SessionId>)> {
    let workspace = params
        .get("workspace_id")
        .map(|value| serde_json::from_value::<Option<WorkspaceId>>(value.clone()))
        .transpose()?
        .flatten();
    let session = params
        .get("session_id")
        .map(|value| serde_json::from_value::<Option<SessionId>>(value.clone()))
        .transpose()?
        .flatten();
    Ok((workspace, session))
}

pub(crate) fn handles(method: &str) -> bool {
    matches!(
        method,
        "tools.tool.set_enabled"
            | "tools.instance.update_configuration"
            | "tools.instance.bind_credential"
            | "tools.instance.clear_credential"
            | "tools.pin.workspace.set"
            | "tools.pin.workspace.clear"
            | "tools.pin.session.set"
            | "tools.pin.session.clear"
    )
}
pub(crate) async fn dispatch<F, Fut>(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
    call: F,
) -> Result<ToolMutationDispatchV1, IpcErrorEnvelope>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<ToolMutationDispatchV1, IpcErrorEnvelope>> + Send + 'static,
{
    let operation = super::operation(
        &request.method,
        &request.params,
        request.id,
        vec![ResolvedCapability::ConfigurationWrite],
        vec![ResourceDescriptor::ExternalService {
            service_id: "aura.tool-config".into(),
            object_ref: None,
        }],
    )
    .map_err(|e| super::error_envelope(request.id, e))?;
    let tool_persistence = services.persistence.clone();
    let tool_settings = services._settings.clone();
    let tool_params = request.params.clone();
    let inspect: super::OwnedInspect = Arc::new(move || {
        let (workspace, session) = configuration_scope(&tool_params)?;
        let authority = tool_persistence.tool_authority_snapshot(workspace, session, None)?;
        Ok(
            json!({"registry_revision":authority.registry_revision,"authority_hash":aura_domain_common::jcs::canonical_sha256(&json!({
            "definitions":authority.definitions,"instances":authority.instances,"configuration":authority.instance_configurations,
            "credential_bindings":authority.instance_credential_bindings,"workspace_pins":authority.workspace_pins,"session_pins":authority.session_pins
        }))?,"settings":tool_settings.tool_settings_snapshot()?}),
        )
    });
    let evidence_inspect = Arc::clone(&inspect);
    let event_persistence = services.persistence.clone();
    let event_method = request.method.clone();
    let event_invocation = request.command_invocation_id;
    let evidence: super::OwnedEvidence<ToolMutationDispatchV1> = Arc::new(
        move |dispatch: &aura_execution_contract::DispatchRequest,
              output: &Result<ToolMutationDispatchV1, IpcErrorEnvelope>| {
            let journal = event_invocation
                .map(|id| event_persistence.tool_mutation_command_v1(id))
                .transpose()?
                .flatten();
            let before = dispatch.plan.prepared.preparation.clone();
            let after = evidence_inspect().unwrap_or(Value::Null);
            let committed = journal.as_ref().is_some_and(|j| {
                j.state == aura_persistence::ToolMutationCommandJournalStateV1::Succeeded
            }) && output.is_ok();
            let side_effect = if before == after && (committed || output.is_err()) {
                SideEffectStateV1::NoEffect
            } else if committed && after != Value::Null {
                SideEffectStateV1::FullyApplied
            } else {
                SideEffectStateV1::Unknown
            };
            Ok(BrokerEvidence {action_kind:event_method.clone(),request_fingerprint:dispatch.operation.fingerprint.0.clone(),before,after,
            domain_journal:journal.map_or(Value::Null,|j|json!({"command_id":j.command_invocation_id,"state":j.state,"request_hash":j.request_hash})),side_effect})
        },
    );
    super::run(
        state,
        &services.broker,
        operation,
        request.command_invocation_id.map_or_else(|| request.id.to_string(), |id| id.to_string()),
        request.command_invocation_id,
        inspect,
        evidence,
        call,
    )
    .await
}
