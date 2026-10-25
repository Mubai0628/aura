#![forbid(unsafe_code)]
//! Noninteractive network tools and session-owned web documents.
pub mod http;
use aura_domain_common::{ArtifactId, ContentHash, UtcTimestamp};
pub use http::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebSearchInput {
    pub query: String,
    #[serde(default = "default_search_results")]
    pub max_results: u32,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebFetchMode {
    #[default]
    Auto,
    HttpOnly,
    HeadlessRenderRequired,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebFetchInput {
    pub url: String,
    #[serde(default)]
    pub mode: WebFetchMode,
    #[serde(default = "default_fetch_bytes")]
    pub max_bytes: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebReadInput {
    pub document_artifact_id: ArtifactId,
    #[serde(default)]
    pub offset_chars: u32,
    #[serde(default = "default_read_chars")]
    pub max_chars: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebSearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,
    /// Search pages without a reliable publication date keep this absent.
    pub published_at: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebSearchResult {
    pub query: String,
    pub surface: String,
    pub hits: Vec<WebSearchHit>,
    pub retrieved_at: UtcTimestamp,
    pub live: bool,
    pub evidence_artifact_id: ArtifactId,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebDocument {
    pub source_url: String,
    pub title: String,
    pub media_type: String,
    pub text: String,
    pub links: Vec<WebSearchHit>,
    pub retrieved_at: UtcTimestamp,
    pub response_hash: ContentHash,
    pub rendered: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebFetchResult {
    pub document_artifact_id: ArtifactId,
    pub source_url: String,
    pub title: String,
    pub excerpt: String,
    pub total_chars: u32,
    pub rendered: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebReadResult {
    pub document_artifact_id: ArtifactId,
    pub source_url: String,
    pub text: String,
    pub next_offset_chars: Option<u32>,
}
fn default_search_results() -> u32 {
    5
}
fn default_fetch_bytes() -> u32 {
    1024 * 1024
}
fn default_read_chars() -> u32 {
    8000
}
