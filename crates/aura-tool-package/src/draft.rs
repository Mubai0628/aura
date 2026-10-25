use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use aura_domain_common::ContentHash;
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_tool_contract::{
    PackageFingerprintV1, PackageRelativePath, PackageRuntimeClosureHashV1,
    PackageSecurityFingerprintV1, TOOL_PACKAGE_IMPORT_POLICY_V1, TOOL_PACKAGE_MANIFEST_NAME,
    ToolPackageFileSha256V1,
};
use aura_tool_domain::{ConformanceDraftRevisionRefV1, DraftPackageSnapshotV1};
use aura_windows_platform::windows_ordinal_compare_ignore_case;
use thiserror::Error;

use crate::{StagedToolPackageV1, ToolPackageStagingErrorV1, stage_directory_package_v1};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftPackageContentFileV1 {
    path: PackageRelativePath,
    bytes: Vec<u8>,
}

impl DraftPackageContentFileV1 {
    #[must_use]
    pub fn new(path: PackageRelativePath, bytes: Vec<u8>) -> Self {
        Self { path, bytes }
    }

    #[must_use]
    pub const fn path(&self) -> &PackageRelativePath {
        &self.path
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Complete unvalidated bytes for one immutable Draft revision. This value is
/// intentionally distinct from `StagedToolPackageV1` and carries no W1/W2
/// validation authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftPackageContentV1 {
    raw_manifest: Vec<u8>,
    files: Vec<DraftPackageContentFileV1>,
}

impl DraftPackageContentV1 {
    pub fn new(
        raw_manifest: Vec<u8>,
        mut files: Vec<DraftPackageContentFileV1>,
    ) -> Result<Self, ToolDraftWorkflowErrorV1> {
        files.sort_by(|left, right| {
            left.path.as_str().as_bytes().cmp(right.path.as_str().as_bytes())
        });
        validate_draft_content(&raw_manifest, &files)?;
        Ok(Self { raw_manifest, files })
    }

    pub fn from_snapshot(
        snapshot: &DraftPackageSnapshotV1,
        raw_manifest: Vec<u8>,
        files: Vec<DraftPackageContentFileV1>,
    ) -> Result<Self, ToolDraftWorkflowErrorV1> {
        snapshot.validate().map_err(|_| ToolDraftWorkflowErrorV1::SnapshotMismatch)?;
        let content = Self::new(raw_manifest, files)?;
        if u64::try_from(content.raw_manifest.len()).ok() != Some(snapshot.raw_manifest_byte_length)
            || ToolPackageFileSha256V1::sha256(&content.raw_manifest)
                != snapshot.raw_manifest_sha256
            || ContentHash::blake3(&content.raw_manifest) != snapshot.raw_manifest_content_hash
            || content.files.len() != snapshot.files.len()
            || !content.files.iter().zip(&snapshot.files).all(|(actual, expected)| {
                actual.path == expected.path
                    && u64::try_from(actual.bytes.len()).ok() == Some(expected.byte_length)
                    && ToolPackageFileSha256V1::sha256(&actual.bytes) == expected.raw_file_sha256
                    && ContentHash::blake3(&actual.bytes) == expected.content_hash
            })
        {
            return Err(ToolDraftWorkflowErrorV1::SnapshotMismatch);
        }
        Ok(content)
    }

    #[must_use]
    pub fn raw_manifest(&self) -> &[u8] {
        &self.raw_manifest
    }

    #[must_use]
    pub fn files(&self) -> &[DraftPackageContentFileV1] {
        &self.files
    }
}

pub struct PreparedToolDraftValidationV1 {
    draft: ConformanceDraftRevisionRefV1,
    staged_package: StagedToolPackageV1,
}

impl PreparedToolDraftValidationV1 {
    #[must_use]
    pub const fn draft(&self) -> &ConformanceDraftRevisionRefV1 {
        &self.draft
    }

    #[must_use]
    pub const fn staged_package(&self) -> &StagedToolPackageV1 {
        &self.staged_package
    }

    pub fn canonical_manifest_hash(
        &self,
    ) -> Result<CanonicalSha256HashV1, ToolDraftWorkflowErrorV1> {
        self.staged_package.verify_integrity()?;
        Ok(CanonicalSha256HashV1::sha256(self.staged_package.canonical_manifest_bytes()))
    }

