//! The catalog entries a model's name could be. Several catalog Providers can list a model under
//! the same name; discovery then links the model to none of them, and the model settings list
//! them so the user can choose the one the connection really serves.
use aura_domain_common::{ContentHash, ModelProfileId, Revision};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ProviderCatalogMatchCandidate;

/// One chat entry of the catalog that a model's name matches, with its Provider's own name and
/// the hash of the entry as listed, which choosing it must still find.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogEntryView {
    pub catalog_provider_id: String,
    pub catalog_model_id: String,
    pub provider_display_name: String,
    pub catalog_object_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileCatalogEntriesRequest {
    pub model_profile_id: ModelProfileId,
}

/// The entries a profile's model name matches, in the catalog's order.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileCatalogEntriesView {
    pub model_profile_id: ModelProfileId,
    pub catalog_revision: Revision,
    pub entries: Vec<ModelCatalogEntryView>,
    /// The entry the profile is linked to, when it is one of them.
    pub chosen: Option<ProviderCatalogMatchCandidate>,
}
