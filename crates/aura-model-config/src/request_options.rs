//! The options a step sends with a model, from its own defaults.
use aura_model_domain::{ModelApiFormat, ModelProfileConfig, ResolvedModelRequestOptions};

pub(crate) fn resolve(
    model: &ModelProfileConfig,
    api_format: ModelApiFormat,
    effective_max_output_tokens: u64,
) -> ResolvedModelRequestOptions {
    let defaults = &model.request_defaults;
    let catalog = model.catalog.as_ref();
    ResolvedModelRequestOptions {
        effective_max_output_tokens,
        temperature: defaults.temperature,
        top_p: defaults.top_p,
        // Only a level the model offers is sent.
        reasoning_effort: defaults.reasoning_effort.and_then(|level| {
            let offered =
                aura_model_domain::offered_reasoning_efforts(defaults, api_format, catalog);
            aura_model_domain::level_to_send(level, &offered)
        }),
        reasoning_by_effort: aura_model_domain::reasoning_by_effort(catalog),
        image_input: aura_model_domain::takes_images(defaults, catalog),
    }
}
