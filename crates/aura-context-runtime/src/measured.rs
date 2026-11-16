//! Calibrates recurring parts against comparable, provider-reported requests.
use std::collections::BTreeMap;

use aura_context_domain::{
    ContextItemEstimate, PROVIDER_MEASURED_ESTIMATOR_ID, TokenEstimateExactness,
};
use aura_domain_common::ContentHash;

use crate::estimator::{NATIVE_REASONING_ESTIMATOR_ID, REFERENCE_BPE_ESTIMATOR_ID};

/// Usage and part estimates from one previous request to the same model and endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasuredRequest {
    reported_text_tokens: u64,
    estimated_text_tokens: u64,
    text_parts: BTreeMap<String, (u64, u64)>,
}

impl MeasuredRequest {
    #[must_use]
    pub fn from_report(
        input_tokens: u64,
        cached_tokens: u64,
        parts: &[ContextItemEstimate],
    ) -> Option<Self> {
        if input_tokens == 0 || input_tokens < cached_tokens {
            return None;
        }
        let mut fixed_tokens = 0_u64;
        let mut estimated_text_tokens = 0_u64;
        let mut text_parts = BTreeMap::new();
        for part in parts {
            let identity = &part.estimator;
            match (identity.estimator_id.as_str(), identity.estimator_version) {
                (REFERENCE_BPE_ESTIMATOR_ID, 1) | (PROVIDER_MEASURED_ESTIMATOR_ID, 2) => {
                    estimated_text_tokens =
                        estimated_text_tokens.checked_add(part.estimated_tokens)?;
                    text_parts.insert(
                        part.content_hash.to_string(),
                        (part.byte_length, part.estimated_tokens),
                    );
                }
                (NATIVE_REASONING_ESTIMATOR_ID, 1) => {
                    fixed_tokens = fixed_tokens.checked_add(part.estimated_tokens)?;
                }
                _ if matches!(
                    identity.exactness,
                    TokenEstimateExactness::Exact | TokenEstimateExactness::UpperBound
                ) =>
                {
                    fixed_tokens = fixed_tokens.checked_add(part.estimated_tokens)?;
                }
                // Snapshots produced by a different estimate may describe different bytes.
                _ => return None,
            }
        }
        let reported_text_tokens = input_tokens.checked_sub(fixed_tokens)?;
        // Some gateways report just the uncached portion or omit entire request components.
        // Compare token estimates on equivalent material. The wire size of opaque data says
        // nothing about completeness.
        if estimated_text_tokens == 0
            || reported_text_tokens.abs_diff(estimated_text_tokens) > estimated_text_tokens / 2
        {
            return None;
        }
        Some(Self { reported_text_tokens, estimated_text_tokens, text_parts })
    }

    /// Corrects the estimate of a recurring part from its previous estimate. Starting from
    /// that estimate makes another correction converge instead of scaling the reference count
    /// afresh and undoing the previous adjustment. New parts use the local reference count.
    #[must_use]
    pub fn sent_again(&self, hash: &ContentHash, byte_length: u64) -> Option<u64> {
        let &(prior_bytes, prior_tokens) = self.text_parts.get(&hash.to_string())?;
        if byte_length != prior_bytes {
            return None;
        }
        let scaled = u128::from(prior_tokens) * u128::from(self.reported_text_tokens);
        u64::try_from(scaled.div_ceil(u128::from(self.estimated_text_tokens))).ok()
    }
}

#[cfg(test)]
mod tests {
    use aura_context_domain::TokenEstimatorIdentity;

    use super::*;

    fn part(text: &str, bytes: u64, tokens: u64, id: &str, version: u32) -> ContextItemEstimate {
        ContextItemEstimate {
            ordinal: 1,
            content_hash: ContentHash::sha256(text.as_bytes()),
            byte_length: bytes,
            estimated_tokens: tokens,
            estimator: TokenEstimatorIdentity {
                estimator_id: id.to_owned(),
                estimator_version: version,
                exactness: TokenEstimateExactness::Approximate,
            },
        }
    }

    #[test]
    fn corrects_recurring_text_without_scaling_opaque_records() {
        let mut text = part("recurring", 500_000, 290_000, REFERENCE_BPE_ESTIMATOR_ID, 1);
        let reasoning = part("opaque", 3_924_258, 145_797, NATIVE_REASONING_ESTIMATOR_ID, 1);
        let usage = 442_347;
        let measurement =
            MeasuredRequest::from_report(usage, 441_216, &[text.clone(), reasoning.clone()])
                .expect("valid usage for a large native continuation");
        assert_eq!(measurement.sent_again(&text.content_hash, text.byte_length), Some(296_550));
        assert_eq!(measurement.sent_again(&reasoning.content_hash, reasoning.byte_length), None);
        assert_eq!(measurement.sent_again(&text.content_hash, 10), None);

        text.estimated_tokens = 296_550;
        text.estimator.estimator_id = PROVIDER_MEASURED_ESTIMATOR_ID.to_owned();
        text.estimator.estimator_version = 2;
        let next = MeasuredRequest::from_report(usage, 441_216, &[text.clone(), reasoning])
            .expect("comparable prior calibration");
        assert_eq!(next.sent_again(&text.content_hash, text.byte_length), Some(296_550));
    }

    #[test]
    fn an_incomplete_report_or_legacy_byte_estimate_cannot_calibrate_text() {
        let text = part("recurring", 10_000, 3_200, REFERENCE_BPE_ESTIMATOR_ID, 1);
        assert!(MeasuredRequest::from_report(900, 2_500, std::slice::from_ref(&text)).is_none());
        assert!(MeasuredRequest::from_report(400, 0, &[text]).is_none());
        let legacy = part("recurring", 10_000, 3_200, PROVIDER_MEASURED_ESTIMATOR_ID, 1);
        assert!(MeasuredRequest::from_report(3_200, 0, &[legacy]).is_none());
    }
}
