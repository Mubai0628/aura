use crate::{JsonRpcRequest, RuntimeServices, RuntimeState};
use aura_domain_common::{UtcTimestamp, jcs::canonical_sha256};
use aura_execution_contract::{BrokerEvidence, SideEffectStateV1};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::{ResolvedCapability, ResourceDescriptor};
use aura_tool_contract::*;
use aura_tool_registry::BuiltinCoreHandlerV1;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn dispatch_tools(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<Value, IpcErrorEnvelope> {
    let input: ToolBrokerInvokeRequest = serde_json::from_value(request.params.clone())
        .map_err(|e| super::error_envelope(request.id, e))?;
    let mutation = matches!(input.action, ToolBrokerAction::Load(_) | ToolBrokerAction::Unload(_));
    let operation = super::operation(
        &request.method,
        &request.params,
        request.id,
        vec![if mutation {
            ResolvedCapability::ToolEphemeralExposureWrite
        } else {
            ResolvedCapability::ToolRegistryRead
        }],
        vec![ResourceDescriptor::ToolRegistrySnapshot {
            resource_fingerprint: input.registry_snapshot_ref.content_hash.clone(),
        }],
    )
    .map_err(|e| super::error_envelope(request.id, e))?;
    let snapshot = services
        .persistence
        .read_tool_registry_search_corpus(&input.registry_snapshot_ref)
        .map_err(|e| super::error_envelope(request.id, e))?
        .ok_or_else(|| super::error_envelope(request.id, "exact Registry unavailable"))?
        .snapshot;
    let (workspace, session, turn) = match snapshot.scope {
        ToolRegistryScopeV1::Workspace { workspace_id } => (workspace_id, None, None),
        ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, .. }
        | ToolRegistryScopeV1::ChildAgent { workspace_id, session_id, turn_task_id, .. } => {
            (workspace_id, Some(session_id), Some(turn_task_id))
        }
    };
    let registry_persistence = services.persistence.clone();
    let inspect_settings = services._settings.clone();
    let inspect_ref = input.registry_snapshot_ref.clone();
    let inspect_scope = snapshot.scope.clone();
    let inspect_session = session;
    let inspect_turn = turn;
    let inspect: super::OwnedInspect = Arc::new(move || {
        let current = registry_persistence
            .read_tool_registry_search_corpus(&inspect_ref)?
            .ok_or_else(|| anyhow::anyhow!("Registry retired"))?;
        if mutation {
            anyhow::ensure!(
                registry_persistence.read_tool_registry_scope_head(&inspect_scope)?.as_ref()
                    == Some(&inspect_ref),
                "Registry scope head changed; a new exact request is required"
            );
        }
        let authority = registry_persistence.tool_authority_snapshot(
            Some(workspace),
            inspect_session,
            inspect_turn,
        )?;
        let permission = inspect_session
            .map(|s| registry_persistence.session_permission_state(s))
            .transpose()?;
        Ok(
            json!({"snapshot":current.snapshot.snapshot_ref,"authority_revision":authority.registry_revision,
            "settings":inspect_settings.tool_settings_snapshot()?,"permission":permission}),
        )
    });
    let evidence_persistence = services.persistence.clone();
    let event_method = request.method.clone();
    let evidence: super::OwnedEvidence<Value> = Arc::new(
        move |dispatch: &aura_execution_contract::DispatchRequest,
              output: &Result<Value, IpcErrorEnvelope>| {
            let after = output
                .as_ref()
                .ok()
                .and_then(|v| v.get("resulting_registry_snapshot_ref"))
                .filter(|v| !v.is_null())
                .map(|v| serde_json::from_value::<ToolRegistrySnapshotRef>(v.clone()))
                .transpose()?
                .map(|r| evidence_persistence.read_tool_registry_search_corpus(&r))
                .transpose()?
                .flatten();
            let changed =
                output.as_ref().ok().and_then(|v| v.get("changed")).and_then(Value::as_bool);
            let side_effect = if !mutation || changed == Some(false) {
                SideEffectStateV1::NoEffect
            } else if changed == Some(true) && after.is_some() {
                SideEffectStateV1::FullyApplied
            } else {
                SideEffectStateV1::Unknown
            };
            Ok(BrokerEvidence {
                action_kind: event_method.clone(),
                request_fingerprint: dispatch.operation.fingerprint.0.clone(),
                before: dispatch.plan.prepared.preparation.clone(),
                after: after.map_or(Value::Null, |c| json!({"snapshot":c.snapshot.snapshot_ref})),
                domain_journal: json!({"changed":changed,"atomic_registry_publication":side_effect==SideEffectStateV1::FullyApplied}),
                side_effect,
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
        request.command_invocation_id.map_or_else(|| request.id.to_string(), |id| id.to_string()),
        request.command_invocation_id,
        inspect,
        evidence,
        move || async move {
            let (handler, arguments) = match input.action {
                ToolBrokerAction::Search(v) => {
                    (BuiltinCoreHandlerV1::ToolsSearch, serde_json::to_value(v))
                }
                ToolBrokerAction::Describe(v) => {
                    (BuiltinCoreHandlerV1::ToolsDescribe, serde_json::to_value(v))
                }
                ToolBrokerAction::ListLoaded => (
                    BuiltinCoreHandlerV1::ToolsListLoaded,
                    serde_json::to_value(ToolListLoadedRequestV1 {
                        registry_snapshot_ref: input.registry_snapshot_ref.clone(),
                    }),
                ),
                ToolBrokerAction::Load(v) => {
                    (BuiltinCoreHandlerV1::ToolsLoad, serde_json::to_value(v))
                }
                ToolBrokerAction::Unload(v) => {
                    (BuiltinCoreHandlerV1::ToolsUnload, serde_json::to_value(v))
                }
            };
            let arguments = arguments.map_err(|e| super::error_envelope(handler_request.id, e))?;
            let build = if mutation {
                let settings = handler_settings
                    .tool_settings_snapshot()
                    .map_err(|e| super::error_envelope(handler_request.id, e))?;
                let permission = session
                    .map(|s| handler_persistence.session_permission_state(s))
                    .transpose()
                    .map_err(|e| super::error_envelope(handler_request.id, e))?;
                Some(aura_persistence::ToolRegistryBuildRequestV1 {
                    scope: snapshot.scope.clone(),
                    settings,
                    mode_permission_context_hash: canonical_sha256(&permission)
                        .map_err(|e| super::error_envelope(handler_request.id, e))?,
                    core_exposed_tool_ids: snapshot
                        .catalog_entries
                        .iter()
                        .filter(|e| e.load_state == ToolLoadStateV1::Core)
                        .map(|e| e.tool_id)
                        .collect(),
                    non_package_candidates: vec![],
                    created_at: UtcTimestamp::now(),
                    correlation_id: handler_request.id,
                })
            } else {
                None
            };
            let result = aura_tool_service::p07_builtin::invoke_broker_service(
                &handler_persistence,
                handler,
                &arguments,
                &snapshot,
                build,
                handler_request.id,
            )
            .map_err(|e| super::error_envelope(handler_request.id, e))?;
            if result.changed == Some(false)
                && result.resulting_registry_snapshot_ref.as_ref()
                    != Some(&input.registry_snapshot_ref)
            {
                return Err(super::error_envelope(
                    handler_request.id,
                    "Registry changed during no-op; do not adopt a different snapshot",
                ));
            }
            serde_json::to_value(result).map_err(|e| super::error_envelope(handler_request.id, e))
        },
    )
    .await
}
