//! Management projection for a ReadOnly session's "allow for this session" on
//! one memory Vault.
use aura_domain_common::{PermissionGrantId, Revision, SessionId, UtcTimestamp};
use aura_security_contract::MemorySessionConsentRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySessionConsentView {
    pub grant_id: PermissionGrantId,
    pub grant_revision: Revision,
    pub scope: MemorySessionConsentRef,
    /// False once revoked; a revoked answer is kept for the record and covers nothing.
    pub in_force: bool,
    pub issued_at: UtcTimestamp,
    pub revoked_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListMemorySessionConsentsInput {
    pub session_id: SessionId,
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeMemorySessionConsentInput {
    pub session_id: SessionId,
    pub grant_id: PermissionGrantId,
    pub expected_revision: Revision,
}
