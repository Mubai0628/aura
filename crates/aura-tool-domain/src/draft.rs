use std::ffi::OsStr;

use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{ArtifactId, ContentHash, Revision, UtcTimestamp};
use aura_domain_common::{ToolConformanceRunId, ToolDraftId, ToolPackageId};
use aura_tool_contract::{
    PackageFingerprintV1, PackageKey, PackageRelativePath, PackageRuntimeClosureHashV1,
    PackageSecurityFingerprintV1, TOOL_PACKAGE_IMPORT_POLICY_V1, TOOL_PACKAGE_MANIFEST_NAME,
    ToolPackageFileSha256V1,
};
use aura_windows_platform::windows_ordinal_compare_ignore_case;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CanonicalToolPackageManifestV1, ConformanceDraftRevisionRefV1};

pub const TOOL_DRAFT_SNAPSHOT_TRANSFER_SCHEMA_VERSION_V1: u32 = 1;
pub const TOOL_DRAFT_SNAPSHOT_TRANSFER_MEDIA_TYPE_V1: &str =
    "application/vnd.aura.tool-draft-snapshot.v1+json";

/// Exact immutable Artifact identity used by the Draft Snapshot Transfer
/// envelope. The Artifact store still verifies its own content hash; these
/// fields additionally bind the transfer contract to the authored SHA-256
/// bytes and length.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftTransferArtifactRefV1 {
    pub artifact_id: ArtifactId,
    pub byte_length: u64,
    pub sha256: ToolPackageFileSha256V1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftSnapshotTransferFileV1 {
    pub path: PackageRelativePath,
    pub artifact: ToolDraftTransferArtifactRefV1,
}

/// Canonical transport envelope for replacing unvalidated Draft content.
///
/// This is deliberately not a Tool Package Manifest and validation here does
/// not imply that `raw_manifest_utf8` is a valid manifest. Core must authorize
/// and verify the envelope Artifact and every child Artifact before converting
/// this value to `DraftPackageContentV1`.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftSnapshotTransferV1 {
    pub schema_version: u32,
    pub raw_manifest_utf8: String,
    pub raw_manifest_byte_length: u64,
    pub raw_manifest_sha256: ToolPackageFileSha256V1,
    pub files: Vec<ToolDraftSnapshotTransferFileV1>,
    pub snapshot_hash: CanonicalSha256HashV1,
}

impl ToolDraftSnapshotTransferV1 {
    pub fn new(
        raw_manifest_utf8: String,
        mut files: Vec<ToolDraftSnapshotTransferFileV1>,
    ) -> Result<Self, ToolDraftContractError> {
        files.sort_by(|left, right| {
            left.path.as_str().as_bytes().cmp(right.path.as_str().as_bytes())
        });
        let raw_manifest_byte_length = u64::try_from(raw_manifest_utf8.len())
            .map_err(|_| ToolDraftContractError::ImportPolicyExceeded)?;
        let raw_manifest_sha256 = ToolPackageFileSha256V1::sha256(raw_manifest_utf8.as_bytes());
        let snapshot_hash = canonical_sha256(&DraftPackageSnapshotHashPreimageV1 {
            preimage_version: 1,
            raw_manifest_byte_length,
            raw_manifest_sha256: raw_manifest_sha256.clone(),
            files: files
                .iter()
                .map(|file| DraftPackageSnapshotHashFileV1 {
                    path: file.path.clone(),
                    byte_length: file.artifact.byte_length,
                    raw_file_sha256: file.artifact.sha256.clone(),
                })
                .collect(),
        })
        .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?;
        let transfer = Self {
            schema_version: TOOL_DRAFT_SNAPSHOT_TRANSFER_SCHEMA_VERSION_V1,
            raw_manifest_utf8,
            raw_manifest_byte_length,
            raw_manifest_sha256,
            files,
            snapshot_hash,
        };
        transfer.validate()?;
        Ok(transfer)
    }

