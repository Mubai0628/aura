#![forbid(unsafe_code)]

use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{
    ArtifactId, ContentHash, DeferredMutationId, ModelAvailabilityTestId, ModelDiscoveryRunId,
    SessionId, UtcTimestamp, WorkspaceId, WorkspacePath,
};
use aura_domain_common::{
    ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId, ToolExecutionAttemptId,
    ToolPackageId, ToolRegistrySnapshotId,
};
use aura_workspace_domain::{ArtifactExportTarget, FileStorageSource, FsMutationDisposition};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactScope {
    Session,
    RuntimeEphemeral,
    WorkspaceCache,
    SystemCache,
    Quarantine,
    PackageStore,
    ToolExecution,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactOwnerRef {
    Session { session_id: SessionId },
    Operation { operation: ArtifactOperationOwnerRef },
    System { owner: ArtifactSystemOwner },
    Tool { owner: ToolArtifactOwnerRef },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactOperationOwnerRef {
    ModelDiscovery { model_discovery_run_id: ModelDiscoveryRunId },
    ModelAvailabilityTest { model_availability_test_id: ModelAvailabilityTestId },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSystemOwner {
    ProviderCatalogCache,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolArtifactOwnerRef {
    PackageVersion {
        tool_package_id: ToolPackageId,
        package_version: CanonicalSemVer,
        package_fingerprint: CanonicalHashV1,
    },
    Draft {
        tool_draft_id: ToolDraftId,
    },
    ConformanceRun {
        tool_conformance_run_id: ToolConformanceRunId,
    },
    Batch {
        tool_batch_id: ToolBatchId,
    },
    ToolCall {
        tool_call_id: ToolCallId,
    },
    ExecutionAttempt {
        tool_execution_attempt_id: ToolExecutionAttemptId,
    },
    RegistrySnapshot {
        tool_registry_snapshot_id: ToolRegistrySnapshotId,
    },
}

#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ArtifactAccess(u32);

impl ArtifactAccess {
    pub const NONE: Self = Self(0);
    pub const USER_READ: Self = Self(1 << 0);
    pub const MODEL_READ: Self = Self(1 << 1);
    pub const EXPORT: Self = Self(1 << 2);
    pub const ATTACH: Self = Self(1 << 3);
    pub const SEARCH: Self = Self(1 << 4);
    pub const ALL: Self = Self(
        Self::USER_READ.0 | Self::MODEL_READ.0 | Self::EXPORT.0 | Self::ATTACH.0 | Self::SEARCH.0,
    );

    #[must_use]
    pub const fn from_bits(bits: u32) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 { Some(Self(bits)) } else { None }
    }

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl<'de> Deserialize<'de> for ArtifactAccess {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bits = u32::deserialize(deserializer)?;
        Self::from_bits(bits).ok_or_else(|| D::Error::custom("artifact access has unknown bits"))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRecord {
    pub artifact_id: ArtifactId,
    pub owner: ArtifactOwnerRef,
    pub scope: ArtifactScope,
    pub media_type: String,
    pub access: ArtifactAccess,
    pub byte_length: u64,
    pub content_hash: ContentHash,
    pub inline: bool,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactAccessSubjectV1 {
    Session { session_id: SessionId },
    Workspace { workspace_id: WorkspaceId },
    ToolCall { tool_call_id: ToolCallId },
    ExecutionAttempt { tool_execution_attempt_id: ToolExecutionAttemptId },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactAccessLinkV1 {
    pub artifact_id: ArtifactId,
    pub subject: ArtifactAccessSubjectV1,
    pub access: ArtifactAccess,
    pub created_at: UtcTimestamp,
    pub evidence_hash: CanonicalHashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactAccessLinkEvidencePreimageV1<'a> {
    kind: &'static str,
    preimage_version: u8,
    artifact_id: ArtifactId,
    subject: &'a ArtifactAccessSubjectV1,
    access: ArtifactAccess,
    created_at: UtcTimestamp,
}

impl ArtifactAccessLinkV1 {
    pub fn new(
        artifact_id: ArtifactId,
        subject: ArtifactAccessSubjectV1,
        access: ArtifactAccess,
        created_at: UtcTimestamp,
    ) -> Result<Self, ArtifactAccessLinkErrorV1> {
        if access == ArtifactAccess::NONE {
            return Err(ArtifactAccessLinkErrorV1::EmptyAccess);
        }
        let evidence_hash =
            artifact_access_link_evidence_hash(artifact_id, &subject, access, created_at)?;
        Ok(Self { artifact_id, subject, access, created_at, evidence_hash })
    }

    pub fn validate(&self) -> Result<(), ArtifactAccessLinkErrorV1> {
        if self.access == ArtifactAccess::NONE {
            return Err(ArtifactAccessLinkErrorV1::EmptyAccess);
        }
        let expected = artifact_access_link_evidence_hash(
            self.artifact_id,
            &self.subject,
            self.access,
            self.created_at,
        )?;
        if expected != self.evidence_hash {
            return Err(ArtifactAccessLinkErrorV1::EvidenceHashMismatch);
        }
        Ok(())
    }
}

fn artifact_access_link_evidence_hash(
    artifact_id: ArtifactId,
    subject: &ArtifactAccessSubjectV1,
    access: ArtifactAccess,
    created_at: UtcTimestamp,
) -> Result<CanonicalHashV1, ArtifactAccessLinkErrorV1> {
    canonical_sha256(&ArtifactAccessLinkEvidencePreimageV1 {
        kind: "aura.p07.artifact_access_link",
        preimage_version: 1,
        artifact_id,
        subject,
        access,
        created_at,
    })
    .map_err(|_| ArtifactAccessLinkErrorV1::Canonicalization)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactAccessLinkErrorV1 {
    EmptyAccess,
    Canonicalization,
    EvidenceHashMismatch,
}

impl std::fmt::Display for ArtifactAccessLinkErrorV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyAccess => "artifact access links must grant at least one capability",
            Self::Canonicalization => "artifact access link evidence cannot be canonicalized",
            Self::EvidenceHashMismatch => {
                "artifact access link evidence hash does not match its immutable facts"
            }
        })
    }
}

impl std::error::Error for ArtifactAccessLinkErrorV1 {}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactWriteRequestV1 {
    pub owner: ArtifactOwnerRef,
    pub scope: ArtifactScope,
    pub media_type: String,
    pub access: ArtifactAccess,
    pub byte_length: u64,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactWriteResultV1 {
    pub artifact: ArtifactRecord,
    pub content_object_reused: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReadRequestV1 {
    pub artifact_id: ArtifactId,
    pub subject: ArtifactAccessSubjectV1,
    pub required_access: ArtifactAccess,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReadGrantV1 {
    pub artifact: ArtifactRecord,
    pub granted_access: ArtifactAccess,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureWorkspaceFileResult {
    pub artifact: ArtifactRecord,
    pub path: WorkspacePath,
    pub source: FileStorageSource,
    pub logical_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactExportResult {
    pub artifact: ArtifactRecord,
    pub target: ArtifactExportTarget,
    pub disposition: FsMutationDisposition,
    pub content_hash: ContentHash,
    pub deferred_mutation_id: Option<DeferredMutationId>,
    pub requires_reread: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactAccess, ArtifactAccessLinkV1, ArtifactAccessSubjectV1, ArtifactOwnerRef,
        ToolArtifactOwnerRef,
    };
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{ArtifactId, SessionId, UtcTimestamp};
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use serde_json::json;

    #[test]
    fn tool_owner_is_a_single_typed_direct_owner() {
        let owner = ArtifactOwnerRef::Tool {
            owner: ToolArtifactOwnerRef::ExecutionAttempt {
                tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            },
        };
        let wire = serde_json::to_value(owner).expect("owner wire");
        assert_eq!(wire["type"], json!("tool"));
        assert_eq!(wire["owner"]["type"], json!("execution_attempt"));
        let _ = ToolCallId::new();
    }

    #[test]
    fn unknown_access_bits_are_rejected() {
        assert!(serde_json::from_value::<ArtifactAccess>(json!(1_u32 << 31)).is_err());
    }

    #[test]
    fn access_link_is_hash_bound_and_rejects_empty_access() {
        let created_at = UtcTimestamp::now();
        let link = ArtifactAccessLinkV1::new(
            ArtifactId::new(),
            ArtifactAccessSubjectV1::Session { session_id: SessionId::new() },
            ArtifactAccess::MODEL_READ,
            created_at,
        )
        .expect("access link");
        link.validate().expect("valid evidence");

        let mut wire = serde_json::to_value(&link).expect("link wire");
        wire["evidence_hash"] =
            serde_json::to_value(CanonicalHashV1::sha256(b"tampered")).expect("hash wire");
        let tampered: ArtifactAccessLinkV1 = serde_json::from_value(wire).expect("wire shape");
        assert!(tampered.validate().is_err());
        assert!(
            ArtifactAccessLinkV1::new(
                ArtifactId::new(),
                ArtifactAccessSubjectV1::Session { session_id: SessionId::new() },
                ArtifactAccess::NONE,
                created_at,
            )
            .is_err()
        );
    }
}