    pub fn runtime_closure_hash(
        &self,
    ) -> Result<PackageRuntimeClosureHashV1, ToolDraftWorkflowErrorV1> {
        self.staged_package
            .runtime_closure()
            .canonical_hash()
            .map_err(|_| ToolDraftWorkflowErrorV1::EvidenceInvalid)
    }

    pub fn package_fingerprint(&self) -> Result<PackageFingerprintV1, ToolDraftWorkflowErrorV1> {
        Ok(self.staged_package.fingerprint_evidence()?.package_fingerprint().clone())
    }

    pub fn package_security_fingerprint(
        &self,
    ) -> Result<PackageSecurityFingerprintV1, ToolDraftWorkflowErrorV1> {
        Ok(self.staged_package.fingerprint_evidence()?.package_security_fingerprint().clone())
    }
}

pub fn validate_tool_draft_revision_v1(
    snapshot: &DraftPackageSnapshotV1,
    raw_manifest: Vec<u8>,
    files: Vec<DraftPackageContentFileV1>,
    staging_parent: &Path,
) -> Result<PreparedToolDraftValidationV1, ToolDraftWorkflowErrorV1> {
    let content = DraftPackageContentV1::from_snapshot(snapshot, raw_manifest, files)?;
    let source = tempfile::tempdir_in(staging_parent).map_err(ToolDraftWorkflowErrorV1::Io)?;
    write_source_tree(source.path(), &content)?;
    let staged_package = stage_directory_package_v1(source.path(), staging_parent)?;
    let evidence = staged_package.fingerprint_evidence()?;
    let canonical_manifest_hash =
        CanonicalSha256HashV1::sha256(staged_package.canonical_manifest_bytes());
    if canonical_manifest_hash != *staged_package.runtime_closure().canonical_manifest_hash()
        || evidence.package_fingerprint().0.algorithm()
            != aura_domain_common::ContentHashAlgorithm::Sha256
        || !evidence.package_security_fingerprint().uses_blake3()
    {
        return Err(ToolDraftWorkflowErrorV1::EvidenceInvalid);
    }
    Ok(PreparedToolDraftValidationV1 { draft: snapshot.draft.clone(), staged_package })
}

fn validate_draft_content(
    raw_manifest: &[u8],
    files: &[DraftPackageContentFileV1],
) -> Result<(), ToolDraftWorkflowErrorV1> {
    let policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
    if u64::try_from(raw_manifest.len()).map_or(true, |length| length > policy.max_manifest_bytes)
        || files.len() > usize::try_from(policy.max_entries.saturating_sub(1)).unwrap_or(usize::MAX)
        || !files
            .windows(2)
            .all(|pair| pair[0].path.as_str().as_bytes() < pair[1].path.as_str().as_bytes())
    {
        return Err(ToolDraftWorkflowErrorV1::ImportPolicyExceeded);
    }
    let mut total = u64::try_from(raw_manifest.len())
        .map_err(|_| ToolDraftWorkflowErrorV1::ImportPolicyExceeded)?;
    let mut exact = BTreeSet::new();
    for (index, file) in files.iter().enumerate() {
        if file.path.as_str() == TOOL_PACKAGE_MANIFEST_NAME
            || !exact.insert(file.path.as_str())
            || u64::try_from(file.bytes.len()).map_or(true, |length| length > policy.max_file_bytes)
        {
            return Err(ToolDraftWorkflowErrorV1::ImportPolicyExceeded);
        }
        for previous in &files[..index] {
            if windows_ordinal_compare_ignore_case(
                previous.path.as_str().as_ref(),
                file.path.as_str().as_ref(),
            )
            .map_err(ToolDraftWorkflowErrorV1::Io)?
            .is_eq()
            {
                return Err(ToolDraftWorkflowErrorV1::ImportPolicyExceeded);
            }
        }
        total = total
            .checked_add(
                u64::try_from(file.bytes.len())
                    .map_err(|_| ToolDraftWorkflowErrorV1::ImportPolicyExceeded)?,
            )
            .ok_or(ToolDraftWorkflowErrorV1::ImportPolicyExceeded)?;
    }
    if total > policy.max_expanded_bytes {
        return Err(ToolDraftWorkflowErrorV1::ImportPolicyExceeded);
    }
    Ok(())
}

fn write_source_tree(
    root: &Path,
    content: &DraftPackageContentV1,
) -> Result<(), ToolDraftWorkflowErrorV1> {
    write_new_file(root.join(TOOL_PACKAGE_MANIFEST_NAME), content.raw_manifest())?;
    for file in content.files() {
        let destination = root.join(file.path().as_str().replace('/', "\\"));
        write_new_file(destination, file.bytes())?;
    }
    Ok(())
}

fn write_new_file(path: std::path::PathBuf, bytes: &[u8]) -> Result<(), ToolDraftWorkflowErrorV1> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ToolDraftWorkflowErrorV1::Io)?;
    }
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(ToolDraftWorkflowErrorV1::Io)?;
    output.write_all(bytes).map_err(ToolDraftWorkflowErrorV1::Io)?;
    output.flush().map_err(ToolDraftWorkflowErrorV1::Io)
}

