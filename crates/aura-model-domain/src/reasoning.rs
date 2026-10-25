//! How much a
//! reasoning model thinks before it answers, and which levels a model offers.
//!
//! A model profile may name one level; nothing named is the provider's own default, so nothing
//! is sent and a model that takes no such setting is never asked to. The levels offered come from
//! the catalog when it lists them for the model, none when the catalog says the model does not
//! reason, and otherwise the levels its API format takes. Each format sends a level its own way:
//! `reasoning.effort` (OpenAI Responses), `reasoning_effort` (Chat Completions) and a thinking
//! budget (Anthropic Messages).
//!
//! `max` is its own level, and
//! each level is sent under its catalog name; a profile may list the levels its model offers
//! itself; an Anthropic model whose catalog lists effort levels is sent `output_config.effort`,
//! one that lists none a thinking budget.
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CatalogMatchState, ModelApiFormat, ModelCatalogMetadata};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ReasoningEffort {
    /// The word the OpenAI formats use for the level.
    #[must_use]
    pub const fn openai_name(self) -> &'static str {
        match self {
            Self::Off => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// The effort the Anthropic format takes for the level, where it takes one.
    #[must_use]
    pub const fn anthropic_effort_name(self) -> Option<&'static str> {
        match self {
            Self::Off | Self::Minimal => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::Xhigh => Some("xhigh"),
            Self::Max => Some("max"),
        }
    }

    /// The thinking budget the Anthropic format is given for the level; `None` is thinking off.
    #[must_use]
    pub const fn anthropic_budget_tokens(self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::Minimal => Some(1_024),
            Self::Low => Some(4_096),
            Self::Medium => Some(10_240),
            Self::High => Some(20_480),
            Self::Xhigh => Some(32_000),
            Self::Max => Some(64_000),
        }
    }

    fn from_catalog(word: &str) -> Option<Self> {
        Some(match word.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "disabled" => Self::Off,
            "minimal" => Self::Minimal,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            "xhigh" => Self::Xhigh,
            "max" | "maximum" => Self::Max,
            _ => return None,
        })
    }
}

/// The levels a model offers, lowest first; empty when it offers none.
#[must_use]
pub fn reasoning_efforts(
    api_format: ModelApiFormat,
    catalog: Option<&ModelCatalogMetadata>,
) -> Vec<ReasoningEffort> {
    if let Some(catalog) =
        catalog.filter(|catalog| catalog.match_state == CatalogMatchState::Matched)
    {
        let listed = catalog_levels(&catalog.snapshot.reasoning_options);
        if !listed.is_empty() {
            return listed;
        }
        if !catalog.snapshot.capabilities.reasoning {
            return Vec::new();
        }
    }
    match api_format {
        ModelApiFormat::OpenAiResponses | ModelApiFormat::OpenAiChatCompletions => {
            vec![ReasoningEffort::Low, ReasoningEffort::Medium, ReasoningEffort::High]
        }
        ModelApiFormat::AnthropicMessages => vec![
            ReasoningEffort::Off,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
        ],
    }
}

/// The level a request sends for `level`, given the levels the model offers. The level
/// itself when offered; an older version stored a catalog's `max` as `xhigh`, so a stored `xhigh` the model
/// does not offer goes out as its `max`; any other level the model does not take is not sent. A
/// stored level is never a reason to refuse the configuration: catalogs change, and an older
/// Aura stored names a newer one reads differently.
#[must_use]
pub fn level_to_send(
    level: ReasoningEffort,
    offered: &[ReasoningEffort],
) -> Option<ReasoningEffort> {
    if offered.contains(&level) {
        Some(level)
    } else if level == ReasoningEffort::Xhigh && offered.contains(&ReasoningEffort::Max) {
        Some(ReasoningEffort::Max)
    } else {
        None
    }
}

