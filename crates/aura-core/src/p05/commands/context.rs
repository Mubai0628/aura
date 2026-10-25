use std::collections::BTreeMap;

use aura_domain_common::{
    CommandInvocationId, CorrelationId, ModelProfileId, Revision, UtcTimestamp,
};
use aura_model_config::{
    ModelConfigError, reset_runtime_context_window_correction, set_user_context_window,
};
use aura_model_domain::{
    ContextWindowClearUserOverrideRequest, ContextWindowCorrectRequest,
    ContextWindowMutationResult, ContextWindowResetRuntimeCorrectionRequest,
    ModelProfileUpdatedPayload, model_ipc_method,
};
use aura_persistence::ModelProjectionDraft;
use aura_runtime_contract::IpcErrorEnvelope;
use serde_json::Value;

use crate::p05::{
    P05Services,
    config_journal::{ModelConfigCommandPlan, ModelConfigJournalError},
};

use super::{
    P05CommandDispatch, decode, journal_error, prepared_snapshot, profile_changed_fields,
    require_command_id, user_event,
};

#[derive(Clone, Copy)]
enum ContextWindowMutation {
    SetUserOverride { model_profile_id: ModelProfileId, window_tokens: Option<u64> },
    ResetRuntimeCorrection { model_profile_id: ModelProfileId },
}

impl ContextWindowMutation {
    const fn model_profile_id(self) -> ModelProfileId {
        match self {
            Self::SetUserOverride { model_profile_id, .. }
            | Self::ResetRuntimeCorrection { model_profile_id } => model_profile_id,
        }
    }

    fn apply(
        self,
        registry: &mut aura_model_domain::ModelRegistryConfig,
        updated_at: UtcTimestamp,
    ) -> Result<aura_model_domain::ContextWindowUpdateResult, ModelConfigError> {
        match self {
            Self::SetUserOverride { model_profile_id, window_tokens } => {
                set_user_context_window(registry, model_profile_id, window_tokens, updated_at)
            }
            Self::ResetRuntimeCorrection { model_profile_id } => {
                reset_runtime_context_window_correction(registry, model_profile_id, updated_at)
            }
        }
    }
}

impl P05Services {
    pub(super) fn dispatch_context_window_command(
        &self,
        method_name: &str,
        params: &Value,
        command_invocation_id: Option<CommandInvocationId>,
        correlation_id: CorrelationId,
    ) -> Result<P05CommandDispatch, IpcErrorEnvelope> {
        let (expected_revision, mutation) = match method_name {
            model_ipc_method::CONTEXT_WINDOW_CORRECT => {
                let request: ContextWindowCorrectRequest = decode(params, correlation_id)?;
                (
                    request.expected_revision,
                    ContextWindowMutation::SetUserOverride {
                        model_profile_id: request.model_profile_id,
                        window_tokens: Some(request.window_tokens),
                    },
                )
            }
            model_ipc_method::CONTEXT_WINDOW_CLEAR_USER_OVERRIDE => {
                let request: ContextWindowClearUserOverrideRequest =
                    decode(params, correlation_id)?;
                (
                    request.expected_revision,
                    ContextWindowMutation::SetUserOverride {
                        model_profile_id: request.model_profile_id,
                        window_tokens: None,
                    },
                )
            }
            model_ipc_method::CONTEXT_WINDOW_RESET_RUNTIME_CORRECTION => {
                let request: ContextWindowResetRuntimeCorrectionRequest =
                    decode(params, correlation_id)?;
                (
                    request.expected_revision,
                    ContextWindowMutation::ResetRuntimeCorrection {
                        model_profile_id: request.model_profile_id,
                    },
                )
            }
            _ => unreachable!("context-window dispatcher received an unrelated method"),
        };
        let command_invocation_id = require_command_id(command_invocation_id, correlation_id)?;
        let model_profile_id = mutation.model_profile_id();
        let completion = self
            .config_journal
            .execute(command_invocation_id, correlation_id, |journal_id| {
                let prepared = self.model_config.prepare_update(
                    journal_id.to_string(),
                    expected_revision.value(),
                    |registry| mutation.apply(registry, UtcTimestamp::now()),
                )?;
                let previous = prepared
                    .previous
                    .registry
                    .models
                    .get(&model_profile_id)
                    .cloned()
                    .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
                let profile = prepared
                    .candidate_registry
                    .models
                    .get(&model_profile_id)
                    .cloned()
                    .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
                let result = ContextWindowMutationResult {
                    changed: prepared.changed,
                    revision: prepared.candidate_revision,
                    value: prepared.value.clone(),
                };
                if !prepared.changed {
                    return Ok(ModelConfigCommandPlan::NoChange {
                        success_result: serde_json::to_value(result)?,
                    });
                }
                let endpoint = prepared
                    .candidate_registry
                    .endpoints
                    .get(&profile.endpoint_id)
                    .ok_or(ModelConfigError::EndpointNotFound(profile.id))?;
                let snapshot = prepared_snapshot(&prepared);
                let status_overrides = BTreeMap::new();
                let prepared_bundle =
                    prepared.prepared_bundle.ok_or(ModelConfigError::InternalInvariant(
                        "changed context-window mutation lacks a prepared bundle",
                    ))?;
                Ok(ModelConfigCommandPlan::Publish {
                    prepared_bundle: Box::new(prepared_bundle),
                    expected_models_revision: Revision::new(expected_revision.value()),
                    reserved_models_revision: Some(prepared.candidate_revision),
                    expected_aura_revision: None,
                    reserved_aura_revision: None,
                    credential_slot_creations: Vec::new(),
                    session_binding_clears: Vec::new(),
                    metadata_events: vec![user_event(
                        "ModelProfileUpdated",
                        correlation_id,
                        None,
                        &ModelProfileUpdatedPayload {
                            provider_id: endpoint.provider_id,
                            endpoint_id: endpoint.id,
                            profile: profile.clone(),
                            changed_fields: profile_changed_fields(&previous, &profile),
                            models_revision: prepared.candidate_revision,
                        },
                    )?],
                    projection_drafts: vec![
                        ModelProjectionDraft::ModelSettings(
                            self.projections
                                .model_settings(&snapshot, &status_overrides)
                                .map_err(ModelConfigJournalError::from)?,
                        ),
                        ModelProjectionDraft::EndpointDetail(Box::new(
                            self.projections
                                .endpoint_detail(&snapshot, endpoint.id, &status_overrides)
                                .map_err(ModelConfigJournalError::from)?,
                        )),
                        ModelProjectionDraft::ModelProfile(Box::new(
                            self.projections
                                .model_profile(&snapshot, profile.id, &status_overrides)
                                .map_err(ModelConfigJournalError::from)?,
                        )),
                    ],
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
