use aura_domain_common::{ArtifactId, ModelExecutionSnapshotId, ModelProfileId, Revision};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

use crate::{ModelProfileConfig, RuntimeWindowCorrectionBasis};

pub const CONTEXT_WINDOW_AUTOMATIC_CANDIDATES: [u64; 4] = [256_000, 128_000, 64_000, 32_000];

#[must_use]
pub fn next_context_window_candidate(current_tokens: u64) -> Option<u64> {
    CONTEXT_WINDOW_AUTOMATIC_CANDIDATES.into_iter().find(|candidate| *candidate < current_tokens)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindowUnit {
    Tokens,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowTokenInputContract {
    pub unit: ContextWindowUnit,
    #[schemars(range(min = 1))]
    pub minimum_tokens: u64,
    pub maximum_tokens: Option<u64>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextWindowTokenInputContractError {
    #[error("context-window minimum must be at least one token")]
    InvalidMinimum,
    #[error("context-window maximum must not be smaller than the minimum")]
    InvalidMaximum,
}

impl ContextWindowTokenInputContract {
    pub fn new(
        minimum_tokens: u64,
        maximum_tokens: Option<u64>,
    ) -> Result<Self, ContextWindowTokenInputContractError> {
        let contract = Self { unit: ContextWindowUnit::Tokens, minimum_tokens, maximum_tokens };
        contract.validate()?;
        Ok(contract)
    }

    pub fn validate(&self) -> Result<(), ContextWindowTokenInputContractError> {
        if self.minimum_tokens == 0 {
            return Err(ContextWindowTokenInputContractError::InvalidMinimum);
        }
        if self.maximum_tokens.is_some_and(|maximum| maximum < self.minimum_tokens) {
            return Err(ContextWindowTokenInputContractError::InvalidMaximum);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ContextWindowTokenInputContract {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireContract {
            unit: ContextWindowUnit,
            minimum_tokens: u64,
            maximum_tokens: Option<u64>,
        }

        let wire = WireContract::deserialize(deserializer)?;
        let contract = Self {
            unit: wire.unit,
            minimum_tokens: wire.minimum_tokens,
            maximum_tokens: wire.maximum_tokens,
        };
        contract.validate().map_err(D::Error::custom)?;
        Ok(contract)
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextWindowResolution {
    P06RebuildRequired {
        failed_execution_snapshot_id: ModelExecutionSnapshotId,
        model_profile_id: ModelProfileId,
        provider_error_ref: ArtifactId,
        local_estimated_input_tokens: u64,
        current_effective_window_tokens: u64,
    },
    UserCorrectionRequired {
        failed_execution_snapshot_id: ModelExecutionSnapshotId,
        model_profile: ModelProfileConfig,
        provider_error_ref: ArtifactId,
        local_estimated_input_tokens: u64,
        current_effective_window_tokens: u64,
        input_contract: ContextWindowTokenInputContract,
    },
    AutomaticCandidateSelected {
        failed_execution_snapshot_id: ModelExecutionSnapshotId,
        model_profile_id: ModelProfileId,
        provider_error_ref: ArtifactId,
        candidate_window_tokens: u64,
        remaining_candidates: Vec<u64>,
    },
    RuntimeCorrectionApplied {
        failed_execution_snapshot_id: ModelExecutionSnapshotId,
        model_profile_id: ModelProfileId,
        provider_error_ref: ArtifactId,
        corrected_window_tokens: u64,
        correction_basis: RuntimeWindowCorrectionBasis,
        models_revision: Revision,
    },
    AutomaticCandidatesExhausted {
        failed_execution_snapshot_id: ModelExecutionSnapshotId,
        model_profile: ModelProfileConfig,
        provider_error_ref: ArtifactId,
        local_estimated_input_tokens: u64,
        current_effective_window_tokens: u64,
        input_contract: ContextWindowTokenInputContract,
    },
}

#[must_use]
pub fn context_window_rebuild_threshold(current_effective_window_tokens: u64) -> u64 {
    current_effective_window_tokens.saturating_mul(4).div_ceil(5)
}

#[must_use]
pub fn context_window_requires_rebuild(
    local_estimated_input_tokens: u64,
    current_effective_window_tokens: u64,
) -> bool {
    local_estimated_input_tokens
        >= context_window_rebuild_threshold(current_effective_window_tokens)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CONTEXT_WINDOW_AUTOMATIC_CANDIDATES, ContextWindowTokenInputContract,
        context_window_rebuild_threshold, context_window_requires_rebuild,
        next_context_window_candidate,
    };

    /// The frozen ladder is `256000 → 128000 → 64000 → 32000`, each step is
    /// strictly smaller than the current window, and 32000 has no successor.
    /// That empty successor is what turns the chain into
    /// `automatic_candidates_exhausted` in the Store.
    #[test]
    fn automatic_candidate_ladder_is_strictly_descending_and_finite() {
        assert_eq!(CONTEXT_WINDOW_AUTOMATIC_CANDIDATES, [256_000, 128_000, 64_000, 32_000]);
        let mut window = 256_000;
        let mut ladder = Vec::new();
        while let Some(candidate) = next_context_window_candidate(window) {
            assert!(candidate < window, "candidate {candidate} must be below {window}");
            ladder.push(candidate);
            window = candidate;
        }
        assert_eq!(ladder, [128_000, 64_000, 32_000]);
        assert_eq!(next_context_window_candidate(32_000), None);
        assert_eq!(next_context_window_candidate(1), None);
    }

    #[test]
    fn frozen_eighty_percent_boundary_uses_integer_ceiling() {
        assert_eq!(context_window_rebuild_threshold(101), 81);
        assert!(!context_window_requires_rebuild(80, 101));
        assert!(context_window_requires_rebuild(81, 101));
        assert_eq!(context_window_rebuild_threshold(u64::MAX), u64::MAX.div_ceil(5));
    }

    #[test]
    fn token_input_contract_rejects_zero_and_inverted_ranges() {
        assert!(ContextWindowTokenInputContract::new(0, None).is_err());
        assert!(ContextWindowTokenInputContract::new(10, Some(9)).is_err());
        assert!(ContextWindowTokenInputContract::new(10, Some(10)).is_ok());
        assert!(
            serde_json::from_value::<ContextWindowTokenInputContract>(json!({
                "unit": "tokens",
                "minimum_tokens": 0,
                "maximum_tokens": null
            }))
            .is_err()
        );
    }
}
