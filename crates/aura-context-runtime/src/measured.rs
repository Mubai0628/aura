//! Counting a request close to what its provider will count.
//!
//! Without a tokenizer for the model there is only a rule of thumb per character class
//! ([`crate::character_class_tokens`]), which can be low. A conversation repeats almost all of its
//! previous request, and the provider said how many tokens that request really was. A part sent
//! again is counted at that rate. A part the model has not seen is counted by what it is
//! ([`crate::BudgetTextKind`]): the provider's own record of an answer at the same rate, text by
//! character class or at the rate where that is higher, and text that carries such records
//! serialized at half again the rate, never below half a token a byte. The first request, with no
//! count yet, is counted by the rule. Only the same model's previous request is used, and only a
//! plausible count: another model's tokenizer, or a count too small to be the whole request (a
//! proxy that reports only uncached tokens), leaves every part to the rule.
use std::collections::BTreeSet;

use aura_domain_common::ContentHash;

/// Fewer than one token per ten bytes is not a count of a whole request.
const PLAUSIBLE_BYTES_PER_TOKEN: u64 = 10;
/// Text that carries a provider's records serialized gets half again the provider's rate.
const SERIALIZED_PERCENT: u64 = 150;

/// The previous request of the same model: what its provider counted and its text parts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasuredRequest {
    /// The provider's count, never above the request's bytes.
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
        Some(Self { tokens: input_tokens.min(bytes), bytes, parts: hashes })
    }

    /// The tokens of a part the model was sent before, or `None` for one it has not seen.
    #[must_use]
    pub fn sent_again(&self, hash: &ContentHash, byte_length: u64) -> Option<u64> {
        self.parts.contains(&hash.to_string()).then(|| self.at_rate(byte_length))
    }

    /// The tokens of these bytes at the provider's rate: a part sent again, or the provider's own
    /// record of an answer, which it counts natively rather than as its bytes - encrypted
    /// reasoning is most of a reasoning model's request and counts far below a token a byte.
    #[must_use]
    pub fn at_rate(&self, byte_length: u64) -> u64 {
        let scaled = u128::from(byte_length) * u128::from(self.tokens);
        u64::try_from(scaled.div_ceil(u128::from(self.bytes))).unwrap_or(u64::MAX)
    }

    /// The tokens of text the model has not seen: by character class, or at the provider's rate
    /// when that is higher, as a tokenizer denser than the rule shows it.
    #[must_use]
    pub fn new_text(&self, text: &str) -> u64 {
        let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
        crate::character_class_tokens(text).max(self.at_rate(bytes)).min(bytes)
    }

    /// The tokens of text that carries a provider's records serialized, as a summary request's
    /// sources do: read as text, an encoding counts densely and not at the provider's rate for
    /// the record. Half again the provider's rate, never below half a token a byte and never
    /// above the bytes.
    #[must_use]
    pub fn serialized(&self, byte_length: u64) -> u64 {
        let scaled =
            u128::from(byte_length) * u128::from(self.tokens) * u128::from(SERIALIZED_PERCENT);
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
    fn a_part_sent_again_is_counted_at_the_providers_rate() {
        let measured = MeasuredRequest::new(3_000, [(hash("a"), 8_000), (hash("b"), 2_000)])
            .expect("a plausible count");
        // 3,000 tokens for 10,000 bytes: 0.3 tokens a byte.
        assert_eq!(measured.sent_again(&hash("a"), 8_000), Some(2_400));
        assert_eq!(measured.sent_again(&hash("c"), 8_000), None, "a new part keeps its bytes");
        assert_eq!(measured.at_rate(5_000), 1_500, "a provider's record at the same rate");
    }

    /// New text by character class, at the provider's rate where that is higher.
    #[test]
    fn new_text_is_counted_by_character_class_or_the_denser_providers_rate() {
        let text = "fn main() {}".repeat(100);
        // 1,200 bytes of code: 400 tokens by character class.
        let sparse = MeasuredRequest::new(1_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(sparse.new_text(&text), 400, "0.1 a byte is below the rule");
        let dense = MeasuredRequest::new(5_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(dense.new_text(&text), 600, "0.5 a byte is above it");
        assert_eq!(dense.new_text("上下文"), 5, "nine bytes: three by the rule, five at the rate");
    }

    /// Serialized records at half again the provider's rate, at least half a token a byte and
    /// at most the bytes.
    #[test]
    fn serialized_records_are_counted_at_half_again_the_providers_rate() {
        // 2,500 tokens for 10,000 bytes: 0.25 a byte; half again is under 0.5.
        let sparse = MeasuredRequest::new(2_500, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(sparse.serialized(8_000), 4_000, "half a token a byte");
        // 5,000 tokens for 10,000 bytes: half again is 0.75.
        let dense = MeasuredRequest::new(5_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(dense.serialized(8_000), 6_000);
        let densest = MeasuredRequest::new(9_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(densest.serialized(8_000), 8_000, "never above the bytes");
    }

    #[test]
    fn a_count_above_the_bytes_or_too_small_to_be_the_request_is_not_used() {
        let dense = MeasuredRequest::new(50_000, [(hash("a"), 10_000)]).expect("plausible");
        assert_eq!(dense.sent_again(&hash("a"), 10_000), Some(10_000), "never above the bytes");
        assert_eq!(MeasuredRequest::new(20, [(hash("a"), 50_000)]), None);
        assert_eq!(MeasuredRequest::new(0, []), None);
    }
}
