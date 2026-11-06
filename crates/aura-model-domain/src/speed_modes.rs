//! The speed modes a model can be asked for, such as `fast`, `priority` or `flex`.
//!
//! The catalog lists each mode of a model with the request body fields and headers it sets and
//! the price it is charged at. A model's settings allow its modes or not; once they are allowed,
//! a conversation chooses one for each turn, and a turn that chose none goes in the standard mode,
//! which sends nothing. Only a mode the model's matched catalog entry offers is sent, and a mode
//! sets only the body fields and headers listed here, each to a plain word: the catalog is
//! information, never a way to put into a request what Aura does not know.
use std::collections::BTreeMap;

use aura_domain_common::ModelProfileId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    CatalogMatchState, ModelCatalogMetadata, ModelProfileConfig, ModelRegistryConfig,
    ModelRequestDefaults, ProviderCatalogModelMode, ProviderCatalogPrice,
};

/// Request body fields a mode may set.
pub const SPEED_MODE_BODY_FIELDS: [&str; 2] = ["service_tier", "speed"];
/// Request headers a mode may set.
pub const SPEED_MODE_HEADERS: [&str; 1] = ["anthropic-beta"];

/// Whether `word` may name a mode or be sent as one of its values: 1 to 64 ASCII letters,
/// digits, `-`, `_` or `.`.
#[must_use]
pub fn is_speed_mode_word(word: &str) -> bool {
    (1..=64).contains(&word.len())
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// A mode a request is sent with: its name, and the body fields and headers it sets.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpeedMode {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub body: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// Whether every field and header `mode` sets is on Aura's list, with a plain word for its value.
/// A mode setting nothing is no mode.
fn sendable(mode: &ProviderCatalogModelMode) -> bool {
    let listed = |fields: &BTreeMap<String, String>, allowed: &[&str]| {
        fields
            .iter()
            .all(|(name, value)| allowed.contains(&name.as_str()) && is_speed_mode_word(value))
    };
    !(mode.body.is_empty() && mode.headers.is_empty())
        && listed(&mode.body, &SPEED_MODE_BODY_FIELDS)
        && listed(&mode.headers, &SPEED_MODE_HEADERS)
}

/// The modes a model's catalog entry offers, by name: those of an entry the model was matched to
/// that set only listed fields and headers. A mode asking for anything else is left out whole.
#[must_use]
pub fn offered_speed_modes(
    catalog: Option<&ModelCatalogMetadata>,
) -> BTreeMap<&str, &ProviderCatalogModelMode> {
    catalog
        .filter(|catalog| catalog.match_state == CatalogMatchState::Matched)
        .map(|catalog| {
            catalog
                .snapshot
                .modes
                .iter()
                .filter(|(name, mode)| is_speed_mode_word(name) && sendable(mode))
                .map(|(name, mode)| (name.as_str(), mode))
                .collect()
        })
        .unwrap_or_default()
}

/// The mode a turn's requests are sent with: the one its conversation chose, when the model's
/// settings allow speed modes and its catalog entry offers that one.
#[must_use]
pub fn speed_mode_to_send(
    chosen: Option<&str>,
    defaults: &ModelRequestDefaults,
    catalog: Option<&ModelCatalogMetadata>,
) -> Option<ModelSpeedMode> {
    let name = chosen.filter(|_| defaults.allow_speed_modes)?;
    let mode = offered_speed_modes(catalog).remove(name)?;
    Some(ModelSpeedMode {
        name: name.to_owned(),
        body: mode.body.clone(),
        headers: mode.headers.clone(),
    })
}

/// One mode a model offers, and the price it is charged at.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpeedModeView {
    pub name: String,
    pub price: Option<ProviderCatalogPrice>,
}

/// The modes one model offers, by name, and its standard price.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpeedModesView {
    pub model_profile_id: ModelProfileId,
    pub standard_price: Option<ProviderCatalogPrice>,
    pub modes: Vec<ModelSpeedModeView>,
}

/// The modes a conversation can choose for a model, by name: none until its settings allow them.
#[must_use]
pub fn allowed_speed_modes(model: &ModelProfileConfig) -> Vec<String> {
    if !model.request_defaults.allow_speed_modes {
        return Vec::new();
    }
    offered_speed_modes(model.catalog.as_ref()).into_keys().map(str::to_owned).collect()
}

/// The modes of every model of a registry that offers any, allowed or not: the settings show
/// whether a model has modes to allow, and at what price.
#[must_use]
pub fn registry_speed_modes(registry: &ModelRegistryConfig) -> Vec<ModelSpeedModesView> {
    registry
        .models
        .values()
        .filter_map(|model| {
            let catalog = model.catalog.as_ref()?;
            let modes = offered_speed_modes(Some(catalog))
                .into_iter()
                .map(|(name, mode)| ModelSpeedModeView {
                    name: name.to_owned(),
                    price: mode.price.clone(),
                })
                .collect::<Vec<_>>();
            (!modes.is_empty()).then(|| ModelSpeedModesView {
                model_profile_id: model.id,
                standard_price: catalog.snapshot.price.clone(),
                modes,
            })
        })
        .collect()
}
