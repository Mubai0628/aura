use crate::{JsonRpcRequest, RuntimeServices, RuntimeState};
use aura_config::ConfigModule;
use aura_domain_common::jcs::canonical_sha256;
use aura_execution_contract::{BrokerEvidence, SideEffectStateV1};
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::{ResolvedCapability, ResourceDescriptor};
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) async fn dispatch_model(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<crate::p05::P05CommandDispatch, IpcErrorEnvelope> {
    let mutation = aura_ipc::routes::side_effecting(&request.method);
    let operation = super::operation(
        &request.method,
        &request.params,
        request.id,
        vec![if mutation {
            ResolvedCapability::ConfigurationWrite
        } else {
            ResolvedCapability::ConfigurationRead
        }],
        vec![ResourceDescriptor::ExternalService {
            service_id: "aura.model-config".into(),
            object_ref: None,
        }],
    )
    .map_err(|e| super::error_envelope(request.id, e))?;
    let settings = Arc::clone(&services._settings);
    let inspect: super::OwnedInspect = Arc::new(move || model_facts(&settings));
    let evidence_inspect = Arc::clone(&inspect);
    let evidence_persistence = Arc::clone(&services.persistence);
    let event_method = request.method.clone();
    let event_invocation = request.command_invocation_id;
    let evidence: super::OwnedEvidence<crate::p05::P05CommandDispatch> = Arc::new(
        move |dispatch: &aura_execution_contract::DispatchRequest,
              output: &Result<crate::p05::P05CommandDispatch, IpcErrorEnvelope>| {
            let before = dispatch.plan.prepared.preparation.clone();
            let after = evidence_inspect().unwrap_or(Value::Null);
            let journal = event_invocation
                .map(|id| evidence_persistence.model_config_mutation_journal_by_command(id))
                .transpose()?
                .flatten();
            let committed = journal.as_ref().is_some_and(|j| {
                matches!(
                    j.state,
                    aura_persistence::ModelConfigMutationJournalState::MetadataCommitted
                        | aura_persistence::ModelConfigMutationJournalState::CleanupPending
                        | aura_persistence::ModelConfigMutationJournalState::Completed
                )
            });
            let aborted = journal.as_ref().is_some_and(|j| {
                j.state == aura_persistence::ModelConfigMutationJournalState::Aborted
            });
            let explicit_noop =
                output.as_ref().is_ok_and(|o| o.value.get("changed") == Some(&Value::Bool(false)));
            let side_effect = if !mutation || explicit_noop && before == after {
                SideEffectStateV1::NoEffect
            } else if committed && after != Value::Null {
                SideEffectStateV1::FullyApplied
            } else if aborted && before == after {
                SideEffectStateV1::Compensated
            } else if output.is_err() && journal.is_none() && before == after {
                SideEffectStateV1::NoEffect
            } else {
                SideEffectStateV1::Unknown
            };
            let domain_journal = journal.map_or(Value::Null, |j| json!({
            "journal_id":j.journal_id,"state":j.state,
            "models_revision":j.payload.reserved_models_revision,"aura_revision":j.payload.reserved_aura_revision,
        }));
            Ok(BrokerEvidence {
                action_kind: event_method.clone(),
                request_fingerprint: dispatch.operation.fingerprint.0.clone(),
                before,
                after,
                domain_journal,
                side_effect,
            })
        },
    );
    let handler_request = request.clone();
    // The model authority is a real dependency of this broker route, so a
    // damaged `models.json` refuses it with a stable `Unavailable` code.
    let handler_p05 = services
        .p05()
        .map_err(|_| crate::MissingCapability::P05ModelAuthority.envelope(request.id))?
        .clone();
    let channel = crate::authenticated_configuration_channel_identity(state)
        .map_err(|e| super::error_envelope(request.id, e))?;
    super::run(
        state,
        &services.broker,
        operation,
        request.command_invocation_id.map_or_else(|| request.id.to_string(), |id| id.to_string()),
        request.command_invocation_id,
        inspect,
        evidence,
        move || async move {
            handler_p05
                .dispatch_main_command(
                    &handler_request.method,
                    &handler_request.params,
                    handler_request.command_invocation_id,
                    handler_request.id,
                    channel,
                )
                .await
                .expect("closed Main model route")
        },
    )
    .await
}

fn model_facts(settings: &aura_config::SettingsFacade) -> anyhow::Result<Value> {
    let models = settings.snapshot(ConfigModule::Models)?;
    let aura = settings.snapshot(ConfigModule::Aura)?;
    // Hash public config facts; never persist Secret-input payloads, WinCred
    // targets, catalog bodies, or generic response values in receipt evidence.
    Ok(
        json!({"models":canonical_sha256(&*models.document)?,"aura":canonical_sha256(&*aura.document)?}),
    )
}
