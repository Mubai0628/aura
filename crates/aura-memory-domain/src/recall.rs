use crate::{MemoryDocument, MemoryHistory, MemoryMetadata, MemoryScope};
use aura_domain_common::WorkspacePath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySearch {
    pub query: String,
    pub category: Option<String>,
    pub limit: Option<u32>,
    #[serde(default)]
    pub include_inactive: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySearchHit {
    pub scope: MemoryScope,
    pub path: String,
    pub metadata: MemoryMetadata,
    pub title: String,
    pub hash: String,
    pub excerpt: String,
    pub score: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySearchResult {
    pub hits: Vec<MemorySearchHit>,
    pub incomplete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryRead {
    pub path: WorkspacePath,
    pub note: MemoryDocument,
    pub hash: String,
    pub history: Vec<MemoryHistory>,
}