    pub fn validate(&self) -> Result<(), ToolDraftContractError> {
        let manifest_bytes = self.raw_manifest_utf8.as_bytes();
        if self.schema_version != TOOL_DRAFT_SNAPSHOT_TRANSFER_SCHEMA_VERSION_V1
            || u64::try_from(manifest_bytes.len()).ok() != Some(self.raw_manifest_byte_length)
            || ToolPackageFileSha256V1::sha256(manifest_bytes) != self.raw_manifest_sha256
            || self.raw_manifest_byte_length > TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes
            || self.files.len()
                > usize::try_from(TOOL_PACKAGE_IMPORT_POLICY_V1.max_entries.saturating_sub(1))
                    .unwrap_or(usize::MAX)
        {
            return Err(ToolDraftContractError::SnapshotIntegrity);
        }

        let mut total = self.raw_manifest_byte_length;
        for (index, file) in self.files.iter().enumerate() {
            if file.path.as_str() == TOOL_PACKAGE_MANIFEST_NAME
                || file.artifact.byte_length > TOOL_PACKAGE_IMPORT_POLICY_V1.max_file_bytes
            {
                return Err(ToolDraftContractError::ImportPolicyExceeded);
            }
            if index > 0
                && self.files[index - 1].path.as_str().as_bytes() >= file.path.as_str().as_bytes()
            {
                return Err(ToolDraftContractError::SnapshotIntegrity);
            }
            for previous in &self.files[..index] {
                if windows_ordinal_compare_ignore_case(
                    OsStr::new(previous.path.as_str()),
                    OsStr::new(file.path.as_str()),
                )
                .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?
                .is_eq()
                {
                    return Err(ToolDraftContractError::SnapshotIntegrity);
                }
            }
            total = total
                .checked_add(file.artifact.byte_length)
                .ok_or(ToolDraftContractError::ImportPolicyExceeded)?;
        }
        if total > TOOL_PACKAGE_IMPORT_POLICY_V1.max_expanded_bytes
            || snapshot_hash_from_transfer(self)? != self.snapshot_hash
        {
            return Err(ToolDraftContractError::SnapshotIntegrity);
        }
        Ok(())
    }
}

