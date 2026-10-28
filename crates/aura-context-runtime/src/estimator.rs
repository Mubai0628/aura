use std::{collections::BTreeMap, sync::Arc};

use aura_context_domain::{
    CHARACTER_CLASS_ESTIMATOR_ID, CONTEXT_BUDGET_POLICY_V1, ContextBudgetError,
    ContextBudgetSnapshot, ContextItemEstimate, PROVIDER_MEASURED_ESTIMATOR_ID,
    TokenEstimateExactness, TokenEstimatorIdentity, UTF8_BYTE_UPPER_BOUND_ESTIMATOR_ID,
    request_input_budget_v1, utf8_byte_upper_bound,
};

use crate::MeasuredRequest;
use aura_domain_common::{ContentHash, ProviderId};
use aura_model_domain::ModelAttachmentSnapshot;

/// An image's count when no estimator is registered for its model: the most an image costs
/// at the connected providers after they scale it (Anthropic about 1,600 tokens, OpenAI 1,105 for
/// a 1280x800 screenshot).
pub const IMAGE_UPPER_BOUND_TOKENS: u64 = 1_600;
pub const IMAGE_UPPER_BOUND_ESTIMATOR_ID: &str = "image_upper_bound_v1";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModelEstimatorKey {
    pub provider_id: ProviderId,
    pub provider_model_name: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MediaEstimatorKey {
    pub model: ModelEstimatorKey,
    pub media_type: String,
}

pub trait RegisteredExactTokenEstimator: Send + Sync {
    fn identity(&self) -> TokenEstimatorIdentity;
    fn estimate_canonical_text(&self, text: &str) -> Result<u64, ContextBudgetError>;
}

pub trait RegisteredNativeAttachmentEstimator: Send + Sync {
    fn identity(&self) -> TokenEstimatorIdentity;
    fn estimate_native_attachment(
        &self,
        attachment: &ModelAttachmentSnapshot,
    ) -> Result<u64, ContextBudgetError>;
}

#[derive(Clone, Debug)]
pub struct CanonicalTextBudgetItem {
    pub ordinal: u32,
    pub content_hash: ContentHash,
    pub canonical_text: String,
}

#[derive(Clone, Debug)]
pub struct NativeAttachmentBudgetItem {
    pub ordinal: u32,
    pub attachment: ModelAttachmentSnapshot,
}

#[derive(Default)]
pub struct ContextEstimatorRegistry {
    exact_text: BTreeMap<ModelEstimatorKey, Arc<dyn RegisteredExactTokenEstimator>>,
    native_media: BTreeMap<MediaEstimatorKey, Arc<dyn RegisteredNativeAttachmentEstimator>>,
    /// Parts no provider has counted go byte for byte instead of by character class.
    unmeasured_by_bytes: bool,
}

impl ContextEstimatorRegistry {
    /// Counts the parts no provider has counted byte for byte, a bound no tokenizer exceeds,
    /// instead of by character class ([`crate::character_class_tokens`]).
    #[must_use]
    pub fn counting_unmeasured_by_bytes(mut self) -> Self {
        self.unmeasured_by_bytes = true;
        self
    }

    pub fn register_exact_text(
        &mut self,
        key: ModelEstimatorKey,
        estimator: Arc<dyn RegisteredExactTokenEstimator>,
    ) -> Result<(), ContextBudgetError> {
        if estimator.identity().exactness != TokenEstimateExactness::Exact {
            return Err(ContextBudgetError::InvalidEstimatorIdentity);
        }
        self.exact_text.insert(key, estimator);
        Ok(())
    }

    pub fn register_native_media(
        &mut self,
        key: MediaEstimatorKey,
        estimator: Arc<dyn RegisteredNativeAttachmentEstimator>,
    ) -> Result<(), ContextBudgetError> {
        if estimator.identity().exactness != TokenEstimateExactness::Exact {
            return Err(ContextBudgetError::InvalidEstimatorIdentity);
        }
        self.native_media.insert(key, estimator);
        Ok(())
    }

    pub fn build_budget(
        &self,
        model: &ModelEstimatorKey,
        runtime_effective_window_tokens: u64,
        effective_max_output_tokens: u64,
        text_items: &[CanonicalTextBudgetItem],
        native_attachments: &[NativeAttachmentBudgetItem],
    ) -> Result<ContextBudgetSnapshot, ContextBudgetError> {
        let budget = self.estimate_budget(
            model,
            runtime_effective_window_tokens,
            effective_max_output_tokens,
            text_items,
            native_attachments,
        )?;
        if budget.estimated_input_tokens > budget.request_input_budget_tokens {
            return Err(ContextBudgetError::RequestBudgetExceeded);
        }
        Ok(budget)
    }

    pub fn estimate_budget(
        &self,
        model: &ModelEstimatorKey,
        runtime_effective_window_tokens: u64,
        effective_max_output_tokens: u64,
        text_items: &[CanonicalTextBudgetItem],
        native_attachments: &[NativeAttachmentBudgetItem],
    ) -> Result<ContextBudgetSnapshot, ContextBudgetError> {
        self.estimate_budget_measured(
            model,
            runtime_effective_window_tokens,
            effective_max_output_tokens,
            text_items,
            native_attachments,
            None,
        )
    }

    /// The same estimate, with the parts sent again counted at the rate the provider
    /// reported for the previous request of this model ([`MeasuredRequest`]).
    pub fn estimate_budget_measured(
        &self,
        model: &ModelEstimatorKey,
        runtime_effective_window_tokens: u64,
        effective_max_output_tokens: u64,
        text_items: &[CanonicalTextBudgetItem],
        native_attachments: &[NativeAttachmentBudgetItem],
        measured: Option<&MeasuredRequest>,
    ) -> Result<ContextBudgetSnapshot, ContextBudgetError> {
        validate_ordinals(text_items, native_attachments)?;
        let exact = self.exact_text.get(model);
        let mut item_estimates = Vec::with_capacity(text_items.len() + native_attachments.len());
        let mut estimated_input_tokens = 0_u64;

        for item in text_items {
            if item.content_hash != ContentHash::sha256(item.canonical_text.as_bytes()) {
                return Err(ContextBudgetError::ContentHashMismatch);
            }
            let byte_length = utf8_byte_upper_bound(&item.canonical_text);
            // With the provider's count of the previous request, a part not sent before is
            // counted at half again its rate rather than byte for byte.
            let sent_again = measured.map(|m| {
                m.sent_again(&item.content_hash, byte_length)
                    .unwrap_or_else(|| m.new_part(byte_length))
            });
            let (estimated_tokens, estimator) = match (exact, sent_again) {
                (Some(estimator), _) => {
                    (estimator.estimate_canonical_text(&item.canonical_text)?, estimator.identity())
                }
                (None, Some(tokens)) => (
                    tokens,
                    TokenEstimatorIdentity {
                        estimator_id: PROVIDER_MEASURED_ESTIMATOR_ID.to_owned(),
                        estimator_version: 1,
                        exactness: TokenEstimateExactness::Calibrated,
                    },
                ),
                (None, None) if self.unmeasured_by_bytes => (
                    utf8_byte_upper_bound(&item.canonical_text),
                    TokenEstimatorIdentity {
                        estimator_id: UTF8_BYTE_UPPER_BOUND_ESTIMATOR_ID.to_owned(),
                        estimator_version: 1,
                        exactness: TokenEstimateExactness::UpperBound,
                    },
                ),
                (None, None) => (
                    crate::character_class_tokens(&item.canonical_text),
                    TokenEstimatorIdentity {
                        estimator_id: CHARACTER_CLASS_ESTIMATOR_ID.to_owned(),
                        estimator_version: 1,
                        exactness: TokenEstimateExactness::Approximate,
                    },
                ),
            };
            estimated_input_tokens = estimated_input_tokens
                .checked_add(estimated_tokens)
                .ok_or(ContextBudgetError::ArithmeticOverflow)?;
            item_estimates.push(ContextItemEstimate {
                ordinal: item.ordinal,
                content_hash: item.content_hash.clone(),
                byte_length: u64::try_from(item.canonical_text.len())
                    .map_err(|_| ContextBudgetError::ArithmeticOverflow)?,
                estimated_tokens,
                estimator,
            });
        }

        for item in native_attachments {
            let key = MediaEstimatorKey {
                model: model.clone(),
                media_type: canonical_media_type(&item.attachment.media_type),
            };
            let (estimated_tokens, estimator) = match self.native_media.get(&key) {
                Some(estimator) => {
                    (estimator.estimate_native_attachment(&item.attachment)?, estimator.identity())
                }
                // An image no registered estimator counts is counted at what the connected
                // providers charge an image at most once they have scaled it.
                None if aura_model_domain::is_model_image(&key.media_type) => (
                    IMAGE_UPPER_BOUND_TOKENS,
                    TokenEstimatorIdentity {
                        estimator_id: IMAGE_UPPER_BOUND_ESTIMATOR_ID.to_owned(),
                        estimator_version: 1,
                        exactness: TokenEstimateExactness::UpperBound,
                    },
                ),
                None => return Err(ContextBudgetError::NativeAttachmentEstimatorMissing),
            };
            estimated_input_tokens = estimated_input_tokens
                .checked_add(estimated_tokens)
                .ok_or(ContextBudgetError::ArithmeticOverflow)?;
            item_estimates.push(ContextItemEstimate {
                ordinal: item.ordinal,
                content_hash: item.attachment.content_hash.clone(),
                byte_length: item.attachment.byte_length,
                estimated_tokens,
                estimator,
            });
        }
        item_estimates.sort_by_key(|item| item.ordinal);

        let (safety_reserve_tokens, request_input_budget_tokens) =
            request_input_budget_v1(runtime_effective_window_tokens, effective_max_output_tokens)?;
        Ok(ContextBudgetSnapshot {
            policy: CONTEXT_BUDGET_POLICY_V1.to_owned(),
            runtime_effective_window_tokens,
            estimated_input_tokens,
            effective_max_output_tokens,
            output_reserve_tokens: effective_max_output_tokens,
            safety_reserve_tokens,
            request_input_budget_tokens,
            item_estimates,
        })
    }
}

fn validate_ordinals(
    text_items: &[CanonicalTextBudgetItem],
    native_attachments: &[NativeAttachmentBudgetItem],
) -> Result<(), ContextBudgetError> {
    let mut ordinals = text_items.iter().map(|item| item.ordinal).collect::<Vec<_>>();
    ordinals.extend(native_attachments.iter().map(|item| item.ordinal));
    ordinals.sort_unstable();
    if ordinals
        .iter()
        .enumerate()
        .all(|(index, ordinal)| u32::try_from(index + 1).is_ok_and(|expected| *ordinal == expected))
    {
        Ok(())
    } else {
        Err(ContextBudgetError::InvalidItemOrder)
    }
}

fn canonical_media_type(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{ArtifactId, EventId};
    use aura_model_domain::ModelAttachmentSnapshot;

    use super::*;

    struct Exact;

    impl RegisteredExactTokenEstimator for Exact {
        fn identity(&self) -> TokenEstimatorIdentity {
            TokenEstimatorIdentity {
                estimator_id: "fixture_exact".to_owned(),
                estimator_version: 3,
                exactness: TokenEstimateExactness::Exact,
            }
        }

        fn estimate_canonical_text(&self, text: &str) -> Result<u64, ContextBudgetError> {
            u64::try_from(text.chars().count()).map_err(|_| ContextBudgetError::ArithmeticOverflow)
        }
    }

    struct Native;

    impl RegisteredNativeAttachmentEstimator for Native {
        fn identity(&self) -> TokenEstimatorIdentity {
            TokenEstimatorIdentity {
                estimator_id: "fixture_media_exact".to_owned(),
                estimator_version: 1,
                exactness: TokenEstimateExactness::Exact,
            }
        }

        fn estimate_native_attachment(
            &self,
            _attachment: &ModelAttachmentSnapshot,
        ) -> Result<u64, ContextBudgetError> {
            Ok(97)
        }
    }

    fn key() -> ModelEstimatorKey {
        ModelEstimatorKey {
            provider_id: ProviderId::new(),
            provider_model_name: "fixture-model".to_owned(),
        }
    }

    fn text(ordinal: u32, value: &str) -> CanonicalTextBudgetItem {
        CanonicalTextBudgetItem {
            ordinal,
            content_hash: ContentHash::sha256(value.as_bytes()),
            canonical_text: value.to_owned(),
        }
    }

    fn attachment(ordinal: u32) -> NativeAttachmentBudgetItem {
        NativeAttachmentBudgetItem {
            ordinal,
            attachment: ModelAttachmentSnapshot {
                source_event_id: EventId::new(),
                artifact_id: ArtifactId::new(),
                file_reference_id: None,
                display_name: "image.png".to_owned(),
                media_type: "image/png".to_owned(),
                byte_length: 32,
                content_hash: ContentHash::sha256(&[0; 32]),
            },
        }
    }

    /// Before any provider count, a part is counted by character class; byte for byte only
    /// when asked.
    #[test]
    fn counts_by_character_class_without_a_count_and_keeps_v1_reserve() {
        let budget = ContextEstimatorRegistry::default()
            .build_budget(&key(), 32_768, 1_024, &[text(1, "你好, world")], &[])
            .expect("budget");
        assert_eq!(budget.estimated_input_tokens, 2 + 3, "two characters, seven ASCII");
        assert_eq!(budget.safety_reserve_tokens, 4_096);
        let estimator = &budget.item_estimates[0].estimator;
        assert_eq!(estimator.estimator_id, CHARACTER_CLASS_ESTIMATOR_ID);
        assert_eq!(estimator.exactness, TokenEstimateExactness::Approximate);
        let bytes = ContextEstimatorRegistry::default()
            .counting_unmeasured_by_bytes()
            .build_budget(&key(), 32_768, 1_024, &[text(1, "你好")], &[])
            .expect("budget");
        assert_eq!(bytes.estimated_input_tokens, 6);
        assert_eq!(bytes.item_estimates[0].estimator.exactness, TokenEstimateExactness::UpperBound);
    }

    /// A first request whose bytes pass the budget goes when its text does not: a paste of
    /// records or code is about a third of its bytes.
    #[test]
    fn a_first_request_past_the_budget_in_bytes_fits_by_character_class() {
        let paste = "Record 2-00042: sensor 42 at station 67 read 554 units.\n".repeat(4_000);
        let budget = ContextEstimatorRegistry::default()
            .build_budget(&key(), 256_000, 32_000, &[text(1, &paste)], &[])
            .expect("fits by character class");
        assert!(paste.len() as u64 > budget.request_input_budget_tokens, "past it in bytes");
        assert_eq!(budget.estimated_input_tokens, (paste.len() as u64).div_ceil(3));
    }

    /// With the provider's count of the previous request, a part sent again is counted at
    /// its rate and a new part at half again that rate, at least half a token a byte.
    #[test]
    fn a_measured_request_counts_new_parts_at_half_again_its_rate() {
        let (old, new) = ("a".repeat(8_000), "b".repeat(4_000));
        let measured = MeasuredRequest::new(2_000, [(ContentHash::sha256(old.as_bytes()), 8_000)])
            .expect("plausible");
        let budget = ContextEstimatorRegistry::default()
            .estimate_budget_measured(
                &key(),
                256_000,
                4_096,
                &[text(1, &old), text(2, &new)],
                &[],
                Some(&measured),
            )
            .expect("budget");
        // 2,000 tokens for 8,000 bytes plus a tenth: 2,200; the new 4,000 bytes at half a token.
        assert_eq!(budget.estimated_input_tokens, 2_200 + 2_000);
    }

    #[test]
    fn registered_exact_estimator_does_not_reduce_safety_reserve() {
        let model = key();
        let mut registry = ContextEstimatorRegistry::default();
        registry.register_exact_text(model.clone(), Arc::new(Exact)).expect("register");
        let budget =
            registry.build_budget(&model, 256_000, 4_096, &[text(1, "你好")], &[]).expect("budget");
        assert_eq!(budget.estimated_input_tokens, 2);
        assert_eq!(budget.safety_reserve_tokens, 12_800);
        assert_eq!(budget.item_estimates[0].estimator.estimator_version, 3);
    }

    #[test]
    fn native_media_fails_closed_until_provider_model_media_estimator_is_registered() {
        let model = key();
        let mut registry = ContextEstimatorRegistry::default();
        let mut pdf = attachment(1);
        pdf.attachment.media_type = "application/pdf".to_owned();
        assert_eq!(
            registry.build_budget(&model, 32_768, 1_024, &[], &[pdf]),
            Err(ContextBudgetError::NativeAttachmentEstimatorMissing)
        );
        // An image is counted at the most an image costs until its model's estimator is
        // registered.
        let bound =
            registry.build_budget(&model, 32_768, 1_024, &[], &[attachment(1)]).expect("budget");
        assert_eq!(bound.estimated_input_tokens, IMAGE_UPPER_BOUND_TOKENS);
        assert_eq!(bound.item_estimates[0].estimator.exactness, TokenEstimateExactness::UpperBound);
        registry
            .register_native_media(
                MediaEstimatorKey { model: model.clone(), media_type: "image/png".to_owned() },
                Arc::new(Native),
            )
            .expect("register");
        let budget =
            registry.build_budget(&model, 32_768, 1_024, &[], &[attachment(1)]).expect("budget");
        assert_eq!(budget.estimated_input_tokens, 97);
        assert_eq!(budget.item_estimates[0].estimator.estimator_id, "fixture_media_exact");
    }
}
