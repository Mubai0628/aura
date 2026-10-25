//! A
//! provider's default connection and its backups, saved at the revision the settings page showed.
use std::collections::BTreeMap;

use aura_domain_common::{CommandInvocationId, CorrelationId, Revision, UtcTimestamp};
use aura_model_config::{ModelConfigError, set_provider_connections};
use aura_model_domain::{
    ModelProviderConnectionsChangedPayload, ModelProviderConnectionsRequest,
    ModelProviderMutationResult,
};
use aura_persistence::ModelProjectionDraft;
use aura_runtime_contract::IpcErrorEnvelope;
use serde_json::Value;

use crate::p05::{
    P05Services,
    config_journal::{ModelConfigCommandPlan, ModelConfigJournalError},
};

use super::{
    P05CommandDispatch, decode, journal_error, prepared_snapshot, require_command_id, user_event,
};

impl P05Services {
    pub(super) fn dispatch_provider_connections(
        &self,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let request: ModelProviderConnectionsRequest = decode(params, correlation_id)?;
        let command_invocation_id = require_command_id(command_invocation_id, correlation_id)?;
        let completion = self
            .config_journal
            .execute(command_invocation_id, correlation_id, |journal_id| {
                let prepared = self.model_config.prepare_update(
                    journal_id.to_string(),
                    request.expected_revision.value(),
                    |registry| {
                        set_provider_connections(
                            registry,
                            request.provider_id,
                            request.default_endpoint_id,
                            request.backup_endpoint_ids.clone(),
                            UtcTimestamp::now(),
                        )
                    },
                )?;
                let previous = prepared
                    .previous
                    .registry
                    .providers
                    .get(&request.provider_id)
                    .cloned()
                    .ok_or(ModelConfigError::ProviderNotFoundById(request.provider_id))?;
                let provider = prepared
                    .candidate_registry
                    .providers
                    .get(&request.provider_id)
                    .cloned()
                    .ok_or(ModelConfigError::ProviderNotFoundById(request.provider_id))?;
                let result = ModelProviderMutationResult {
                    changed: prepared.changed,
                    revision: prepared.candidate_revision,
                    value: provider.clone(),
                };
                if !prepared.changed {
                    return Ok(ModelConfigCommandPlan::NoChange {
                        success_result: serde_json::to_value(result)?,
                    });
                }
                let model_settings = self
                    .projections
                    .model_settings(&prepared_snapshot(&prepared), &BTreeMap::new())
                    .map_err(ModelConfigJournalError::from)?;
                let prepared_bundle =
                    prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                        "changed Provider mutation lacks a prepared bundle",
                    ))?;
                Ok(ModelConfigCommandPlan::Publish {
                    prepared_bundle: Box::new(prepared_bundle),
                    expected_models_revision: Revision::new(request.expected_revision.value()),
                    reserved_models_revision: Some(prepared.candidate_revision),
                    expected_aura_revision: None,
                    reserved_aura_revision: None,
                    credential_slot_creations: Vec::new(),
                    session_binding_clears: Vec::new(),
                    metadata_events: vec![user_event(
                        "ModelProviderConnectionsChanged",
                        correlation_id,
                        None,
                        &ModelProviderConnectionsChangedPayload {
                            provider_id: request.provider_id,
                            previous_default_endpoint_id: previous.default_endpoint_id,
                            default_endpoint_id: provider.default_endpoint_id,
                            previous_backup_endpoint_ids: previous.backup_endpoint_ids,
                            backup_endpoint_ids: provider.backup_endpoint_ids.clone(),
                            models_revision: prepared.candidate_revision,
                        },
                    )?],
                    projection_drafts: vec![ModelProjectionDraft::ModelSettings(model_settings)],
                    success_result: serde_json::to_value(result)?,
                })
            })
            .map_err(|error| journal_error(error, correlation_id))?;
        Ok(P05CommandDispatch {
            value: completion.success_result,
            projection_changes: completion.projection_changes,
        })
    }
}
