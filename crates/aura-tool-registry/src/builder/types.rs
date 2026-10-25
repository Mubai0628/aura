use super::*;

/// Trusted source evidence is created by Core adapters, never deserialized from
/// Package, model, Renderer, or external Runtime input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolSourceEligibilityEvidenceV1 {
    VerifiedAgentBuiltinRelease {
        release_manifest_hash: CanonicalHashV1,
    },
    VerifiedBuiltinRelease {
        release_manifest_hash: CanonicalHashV1,
        verification_evidence_hash: CanonicalHashV1,
    },
    Package {
        current_security_fingerprint: aura_tool_contract::PackageSecurityFingerprintV1,
        trust_record: Option<ToolPackageTrustRecordV1>,
        payload_verified: bool,
        conformance_passed: bool,
        instance_configuration_ready: bool,
        credential_binding_ready: bool,
        deletion_pending: bool,
    },
    ExternalSource {
        source_enabled: bool,
        source_trusted: bool,
        source_runtime_healthy: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryCandidateV1 {
    pub definition: ToolDefinitionRecordV1,
    pub instance: ToolInstanceRecordV1,
    pub source_evidence: ToolSourceEligibilityEvidenceV1,
    /// The authoritative installed/source adapter selected this exact version
    /// for unpinned discovery. Registry never infers "latest" by SemVer.
    pub active_for_unpinned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentRegistryBuildInputV1 {
    pub registry_revision: Revision,
    pub scope: ToolRegistryScopeV1,
    pub settings: ToolSettingsSnapshotV1,
    /// Hash of already-resolved mode and permission inputs. Registry includes it
    /// in its own context hash but does not reinterpret P04/P08 policy.
    pub mode_permission_context_hash: CanonicalHashV1,
    pub core_exposed_tool_ids: BTreeSet<ToolId>,
    pub candidates: Vec<RegistryCandidateV1>,
    pub workspace_pins: Vec<ToolPinV1>,
    pub session_pins: Vec<ToolPinV1>,
    pub ephemeral_loads: Vec<ToolEphemeralLoadRecordV1>,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryExclusionReasonV1 {
    Disabled,
    TrustRequired,
    RuntimeUnavailable,
    PayloadUnavailable,
    ConformanceRequired,
    InstanceConfigurationRequired,
    CredentialBindingRequired,
    DeletionPending,
    AudienceDenied,
    ReservedNamespace,
    InactiveVersion,
    ExactVersionUnavailable,
    PinUnresolved,
    PinDegraded,
    EphemeralShadowedByPin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryExclusionV1 {
    pub tool_id: ToolId,
    pub requested_version: Option<ToolVersion>,
    pub reason: RegistryExclusionReasonV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentRegistryBuildOutputV1 {
    pub snapshot: ToolRegistrySnapshotV1,
    pub search_documents: Vec<ToolSearchDocumentV1>,
    pub exclusions: Vec<RegistryExclusionV1>,
}

pub const CHILD_REGISTRY_POLICY_VERSION_V1: u32 = 1;

/// Exact parent-Snapshot definition material supplied by Core. The child
/// derivation validates this material against the immutable parent `trs_`
/// before it evaluates any child policy input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildRegistryParentToolV1 {
    pub definition: ToolDefinitionRecordV1,
    pub instance: ToolInstanceRecordV1,
}

/// P13-owned, already resolved policy intersections consumed by P07. P07 does
/// not reinterpret Workspace Mode, permission, delegation, or token policy;
/// it proves that each ordinary parent Tool survives every frozen allow-set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildRegistryDerivationInputV1 {
    pub registry_revision: Revision,
    pub scope: ToolRegistryScopeV1,
    pub parent_snapshot: ToolRegistrySnapshotV1,
    pub parent_tools: Vec<ChildRegistryParentToolV1>,
    pub child_runtime_required_candidates: Vec<RegistryCandidateV1>,
    pub workspace_mode_hash: CanonicalHashV1,
    pub user_subagent_policy_revision: Revision,
    pub user_subagent_policy_hash: CanonicalHashV1,
    pub parent_permission_snapshot_id: PermissionSnapshotId,
    pub parent_permission_snapshot_hash: CanonicalHashV1,
    pub system_hard_allowed_tool_ids: BTreeSet<ToolId>,
    pub user_subagent_allowed_tool_ids: BTreeSet<ToolId>,
    pub parent_permission_allowed_tool_ids: BTreeSet<ToolId>,
    pub workspace_mode_allowed_tool_ids: BTreeSet<ToolId>,
    pub parent_delegated_tool_ids: BTreeSet<ToolId>,
    pub runtime_suitable_tool_ids: BTreeSet<ToolId>,
    pub capacity_allowed_tool_ids: BTreeSet<ToolId>,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChildRegistryExclusionReasonV1 {
    ParentAgentOnly,
    SystemHardDenied,
    UserSubagentPolicy,
    ParentPermission,
    WorkspaceMode,
    ParentDelegation,
    RuntimeUnsuitable,
    CapacityOrTokenBudget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildRegistryExclusionV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub reason: ChildRegistryExclusionReasonV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildRegistryDerivationOutputV1 {
    pub snapshot: ToolRegistrySnapshotV1,
    pub search_documents: Vec<ToolSearchDocumentV1>,
    pub exclusions: Vec<ChildRegistryExclusionV1>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ChildRegistryDerivationErrorV1 {
    #[error("child Registry derivation requires matching child and parent Session scopes")]
    InvalidScope,
    #[error("the immutable parent Registry Snapshot is invalid")]
    ParentSnapshotInvalid,
    #[error("parent definition material does not exactly match the parent Snapshot")]
    ParentDefinitionMismatch,
    #[error("a ChildRuntimeRequired injection is not a signed built-in Tool")]
    InvalidRequiredCoreTool,
    #[error("a ChildRuntimeRequired injection conflicts with the parent Snapshot")]
    RequiredCoreToolConflict,
    #[error("child Registry derivation could not be canonicalized")]
    Canonicalization,
    #[error("the derived child Registry Snapshot is invalid")]
    SnapshotInvalid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSearchDocumentV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub definition_fingerprint: CanonicalHashV1,
    pub model_description: String,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RegistryBuildError {
    #[error("registry candidate identity is inconsistent")]
    CandidateIdentity,
    #[error("registry candidate source evidence is inconsistent")]
    CandidateSourceEvidence,
    #[error("registry contains duplicate exact Tool/Version candidates")]
    DuplicateExactCandidate,
    #[error("different Tool identities claim the same canonical name")]
    DuplicateCanonicalName,
    #[error("registry contains multiple authoritative active versions for one Tool")]
    DuplicateActiveVersion,
    #[error("registry contains duplicate Pins or Ephemeral Loads for one Tool")]
    DuplicateSelection,
    #[error("parent registry builder requires a workspace or parent session scope")]
    InvalidScope,
    #[error("persisted pinned/loaded Tool count exceeds the frozen dynamic Schema limit")]
    CapacityRemediationRequired,
    #[error("registry snapshot could not be constructed")]
    SnapshotInvalid,
}

impl RegistryBuildError {
    #[must_use]
    pub const fn public_code(&self) -> ToolRegistryErrorCode {
        match self {
            Self::CapacityRemediationRequired => ToolRegistryErrorCode::CapacityExceeded,
            Self::CandidateIdentity
            | Self::CandidateSourceEvidence
            | Self::DuplicateExactCandidate
            | Self::DuplicateCanonicalName
            | Self::DuplicateActiveVersion
            | Self::DuplicateSelection
            | Self::InvalidScope
            | Self::SnapshotInvalid => ToolRegistryErrorCode::SnapshotCorrupted,
        }
    }
}
