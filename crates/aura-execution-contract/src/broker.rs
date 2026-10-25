use aura_domain_common::jcs::CanonicalSha256HashV1;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stored evidence from the owning service, not a renderer success flag.
/// Domain facts contain identities/revisions/digests, never credential values.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerEvidence {
    pub action_kind: String,
    pub request_fingerprint: CanonicalSha256HashV1,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    pub domain_journal: serde_json::Value,
    pub side_effect: crate::SideEffectStateV1,
}
