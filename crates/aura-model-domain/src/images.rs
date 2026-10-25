//! Whether a model is sent images, such as the
//! built-in browser's screenshots. A profile says so when its user set it. Otherwise it is sent them, unless the catalog matched it and
//! lists no image input - its provider would refuse the request, and the turn with it.
use crate::{CatalogMatchState, ModelCatalogMetadata, ModelRequestDefaults};

/// The image types a request may carry.
pub const MODEL_IMAGE_MEDIA_TYPES: &[&str] =
    &["image/jpeg", "image/png", "image/webp", "image/gif"];

/// Whether a request may carry this media type as an image.
#[must_use]
pub fn is_model_image(media_type: &str) -> bool {
    MODEL_IMAGE_MEDIA_TYPES.contains(&media_type.trim().to_ascii_lowercase().as_str())
}

/// Whether the profile's model is sent images.
#[must_use]
pub fn takes_images(
    defaults: &ModelRequestDefaults,
    catalog: Option<&ModelCatalogMetadata>,
) -> bool {
    defaults.image_input.unwrap_or_else(|| {
        catalog
            .filter(|catalog| catalog.match_state == CatalogMatchState::Matched)
            .is_none_or(|catalog| catalog.snapshot.input_modalities.contains("image"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProviderCatalogModelCapabilities, ProviderCatalogModelEntry};
    use aura_domain_common::{ContentHash, Revision};
    use std::collections::BTreeSet;

    fn catalog(modalities: &[&str], state: CatalogMatchState) -> ModelCatalogMetadata {
        ModelCatalogMetadata {
            catalog_provider_id: "fixture".into(),
            catalog_model_id: "fixture-model".into(),
            catalog_revision: Revision::INITIAL,
            catalog_content_hash: ContentHash::sha256(b"catalog"),
            catalog_object_hash: ContentHash::sha256(b"object"),
            snapshot: ProviderCatalogModelEntry {
                catalog_model_id: "fixture-model".into(),
                display_name: "Fixture".into(),
                family: None,
                description: None,
                aliases: BTreeSet::new(),
                context_window_tokens: None,
                input_limit_tokens: None,
                output_limit_tokens: None,
                input_modalities: modalities.iter().map(|m| (*m).to_owned()).collect(),
                output_modalities: BTreeSet::new(),
                capabilities: ProviderCatalogModelCapabilities {
                    attachment: false,
                    reasoning: false,
                    tool_call: true,
                    structured_output: false,
                    temperature: true,
                    interleaved: None,
                },
                reasoning_options: BTreeSet::new(),
                knowledge_cutoff: None,
                release_date: None,
                last_updated: None,
                open_weights: None,
                status: None,
                price: None,
            },
            matched_at: None,
            match_state: state,
        }
    }

    #[test]
    fn the_catalog_withholds_images_only_from_a_model_it_matched_without_them() {
        let unset = ModelRequestDefaults::default();
        let text = catalog(&["text"], CatalogMatchState::Matched);
        assert!(!takes_images(&unset, Some(&text)), "DeepSeek V4 Pro: the catalog says text only");
        assert!(takes_images(
            &unset,
            Some(&catalog(&["image", "text"], CatalogMatchState::Matched))
        ));
        let unmatched = catalog(&["text"], CatalogMatchState::Unmatched);
        assert!(takes_images(&unset, Some(&unmatched)), "an entry not matched says nothing");
        let said =
            ModelRequestDefaults { image_input: Some(true), ..ModelRequestDefaults::default() };
        assert!(takes_images(&said, Some(&text)), "the user's word comes first");
    }

    // A model is sent images unless its user
    // said no, or the catalog knows it takes none.
    #[test]
    fn the_users_word_comes_first_and_a_model_takes_images_unless_the_catalog_knows_it_does_not() {
        let mut defaults = ModelRequestDefaults::default();
        assert!(takes_images(&defaults, None), "a model the catalog does not know is sent images");
        defaults.image_input = Some(false);
        assert!(!takes_images(&defaults, None));
        defaults.image_input = Some(true);
        assert!(takes_images(&defaults, None));
        assert!(is_model_image("image/jpeg") && is_model_image(" Image/PNG"));
        assert!(!is_model_image("application/pdf"));
    }
}
