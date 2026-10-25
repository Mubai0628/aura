use crate::{ApprovalDecisionKind, AuthorizationDecision, AuthorizationScope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDenyReason {
    ModeDenied,
    ApprovalRequired,
    InvalidOperation,
    IdentityMismatch,
    SourceDisabled,
    SourceUntrusted,
    HardBoundary,
    DeniedForTurn,
    NetworkDeclaration,
    ProfileProhibited,
    BackendUnavailable,
    DispatchAlreadyStarted,
}

/// What a "本会话允许" answer would create for a Native Shell call.
///
/// The confirmation dialog has to describe the choice it is offering, and the
/// choice is not the same for every backend. For an ordinary restricted backend a
/// session approval covers the Operation's own kind of work; for Native it creates
/// one reusable consent that later calls can be derived from. Saying which one is
/// on the table is a fact about the proposal, so it travels with the proposal
/// rather than being guessed in the Renderer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionConsentFact {
    /// The built-in Shell the user actually confirmed. Audit and wording only.
    pub shell: crate::NativeShell,
    /// The trusted built-in Shells a session approval would cover. This is the range
    /// the dialog has to describe, and it is not derived in the Renderer.
    pub shells: std::collections::BTreeSet<crate::NativeShell>,
    /// The principal the consent would be bound to.
    pub principal_id: aura_domain_common::PrincipalId,
    /// The session permission revision it would be bound to. Any change to this
    /// session's permissions ends it, so the dialog can say so truthfully.
    pub session_permission_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationProposal {
    pub scope: AuthorizationScope,
    pub allowed_decisions: Vec<ApprovalDecisionKind>,
    /// Stable reason codes; actual command/resources come from immutable content.
    pub reasons: Vec<String>,
    /// Only capabilities not already covered by verified authority are requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_capabilities: Option<Vec<crate::ResolvedCapability>>,
    /// Present only when a session approval here would create a reusable Native
    /// session consent. Absent means the session answer keeps its existing, exact
    /// meaning for this Operation's own class of work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_consent: Option<NativeSessionConsentFact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyAuthorization {
    Allow { authorization: AuthorizationDecision },
    Ask { proposal: AuthorizationProposal },
    Deny { reason: PolicyDenyReason },
}