#[derive(Debug, Error)]
pub enum ToolDraftWorkflowErrorV1 {
    #[error("Draft content exceeds the immutable Package import policy")]
    ImportPolicyExceeded,
    #[error("Draft bytes do not match the immutable Draft snapshot")]
    SnapshotMismatch,
    #[error("Draft validation evidence is inconsistent")]
    EvidenceInvalid,
    #[error("Draft filesystem materialization failed")]
    Io(#[source] std::io::Error),
    #[error("Draft staging failed")]
    Staging(#[from] ToolPackageStagingErrorV1),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use aura_domain_common::ToolDraftId;
    use aura_domain_common::{ArtifactId, ContentHash, Revision, UtcTimestamp};
    use aura_tool_contract::{
        PackageRelativePath, TOOL_PACKAGE_MANIFEST_NAME, ToolPackageFileSha256V1,
    };
    use aura_tool_domain::{DraftPackageFileRefV1, DraftPackageSnapshotV1, ToolDraftActorV1};

    use super::{DraftPackageContentFileV1, validate_tool_draft_revision_v1};

    const MANIFEST: &[u8] =
        include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
    const RUNTIME: &[u8] = b"stdio fixture runtime";

    fn snapshot(runtime: &[u8]) -> (DraftPackageSnapshotV1, Vec<DraftPackageContentFileV1>) {
        let path = PackageRelativePath::new("bin/fixture.exe").expect("path");
        let artifact_id = ArtifactId::new();
        let files = vec![DraftPackageContentFileV1::new(path.clone(), runtime.to_vec())];
        let snapshot = DraftPackageSnapshotV1::new(
            ToolDraftId::new(),
            Revision::INITIAL,
            None,
            ArtifactId::new(),
            u64::try_from(MANIFEST.len()).expect("manifest length"),
            ToolPackageFileSha256V1::sha256(MANIFEST),
            ContentHash::blake3(MANIFEST),
            vec![DraftPackageFileRefV1 {
                path,
                byte_length: u64::try_from(runtime.len()).expect("runtime length"),
                raw_file_sha256: ToolPackageFileSha256V1::sha256(runtime),
                content_artifact_id: artifact_id,
                content_hash: ContentHash::blake3(runtime),
            }],
            ToolDraftActorV1::User,
            UtcTimestamp::now(),
        )
        .expect("snapshot");
        (snapshot, files)
    }

    #[test]
    fn exact_snapshot_materializes_through_w1_and_w2() {
        let staging = tempfile::tempdir().expect("staging");
        let (snapshot, files) = snapshot(RUNTIME);
        let prepared =
            validate_tool_draft_revision_v1(&snapshot, MANIFEST.to_vec(), files, staging.path())
                .expect("validation");
        assert_eq!(prepared.draft(), &snapshot.draft);
        assert_eq!(
            fs::read(prepared.staged_package().root().join(TOOL_PACKAGE_MANIFEST_NAME))
                .expect("canonical manifest"),
            prepared.staged_package().canonical_manifest_bytes()
        );
    }

    #[test]
    fn stale_or_tampered_snapshot_bytes_are_rejected_before_w1() {
        let staging = tempfile::tempdir().expect("staging");
        let (snapshot, files) = snapshot(RUNTIME);
        assert!(
            validate_tool_draft_revision_v1(&snapshot, b"{}".to_vec(), files, staging.path(),)
                .is_err()
        );
    }
}
