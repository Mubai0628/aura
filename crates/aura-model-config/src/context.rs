use aura_domain_common::{ArtifactId, ModelProfileId, UtcTimestamp};
use aura_model_domain::{
    ContextWindowUpdateResult, ModelRegistryConfig, RuntimeWindowCorrectionBasis,
};

use crate::{ModelConfigError, endpoint_fingerprint, model_name_hash};

pub use aura_model_domain::{CONTEXT_WINDOW_AUTOMATIC_CANDIDATES, next_context_window_candidate};

pub fn set_user_context_window(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    window_tokens: Option<u64>,
    updated_at: UtcTimestamp,
) -> Result<ContextWindowUpdateResult, ModelConfigError> {
    if window_tokens == Some(0) {
        return Err(ModelConfigError::InvalidContextWindow);
    }
    let model = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    let expected_effective = model
        .context
        .user_override_window_tokens
        .or(model.context.catalog_window_tokens)
        .unwrap_or(aura_model_domain::DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS);
    if model.context.user_override_window_tokens == window_tokens
        && model.context.runtime_corrected_window_tokens.is_none()
        && model.context.runtime_correction_basis.is_none()
        && model.context.runtime_effective_window_tokens == expected_effective
    {
        return Ok(update_result(model));
    }
    model.context.user_override_revision = model
        .context
        .user_override_revision
        .checked_add(1)
        .ok_or(ModelConfigError::UserOverrideRevisionExhausted)?;
    model.context.user_override_window_tokens = window_tokens;
    model.context.runtime_corrected_window_tokens = None;
    model.context.runtime_correction_basis = None;
    model.context.runtime_effective_window_tokens = model.context.effective_window_tokens(false);
    model.updated_at = updated_at;
    Ok(update_result(model))
}

pub fn apply_runtime_context_window_correction(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    corrected_window_tokens: u64,
    provider_error_ref: ArtifactId,
    corrected_at: UtcTimestamp,
) -> Result<ContextWindowUpdateResult, ModelConfigError> {
    apply_runtime_context_window_candidates(
        registry,
        model_profile_id,
        &[corrected_window_tokens],
        provider_error_ref,
        corrected_at,
    )
}

/// The owner supplies the durable, ordered trial chain after its last request
/// succeeds. Failed candidates never become intermediate configuration writes.
pub fn apply_runtime_context_window_candidates(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    attempted_windows: &[u64],
    provider_error_ref: ArtifactId,
    corrected_at: UtcTimestamp,
) -> Result<ContextWindowUpdateResult, ModelConfigError> {
    let model = registry
        .models
        .get(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    let mut current = model.context.runtime_effective_window_tokens;
    if attempted_windows.is_empty() {
        return Err(ModelConfigError::NoSmallerContextWindowCandidate);
    }
    for candidate in attempted_windows {
        let expected_candidate = next_context_window_candidate(current)
            .ok_or(ModelConfigError::NoSmallerContextWindowCandidate)?;
        if *candidate != expected_candidate {
            return Err(ModelConfigError::InvalidRuntimeContextWindowCandidate {
                expected: expected_candidate,
                actual: *candidate,
            });
        }
        current = *candidate;
    }
    let corrected_window_tokens = current;
    let endpoint = registry
        .endpoints
        .get(&model.endpoint_id)
        .ok_or(ModelConfigError::EndpointIdNotFound(model.endpoint_id))?;
    let basis = RuntimeWindowCorrectionBasis {
        endpoint_fingerprint: endpoint_fingerprint(endpoint)?,
        model_name_hash: model_name_hash(&model.provider_model_name),
        user_override_revision: model.context.user_override_revision,
        provider_error_ref,
        corrected_at,
    };
    let model = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    model.context.runtime_corrected_window_tokens = Some(corrected_window_tokens);
    model.context.runtime_correction_basis = Some(basis);
    model.context.runtime_effective_window_tokens = corrected_window_tokens;
    model.updated_at = corrected_at;
    Ok(update_result(model))
}

pub fn reset_runtime_context_window_correction(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    updated_at: UtcTimestamp,
) -> Result<ContextWindowUpdateResult, ModelConfigError> {
    let model = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    let expected_effective = model.context.effective_window_tokens(false);
    if model.context.runtime_corrected_window_tokens.is_none()
        && model.context.runtime_correction_basis.is_none()
        && model.context.runtime_effective_window_tokens == expected_effective
    {
        return Ok(update_result(model));
    }
    model.context.runtime_corrected_window_tokens = None;
    model.context.runtime_correction_basis = None;
    model.context.runtime_effective_window_tokens = model.context.effective_window_tokens(false);
    model.updated_at = updated_at;
    Ok(update_result(model))
}

pub fn set_historical_snapshot_max_tokens(
    registry: &mut ModelRegistryConfig,
    model_profile_id: ModelProfileId,
    max_tokens: u64,
    updated_at: UtcTimestamp,
) -> Result<ContextWindowUpdateResult, ModelConfigError> {
    if max_tokens == 0 {
        return Err(ModelConfigError::InvalidHistoricalSnapshotLimit);
    }
    let model = registry
        .models
        .get_mut(&model_profile_id)
        .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
    if model.context.historical_snapshot_max_tokens == max_tokens {
        return Ok(update_result(model));
    }
    model.context.historical_snapshot_max_tokens = max_tokens;
    model.updated_at = updated_at;
    Ok(update_result(model))
}

fn update_result(model: &aura_model_domain::ModelProfileConfig) -> ContextWindowUpdateResult {
    ContextWindowUpdateResult {
        model_profile_id: model.id,
        runtime_effective_window_tokens: model.context.runtime_effective_window_tokens,
        user_override_revision: model.context.user_override_revision,
        experience_warning: model.context.runtime_effective_window_tokens <= 32_000,
    }
}
