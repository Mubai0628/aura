use super::HttpHeader;
use aura_domain_common::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpBodyKind {
    Text,
    Json,
    Binary,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpRequestResult {
    pub status: u16,
    pub headers: Vec<HttpHeader>,
    pub body_kind: HttpBodyKind,
    pub text: Option<String>,
    pub json: Option<serde_json::Value>,
    pub truncated: bool,
    pub bytes_received: u64,
    /// Safe display URL: credentials and query values are never exposed here.
    pub final_url: String,
    pub redirects: Vec<String>,
    pub elapsed_ms: u64,
    pub transfer_complete: bool,
    pub hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpDownloadResult {
    pub status: u16,
    pub path: String,
    pub bytes_written: u64,
    pub hash: ContentHash,
    pub content_type: Option<String>,
    pub final_url: String,
    pub overwritten: bool,
    pub published: bool,
}
