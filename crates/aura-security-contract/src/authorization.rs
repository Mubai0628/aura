use crate::{PermissionMode, SecuritySourceRef};
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{
    ApprovalRequestId, CredentialId, CredentialRevision, PermissionGrantId, PermissionSnapshotId,
    Revision, SecurityOperationId, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Policy profile, independent of the UI's file-access shorthand.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SecurityProfile {
    Balanced,
    Strict,
    Guided,
    FullAccess,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum BackendClass {
    CoreBroker,
    RestrictedUserOffline,
    RestrictedUserOnline,
    Native,
    AppContainer,
    Container,
}

/// Independent enforcement requirements, not a total ordering of backends.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementGuarantee {
    /// The process tree cannot mutate user/workspace files. Backend-owned
    /// scratch is separate; AuthorizedWritesOnly alone does not imply this.
    ReadOnlyUserFiles,
    AuthorizedReadsOnly,
    CredentialIsolation,
    IdentityIsolation,
    AuthorizedWritesOnly,
    /// Account-bound direct IPv4/IPv6 connections are blocked. This deliberately
    /// excludes the accepted Dnscache service-mediated DNS path; it is not the
    /// historical OfflineNetwork guarantee and must use a new dispatch binding.
    DirectNetworkBlockedV1,
    OfflineNetwork,
    ExactNetworkOrigins,
    ChildProcessControl,
    BoundedOutput,
}

impl SecurityProfile {
    /// ReadOnly and Workspace retain their capability ceilings within Balanced.
    /// Choosing Strict/Guided is a separate profile decision, not a mode alias.
    #[must_use]
    pub const fn for_permission_mode(mode: PermissionMode) -> Self {
        match mode {
            PermissionMode::FullAccess => Self::FullAccess,
            PermissionMode::ReadOnly | PermissionMode::WorkspaceReadWrite => Self::Balanced,
        }
    }
}

/// References a domain authority and exact secret generation, never a Secret
/// value, WinCred target or a permission to obtain either.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialUseRef {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub authority: SecuritySourceRef,
    pub use_fingerprint: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(transparent)]
pub struct OperationFingerprint(pub CanonicalSha256HashV1);

/// Limits are explicit. An empty allowed class set grants no dispatch route.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendRequirement {
    pub allowed_classes: std::collections::BTreeSet<BackendClass>,
    pub guarantees: std::collections::BTreeSet<EnforcementGuarantee>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "lifetime", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorizationLifetime {
    Once { operation_id: SecurityOperationId },
    TurnTask { session_id: SessionId, turn_task_id: TurnTaskId },
    Session { session_id: SessionId },
    Workspace { workspace_id: WorkspaceId },
}

/// The immutable identity of the approval that first produced a reusable Native
/// session consent.
///
/// A native session consent is the one authorization that deliberately outlives
/// the exact operation it was approved for, so the scope carrying it has to say
/// which approval it came from. `operation_fingerprint` then describes the
/// operation a *derived* decision is issued for, and is allowed to differ from
/// [`Self::approved_operation_fingerprint`] only because this reference exists.
/// Nothing else may compare a stored grant's scope with a decision's scope
/// across two different operations.
///
/// Two revisions are named because the Grant's revision moves when it is revoked
/// and does not move for any other reason. `grant_revision` is the revision the
/// consent was issued at, and `revoked_grant_revision` is the one a withdrawal
/// produced. That is what lets a reader tell "still live", "withdrawn" and "this
/// row does not describe its Grant" apart from the stored facts alone, instead of
/// treating every revision change as corruption and reporting a successful
/// withdrawal as a failure.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionConsentRef {
    #[serde(default)]
    pub lifetime: crate::ShellConsentLifetime,
    /// The consent Grant. Its revisions are checked on every use, so revocation and
    /// consumption are visible without a second lookup.
    pub grant_id: PermissionGrantId,
    /// The revision the consent was issued at.
    pub grant_revision: Revision,
    /// The revision a withdrawal produced, recorded on the Grant itself when the
    /// user withdraws. Absent while the consent is live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_grant_revision: Option<Revision>,
    /// The approval fact this consent was resolved from. A second approval is a
    /// second consent, never a resurrection of this one.
    pub approval_id: ApprovalRequestId,
    /// Issuing Core for audit. Only LegacyRuntime uses this as an expiry condition.
    pub core_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    /// Approved revision for audit. Only LegacyRuntime expires on a later revision.
    pub session_permission_revision: u64,
    /// The actor the user saw, the Shell they confirmed, and the range that answer
    /// covers.
    pub approved: crate::NativeSessionConsentScope,
    /// The operation the user actually confirmed. Kept for audit and for the
    /// management surface; never used as an equality condition on new work.
    pub approved_operation_fingerprint: OperationFingerprint,
}

