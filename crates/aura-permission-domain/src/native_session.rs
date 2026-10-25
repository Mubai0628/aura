//! Contract types for the reusable Native session consent.
//!
//! These live in the contract layer rather than beside the Core service that
//! produces them, because the management surface is an IPC route and every route
//! payload has to be part of the generated contract. The authorization facts
//! themselves are owned by the Store; these are the projection a person reads.
//!
//! The vocabulary is deliberately small. A consent covers one session, one
//! workspace, the approved built-in Shell range and one principal. New answers
//! last for the session; legacy answers retain their old runtime/revision limits.
//! Nothing here grants anything: dispatch re-checks every one of these facts in its
//! own transaction.

use aura_domain_common::{
    ApprovalRequestId, CorrelationId, PermissionGrantId, PrincipalId, Revision, RuntimeInstanceId,
    SessionId, UtcTimestamp, WorkspaceId,
};
use aura_security_contract::{NativeShell, OperationFingerprint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether a consent still authorizes new work right now.
///
/// "Revoked" and "expired" are different things to the person reading the list:
/// one was their own action, the other is a window that closed. A `consumed`
/// one-time authorization is neither, and is never a consent.
///
/// `Superseded` identifies a legacy answer from a different Core runtime. New
/// session answers survive Core restart and permission mode changes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSessionConsentAvailability {
    InForce,
    Revoked,
    Consumed,
    Expired,
    /// A legacy answer issued by a Core runtime other than the one now running.
    Superseded,
    NotYetValid,
}

impl NativeSessionConsentAvailability {
    #[must_use]
    pub const fn is_in_force(self) -> bool {
        matches!(self, Self::InForce)
    }
}

/// What one Native session consent covers.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionConsentScopeView {
    #[serde(default)]
    pub lifetime: aura_security_contract::ShellConsentLifetime,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    /// The built-in Shell the user actually confirmed. Audit and wording; the
    /// covered range is `shells`.
    pub approved_shell: NativeShell,
    /// The trusted built-in Shells this consent authorizes. It is stored rather than
    /// derived from `approved_shell`, because "which tool was approved" and "what
    /// the approval covers" are different facts.
    pub shells: std::collections::BTreeSet<NativeShell>,
    /// The actor the consent was approved for. An Agent-initiated call in the same
    /// session is a different principal and is not covered.
    pub principal_id: PrincipalId,
    /// Revision when the user answered, retained for audit. Only legacy answers
    /// expire when this revision changes; new session answers persist.
    pub approved_permission_revision: u64,
    /// Issuing Core for audit; only legacy records expire on a Core restart.
    pub core_runtime_instance_id: RuntimeInstanceId,
    /// The Operation the user actually confirmed. It is audit evidence, never an
    /// equality condition on later work.
    pub approved_operation_fingerprint: OperationFingerprint,
}

/// One consent as the management surface shows it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionConsentView {
    /// The authorization that carries the consent. This is the withdrawal handle.
    pub grant_id: PermissionGrantId,
    pub grant_revision: Revision,
    pub approval_id: ApprovalRequestId,
    pub scope: NativeSessionConsentScopeView,
    pub availability: NativeSessionConsentAvailability,
    pub issued_at: UtcTimestamp,
    /// The confirmation window that produced it. It bounds the *approval*, not the
    /// consent: an in-force consent does not become invalid when this passes.
    pub approved_through: UtcTimestamp,
    pub revoked_at: Option<UtcTimestamp>,
    /// The Native backend this consent authorizes. A display fact, not an
    /// authorization input.
    pub backend_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListNativeSessionConsentsInput {
    pub session_id: SessionId,
    /// Include revoked, consumed and expired consents. The default is in-force
    /// only, because that is the list a person acts on.
    #[serde(default)]
    pub include_inactive: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeNativeSessionConsentInput {
    pub session_id: SessionId,
    pub grant_id: PermissionGrantId,
    /// The revision the caller last saw. Withdrawal is a compare-and-swap, so a
    /// stale list cannot withdraw an authorization that has since changed.
    pub expected_revision: Revision,
    pub correlation_id: CorrelationId,
}
