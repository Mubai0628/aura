use aura_domain_common::{ContentHash, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "trust_state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TrustState {
    SystemTrusted,
    Trusted { fingerprint: ContentHash, trusted_at: UtcTimestamp },
    Untrusted,
    Revoked,
}