impl NativeSessionConsentRef {
    /// Whether `revision` is a revision this consent's Grant may hold.
    ///
    /// Exactly two are valid: the one it was issued at, and the one a withdrawal
    /// produced. Anything else means this reference does not describe that Grant.
    #[must_use]
    pub fn accepts_grant_revision(&self, revision: Revision) -> bool {
        revision == self.grant_revision || Some(revision) == self.revoked_grant_revision
    }
}

/// The content fingerprint binds Source, Capability, Resource, Risk and exact
/// credential generations. Backend requirements bound permitted rebinding.
///
/// `consent` is absent for every historical and exact-operation authorization.
/// It is present only on a reusable Native session consent, which is the single
/// documented exception to "a Grant authorizes exactly the operation it was
/// approved for".
///
/// `file_consent` is the same exception for file targets, and it is a **separate field on
/// purpose**. The two consents answer different questions — "this conversation may keep running
/// ordinary Shell commands" versus "this conversation may keep using this file capability on
/// this target" — and they carry different facts (a set of Shells versus a set of capabilities
/// and named targets). One shared field would have to be read as whichever shape happened to be
/// present, which is exactly the ambiguity that would make a reuse check unsound.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationScope {
    pub operation_fingerprint: OperationFingerprint,
    pub profile: SecurityProfile,
    pub backend_requirement: BackendRequirement,
    pub lifetime: AuthorizationLifetime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<NativeSessionConsentRef>,
    /// The reusable file consent, present only when an ordinary file approval recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_consent: Option<crate::FileSessionConsentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_consent: Option<crate::MemorySessionConsentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_consent: Option<crate::NetworkSessionConsentRef>,
    /// A ReadOnly answer covering one MCP server for the rest of the turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_consent: Option<crate::McpTurnConsentRef>,
    /// The missing capabilities actually shown in a composite approval. Historical exact
    /// approvals omit this field; their exact operation fingerprint remains authoritative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_limit: Option<Vec<crate::ResolvedCapability>>,
}

impl AuthorizationScope {
    /// Builds the scope of a decision derived for `operation_fingerprint` from a stored
    /// scope, or `None` when the stored scope cannot authorize that operation.
    ///
    /// The same fingerprint always derives, and the result is the stored scope verbatim:
    /// that is the historical exact-operation case and it stays bit-for-bit compatible
    /// with [`Self::derive_for`]'s original meaning.
    ///
    /// A *different* fingerprint is only ever an approved reuse, so it requires one of the two
    /// consents. Without one this returns `None` rather than a rewritten scope. Handing back a
    /// scope that carries operation B's fingerprint because the caller asked for B would make
    /// every reader downstream — policy, the final dispatch guard, the Store reservation — see
    /// an authorization for work the user never approved, and would leave the invariant resting
    /// on each of them re-checking the fingerprint rather than on the scope itself.
    ///
    /// Which consent applies, and whether it really covers this operation, is decided by policy,
    /// which sees the operation itself. This function only decides whether the difference is
    /// *provable*.
    #[must_use]
    pub fn derive_for(stored: &Self, operation_fingerprint: &OperationFingerprint) -> Option<Self> {
        if stored.operation_fingerprint == *operation_fingerprint {
            return Some(stored.clone());
        }
        if stored.consent.is_none()
            && stored.file_consent.is_none()
            && stored.network_consent.is_none()
            && stored.memory_consent.is_none()
            && stored.mcp_consent.is_none()
        {
            return None;
        }
        Some(Self { operation_fingerprint: operation_fingerprint.clone(), ..stored.clone() })
    }