/// The levels a profile offers: the ones its user listed, else what the catalog and format give.
#[must_use]
pub fn offered_reasoning_efforts(
    defaults: &crate::ModelRequestDefaults,
    api_format: ModelApiFormat,
    catalog: Option<&ModelCatalogMetadata>,
) -> Vec<ReasoningEffort> {
    match &defaults.reasoning_efforts {
        Some(listed) => listed.iter().copied().collect::<BTreeSet<_>>().into_iter().collect(),
        None => reasoning_efforts(api_format, catalog),
    }
}

/// Whether the model takes its level as an effort (its catalog lists effort levels) rather than
/// as a thinking budget: what decides how the Anthropic format sends it.
#[must_use]
pub fn reasoning_by_effort(catalog: Option<&ModelCatalogMetadata>) -> bool {
    catalog
        .filter(|catalog| catalog.match_state == CatalogMatchState::Matched)
        .is_some_and(|catalog| !catalog_levels(&catalog.snapshot.reasoning_options).is_empty())
}

/// The levels of every model of a registry that offers any.
#[must_use]
pub fn registry_reasoning_efforts(
    registry: &crate::ModelRegistryConfig,
) -> Vec<crate::ModelReasoningEffortsView> {
    registry
        .models
        .values()
        .filter_map(|model| {
            let endpoint = registry.endpoints.get(&model.endpoint_id)?;
            let levels = offered_reasoning_efforts(
                &model.request_defaults,
                endpoint.api_format,
                model.catalog.as_ref(),
            );
            (!levels.is_empty()).then_some(crate::ModelReasoningEffortsView {
                model_profile_id: model.id,
                reasoning_efforts: levels,
            })
        })
        .collect()
}

