use aura_domain_common::{PermissionGrantId, Revision, SessionId, UtcTimestamp};
use aura_security_contract::NetworkSessionConsentRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NetworkSessionConsentAvailability {
    InForce,
    Revoked,
    Expired,
    NotYetValid,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NetworkSessionConsentView {
    pub grant_id: PermissionGrantId,
    pub grant_revision: Revision,
    pub scope: NetworkSessionConsentRef,
    pub availability: NetworkSessionConsentAvailability,
    pub issued_at: UtcTimestamp,
    pub expires_at: Option<UtcTimestamp>,
    pub revoked_at: Option<UtcTimestamp>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListNetworkSessionConsentsInput {
    pub session_id: SessionId,
    #[serde(default)]
    pub include_inactive: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevokeNetworkSessionConsentInput {
    pub session_id: SessionId,
    pub grant_id: PermissionGrantId,
    pub expected_revision: Revision,
}
