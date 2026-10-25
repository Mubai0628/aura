use aura_domain_common::{NetworkGrantId, SessionId, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Credential-free read. Authority is reloaded from Core's Store; grant IDs
/// identify existing approvals and cannot declare origins or destination classes.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerHttpReadRequest {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub url: String,
    pub network_grant_id: NetworkGrantId,
    pub max_bytes: u32,
}
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerHttpReadResult {
    pub status: u16,
    pub body_base64: String,
    pub content_hash: aura_domain_common::ContentHash,
    pub final_origin: crate::NormalizedOrigin,
}