/// The catalog keeps each reasoning option as its JSON text, such as
/// `{"type":"effort","values":["low","medium","high"]}`.
fn catalog_levels(options: &BTreeSet<String>) -> Vec<ReasoningEffort> {
    let mut levels = BTreeSet::new();
    for raw in options {
        let Ok(option) = serde_json::from_str::<serde_json::Value>(raw) else { continue };
        if option["type"] != "effort" {
            continue;
        }
        let words = option["values"].as_array().into_iter().flatten();
        levels.extend(
            words.filter_map(serde_json::Value::as_str).filter_map(ReasoningEffort::from_catalog),
        );
    }
    levels.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProviderCatalogModelCapabilities, ProviderCatalogModelEntry};
    use aura_domain_common::{ContentHash, Revision};

    fn catalog(
        reasoning: bool,
        options: &[&str],
        state: CatalogMatchState,
    ) -> ModelCatalogMetadata {
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
                input_modalities: BTreeSet::new(),
                output_modalities: BTreeSet::new(),
                capabilities: ProviderCatalogModelCapabilities {
                    attachment: false,
                    reasoning,
                    tool_call: true,
                    structured_output: false,
                    temperature: true,
                    interleaved: None,
                },
                reasoning_options: options.iter().map(|o| (*o).to_owned()).collect(),
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

    /// The catalog lists DeepSeek V4 Flash as low/high/max and gpt-6.1-sol up to max;
    /// `max` was taken for `xhigh` and sent as "xhigh".
    #[test]
    fn max_is_its_own_level_and_goes_out_under_its_catalog_name() {
        let deepseek = catalog(
            true,
            &[r#"{"type":"toggle"}"#, r#"{"type":"effort","values":["low","high","max"]}"#],
            CatalogMatchState::Matched,
        );
        assert_eq!(
            reasoning_efforts(ModelApiFormat::OpenAiChatCompletions, Some(&deepseek)),
            [ReasoningEffort::Low, ReasoningEffort::High, ReasoningEffort::Max]
        );
        let sol = catalog(
            true,
            &[r#"{"type":"effort","values":["low","medium","high","xhigh","max"]}"#],
            CatalogMatchState::Matched,
        );
        let levels = reasoning_efforts(ModelApiFormat::OpenAiResponses, Some(&sol));
        assert_eq!(levels.last(), Some(&ReasoningEffort::Max));
        assert!(levels.contains(&ReasoningEffort::Xhigh));
        assert_eq!(ReasoningEffort::Max.openai_name(), "max");
        assert_eq!(ReasoningEffort::Xhigh.openai_name(), "xhigh");
        assert_eq!(ReasoningEffort::Max.anthropic_effort_name(), Some("max"));
        assert_eq!(ReasoningEffort::Off.anthropic_effort_name(), None);
    }

    /// Stored a catalog's `max` as `xhigh`; such a stored level goes out as the
    /// model's `max`, and a level the model does not take at all is not sent.
    #[test]
    fn a_stored_level_goes_out_under_the_models_own_name_or_not_at_all() {
        use ReasoningEffort::{High, Low, Max, Medium, Xhigh};
        assert_eq!(level_to_send(Xhigh, &[Low, High, Max]), Some(Max));
        assert_eq!(level_to_send(Xhigh, &[Low, High, Xhigh, Max]), Some(Xhigh));
        assert_eq!(level_to_send(Max, &[Low, High, Max]), Some(Max));
        assert_eq!(level_to_send(Medium, &[Low, High, Max]), None);
        assert_eq!(level_to_send(Max, &[Low, Medium, High]), None);
    }

    #[test]
    fn a_profile_may_list_the_levels_its_model_offers() {
        let defaults = crate::ModelRequestDefaults {
            reasoning_efforts: Some(vec![
                ReasoningEffort::Max,
                ReasoningEffort::Low,
                ReasoningEffort::Low,
            ]),
            ..Default::default()
        };
        assert_eq!(
            offered_reasoning_efforts(&defaults, ModelApiFormat::OpenAiResponses, None),
            [ReasoningEffort::Low, ReasoningEffort::Max]
        );
        let unlisted = crate::ModelRequestDefaults::default();
        assert_eq!(
            offered_reasoning_efforts(&unlisted, ModelApiFormat::OpenAiResponses, None),
            [ReasoningEffort::Low, ReasoningEffort::Medium, ReasoningEffort::High]
        );
    }

    #[test]
    fn a_model_whose_catalog_lists_effort_levels_takes_its_level_as_an_effort() {
        let effort = catalog(
            true,
            &[r#"{"type":"effort","values":["low","medium","high","xhigh","max"]}"#],
            CatalogMatchState::Matched,
        );
        let budget =
            catalog(true, &[r#"{"type":"budget_tokens","min":1024}"#], CatalogMatchState::Matched);
        assert!(reasoning_by_effort(Some(&effort)));
        assert!(!reasoning_by_effort(Some(&budget)));
        assert!(!reasoning_by_effort(None));
    }

    #[test]
    fn the_catalog_list_decides_when_there_is_one() {
        let listed = catalog(
            true,
            &[r#"{"type":"effort","values":["minimal","high","xhigh","unheard-of"]}"#],
            CatalogMatchState::Matched,
        );
        assert_eq!(
            reasoning_efforts(ModelApiFormat::OpenAiResponses, Some(&listed)),
            [ReasoningEffort::Minimal, ReasoningEffort::High, ReasoningEffort::Xhigh]
        );
    }

    #[test]
    fn a_model_the_catalog_says_does_not_reason_offers_nothing() {
        let plain = catalog(false, &[], CatalogMatchState::Matched);
        assert!(reasoning_efforts(ModelApiFormat::OpenAiChatCompletions, Some(&plain)).is_empty());
    }

    #[test]
    fn otherwise_the_api_format_decides() {
        let unmatched = catalog(false, &[], CatalogMatchState::Unmatched);
        assert_eq!(
            reasoning_efforts(ModelApiFormat::OpenAiResponses, Some(&unmatched)),
            [ReasoningEffort::Low, ReasoningEffort::Medium, ReasoningEffort::High]
        );
        assert_eq!(
            reasoning_efforts(ModelApiFormat::AnthropicMessages, None)[0],
            ReasoningEffort::Off,
            "thinking can be switched off in Anthropic's format"
        );
        assert_eq!(ReasoningEffort::Off.openai_name(), "none");
        assert_eq!(ReasoningEffort::Medium.anthropic_budget_tokens(), Some(10_240));
    }
}
