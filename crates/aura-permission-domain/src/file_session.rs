//! Management projection for explicit session file approval.
use aura_domain_common::{PermissionGrantId, Revision, SessionId, UtcTimestamp};
use aura_security_contract::FileSessionConsentRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSessionConsentAvailability {
    InForce,
    Revoked,
    Expired,
    TargetChanged,
    Unavailable,
    NotYetValid,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileSessionConsentView {
    pub grant_id: PermissionGrantId,
    pub grant_revision: Revision,
    pub scope: FileSessionConsentRef,
    pub availability: FileSessionConsentAvailability,
    pub issued_at: UtcTimestamp,
    pub expires_at: Option<UtcTimestamp>,
    pub revoked_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListFileSessionConsentsInput {
    pub session_id: SessionId,
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeFileSessionConsentInput {
    pub session_id: SessionId,
    pub grant_id: PermissionGrantId,
    pub expected_revision: Revision,
}
