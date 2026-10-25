use aura_domain_common::{EndpointId, ModelProfileId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::CatalogMatchState;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDiscoveryState {
    Queued,
    Running,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDiscoveryStage {
    Requesting,
    Parsing,
    EnrichingCatalog,
    Completed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDiscoveryOutcome {
    Succeeded,
    Partial,
    Failed,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProfileDiscoveryState {
    ReturnedInLatestDiscovery,
    NotReturnedInLatestDiscovery,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AddManualModelInput {
    pub endpoint_id: EndpointId,
    pub provider_model_name: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub context_window_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryProfileResult {
    pub model_profile_id: ModelProfileId,
    pub provider_model_name: String,
    pub created: bool,
    pub discovery_state: ModelProfileDiscoveryState,
    pub catalog_match_state: CatalogMatchState,
    pub needs_manual_confirmation: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDiscoveryMergeResult {
    pub endpoint_id: EndpointId,
    pub returned_profiles: Vec<ModelDiscoveryProfileResult>,
    pub not_returned_profile_ids: Vec<ModelProfileId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManualModelCreateResult {
    pub model_profile_id: ModelProfileId,
    pub needs_manual_confirmation: bool,
}
