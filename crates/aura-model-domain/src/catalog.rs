use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use aura_domain_common::{
    ArtifactId, ContentHash, NormalizedIssueCode, ProviderId, Revision, UtcTimestamp,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("catalog decimal must be a canonical non-negative decimal string")]
pub struct CatalogDecimalError;

/// A non-negative base-10 value used for remote catalog pricing.
///
/// The canonical form has no sign, exponent, leading zeroes, or trailing
/// fractional zeroes. It deliberately avoids binary floating-point.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CatalogDecimal(String);

impl CatalogDecimal {
    pub fn new(value: impl Into<String>) -> Result<Self, CatalogDecimalError> {
        let value = value.into();
        validate_catalog_decimal(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CatalogDecimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for CatalogDecimal {
    type Err = CatalogDecimalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for CatalogDecimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CatalogDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?.parse().map_err(D::Error::custom)
    }
}

impl JsonSchema for CatalogDecimal {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CatalogDecimal")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": r"^(?:0|[1-9][0-9]*)(?:\.[0-9]*[1-9])?$"
        })
    }
}

fn validate_catalog_decimal(value: &str) -> Result<(), CatalogDecimalError> {
    let (whole, fraction) =
        value.split_once('.').map_or((value, None), |(whole, fraction)| (whole, Some(fraction)));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || (whole.len() > 1 && whole.starts_with('0'))
        || fraction.is_some_and(|fraction| {
            fraction.is_empty()
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
                || fraction.ends_with('0')
        })
    {
        return Err(CatalogDecimalError);
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogPrice {
    pub currency: CatalogPriceCurrency,
    pub unit: CatalogPriceUnit,
    pub input: Option<CatalogDecimal>,
    pub output: Option<CatalogDecimal>,
    pub cache_read: Option<CatalogDecimal>,
    pub cache_write: Option<CatalogDecimal>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogPriceCurrency {
    Usd,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogPriceUnit {
    PerMillionTokens,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogModelCapabilities {
    pub attachment: bool,
    pub reasoning: bool,
    pub tool_call: bool,
    pub structured_output: bool,
    pub temperature: bool,
    pub interleaved: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogModelEntry {
    pub catalog_model_id: String,
    pub display_name: String,
    pub family: Option<String>,
    pub description: Option<String>,
    pub aliases: BTreeSet<String>,
    pub context_window_tokens: Option<u64>,
    pub input_limit_tokens: Option<u64>,
    pub output_limit_tokens: Option<u64>,
    pub input_modalities: BTreeSet<String>,
    pub output_modalities: BTreeSet<String>,
    pub capabilities: ProviderCatalogModelCapabilities,
    pub reasoning_options: BTreeSet<String>,
    pub knowledge_cutoff: Option<String>,
    pub release_date: Option<String>,
    pub last_updated: Option<String>,
    pub open_weights: Option<bool>,
    pub status: Option<String>,
    pub price: Option<ProviderCatalogPrice>,
    /// models.dev's `type`: `chat`, `image`, `embedding`, `video`, `speech`, `transcription`,
    /// `reranking`, `realtime`. A model without one is a chat model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_type: Option<String>,
    /// Where the model is served when it is not where its provider's other models are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<ProviderCatalogModelRoute>,
    /// The message field that carries the model's reasoning in Chat Completions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_field: Option<String>,
    /// Prices from a context length on, by ascending length; `price` applies below the first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub price_tiers: Vec<ProviderCatalogPriceTier>,
    /// Modes the model can be asked for, such as `fast` or `flex`, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<String, ProviderCatalogModelMode>,
}

impl ProviderCatalogModelEntry {
    /// Whether the model answers in a chat: models.dev names no type, or `chat`.
    #[must_use]
    pub fn is_chat(&self) -> bool {
        self.model_type.as_deref().is_none_or(|kind| kind == "chat")
    }
}

/// A model's own route, as models.dev gives it: untrusted, a suggestion only.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogModelRoute {
    pub api_url: Option<String>,
    pub npm_package: Option<String>,
    /// `responses` or `completions` within an OpenAI-style package.
    pub shape: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogPriceTier {
    /// The context length, in tokens, from which the price applies.
    pub from_context_tokens: u64,
    pub price: ProviderCatalogPrice,
}

/// One mode of a model and what asking for it changes. Only request body fields and headers on
/// Aura's list are kept, each with a string value; a mode that needs anything else is left out.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogModelMode {
    pub price: Option<ProviderCatalogPrice>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub body: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// Renderer-safe catalog item paired with the immutable object hash required
/// by catalog-backed Profile create/update CAS commands.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogModelView {
    pub catalog_object_hash: ContentHash,
    pub model: ProviderCatalogModelEntry,
    #[serde(default)]
    pub route: ProviderCatalogModelRouteView,
}

/// Which of its Provider's preset variants a catalog model goes by.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderCatalogModelRouteView {
    /// Any connection of the Provider takes it.
    #[default]
    Any,
    /// Only a connection of this variant takes it.
    Variant { variant_id: String },
    /// No variant takes it: the catalog serves it by a protocol Aura does not speak, or at an
    /// address the Provider's profile does not have.
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogEntry {
    pub catalog_provider_id: String,
    pub display_name: String,
    pub npm_package: Option<String>,
    pub api_url: Option<String>,
    pub documentation_url: Option<String>,
    pub model_order: Vec<String>,
    pub models: BTreeMap<String, ProviderCatalogModelEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogSnapshot {
    pub catalog_revision: Revision,
    pub catalog_content_hash: ContentHash,
    pub checked_at: UtcTimestamp,
    pub last_successful_refresh_at: UtcTimestamp,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub raw_artifact_id: ArtifactId,
    pub provider_order: Vec<String>,
    pub providers: BTreeMap<String, ProviderCatalogEntry>,
    /// The parser version that read the providers; 0 for snapshots from before it was kept.
    #[serde(default)]
    pub parser_revision: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogState {
    Unavailable,
    LoadingWithoutCache,
    Fresh,
    Stale,
    RefreshingWithLkg,
    RefreshFailedWithLkg,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogStatus {
    pub state: ProviderCatalogState,
    pub snapshot: Option<ProviderCatalogSnapshot>,
    pub checked_at: Option<UtcTimestamp>,
    pub last_successful_refresh_at: Option<UtcTimestamp>,
    pub last_error_code: Option<NormalizedIssueCode>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogConfigurationClass {
    TrustedPreset,
    ManualConfiguration,
    CatalogInformationOnly,
}

/// Renderer-safe initial values for a catalog Provider connection form.
///
/// Aura presets are compiled into the signed application and may define the
/// complete execution contract. A models.dev API suggestion is limited to an
/// HTTPS Base URL and the protocol the catalog's package speaks: a draft the
/// user saves, never trusted execution configuration.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderCatalogConnectionDefaultsView {
    AuraPreset {
        variants: Vec<ProviderPresetVariantView>,
        /// Each model goes by its own variant, so the Provider is reached through one
        /// connection of each.
        #[serde(default)]
        by_model: bool,
    },
    ModelsDevApiSuggestion {
        base_url: String,
        /// Where the catalog's package for the Provider speaks a protocol Aura speaks.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        protocol: Option<ModelsDevProtocolSuggestion>,
    },
}

/// The protocol a catalog Provider's package speaks, the key that package sends and, for Chat
/// Completions, the defaults it sends with.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelsDevProtocolSuggestion {
    pub api_format: crate::ModelApiFormat,
    pub auth_scheme: crate::EndpointAuthScheme,
    pub chat_instruction_role: Option<crate::ChatInstructionRole>,
    pub chat_output_token_field: Option<crate::ChatOutputTokenField>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPresetVariantView {
    pub variant_id: String,
    pub display_name: String,
    /// With a `{name}` in place of each of `base_url_parameters`.
    pub base_url: String,
    pub api_format: crate::ModelApiFormat,
    pub auth_scheme: crate::EndpointAuthScheme,
    pub discovery_mode: crate::ModelDiscoveryMode,
    pub chat_instruction_role: Option<crate::ChatInstructionRole>,
    pub chat_output_token_field: Option<crate::ChatOutputTokenField>,
    pub recommended: bool,
    /// The values of the user's own the base URL takes, such as an account or a region.
    #[serde(default)]
    pub base_url_parameters: Vec<ProviderPresetParameterView>,
}

/// A non-secret value of the user's own in a preset's base URL.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPresetParameterView {
    /// As it stands in the base URL, between braces.
    pub name: String,
    pub display_name: String,
    pub kind: ProviderPresetParameterKind,
    pub example: String,
}

/// Where a parameter stands in a base URL, and so what it may hold: never enough to move a
/// request, or the key it carries, to another host or path.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPresetParameterKind {
    /// One whole label of the host: lower-case letters, digits and inner hyphens, at most 63.
    DomainLabel,
    /// One whole segment of the path: letters, digits and `-`, `.`, `_`, `~`, at most 128, and
    /// neither `.` nor `..`.
    PathSegment,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIconRef {
    pub artifact_id: ArtifactId,
    pub content_hash: ContentHash,
    pub byte_length: u64,
    pub pixel_size: u16,
}

impl ProviderIconRef {
    pub const PIXEL_SIZE: u16 = 128;
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogCardView {
    pub catalog_provider_id: String,
    pub display_name: String,
    pub icon_ref: Option<ProviderIconRef>,
    pub fallback_initial: String,
    pub catalog_model_count: u32,
    pub configuration_class: ProviderCatalogConfigurationClass,
    pub connection_defaults: Option<ProviderCatalogConnectionDefaultsView>,
    pub local_provider_id: Option<ProviderId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogSummaryView {
    pub catalog_provider_id: String,
    pub display_name: String,
    pub icon_ref: Option<ProviderIconRef>,
    pub fallback_initial: String,
    pub catalog_model_count: u32,
    pub configuration_class: ProviderCatalogConfigurationClass,
    pub connection_defaults: Option<ProviderCatalogConnectionDefaultsView>,
    pub local_provider_id: Option<ProviderId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogModelPage {
    pub catalog_revision: Revision,
    pub catalog_content_hash: ContentHash,
    pub provider: ProviderCatalogSummaryView,
    pub models: Vec<ProviderCatalogModelView>,
    pub next_model_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderCatalogMatch {
    Matched { catalog_provider_id: String, catalog_model_id: String },
    Unmatched,
    Ambiguous { candidates: Vec<ProviderCatalogMatchCandidate> },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCatalogMatchCandidate {
    pub catalog_provider_id: String,
    pub catalog_model_id: String,
}

#[cfg(test)]
mod tests {
    use super::CatalogDecimal;

    #[test]
    fn catalog_decimal_is_canonical_and_never_float_backed() {
        for valid in ["0", "1", "0.5", "125.0001"] {
            assert!(valid.parse::<CatalogDecimal>().is_ok(), "{valid}");
        }
        for invalid in ["", "-1", "+1", "01", "1.0", "1.", ".5", "1e3"] {
            assert!(invalid.parse::<CatalogDecimal>().is_err(), "{invalid}");
        }
    }
}
