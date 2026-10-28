//! Counting a request close to what its provider will count.
//!
//! Without a tokenizer for the model there is only a rule of thumb per character class
//! ([`crate::character_class_tokens`]), which can be low. A conversation repeats almost all of its
//! previous request, and the provider said how many tokens that request really was. A part sent
//! again is counted at that rate, with a margin; a part the model has not seen at half again that
//! rate, never below half a token a byte. The first request, with no count yet, is counted by the
//! rule. Only the same model's previous request is used, and only a plausible count: another
//! model's tokenizer, or a count too small to be the whole request (a proxy that reports only
//! uncached tokens), leaves every part to the rule.
use std::collections::BTreeSet;

use aura_domain_common::ContentHash;

/// Fewer than one token per ten bytes is not a count of a whole request.
const PLAUSIBLE_BYTES_PER_TOKEN: u64 = 10;
/// A part counted at the provider's rate gets a tenth more.
const MARGIN_PERCENT: u64 = 110;
/// A part the model has not seen gets half again the rate of the parts it has.
const NEW_PART_PERCENT: u64 = 150;

/// The previous request of the same model: what its provider counted and its text parts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasuredRequest {
    /// The provider's count with the margin, never above the request's bytes.
    tokens: u64,
    bytes: u64,
    /// The parts' hashes, as text: a hash orders only by its spelling.
    parts: BTreeSet<String>,
}

impl MeasuredRequest {
    /// The measurement, when the provider's count is plausible for parts of these sizes.
    #[must_use]
    pub fn new(
        input_tokens: u64,
        parts: impl IntoIterator<Item = (ContentHash, u64)>,
    ) -> Option<Self> {
        let mut bytes = 0_u64;
        let mut hashes = BTreeSet::new();
        for (hash, length) in parts {
            bytes = bytes.checked_add(length)?;
            hashes.insert(hash.to_string());
        }
        if bytes == 0 || input_tokens.checked_mul(PLAUSIBLE_BYTES_PER_TOKEN)? < bytes {
            return None;
        }
        let tokens = input_tokens.checked_mul(MARGIN_PERCENT)?.div_ceil(100).min(bytes);
        Some(Self { tokens, bytes, parts: hashes })
    }

    /// The tokens of a part the model was sent before, or `None` for one it has not seen.
    #[must_use]
    pub fn sent_again(&self, hash: &ContentHash, byte_length: u64) -> Option<u64> {
        if !self.parts.contains(&hash.to_string()) {
            return None;
        }
        let scaled = u128::from(byte_length) * u128::from(self.tokens);
        u64::try_from(scaled.div_ceil(u128::from(self.bytes))).ok()
    }

    /// The tokens of a part the model
    /// has not seen - half again the provider's rate, never below half a token a byte (Chinese
    /// runs about a third to a half, prose and code about a quarter) and never above its bytes.
    #[must_use]
    pub fn new_part(&self, byte_length: u64) -> u64 {
        let scaled =
            u128::from(byte_length) * u128::from(self.tokens) * u128::from(NEW_PART_PERCENT);
        let rated =
            u64::try_from(scaled.div_ceil(u128::from(self.bytes) * 100)).unwrap_or(u64::MAX);
        rated.max(byte_length.div_ceil(2)).min(byte_length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(text: &str) -> ContentHash {
        ContentHash::sha256(text.as_bytes())
    }

    #[test]
    fn a_part_sent_again_is_counted_at_the_providers_rate_with_a_margin() {
        let measured = MeasuredRequest::new(3_000, [(hash("a"), 8_000), (hash("b"), 2_000)])
            .expect("a plausible count");
        // 3,000 tokens for 10,000 bytes, plus a tenth: 0.33 tokens a byte.
        assert_eq!(measured.sent_again(&hash("a"), 8_000), Some(2_640));
        assert_eq!(measured.sent_again(&hash("c"), 8_000), None, "a new part keeps its bytes");
    }

    /// A part the model has not seen, at half again the provider's rate, at least half a
    /// token a byte and at most its bytes.
    #[test]
    fn a_new_part_is_counted_at_half_again_the_providers_rate() {
        // 2,500 tokens for 10,000 bytes, plus a tenth: 0.275 a byte; half again is under 0.5.
        let sparse = MeasuredRequest::new(2_500, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(sparse.new_part(8_000), 4_000, "half a token a byte");
        // 5,000 tokens for 10,000 bytes, plus a tenth: 0.55; half again is 0.825.
        let dense = MeasuredRequest::new(5_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(dense.new_part(8_000), 6_600);
        let densest = MeasuredRequest::new(9_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(densest.new_part(8_000), 8_000, "never above the bytes");
    }

    #[test]
    fn a_count_above_the_bytes_or_too_small_to_be_the_request_is_not_used() {
        let dense = MeasuredRequest::new(50_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(dense.sent_again(&hash("a"), 10_000), Some(10_000), "never above the bytes");
        assert_eq!(MeasuredRequest::new(20, [(hash("a"), 50_000)]), None);
        assert_eq!(MeasuredRequest::new(0, []), None);
    }
}
