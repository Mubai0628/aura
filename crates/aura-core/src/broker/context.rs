use crate::{JsonRpcRequest, RuntimeServices, RuntimeState};
use aura_context_domain::{ContextBrokerReadRequest, ContextBrokerReadResult};
use aura_execution_contract::{BrokerEvidence, SideEffectStateV1};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::{ResolvedCapability, ResourceDescriptor};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn dispatch_context(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<Value, IpcErrorEnvelope> {
    let input: ContextBrokerReadRequest = serde_json::from_value(request.params.clone())
        .map_err(|e| super::error_envelope(request.id, e))?;
    let scope = match &input {
        ContextBrokerReadRequest::Status { session_id, agent_run_id } => {
            Some((*session_id, *agent_run_id))
        }
        ContextBrokerReadRequest::Search(v) => Some((v.session_id, v.agent_run_id)),
        ContextBrokerReadRequest::ReadEvents(v) => Some((v.session_id, v.agent_run_id)),
        ContextBrokerReadRequest::ReadToolResult(v) => Some((v.session_id, v.agent_run_id)),
        ContextBrokerReadRequest::ReadCompaction(v) => Some((v.session_id, v.agent_run_id)),
        ContextBrokerReadRequest::MemorySettings => None,
    };
    let operation = super::operation(
        &request.method,
        &request.params,
        request.id,
        vec![ResolvedCapability::ConfigurationRead],
        vec![ResourceDescriptor::ExternalService {
            service_id: "aura.context".into(),
            object_ref: scope.map(|(s, a)| format!("{s}/{a}")),
        }],
    )
    .map_err(|e| super::error_envelope(request.id, e))?;
    let inspect_persistence = services.persistence.clone();
    let inspect_settings = services._settings.clone();
    let inspect: super::OwnedInspect = Arc::new(move || {
        if let Some((session, agent)) = scope {
            anyhow::ensure!(
                inspect_persistence
                    .workspace_session_projection()?
                    .sessions
                    .iter()
                    .any(|s| s.session_id == session),
                "Context session missing"
            );
            Ok(json!({"session_id":session,"agent_run_id":agent}))
        } else {
            Ok(
                json!({"memory_config":aura_domain_common::jcs::canonical_sha256(&*inspect_settings.snapshot(aura_config::ConfigModule::Memory)?.document)?}),
            )
        }
    });
    let event_method = request.method.clone();
    let evidence: super::OwnedEvidence<Value> = Arc::new(
        move |dispatch: &aura_execution_contract::DispatchRequest,
              output: &Result<Value, IpcErrorEnvelope>| {
            Ok(BrokerEvidence {
                action_kind: event_method.clone(),
                request_fingerprint: dispatch.operation.fingerprint.0.clone(),
                before: dispatch.plan.prepared.preparation.clone(),
                after: output
                    .as_ref()
                    .ok()
                    .map(aura_domain_common::jcs::canonical_sha256)
                    .transpose()?
                    .map_or(Value::Null, |hash| json!({"read_digest":hash})),
                domain_journal: json!({"read_only":true}),
                side_effect: SideEffectStateV1::NoEffect,
            })
        },
    );
    let handler_request = request.clone();
    let handler_persistence = services.persistence.clone();
    let handler_settings = services._settings.clone();
    super::run(
        state,
        &services.broker,
        operation,
        request.id.to_string(),
        request.command_invocation_id,
        inspect,
        evidence,
        move || async move {
            let perform = || -> anyhow::Result<Value> {
                let value = if let Some((session, agent)) = scope {
                    let context = aura_context_tools::ContextToolServices::new(
                        (*handler_persistence).clone(),
                        session,
                        agent,
                    );
                    match input {
                        ContextBrokerReadRequest::Status { .. } => {
                            serde_json::to_value(context.status()?)?
                        }
                        ContextBrokerReadRequest::Search(v) => {
                            serde_json::to_value(context.search(v)?)?
                        }
                        ContextBrokerReadRequest::ReadEvents(v) => {
                            serde_json::to_value(context.read_events(v)?)?
                        }
                        ContextBrokerReadRequest::ReadToolResult(v) => {
                            serde_json::to_value(context.read_tool_result(v)?)?
                        }
                        ContextBrokerReadRequest::ReadCompaction(v) => {
                            serde_json::to_value(context.read_compaction(v)?)?
                        }
                        ContextBrokerReadRequest::MemorySettings => unreachable!("scope checked"),
                    }
                } else {
                    serde_json::to_value(
                        &*handler_settings.snapshot(aura_config::ConfigModule::Memory)?.document,
                    )?
                };
                Ok(serde_json::to_value(ContextBrokerReadResult { value })?)
            };
            perform().map_err(|e| super::error_envelope(handler_request.id, e))
        },
    )
    .await
}