fn snapshot_hash_from_transfer(
    transfer: &ToolDraftSnapshotTransferV1,
) -> Result<CanonicalSha256HashV1, ToolDraftContractError> {
    canonical_sha256(&DraftPackageSnapshotHashPreimageV1 {
        preimage_version: 1,
        raw_manifest_byte_length: transfer.raw_manifest_byte_length,
        raw_manifest_sha256: transfer.raw_manifest_sha256.clone(),
        files: transfer
            .files
            .iter()
            .map(|file| DraftPackageSnapshotHashFileV1 {
                path: file.path.clone(),
                byte_length: file.artifact.byte_length,
                raw_file_sha256: file.artifact.sha256.clone(),
            })
            .collect(),
    })
    .map_err(|_| ToolDraftContractError::SnapshotIntegrity)
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolDraftActorV1 {
    User,
    ParentAgent,
    ChildAgent,
    SystemRuntime,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolDraftKindV1 {
    ContentMutation,
    PackageDeletion,
}

pub const TOOL_PACKAGE_DELETION_POLICY_VERSION_V1: u32 = 1;

/// Immutable package-deletion Draft revision. The four collection hashes are
/// produced from Core-owned SQLite facts; this contract intentionally does not
/// accept Renderer-supplied impact or runtime-reference collections.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageDeleteDraftSnapshotV1 {
    pub draft: ConformanceDraftRevisionRefV1,
    pub parent_revision: Option<Revision>,
    pub tool_package_id: ToolPackageId,
    pub package_revision: Revision,
    pub active_package_version: CanonicalSemVer,
    pub installed_package_versions_hash: CanonicalSha256HashV1,
    pub package_fingerprint: PackageFingerprintV1,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub tool_identity_set_hash: CanonicalSha256HashV1,
    pub persistent_impact_hash: CanonicalSha256HashV1,
    pub runtime_strong_reference_observed_summary_hash: CanonicalSha256HashV1,
    pub deletion_policy_version: u32,
    pub preview_hash: CanonicalSha256HashV1,
    pub initiator: ToolDraftActorV1,
    pub created_at: UtcTimestamp,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolPackageDeleteDraftSnapshotPreimageV1<'a> {
    preimage_version: u32,
    tool_package_id: ToolPackageId,
    package_revision: Revision,
    active_package_version: &'a CanonicalSemVer,
    installed_package_versions_hash: &'a CanonicalSha256HashV1,
    package_fingerprint: &'a PackageFingerprintV1,
    package_security_fingerprint: &'a PackageSecurityFingerprintV1,
    tool_identity_set_hash: &'a CanonicalSha256HashV1,
    persistent_impact_hash: &'a CanonicalSha256HashV1,
    runtime_strong_reference_observed_summary_hash: &'a CanonicalSha256HashV1,
    deletion_policy_version: u32,
    preview_hash: &'a CanonicalSha256HashV1,
    initiator: ToolDraftActorV1,
    created_at: UtcTimestamp,
}

impl ToolPackageDeleteDraftSnapshotV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_draft_id: ToolDraftId,
        revision: Revision,
        parent_revision: Option<Revision>,
        tool_package_id: ToolPackageId,
        package_revision: Revision,
        active_package_version: CanonicalSemVer,
        installed_package_versions_hash: CanonicalSha256HashV1,
        package_fingerprint: PackageFingerprintV1,
        package_security_fingerprint: PackageSecurityFingerprintV1,
        tool_identity_set_hash: CanonicalSha256HashV1,
        persistent_impact_hash: CanonicalSha256HashV1,
        runtime_strong_reference_observed_summary_hash: CanonicalSha256HashV1,
        preview_hash: CanonicalSha256HashV1,
        initiator: ToolDraftActorV1,
        created_at: UtcTimestamp,
    ) -> Result<Self, ToolDraftContractError> {
        let draft_snapshot_hash = canonical_sha256(&ToolPackageDeleteDraftSnapshotPreimageV1 {
            preimage_version: 1,
            tool_package_id,
            package_revision,
            active_package_version: &active_package_version,
            installed_package_versions_hash: &installed_package_versions_hash,
            package_fingerprint: &package_fingerprint,
            package_security_fingerprint: &package_security_fingerprint,
            tool_identity_set_hash: &tool_identity_set_hash,
            persistent_impact_hash: &persistent_impact_hash,
            runtime_strong_reference_observed_summary_hash:
                &runtime_strong_reference_observed_summary_hash,
            deletion_policy_version: TOOL_PACKAGE_DELETION_POLICY_VERSION_V1,
            preview_hash: &preview_hash,
            initiator,
            created_at,
        })
        .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?;
        let snapshot = Self {
            draft: ConformanceDraftRevisionRefV1 {
                tool_draft_id,
                draft_revision: revision,
                draft_snapshot_hash,
            },
            parent_revision,
            tool_package_id,
            package_revision,
            active_package_version,
            installed_package_versions_hash,
            package_fingerprint,
            package_security_fingerprint,
            tool_identity_set_hash,
            persistent_impact_hash,
            runtime_strong_reference_observed_summary_hash,
            deletion_policy_version: TOOL_PACKAGE_DELETION_POLICY_VERSION_V1,
            preview_hash,
            initiator,
            created_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ToolDraftContractError> {
        let expected_parent = if self.draft.draft_revision == Revision::INITIAL {
            None
        } else {
            Some(Revision::new(
                self.draft
                    .draft_revision
                    .value()
                    .checked_sub(1)
                    .ok_or(ToolDraftContractError::RevisionChainInvalid)?,
            ))
        };
        if self.parent_revision != expected_parent
            || self.deletion_policy_version != TOOL_PACKAGE_DELETION_POLICY_VERSION_V1
        {
            return Err(ToolDraftContractError::RevisionChainInvalid);
        }
        let expected_hash = canonical_sha256(&ToolPackageDeleteDraftSnapshotPreimageV1 {
            preimage_version: 1,
            tool_package_id: self.tool_package_id,
            package_revision: self.package_revision,
            active_package_version: &self.active_package_version,
            installed_package_versions_hash: &self.installed_package_versions_hash,
            package_fingerprint: &self.package_fingerprint,
            package_security_fingerprint: &self.package_security_fingerprint,
            tool_identity_set_hash: &self.tool_identity_set_hash,
            persistent_impact_hash: &self.persistent_impact_hash,
            runtime_strong_reference_observed_summary_hash: &self
                .runtime_strong_reference_observed_summary_hash,
            deletion_policy_version: self.deletion_policy_version,
            preview_hash: &self.preview_hash,
            initiator: self.initiator,
            created_at: self.created_at,
        })
        .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?;
        (expected_hash == self.draft.draft_snapshot_hash)
            .then_some(())
            .ok_or(ToolDraftContractError::SnapshotIntegrity)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolPackageDeleteApprovalDecisionV1 {
    ApproveDelete,
    Reject,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPackageDeleteApprovalEvidenceV1 {
    pub draft: ConformanceDraftRevisionRefV1,
    pub target_snapshot_hash: CanonicalSha256HashV1,
    pub preview_hash: CanonicalSha256HashV1,
    pub decision: ToolPackageDeleteApprovalDecisionV1,
    /// Hash of the non-deserializable Core-owned
    /// `UserConfigurationOperationIdentity` used at the authenticated Main
    /// configuration boundary.
    pub user_configuration_operation_identity_binding_hash: ContentHash,
    pub decided_at: UtcTimestamp,
    pub evidence_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolPackageDeleteApprovalEvidencePreimageV1<'a> {
    preimage_version: u32,
    draft: &'a ConformanceDraftRevisionRefV1,
    target_snapshot_hash: &'a CanonicalSha256HashV1,
    preview_hash: &'a CanonicalSha256HashV1,
    decision: ToolPackageDeleteApprovalDecisionV1,
    user_configuration_operation_identity_binding_hash: &'a ContentHash,
    decided_at: UtcTimestamp,
}

impl ToolPackageDeleteApprovalEvidenceV1 {
    pub fn new(
        draft: ConformanceDraftRevisionRefV1,
        target_snapshot_hash: CanonicalSha256HashV1,
        preview_hash: CanonicalSha256HashV1,
        decision: ToolPackageDeleteApprovalDecisionV1,
        user_configuration_operation_identity_binding_hash: ContentHash,
        decided_at: UtcTimestamp,
    ) -> Result<Self, ToolDraftContractError> {
        if target_snapshot_hash != draft.draft_snapshot_hash {
            return Err(ToolDraftContractError::SnapshotIntegrity);
        }
        let evidence_hash = canonical_sha256(&ToolPackageDeleteApprovalEvidencePreimageV1 {
            preimage_version: 1,
            draft: &draft,
            target_snapshot_hash: &target_snapshot_hash,
            preview_hash: &preview_hash,
            decision,
            user_configuration_operation_identity_binding_hash:
                &user_configuration_operation_identity_binding_hash,
            decided_at,
        })
        .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?;
        Ok(Self {
            draft,
            target_snapshot_hash,
            preview_hash,
            decision,
            user_configuration_operation_identity_binding_hash,
            decided_at,
            evidence_hash,
        })
    }

    pub fn validate(&self) -> Result<(), ToolDraftContractError> {
        let expected = Self::new(
            self.draft.clone(),
            self.target_snapshot_hash.clone(),
            self.preview_hash.clone(),
            self.decision,
            self.user_configuration_operation_identity_binding_hash.clone(),
            self.decided_at,
        )?;
        (expected == *self).then_some(()).ok_or(ToolDraftContractError::SnapshotIntegrity)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DraftPackageFileRefV1 {
    pub path: PackageRelativePath,
    pub byte_length: u64,
    pub raw_file_sha256: ToolPackageFileSha256V1,
    pub content_artifact_id: ArtifactId,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftPackageSnapshotHashPreimageV1 {
    preimage_version: u32,
    raw_manifest_byte_length: u64,
    raw_manifest_sha256: ToolPackageFileSha256V1,
    files: Vec<DraftPackageSnapshotHashFileV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DraftPackageSnapshotHashFileV1 {
    path: PackageRelativePath,
    byte_length: u64,
    raw_file_sha256: ToolPackageFileSha256V1,
}

/// An unvalidated, immutable logical Draft revision. Artifact identities are
/// deliberately outside `snapshot_hash`: the hash names the complete byte
/// content, while the record preserves the exact immutable storage refs.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DraftPackageSnapshotV1 {
    pub draft: ConformanceDraftRevisionRefV1,
    pub parent_revision: Option<Revision>,
    pub raw_manifest_artifact_id: ArtifactId,
    pub raw_manifest_byte_length: u64,
    pub raw_manifest_sha256: ToolPackageFileSha256V1,
    pub raw_manifest_content_hash: ContentHash,
    pub files: Vec<DraftPackageFileRefV1>,
    pub actor: ToolDraftActorV1,
    pub created_at: UtcTimestamp,
}

impl DraftPackageSnapshotV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_draft_id: ToolDraftId,
        revision: Revision,
        parent_revision: Option<Revision>,
        raw_manifest_artifact_id: ArtifactId,
        raw_manifest_byte_length: u64,
        raw_manifest_sha256: ToolPackageFileSha256V1,
        raw_manifest_content_hash: ContentHash,
        files: Vec<DraftPackageFileRefV1>,
        actor: ToolDraftActorV1,
        created_at: UtcTimestamp,
    ) -> Result<Self, ToolDraftContractError> {
        let snapshot_hash = snapshot_hash(raw_manifest_byte_length, &raw_manifest_sha256, &files)?;
        let snapshot = Self {
            draft: ConformanceDraftRevisionRefV1 {
                tool_draft_id,
                draft_revision: revision,
                draft_snapshot_hash: snapshot_hash,
            },
            parent_revision,
            raw_manifest_artifact_id,
            raw_manifest_byte_length,
            raw_manifest_sha256,
            raw_manifest_content_hash,
            files,
            actor,
            created_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ToolDraftContractError> {
        let expected_parent = if self.draft.draft_revision == Revision::INITIAL {
            None
        } else {
            Some(Revision::new(
                self.draft
                    .draft_revision
                    .value()
                    .checked_sub(1)
                    .ok_or(ToolDraftContractError::RevisionChainInvalid)?,
            ))
        };
        if self.parent_revision != expected_parent {
            return Err(ToolDraftContractError::RevisionChainInvalid);
        }
        if self.raw_manifest_byte_length > TOOL_PACKAGE_IMPORT_POLICY_V1.max_manifest_bytes {
            return Err(ToolDraftContractError::ImportPolicyExceeded);
        }
        if self.files.len()
            > usize::try_from(TOOL_PACKAGE_IMPORT_POLICY_V1.max_entries.saturating_sub(1))
                .unwrap_or(usize::MAX)
        {
            return Err(ToolDraftContractError::ImportPolicyExceeded);
        }
        let mut total = self.raw_manifest_byte_length;
        for (index, file) in self.files.iter().enumerate() {
            if file.path.as_str() == TOOL_PACKAGE_MANIFEST_NAME
                || file.byte_length > TOOL_PACKAGE_IMPORT_POLICY_V1.max_file_bytes
            {
                return Err(ToolDraftContractError::ImportPolicyExceeded);
            }
            for previous in &self.files[..index] {
                if windows_ordinal_compare_ignore_case(
                    OsStr::new(previous.path.as_str()),
                    OsStr::new(file.path.as_str()),
                )
                .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?
                .is_eq()
                {
                    return Err(ToolDraftContractError::SnapshotIntegrity);
                }
            }
            total = total
                .checked_add(file.byte_length)
                .ok_or(ToolDraftContractError::ImportPolicyExceeded)?;
        }
        if total > TOOL_PACKAGE_IMPORT_POLICY_V1.max_expanded_bytes
            || !self
                .files
                .windows(2)
                .all(|pair| pair[0].path.as_str().as_bytes() < pair[1].path.as_str().as_bytes())
            || snapshot_hash(self.raw_manifest_byte_length, &self.raw_manifest_sha256, &self.files)?
                != self.draft.draft_snapshot_hash
        {
            return Err(ToolDraftContractError::SnapshotIntegrity);
        }
        Ok(())
    }
}

fn snapshot_hash(
    raw_manifest_byte_length: u64,
    raw_manifest_sha256: &ToolPackageFileSha256V1,
    files: &[DraftPackageFileRefV1],
) -> Result<CanonicalSha256HashV1, ToolDraftContractError> {
    canonical_sha256(&DraftPackageSnapshotHashPreimageV1 {
        preimage_version: 1,
        raw_manifest_byte_length,
        raw_manifest_sha256: raw_manifest_sha256.clone(),
        files: files
            .iter()
            .map(|file| DraftPackageSnapshotHashFileV1 {
                path: file.path.clone(),
                byte_length: file.byte_length,
                raw_file_sha256: file.raw_file_sha256.clone(),
            })
            .collect(),
    })
    .map_err(|_| ToolDraftContractError::SnapshotIntegrity)
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftValidationEvidenceV1 {
    pub draft: ConformanceDraftRevisionRefV1,
    pub canonical_manifest_hash: CanonicalSha256HashV1,
    pub runtime_closure_hash: PackageRuntimeClosureHashV1,
    pub package_fingerprint: PackageFingerprintV1,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub canonical_manifest_artifact_id: ArtifactId,
    pub runtime_closure_artifact_id: ArtifactId,
    pub validated_at: UtcTimestamp,
    pub evidence_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolDraftValidationEvidencePreimageV1<'a> {
    preimage_version: u32,
    draft: &'a ConformanceDraftRevisionRefV1,
    canonical_manifest_hash: &'a CanonicalSha256HashV1,
    runtime_closure_hash: &'a PackageRuntimeClosureHashV1,
    package_fingerprint: &'a PackageFingerprintV1,
    package_security_fingerprint: &'a PackageSecurityFingerprintV1,
    canonical_manifest_artifact_id: ArtifactId,
    runtime_closure_artifact_id: ArtifactId,
    validated_at: UtcTimestamp,
}

impl ToolDraftValidationEvidenceV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        draft: ConformanceDraftRevisionRefV1,
        canonical_manifest_hash: CanonicalSha256HashV1,
        runtime_closure_hash: PackageRuntimeClosureHashV1,
        package_fingerprint: PackageFingerprintV1,
        package_security_fingerprint: PackageSecurityFingerprintV1,
        canonical_manifest_artifact_id: ArtifactId,
        runtime_closure_artifact_id: ArtifactId,
        validated_at: UtcTimestamp,
    ) -> Result<Self, ToolDraftContractError> {
        let evidence_hash = canonical_sha256(&ToolDraftValidationEvidencePreimageV1 {
            preimage_version: 1,
            draft: &draft,
            canonical_manifest_hash: &canonical_manifest_hash,
            runtime_closure_hash: &runtime_closure_hash,
            package_fingerprint: &package_fingerprint,
            package_security_fingerprint: &package_security_fingerprint,
            canonical_manifest_artifact_id,
            runtime_closure_artifact_id,
            validated_at,
        })
        .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?;
        Ok(Self {
            draft,
            canonical_manifest_hash,
            runtime_closure_hash,
            package_fingerprint,
            package_security_fingerprint,
            canonical_manifest_artifact_id,
            runtime_closure_artifact_id,
            validated_at,
            evidence_hash,
        })
    }

    pub fn validate(&self) -> Result<(), ToolDraftContractError> {
        let expected = Self::new(
            self.draft.clone(),
            self.canonical_manifest_hash.clone(),
            self.runtime_closure_hash.clone(),
            self.package_fingerprint.clone(),
            self.package_security_fingerprint.clone(),
            self.canonical_manifest_artifact_id,
            self.runtime_closure_artifact_id,
            self.validated_at,
        )?;
        (expected == *self).then_some(()).ok_or(ToolDraftContractError::SnapshotIntegrity)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDraftFileChangeKindV1 {
    Added,
    Modified,
    Removed,
    Unchanged,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftFileChangeV1 {
    pub path: PackageRelativePath,
    pub change: ToolDraftFileChangeKindV1,
    pub byte_length: Option<u64>,
    pub raw_file_sha256: Option<ToolPackageFileSha256V1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftConformanceSummaryV1 {
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftPreviewV1 {
    pub draft: ConformanceDraftRevisionRefV1,
    pub canonical_manifest: CanonicalToolPackageManifestV1,
    pub canonical_manifest_hash: CanonicalSha256HashV1,
    pub runtime_closure_hash: PackageRuntimeClosureHashV1,
    pub package_fingerprint: PackageFingerprintV1,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub file_changes: Vec<ToolDraftFileChangeV1>,
    pub conformance: Vec<ToolDraftConformanceSummaryV1>,
    pub all_conformance_passed: bool,
    pub generated_at: UtcTimestamp,
    pub preview_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolDraftPreviewPreimageV1<'a> {
    preimage_version: u32,
    draft: &'a ConformanceDraftRevisionRefV1,
    canonical_manifest: &'a CanonicalToolPackageManifestV1,
    canonical_manifest_hash: &'a CanonicalSha256HashV1,
    runtime_closure_hash: &'a PackageRuntimeClosureHashV1,
    package_fingerprint: &'a PackageFingerprintV1,
    package_security_fingerprint: &'a PackageSecurityFingerprintV1,
    file_changes: &'a [ToolDraftFileChangeV1],
    conformance: &'a [ToolDraftConformanceSummaryV1],
    all_conformance_passed: bool,
    generated_at: UtcTimestamp,
}

impl ToolDraftPreviewV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        draft: ConformanceDraftRevisionRefV1,
        canonical_manifest: CanonicalToolPackageManifestV1,
        canonical_manifest_hash: CanonicalSha256HashV1,
        runtime_closure_hash: PackageRuntimeClosureHashV1,
        package_fingerprint: PackageFingerprintV1,
        package_security_fingerprint: PackageSecurityFingerprintV1,
        file_changes: Vec<ToolDraftFileChangeV1>,
        mut conformance: Vec<ToolDraftConformanceSummaryV1>,
        generated_at: UtcTimestamp,
    ) -> Result<Self, ToolDraftContractError> {
        conformance.sort_by_key(|entry| entry.tool_conformance_run_id.to_string());
        if conformance.is_empty()
            || !conformance
                .windows(2)
                .all(|pair| pair[0].tool_conformance_run_id != pair[1].tool_conformance_run_id)
        {
            return Err(ToolDraftContractError::SnapshotIntegrity);
        }
        let all_conformance_passed = conformance.iter().all(|entry| entry.passed);
        let preview_hash = canonical_sha256(&ToolDraftPreviewPreimageV1 {
            preimage_version: 1,
            draft: &draft,
            canonical_manifest: &canonical_manifest,
            canonical_manifest_hash: &canonical_manifest_hash,
            runtime_closure_hash: &runtime_closure_hash,
            package_fingerprint: &package_fingerprint,
            package_security_fingerprint: &package_security_fingerprint,
            file_changes: &file_changes,
            conformance: &conformance,
            all_conformance_passed,
            generated_at,
        })
        .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?;
        Ok(Self {
            draft,
            canonical_manifest,
            canonical_manifest_hash,
            runtime_closure_hash,
            package_fingerprint,
            package_security_fingerprint,
            file_changes,
            conformance,
            all_conformance_passed,
            generated_at,
            preview_hash,
        })
    }

    pub fn validate(&self) -> Result<(), ToolDraftContractError> {
        let expected = Self::new(
            self.draft.clone(),
            self.canonical_manifest.clone(),
            self.canonical_manifest_hash.clone(),
            self.runtime_closure_hash.clone(),
            self.package_fingerprint.clone(),
            self.package_security_fingerprint.clone(),
            self.file_changes.clone(),
            self.conformance.clone(),
            self.generated_at,
        )?;
        (expected == *self).then_some(()).ok_or(ToolDraftContractError::SnapshotIntegrity)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolDraftApprovalDecisionV1 {
    Install,
    InstallAndTrust,
    Reject,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftApprovalEvidenceV1 {
    pub draft: ConformanceDraftRevisionRefV1,
    pub decision: ToolDraftApprovalDecisionV1,
    pub actor: ToolDraftActorV1,
    pub decided_at: UtcTimestamp,
    pub canonical_manifest_hash: CanonicalSha256HashV1,
    pub runtime_closure_hash: PackageRuntimeClosureHashV1,
    pub package_fingerprint: PackageFingerprintV1,
    pub package_security_fingerprint: PackageSecurityFingerprintV1,
    pub conformance_run_ids: Vec<ToolConformanceRunId>,
    pub preview_hash: CanonicalSha256HashV1,
    pub evidence_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolDraftApprovalEvidencePreimageV1<'a> {
    preimage_version: u32,
    draft: &'a ConformanceDraftRevisionRefV1,
    decision: ToolDraftApprovalDecisionV1,
    actor: ToolDraftActorV1,
    decided_at: UtcTimestamp,
    canonical_manifest_hash: &'a CanonicalSha256HashV1,
    runtime_closure_hash: &'a PackageRuntimeClosureHashV1,
    package_fingerprint: &'a PackageFingerprintV1,
    package_security_fingerprint: &'a PackageSecurityFingerprintV1,
    conformance_run_ids: &'a [ToolConformanceRunId],
    preview_hash: &'a CanonicalSha256HashV1,
}

impl ToolDraftApprovalEvidenceV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        draft: ConformanceDraftRevisionRefV1,
        decision: ToolDraftApprovalDecisionV1,
        actor: ToolDraftActorV1,
        decided_at: UtcTimestamp,
        canonical_manifest_hash: CanonicalSha256HashV1,
        runtime_closure_hash: PackageRuntimeClosureHashV1,
        package_fingerprint: PackageFingerprintV1,
        package_security_fingerprint: PackageSecurityFingerprintV1,
        mut conformance_run_ids: Vec<ToolConformanceRunId>,
        preview_hash: CanonicalSha256HashV1,
    ) -> Result<Self, ToolDraftContractError> {
        conformance_run_ids.sort_by_key(ToString::to_string);
        if conformance_run_ids.is_empty()
            || !conformance_run_ids.windows(2).all(|pair| pair[0] != pair[1])
        {
            return Err(ToolDraftContractError::SnapshotIntegrity);
        }
        let evidence_hash = canonical_sha256(&ToolDraftApprovalEvidencePreimageV1 {
            preimage_version: 1,
            draft: &draft,
            decision,
            actor,
            decided_at,
            canonical_manifest_hash: &canonical_manifest_hash,
            runtime_closure_hash: &runtime_closure_hash,
            package_fingerprint: &package_fingerprint,
            package_security_fingerprint: &package_security_fingerprint,
            conformance_run_ids: &conformance_run_ids,
            preview_hash: &preview_hash,
        })
        .map_err(|_| ToolDraftContractError::SnapshotIntegrity)?;
        Ok(Self {
            draft,
            decision,
            actor,
            decided_at,
            canonical_manifest_hash,
            runtime_closure_hash,
            package_fingerprint,
            package_security_fingerprint,
            conformance_run_ids,
            preview_hash,
            evidence_hash,
        })
    }

    pub fn validate(&self) -> Result<(), ToolDraftContractError> {
        let expected = Self::new(
            self.draft.clone(),
            self.decision,
            self.actor,
            self.decided_at,
            self.canonical_manifest_hash.clone(),
            self.runtime_closure_hash.clone(),
            self.package_fingerprint.clone(),
            self.package_security_fingerprint.clone(),
            self.conformance_run_ids.clone(),
            self.preview_hash.clone(),
        )?;
        (expected == *self).then_some(()).ok_or(ToolDraftContractError::SnapshotIntegrity)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolDraftContractError {
    #[error("Draft revision chain is invalid")]
    RevisionChainInvalid,
    #[error("Draft content exceeds Tool Package V1 import policy")]
    ImportPolicyExceeded,
    #[error("Draft snapshot integrity validation failed")]
    SnapshotIntegrity,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDraftPackageIdentityV1 {
    pub package_key: PackageKey,
    pub package_version: CanonicalSemVer,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::CanonicalSemVer;
    use aura_domain_common::jcs::{CanonicalBlake3HashV1, CanonicalSha256HashV1};
    use aura_domain_common::{ArtifactId, ContentHash, Revision, UtcTimestamp};
    use aura_domain_common::{ToolDraftId, ToolPackageId};
    use aura_tool_contract::{
        PackageFingerprintV1, PackageRelativePath, PackageSecurityFingerprintV1,
        ToolPackageFileSha256V1,
    };

    use super::{
        DraftPackageFileRefV1, DraftPackageSnapshotV1,
        TOOL_DRAFT_SNAPSHOT_TRANSFER_SCHEMA_VERSION_V1, ToolDraftActorV1,
        ToolDraftSnapshotTransferFileV1, ToolDraftSnapshotTransferV1,
        ToolDraftTransferArtifactRefV1, ToolPackageDeleteApprovalDecisionV1,
        ToolPackageDeleteApprovalEvidenceV1, ToolPackageDeleteDraftSnapshotV1,
        snapshot_hash_from_transfer,
    };

    fn draft_snapshot_transfer(paths: &[&str]) -> ToolDraftSnapshotTransferV1 {
        let raw_manifest_utf8 = "{not-yet-validated}".to_owned();
        let mut transfer = ToolDraftSnapshotTransferV1 {
            schema_version: TOOL_DRAFT_SNAPSHOT_TRANSFER_SCHEMA_VERSION_V1,
            raw_manifest_byte_length: raw_manifest_utf8.len() as u64,
            raw_manifest_sha256: ToolPackageFileSha256V1::sha256(raw_manifest_utf8.as_bytes()),
            raw_manifest_utf8,
            files: paths
                .iter()
                .map(|path| ToolDraftSnapshotTransferFileV1 {
                    path: PackageRelativePath::new(*path).expect("path"),
                    artifact: ToolDraftTransferArtifactRefV1 {
                        artifact_id: ArtifactId::new(),
                        byte_length: 3,
                        sha256: ToolPackageFileSha256V1::sha256(b"abc"),
                    },
                })
                .collect(),
            snapshot_hash: CanonicalSha256HashV1::sha256(b"placeholder"),
        };
        transfer.snapshot_hash = snapshot_hash_from_transfer(&transfer).expect("snapshot hash");
        transfer
    }

    #[test]
    fn draft_snapshot_transfer_is_canonical_and_integrity_bound_without_manifest_validation() {
        let transfer = draft_snapshot_transfer(&["bin/a.exe", "docs/readme.txt"]);
        transfer.validate().expect("canonical transfer");

        let mut tampered = transfer.clone();
        tampered.files[0].artifact.byte_length += 1;
        assert!(tampered.validate().is_err());

        let mut unsorted = transfer.clone();
        unsorted.files.swap(0, 1);
        unsorted.snapshot_hash = snapshot_hash_from_transfer(&unsorted).expect("hash");
        assert!(unsorted.validate().is_err());

        let duplicate = draft_snapshot_transfer(&["Bin/tool.exe", "bin/TOOL.exe"]);
        assert!(duplicate.validate().is_err());

        let manifest_path = draft_snapshot_transfer(&["aura-tool-package.json"]);
        assert!(manifest_path.validate().is_err());
    }

    #[test]
    fn snapshot_hash_names_content_and_revision_chain_is_exact() {
        let file = DraftPackageFileRefV1 {
            path: PackageRelativePath::new("bin/tool.exe").expect("path"),
            byte_length: 3,
            raw_file_sha256: ToolPackageFileSha256V1::sha256(b"abc"),
            content_artifact_id: ArtifactId::new(),
            content_hash: ContentHash::blake3(b"abc"),
        };
        let draft_id = ToolDraftId::new();
        let first = DraftPackageSnapshotV1::new(
            draft_id,
            Revision::INITIAL,
            None,
            ArtifactId::new(),
            2,
            ToolPackageFileSha256V1::sha256(b"{}"),
            ContentHash::blake3(b"{}"),
            vec![file.clone()],
            ToolDraftActorV1::User,
            UtcTimestamp::now(),
        )
        .expect("first snapshot");
        let second = DraftPackageSnapshotV1::new(
            draft_id,
            Revision::new(2),
            Some(Revision::INITIAL),
            ArtifactId::new(),
            2,
            ToolPackageFileSha256V1::sha256(b"{}"),
            ContentHash::blake3(b"{}"),
            vec![file],
            ToolDraftActorV1::User,
            UtcTimestamp::now(),
        )
        .expect("second snapshot");
        assert_eq!(first.draft.draft_snapshot_hash, second.draft.draft_snapshot_hash);
    }

    #[test]
    fn package_delete_snapshot_and_approval_are_self_validating() {
        let snapshot = ToolPackageDeleteDraftSnapshotV1::new(
            ToolDraftId::new(),
            Revision::INITIAL,
            None,
            ToolPackageId::new(),
            Revision::new(7),
            CanonicalSemVer::new("1.0.0").expect("semver"),
            CanonicalSha256HashV1::sha256(b"installed versions"),
            PackageFingerprintV1(CanonicalSha256HashV1::sha256(b"package")),
            PackageSecurityFingerprintV1(CanonicalBlake3HashV1::blake3(b"security")),
            CanonicalSha256HashV1::sha256(b"tool identities"),
            CanonicalSha256HashV1::sha256(b"persistent impact"),
            CanonicalSha256HashV1::sha256(b"runtime references"),
            CanonicalSha256HashV1::sha256(b"preview"),
            ToolDraftActorV1::User,
            UtcTimestamp::now(),
        )
        .expect("delete snapshot");
        snapshot.validate().expect("snapshot integrity");

        let approval = ToolPackageDeleteApprovalEvidenceV1::new(
            snapshot.draft.clone(),
            snapshot.draft.draft_snapshot_hash.clone(),
            snapshot.preview_hash.clone(),
            ToolPackageDeleteApprovalDecisionV1::ApproveDelete,
            ContentHash::sha256(b"trusted user configuration operation"),
            UtcTimestamp::now(),
        )
        .expect("approval");
        approval.validate().expect("approval integrity");

        let mut tampered = approval;
        tampered.preview_hash = CanonicalSha256HashV1::sha256(b"different preview");
        assert!(tampered.validate().is_err());
    }
}
