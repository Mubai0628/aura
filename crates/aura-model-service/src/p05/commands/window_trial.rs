//! Publish a successful V2 trial through the existing ModelConfig journal.
use super::{prepared_snapshot, user_event};
use crate::p05::{
    P05Services,
    config_journal::{ModelConfigCommandPlan, ModelConfigJournalError},
};
use aura_domain_common::*;
use aura_model_config::{
    ModelConfigError, apply_runtime_context_window_candidates, endpoint_fingerprint,
};
use aura_model_domain::*;
use aura_persistence::{EventActor, ModelProjectionDraft};
use aura_persistence_v2::model::context_window::WindowCorrection;
use std::collections::BTreeMap;

pub(crate) fn basis_matches(
    record: &WindowCorrection,
    profile: &ModelProfileConfig,
    endpoint: &ModelEndpointConfig,
) -> Result<bool, ModelConfigError> {
    let source = &record.source_profile;
    Ok(source.id == profile.id
        && source.endpoint_id == profile.endpoint_id
        && source.provider_model_name == profile.provider_model_name
        && source.context.user_override_revision == profile.context.user_override_revision
        && source.context.runtime_effective_window_tokens
            == profile.context.runtime_effective_window_tokens
        && record.failed_snapshot.endpoint_fingerprint == endpoint_fingerprint(endpoint)?)
}

impl P05Services {
    pub fn window_trial_matches(
        &self,
        record: &WindowCorrection,
        profile: &ModelProfileConfig,
        endpoint: &ModelEndpointConfig,
    ) -> Result<bool, ModelConfigError> {
        basis_matches(record, profile, endpoint)
    }
    /// The caller has checked the completed request and its immutable trial in
    /// V2. A configuration change invalidates that trial's publication basis.
    pub fn commit_window_trial(
        &self,
        record: &WindowCorrection,
    ) -> Result<Option<ContextWindowResolution>, ModelConfigJournalError> {
        let ContextWindowResolution::AutomaticCandidateSelected {
            provider_error_ref,
            candidate_window_tokens,
            ..
        } = record.view.correction.resolution
        else {
            return Ok(None);
        };
        let completion = self.config_journal.execute(
            record.journal_command_id,
            CorrelationId::new(),
            |journal_id| {
                let current = self.model_config.snapshot()?;
                let profile = current.registry.models.get(&record.source_profile.id);
                let endpoint = profile.and_then(|p| current.registry.endpoints.get(&p.endpoint_id));
                let matches = match (profile, endpoint) {
                    (Some(p), Some(e)) => basis_matches(record, p, e)?,
                    _ => false,
                };
                if !matches {
                    return Ok(ModelConfigCommandPlan::NoChange {
                        success_result: serde_json::Value::Null,
                    });
                }
                let prepared = self.model_config.prepare_update(
                    journal_id.to_string(),
                    current.revision,
                    |registry| {
                        apply_runtime_context_window_candidates(
                            registry,
                            record.source_profile.id,
                            &record.candidate_windows,
                            provider_error_ref,
                            UtcTimestamp::now(),
                        )
                    },
                )?;
                let profile = prepared
                    .candidate_registry
                    .models
                    .get(&record.source_profile.id)
                    .ok_or(ModelConfigError::ProfileNotFound(record.source_profile.id))?;
                let basis = profile.context.runtime_correction_basis.clone().ok_or(
                    ModelConfigError::InternalInvariant("successful trial has no correction basis"),
                )?;
                let resolution = ContextWindowResolution::RuntimeCorrectionApplied {
                    failed_execution_snapshot_id: record.failed_snapshot.id,
                    model_profile_id: profile.id,
                    provider_error_ref,
                    corrected_window_tokens: candidate_window_tokens,
                    correction_basis: basis.clone(),
                    models_revision: prepared.candidate_revision,
                };
                let mut event = user_event(
                    "ModelContextWindowCorrected",
                    CorrelationId::new(),
                    None,
                    &ModelContextWindowCorrectedPayload {
                        model_profile_id: profile.id,
                        failed_execution_snapshot_id: record.failed_snapshot.id,
                        previous_effective_window_tokens: record
                            .source_profile
                            .context
                            .runtime_effective_window_tokens,
                        corrected_window_tokens: candidate_window_tokens,
                        correction_basis: basis,
                        models_revision: prepared.candidate_revision,
                    },
                )?;
                event.actor = EventActor::SystemRuntime;
                let frozen = prepared_snapshot(&prepared);
                let overrides = BTreeMap::new();
                let drafts = vec![
                    ModelProjectionDraft::ModelSettings(
                        self.projections.model_settings(&frozen, &overrides)?,
                    ),
                    ModelProjectionDraft::EndpointDetail(Box::new(
                        self.projections.endpoint_detail(
                            &frozen,
                            profile.endpoint_id,
                            &overrides,
                        )?,
                    )),
                    ModelProjectionDraft::ModelProfile(Box::new(
                        self.projections.model_profile(&frozen, profile.id, &overrides)?,
                    )),
                ];
                Ok(ModelConfigCommandPlan::Publish {
                    prepared_bundle: Box::new(prepared.prepared_bundle.ok_or(
                        ModelConfigError::InternalInvariant("successful trial lacks bundle"),
                    )?),
                    expected_models_revision: Revision::new(current.revision),
                    reserved_models_revision: Some(prepared.candidate_revision),
                    expected_aura_revision: None,
                    reserved_aura_revision: None,
                    credential_slot_creations: Vec::new(),
                    session_binding_clears: Vec::new(),
                    metadata_events: vec![event],
                    projection_drafts: drafts,
                    success_result: serde_json::to_value(Some(resolution))?,
                })
            },
        )?;
        self.persistence.publish_model_projection_changes(&completion.projection_changes)?;
        Ok(serde_json::from_value(completion.success_result)?)
    }
}
