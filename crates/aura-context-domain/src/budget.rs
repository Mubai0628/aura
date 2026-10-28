use aura_domain_common::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CONTEXT_BUDGET_POLICY_V1: &str = "context_budget_policy_v1";
pub const UTF8_BYTE_UPPER_BOUND_ESTIMATOR_ID: &str = "utf8_byte_upper_bound_v1";
/// A part the model was sent before, counted at the rate the provider reported for that
/// request.
pub const PROVIDER_MEASURED_ESTIMATOR_ID: &str = "provider_measured_v1";
/// A part of a request no provider has counted for its model yet, counted by character class.
pub const CHARACTER_CLASS_ESTIMATOR_ID: &str = "character_class_v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenEstimateExactness {
    Exact,
    UpperBound,
    /// Scaled from the provider's own count of the previous request of the same model,
    /// with a margin; neither exact nor a strict bound.
    Calibrated,
    /// A rule of thumb per character class, before any provider count of the model: it can be
    /// low, so a provider that refuses such a request as too long is answered by rebuilding the
    /// request, not by doubting the model's window.
    Approximate,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TokenEstimatorIdentity {
    pub estimator_id: String,
    pub estimator_version: u32,
    pub exactness: TokenEstimateExactness,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextItemEstimate {
    pub ordinal: u32,
    pub content_hash: ContentHash,
    pub byte_length: u64,
    pub estimated_tokens: u64,
    pub estimator: TokenEstimatorIdentity,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBudgetSnapshot {
    pub policy: String,
    pub runtime_effective_window_tokens: u64,
    pub estimated_input_tokens: u64,
    pub effective_max_output_tokens: u64,
    pub output_reserve_tokens: u64,
    pub safety_reserve_tokens: u64,
    pub request_input_budget_tokens: u64,
    pub item_estimates: Vec<ContextItemEstimate>,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ContextBudgetError {
    #[error("registered context estimator identity is invalid")]
    InvalidEstimatorIdentity,
    #[error("context budget item content hash does not match canonical content")]
    ContentHashMismatch,
    #[error("context budget item ordinals must be unique and contiguous from one")]
    InvalidItemOrder,
    #[error("context budget arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("model output reserve must be resolved before context preparation")]
    MissingOutputReserve,
    #[error("native model attachment has no registered provider/model/media estimator")]
    NativeAttachmentEstimatorMissing,
    #[error("context input exceeds the request budget")]
    RequestBudgetExceeded,
}

#[must_use]
pub fn utf8_byte_upper_bound(text: &str) -> u64 {
    u64::try_from(text.len()).unwrap_or(u64::MAX)
}

pub fn safety_reserve_v1(runtime_effective_window_tokens: u64) -> Result<u64, ContextBudgetError> {
    let five_percent = runtime_effective_window_tokens
        .checked_mul(5)
        .ok_or(ContextBudgetError::ArithmeticOverflow)?
        .checked_add(99)
        .ok_or(ContextBudgetError::ArithmeticOverflow)?
        / 100;
    Ok(4_096_u64.max(five_percent))
}

pub fn request_input_budget_v1(
    runtime_effective_window_tokens: u64,
    effective_max_output_tokens: u64,
) -> Result<(u64, u64), ContextBudgetError> {
    if effective_max_output_tokens == 0 {
        return Err(ContextBudgetError::MissingOutputReserve);
    }
    let safety_reserve_tokens = safety_reserve_v1(runtime_effective_window_tokens)?;
    let request_input_budget_tokens = runtime_effective_window_tokens
        .checked_sub(effective_max_output_tokens)
        .and_then(|value| value.checked_sub(safety_reserve_tokens))
        .ok_or(ContextBudgetError::RequestBudgetExceeded)?;
    Ok((safety_reserve_tokens, request_input_budget_tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_uses_five_percent_or_4096_and_checked_subtraction() {
        assert_eq!(safety_reserve_v1(32_768).expect("reserve"), 4_096);
        assert_eq!(safety_reserve_v1(256_000).expect("reserve"), 12_800);
        assert_eq!(safety_reserve_v1(1_000_001).expect("reserve"), 50_001);
        assert_eq!(request_input_budget_v1(32_768, 4_096).expect("budget"), (4_096, 24_576));
        assert_eq!(
            request_input_budget_v1(4_096, 4_096),
            Err(ContextBudgetError::RequestBudgetExceeded)
        );
    }

    #[test]
    fn utf8_fallback_counts_canonical_bytes_not_characters() {
        assert_eq!(utf8_byte_upper_bound("hello"), 5);
        assert_eq!(utf8_byte_upper_bound("你好"), 6);
        assert_eq!(utf8_byte_upper_bound("😀"), 4);
    }
}
