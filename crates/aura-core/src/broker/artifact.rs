use crate::{JsonRpcRequest, RuntimeServices, RuntimeState};
use aura_execution_contract::{BrokerEvidence, SideEffectStateV1};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::{ResolvedCapability, ResourceDescriptor};
use aura_workspace_domain::{ArtifactExportRequest, ArtifactExportTarget};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn dispatch_artifact(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<Value, IpcErrorEnvelope> {
    let input: ArtifactExportRequest = serde_json::from_value(request.params.clone())
        .map_err(|e| super::error_envelope(request.id, e))?;
    let resource = match &input.target {
        ArtifactExportTarget::Workspace { path } => ResourceDescriptor::WorkspacePath {
            workspace_id: input.workspace_id,
            canonical_relative_path: path.clone(),
            file_target: None,
        },
        ArtifactExportTarget::ExternalGrant { external_grant_id, .. } => {
            ResourceDescriptor::ExternalGrant { grant_id: *external_grant_id }
        }
    };
    let capability = match (&input.target, &input.expected) {
        (
            ArtifactExportTarget::Workspace { .. },
            aura_workspace_domain::FsExpectedState::Absent,
        ) => ResolvedCapability::WorkspaceFileCreate,
        (ArtifactExportTarget::Workspace { .. }, _) => ResolvedCapability::WorkspaceFileWrite,
        (_, aura_workspace_domain::FsExpectedState::Absent) => {
            ResolvedCapability::ExternalFileCreate
        }
        _ => ResolvedCapability::ExternalFileWrite,
    };
    let operation = super::operation(
        &request.method,
        &request.params,
        request.id,
        vec![capability],
        vec![resource],
    )
    .map_err(|e| super::error_envelope(request.id, e))?;
    // The security authority is a real dependency of artifact publication
    // evidence and dispatch, so a damaged `settings/security.toml` or `aura.toml`
    // refuses this route with a stable `Unavailable` code.
    let artifact_p04 = services
        .p04()
        .map_err(|_| crate::MissingCapability::P04SecurityAuthority.envelope(request.id))?
        .clone();
    let artifact_input = input.clone();
    let artifact_id = request.id;
    let inspect: super::OwnedInspect = Arc::new(move || {
        artifact_p04
            .broker_artifact_facts(&artifact_input, artifact_id)
            .map_err(|e| anyhow::anyhow!("{}", e.message))
    });
    let evidence_inspect = Arc::clone(&inspect);
    let evidence_persistence = services.persistence.clone();
    let event_method = request.method.clone();
    let evidence: super::OwnedEvidence<Value> = Arc::new(
        move |dispatch: &aura_execution_contract::DispatchRequest,
              output: &Result<Value, IpcErrorEnvelope>| {
            let before = dispatch.plan.prepared.preparation.clone();
            let after = evidence_inspect().unwrap_or(Value::Null);
            let journal = evidence_persistence.workspace_broker_evidence(artifact_id)?;
            let deferred =
                output.as_ref().is_ok_and(|v| v["disposition"] == "deferred_by_editor_activity");
            let verified = after != Value::Null
                && after.pointer("/target/hash") == before.get("artifact_hash");
            let compensated = journal.as_array().is_some_and(|entries| {
                !entries.is_empty() && entries.iter().all(|entry| entry["state"] == "compensated")
            });
            let side_effect = if deferred && before == after {
                SideEffectStateV1::NoEffect
            } else if output.is_err() && compensated {
                SideEffectStateV1::Compensated
            } else if output.is_ok() && verified {
                SideEffectStateV1::FullyApplied
            } else if output.is_err() && before == after {
                SideEffectStateV1::NoEffect
            } else {
                SideEffectStateV1::Unknown
            };
            Ok(BrokerEvidence {
                action_kind: event_method.clone(),
                request_fingerprint: dispatch.operation.fingerprint.0.clone(),
                before,
                after,
                domain_journal: json!({"workspace_journals":journal,"publication_returned":output.is_ok(),"postcondition_verified":verified}),
                side_effect,
            })
        },
    );
    let handler_request = request.clone();
    let handler_p04 = services
        .p04()
        .map_err(|_| crate::MissingCapability::P04SecurityAuthority.envelope(request.id))?
        .clone();
    super::run(
        state,
        &services.broker,
        operation,
        request.command_invocation_id.map_or_else(|| request.id.to_string(), |id| id.to_string()),
        request.command_invocation_id,
        inspect,
        evidence,
        move || async move {
            handler_p04
                .dispatch_main_command(
                    &handler_request.method,
                    &handler_request.params,
                    handler_request.id,
                )
                .await
                .expect("artifact export route")
        },
    )
    .await
}