    /// True when this scope authorizes one exact operation only.
    #[must_use]
    pub const fn is_exact_operation(&self) -> bool {
        self.consent.is_none()
            && self.file_consent.is_none()
            && self.network_consent.is_none()
            && self.memory_consent.is_none()
            && self.mcp_consent.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationGrant {
    pub grant_id: PermissionGrantId,
    pub revision: Revision,
    pub scope: AuthorizationScope,
    pub issued_at: UtcTimestamp,
    pub expires_at: Option<UtcTimestamp>,
    pub revoked_at: Option<UtcTimestamp>,
    pub consumed_at: Option<UtcTimestamp>,
}

impl AuthorizationGrant {
    #[must_use]
    pub fn is_live_at(&self, now: UtcTimestamp) -> bool {
        self.revision.value() > 0
            && self.issued_at <= now
            && self.expires_at.is_none_or(|expires| now < expires)
            && self.revoked_at.is_none()
            && self.consumed_at.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "basis", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorizationBasis {
    /// Core derives this from the live authenticated Main channel for closed
    /// user-interface commands. It is not a grant to Agent, Shell or Native.
    AuthenticatedMain {
        principal_id: aura_domain_common::PrincipalId,
        core_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
        main_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
        channel_id: aura_domain_common::IpcChannelId,
    },
    PermissionMode {
        snapshot_id: PermissionSnapshotId,
        mode: PermissionMode,
        session_permission_revision: u64,
    },
    Grant {
        grant_id: PermissionGrantId,
        revision: Revision,
    },
    Composite {
        slices: Vec<crate::CapabilityAuthorization>,
    },
}

/// Core-issued decision facts. Deserialization does not authenticate a decision;
/// applications must revalidate the authority and cancellation revision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationDecision {
    pub basis: AuthorizationBasis,
    pub scope: AuthorizationScope,
    pub decided_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
}

impl AuthorizationDecision {
    /// The store must supply the current grant revision. This does not reserve
    /// or consume the grant; consumption is atomic with dispatch preparation.
    pub fn from_grant(
        grant: &AuthorizationGrant,
        now: UtcTimestamp,
        expires_at: UtcTimestamp,
    ) -> Option<Self> {
        if !grant.is_live_at(now) || expires_at <= now {
            return None;
        }
        // A Grant with no fixed expiry is bounded by something other than a clock —
        // a reusable consent is bounded by its Core runtime instance, its session
        // permission revision and withdrawal. The short-lived decision still has to
        // become *stricter* than the Grant, so it keeps its own deadline instead of
        // having nothing to compare against.
        if let Some(limit) = grant.expires_at
            && expires_at > limit
        {
            return None;
        }
        Some(Self {
            basis: AuthorizationBasis::Grant { grant_id: grant.grant_id, revision: grant.revision },
            scope: grant.scope.clone(),
            decided_at: now,
            expires_at,
        })
    }
    /// Derives a decision for `operation_fingerprint` from a stored Grant.
    ///
    /// For an exact-operation Grant the derived scope equals the stored scope and
    /// this is the same decision [`Self::from_grant`] would produce — including for a
    /// request naming a *different* operation, which is refused: a Grant approved for
    /// one Operation must not be handed back as an authorization for another just
    /// because the caller named it. For a reusable Native session consent the stored
    /// scope keeps the fingerprint the user approved and the derived scope moves to the
    /// operation being dispatched, carrying [`AuthorizationScope::consent`] as the proof
    /// that the difference is an approved reuse rather than a substituted authorization.
    pub fn from_grant_for_operation(
        grant: &AuthorizationGrant,
        operation_fingerprint: &OperationFingerprint,
        now: UtcTimestamp,
        expires_at: UtcTimestamp,
    ) -> Option<Self> {
        let mut decision = Self::from_grant(grant, now, expires_at)?;
        decision.scope = AuthorizationScope::derive_for(&grant.scope, operation_fingerprint)?;
        Some(decision)
    }
    pub fn fingerprint(
        &self,
    ) -> Result<CanonicalSha256HashV1, aura_domain_common::jcs::CanonicalJsonError> {
        canonical_sha256(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationApproval {
    pub approval_id: aura_domain_common::ApprovalRequestId,
    pub revision: Revision,
    pub scope: AuthorizationScope,
    pub allowed_decisions: Vec<crate::ApprovalDecisionKind>,
    pub state: crate::ApprovalState,
    pub requested_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
}
