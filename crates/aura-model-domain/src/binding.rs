use aura_domain_common::{
    ContentHash, EndpointId, ModelProfileId, ProviderId, Revision, SessionId, UtcTimestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModelBinding {
    pub session_id: SessionId,
    pub model_profile_id: ModelProfileId,
    pub binding_revision: Revision,
    pub bound_at: UtcTimestamp,
    pub readable_ref_at_binding: String,
}

/// Frozen P05 seed consumed by the Session owner while creating a Session and
/// its N-A model-channel authorization in one transaction.
///
/// This type is intentionally Rust-only and cannot cross the Renderer wire.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionModelBindingSeed {
    pub provider_id: ProviderId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub readable_ref_at_binding: String,
    pub models_revision_at_binding: Revision,
    pub endpoint_revision_at_binding: Revision,
    pub model_profile_revision_at_binding: Revision,
    pub endpoint_security_fingerprint_at_binding: ContentHash,
    pub api_format_at_binding: crate::ModelApiFormat,
}

/// Result of resolving the default model against one immutable model-config
/// snapshot. P05 resolves this; the Session owner decides when to persist it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitialSessionModelBinding {
    Bound { seed: SessionModelBindingSeed },
    Unbound { issue_code: aura_domain_common::NormalizedIssueCode },
}
