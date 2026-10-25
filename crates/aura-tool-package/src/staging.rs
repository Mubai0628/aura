use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use aura_domain_common::ContentHash;
use aura_domain_common::jcs::{
    CanonicalSha256HashV1, canonical_json_jcs, canonical_sha256, parse_json_strict,
};
use aura_tool_contract::{
    PackageRelativePath, PackageRuntimeClosureHashV1, PackageRuntimeClosurePolicyVersionV1,
    TOOL_PACKAGE_IMPORT_POLICY_V1, TOOL_PACKAGE_MANIFEST_NAME, ToolPackageContractError,
    ToolPackageFileEntryV1, ToolPackageFileRoleV1, ToolPackageFileSha256V1,
    ToolPackageImportPolicyV1, validate_closed_file_manifest,
};
use aura_tool_domain::{CanonicalToolPackageManifestV1, ToolPackageManifestContractError};
use aura_windows_platform::{
    NativeFileAccess, NativePathInfo, VerifiedFile, final_path_within, inspect_existing_path,
    open_verified_import_source, windows_ordinal_compare_ignore_case,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use thiserror::Error;

use crate::manifest_parser::{
    ParsedToolPackageManifestV1, ToolPackageManifestParseErrorV1,
    parse_tool_package_manifest_with_policy_v1,
};
use crate::{
    DraftPackageContentFileV1, DraftPackageContentV1, ToolDraftWorkflowErrorV1,
    ToolPackageFingerprintErrorV1, ToolPackageFingerprintEvidenceV1,
};

pub(crate) const COPY_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum ToolPackageStagingErrorV1 {
    #[error("package source or staging storage is unavailable")]
    Io(#[source] io::Error),
    #[error("package source root must be an ordinary directory and not a reparse point")]
    InvalidSourceRoot,
    #[error("package ZIP source must be an ordinary file and not a reparse point")]
    InvalidZipSource,
    #[error("package staging parent must be an ordinary directory and not a reparse point")]
    InvalidStagingParent,
    #[error("package manifest is missing from the logical root")]
    ManifestMissing,
    #[error("package manifest exceeds the fixed byte limit")]
    ManifestTooLarge,
    #[error("package ZIP input exceeds the fixed byte limit")]
    ZipInputTooLarge,
    #[error("package ZIP archive is invalid or unreadable")]
    InvalidZipArchive(#[source] zip::result::ZipError),
    #[error("package ZIP entry uses encryption, which Package V1 forbids")]
    EncryptedZipEntryDenied,
    #[error("package ZIP entry uses a compression method outside Stored or Deflate")]
    ZipCompressionMethodDenied,
    #[error("package ZIP compression ratio exceeds the fixed import limit")]
    ZipCompressionRatioExceeded,
    #[error("package entry count exceeds the fixed import limit")]
    EntryCountExceeded,
    #[error("package expanded bytes exceed the fixed import limit")]
    ExpandedSizeExceeded,
    #[error("package file exceeds the fixed per-file byte limit")]
    FileSizeExceeded,
    #[error("package relative path exceeds the fixed depth limit")]
    PathDepthExceeded,
    #[error("package contains an invalid relative path")]
    InvalidPath,
    #[error("package contains paths that alias under Windows case-insensitive comparison")]
    CaseCollision,
    #[error("package contains a symlink, reparse point, or non-ordinary entry")]
    SpecialFileDenied,
    #[error("package actual files do not exactly match the closed manifest files")]
    ClosedFileSetMismatch,
    #[error("package file length changed or differs from the manifest")]
    FileLengthMismatch,
    #[error("package file SHA-256 differs from the manifest")]
    FileHashMismatch,
    #[error("package declared file list violates the shared contract")]
    InvalidDeclaredFiles(#[source] ToolPackageContractError),
    #[error("package manifest failed the strict W1 parser")]
    ManifestInvalid(#[source] ToolPackageManifestParseErrorV1),
    #[error("package runtime root cannot form the authoritative V1 closure")]
    RuntimeClosureInvalid(#[source] ToolPackageManifestContractError),
    #[error("package fingerprint evidence cannot be derived from authoritative staging facts")]
    FingerprintInvalid(#[source] ToolPackageFingerprintErrorV1),
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRuntimeClosureFileV1 {
    pub path: PackageRelativePath,
    pub role: ToolPackageFileRoleV1,
    #[schemars(range(min = 0, max = 67_108_864))]
    pub byte_length: u64,
    pub raw_file_sha256: ToolPackageFileSha256V1,
}

/// An authoritative V1 full-package Runtime Closure. Its fields are private
/// and it has no general `Deserialize` implementation: callers can only obtain
/// it from a staged runtime root whose complete file set was read and hashed,
/// or reconcile stored bytes against that same evidence.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRuntimeClosureV1 {
    policy_version: PackageRuntimeClosurePolicyVersionV1,
    canonical_manifest_hash: CanonicalSha256HashV1,
    #[schemars(length(max = 4_095))]
    files: Vec<PackageRuntimeClosureFileV1>,
}

impl PackageRuntimeClosureV1 {
    pub(crate) fn from_verified_runtime_root(
        canonical_manifest_bytes: &[u8],
        canonical_manifest: &CanonicalToolPackageManifestV1,
        verified_files: &[ToolPackageFileEntryV1],
    ) -> Result<Self, ToolPackageManifestContractError> {
        canonical_manifest.validate()?;
        let expected_bytes = canonical_json_jcs(canonical_manifest)
            .map_err(|_| ToolPackageManifestContractError::CanonicalManifestBytesMismatch)?;
        if canonical_manifest_bytes != expected_bytes || canonical_manifest.files != verified_files
        {
            return Err(ToolPackageManifestContractError::RuntimeClosureMismatch);
        }

        Ok(Self {
            policy_version: PackageRuntimeClosurePolicyVersionV1,
            canonical_manifest_hash: CanonicalSha256HashV1::sha256(canonical_manifest_bytes),
            files: verified_files
                .iter()
                .map(|file| PackageRuntimeClosureFileV1 {
                    path: file.path.clone(),
                    role: file.role,
                    byte_length: file.byte_length,
                    raw_file_sha256: file.sha256.clone(),
                })
                .collect(),
        })
    }

    /// Reconciles stored canonical Closure bytes against a live staged
    /// package. The staged runtime root is re-read first, so a stale evidence
    /// object cannot mask post-assembly mutation.
    pub fn reconcile_stored(
        stored_closure_bytes: &[u8],
        staged_package: &StagedToolPackageV1,
    ) -> Result<Self, ToolPackageManifestContractError> {
        #[derive(Deserialize, Serialize)]
        #[serde(deny_unknown_fields)]
        struct StoredClosureWire {
            policy_version: PackageRuntimeClosurePolicyVersionV1,
            canonical_manifest_hash: CanonicalSha256HashV1,
            files: Vec<PackageRuntimeClosureFileV1>,
        }

        staged_package
            .verify_integrity()
            .map_err(|_| ToolPackageManifestContractError::RuntimeClosureMismatch)?;
        let value = parse_json_strict(stored_closure_bytes)
            .map_err(|_| ToolPackageManifestContractError::RuntimeClosureMismatch)?;
        let wire = serde_json::from_value::<StoredClosureWire>(value)
            .map_err(|_| ToolPackageManifestContractError::RuntimeClosureMismatch)?;
        let canonical_stored = canonical_json_jcs(&wire)
            .map_err(|_| ToolPackageManifestContractError::RuntimeClosureMismatch)?;
        if stored_closure_bytes != canonical_stored {
            return Err(ToolPackageManifestContractError::RuntimeClosureMismatch);
        }
        let stored = Self {
            policy_version: wire.policy_version,
            canonical_manifest_hash: wire.canonical_manifest_hash,
            files: wire.files,
        };
        if stored != staged_package.runtime_closure {
            return Err(ToolPackageManifestContractError::RuntimeClosureMismatch);
        }
        Ok(staged_package.runtime_closure.clone())
    }

    #[must_use]
    pub const fn policy_version(&self) -> PackageRuntimeClosurePolicyVersionV1 {
        self.policy_version
    }

    #[must_use]
    pub const fn canonical_manifest_hash(&self) -> &CanonicalSha256HashV1 {
        &self.canonical_manifest_hash
    }

    #[must_use]
    pub fn files(&self) -> &[PackageRuntimeClosureFileV1] {
        &self.files
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ToolPackageManifestContractError> {
        canonical_json_jcs(self)
            .map_err(|_| ToolPackageManifestContractError::RuntimeClosureMismatch)
    }

    pub fn canonical_hash(
        &self,
    ) -> Result<PackageRuntimeClosureHashV1, ToolPackageManifestContractError> {
        canonical_sha256(self)
            .map(PackageRuntimeClosureHashV1)
            .map_err(|_| ToolPackageManifestContractError::RuntimeClosureMismatch)
    }
}

pub struct StagedToolPackageV1 {
    directory: TempDir,
    parsed_manifest: ParsedToolPackageManifestV1,
    verified_files: Vec<ToolPackageFileEntryV1>,
    runtime_closure: PackageRuntimeClosureV1,
    fingerprint_evidence: ToolPackageFingerprintEvidenceV1,
    source_entry_count: u32,
    source_expanded_bytes: u64,
    runtime_expanded_bytes: u64,
    policy: ToolPackageImportPolicyV1,
}

/// Recomputed, read-only integrity evidence for one already published Runtime
/// Root. Fields are private so callers cannot manufacture payload eligibility
/// from database metadata without re-reading the immutable Package bytes.
pub struct VerifiedInstalledToolPackageV1 {
    runtime_closure: PackageRuntimeClosureV1,
    fingerprint_evidence: ToolPackageFingerprintEvidenceV1,
}

impl VerifiedInstalledToolPackageV1 {
    #[must_use]
    pub const fn runtime_closure(&self) -> &PackageRuntimeClosureV1 {
        &self.runtime_closure
    }

    #[must_use]
    pub const fn fingerprint_evidence(&self) -> &ToolPackageFingerprintEvidenceV1 {
        &self.fingerprint_evidence
    }
}

/// Re-reads an installed full-package Runtime Root and reconstructs the only
/// evidence accepted by Registry eligibility and Runtime dispatch.
pub fn verify_installed_tool_package_v1(
    runtime_root: &Path,
    canonical_manifest_bytes: &[u8],
    canonical_manifest: &CanonicalToolPackageManifestV1,
) -> Result<VerifiedInstalledToolPackageV1, ToolPackageStagingErrorV1> {
    canonical_manifest.validate().map_err(ToolPackageStagingErrorV1::RuntimeClosureInvalid)?;
    let expected_bytes = canonical_json_jcs(canonical_manifest).map_err(|_| {
        ToolPackageStagingErrorV1::RuntimeClosureInvalid(
            ToolPackageManifestContractError::CanonicalManifestBytesMismatch,
        )
    })?;
    if canonical_manifest_bytes != expected_bytes {
        return Err(ToolPackageStagingErrorV1::RuntimeClosureInvalid(
            ToolPackageManifestContractError::CanonicalManifestBytesMismatch,
        ));
    }
    let (verified_files, _) = verify_runtime_root(
        runtime_root,
        canonical_manifest_bytes,
        canonical_manifest,
        TOOL_PACKAGE_IMPORT_POLICY_V1,
    )?;
    let runtime_closure = PackageRuntimeClosureV1::from_verified_runtime_root(
        canonical_manifest_bytes,
        canonical_manifest,
        &verified_files,
    )
    .map_err(ToolPackageStagingErrorV1::RuntimeClosureInvalid)?;
    let fingerprint_evidence = ToolPackageFingerprintEvidenceV1::from_authoritative_closure(
        canonical_manifest_bytes,
        canonical_manifest,
        &runtime_closure,
    )
    .map_err(ToolPackageStagingErrorV1::FingerprintInvalid)?;
    Ok(VerifiedInstalledToolPackageV1 { runtime_closure, fingerprint_evidence })
}

impl StagedToolPackageV1 {
    pub(crate) fn from_runtime_root(
        directory: TempDir,
        parsed_manifest: ParsedToolPackageManifestV1,
        source_entry_count: u32,
        source_expanded_bytes: u64,
        policy: ToolPackageImportPolicyV1,
    ) -> Result<Self, ToolPackageStagingErrorV1> {
        let (verified_files, runtime_expanded_bytes) = verify_runtime_root(
            directory.path(),
            parsed_manifest.canonical_manifest_bytes(),
            parsed_manifest.canonical_manifest(),
            policy,
        )?;
        let runtime_closure = PackageRuntimeClosureV1::from_verified_runtime_root(
            parsed_manifest.canonical_manifest_bytes(),
            parsed_manifest.canonical_manifest(),
            &verified_files,
        )
        .map_err(ToolPackageStagingErrorV1::RuntimeClosureInvalid)?;
        let fingerprint_evidence = ToolPackageFingerprintEvidenceV1::from_authoritative_closure(
            parsed_manifest.canonical_manifest_bytes(),
            parsed_manifest.canonical_manifest(),
            &runtime_closure,
        )
        .map_err(ToolPackageStagingErrorV1::FingerprintInvalid)?;
        Ok(Self {
            directory,
            parsed_manifest,
            verified_files,
            runtime_closure,
            fingerprint_evidence,
            source_entry_count,
            source_expanded_bytes,
            runtime_expanded_bytes,
            policy,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    #[must_use]
    pub const fn source_manifest_byte_length(&self) -> u64 {
        self.parsed_manifest.source_byte_length()
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> &ContentHash {
        self.parsed_manifest.source_sha256()
    }

    #[must_use]
    pub const fn canonical_manifest(&self) -> &CanonicalToolPackageManifestV1 {
        self.parsed_manifest.canonical_manifest()
    }

    #[must_use]
    pub fn canonical_manifest_bytes(&self) -> &[u8] {
        self.parsed_manifest.canonical_manifest_bytes()
    }

    #[must_use]
    pub fn verified_files(&self) -> &[ToolPackageFileEntryV1] {
        &self.verified_files
    }

    #[must_use]
    pub const fn runtime_closure(&self) -> &PackageRuntimeClosureV1 {
        &self.runtime_closure
    }

    /// Returns the W2 preview/evidence only after re-reading the complete
    /// staged Runtime Root and recomputing the Closure and all fingerprints.
    pub fn fingerprint_evidence(
        &self,
    ) -> Result<&ToolPackageFingerprintEvidenceV1, ToolPackageStagingErrorV1> {
        self.verify_integrity()?;
        Ok(&self.fingerprint_evidence)
    }

    #[must_use]
    pub const fn source_entry_count(&self) -> u32 {
        self.source_entry_count
    }

    #[must_use]
    pub const fn source_expanded_bytes(&self) -> u64 {
        self.source_expanded_bytes
    }

    #[must_use]
    pub const fn runtime_expanded_bytes(&self) -> u64 {
        self.runtime_expanded_bytes
    }

    /// Re-reads the complete runtime root and proves it still matches the
    /// canonical Manifest and frozen Closure.
    pub fn verify_integrity(&self) -> Result<(), ToolPackageStagingErrorV1> {
        let (verified_files, runtime_expanded_bytes) = verify_runtime_root(
            self.root(),
            self.canonical_manifest_bytes(),
            self.canonical_manifest(),
            self.policy,
        )?;
        let closure = PackageRuntimeClosureV1::from_verified_runtime_root(
            self.canonical_manifest_bytes(),
            self.canonical_manifest(),
            &verified_files,
        )
        .map_err(ToolPackageStagingErrorV1::RuntimeClosureInvalid)?;
        let fingerprint_evidence = ToolPackageFingerprintEvidenceV1::from_authoritative_closure(
            self.canonical_manifest_bytes(),
            self.canonical_manifest(),
            &closure,
        )
        .map_err(ToolPackageStagingErrorV1::FingerprintInvalid)?;
        if verified_files != self.verified_files
            || runtime_expanded_bytes != self.runtime_expanded_bytes
            || closure != self.runtime_closure
            || fingerprint_evidence != self.fingerprint_evidence
        {
            return Err(ToolPackageStagingErrorV1::RuntimeClosureInvalid(
                ToolPackageManifestContractError::RuntimeClosureMismatch,
            ));
        }
        Ok(())
    }
}

struct SourceFileV1 {
    relative_path: PackageRelativePath,
    absolute_path: PathBuf,
    byte_length: u64,
    manifest: bool,
}

struct DirectoryScanV1 {
    root_guard: VerifiedFile,
    directory_guards: Vec<VerifiedFile>,
    files: Vec<SourceFileV1>,
    all_relative_paths: Vec<String>,
    entry_count: u32,
    expanded_bytes: u64,
}

/// Copies an already-authorized directory import into an attempt-local staging
/// directory. No staged bytes become an installed Package until a later
/// conformance and journal transaction consumes this value.
pub fn stage_directory_package_v1(
    source_root: &Path,
    staging_parent: &Path,
) -> Result<StagedToolPackageV1, ToolPackageStagingErrorV1> {
    stage_directory_package_with_policy_v1(
        source_root,
        staging_parent,
        TOOL_PACKAGE_IMPORT_POLICY_V1,
    )
}

/// Imports an already-authorized directory as unvalidated Draft bytes. The
/// same no-follow scan and immutable import limits used by W1 staging apply,
/// but semantic Manifest validation is intentionally deferred to Validate.
pub fn import_directory_draft_content_v1(
    source_root: &Path,
) -> Result<DraftPackageContentV1, ToolDraftWorkflowErrorV1> {
    let policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
    let mut scan = scan_directory(source_root, policy)?;
    validate_windows_case_uniqueness(&scan.all_relative_paths)?;
    scan.files.sort_by(|left, right| {
        left.relative_path.as_str().as_bytes().cmp(right.relative_path.as_str().as_bytes())
    });

    let manifest = scan
        .files
        .iter()
        .find(|file| file.manifest)
        .ok_or(ToolPackageStagingErrorV1::ManifestMissing)?;
    let raw_manifest = read_verified_file(&scan.root_guard, manifest, policy)?;
    let files = scan
        .files
        .iter()
        .filter(|file| !file.manifest)
        .map(|source| {
            Ok(DraftPackageContentFileV1::new(
                source.relative_path.clone(),
                read_verified_file(&scan.root_guard, source, policy)?,
            ))
        })
        .collect::<Result<Vec<_>, ToolPackageStagingErrorV1>>()?;

    // Keep the verified directory handles alive until every file is read.
    scan.directory_guards.clear();
    drop(scan.root_guard);
    DraftPackageContentV1::new(raw_manifest, files)
}

fn stage_directory_package_with_policy_v1(
    source_root: &Path,
    staging_parent: &Path,
    policy: ToolPackageImportPolicyV1,
) -> Result<StagedToolPackageV1, ToolPackageStagingErrorV1> {
    let staging_parent_info =
        inspect_existing_path(staging_parent, false).map_err(ToolPackageStagingErrorV1::Io)?;
    if !staging_parent_info.is_directory || staging_parent_info.is_reparse_point {
        return Err(ToolPackageStagingErrorV1::InvalidStagingParent);
    }

    let mut scan = scan_directory(source_root, policy)?;
    validate_windows_case_uniqueness(&scan.all_relative_paths)?;
    let parsed_manifest = parse_directory_manifest(&scan, policy)?;
    let declared_files = &parsed_manifest.canonical_manifest().files;
    validate_declared_files(declared_files, policy)?;
    validate_closed_actual_file_set(&scan.files, declared_files)?;

    let directory = tempfile::Builder::new()
        .prefix("aura-tool-package-")
        .tempdir_in(staging_parent)
        .map_err(ToolPackageStagingErrorV1::Io)?;
    let root_info = scan.root_guard.info().clone();
    let declared_by_path =
        declared_files.iter().map(|entry| (entry.path.as_str(), entry)).collect::<BTreeMap<_, _>>();
    scan.files.sort_by(|left, right| {
        left.relative_path.as_str().as_bytes().cmp(right.relative_path.as_str().as_bytes())
    });

    for source in scan.files.iter().filter(|source| !source.manifest) {
        let declared = declared_by_path
            .get(source.relative_path.as_str())
            .copied()
            .ok_or(ToolPackageStagingErrorV1::ClosedFileSetMismatch)?;
        copy_verified_source_file(&root_info, source, directory.path(), Some(declared))?;
    }
    write_canonical_manifest(directory.path(), parsed_manifest.canonical_manifest_bytes())?;

    // Keep all verified directory handles alive until every file has been read.
    scan.directory_guards.clear();
    drop(scan.root_guard);
    StagedToolPackageV1::from_runtime_root(
        directory,
        parsed_manifest,
        scan.entry_count,
        scan.expanded_bytes,
        policy,
    )
}

fn parse_directory_manifest(
    scan: &DirectoryScanV1,
    policy: ToolPackageImportPolicyV1,
) -> Result<ParsedToolPackageManifestV1, ToolPackageStagingErrorV1> {
    let source = scan
        .files
        .iter()
        .find(|file| file.manifest)
        .ok_or(ToolPackageStagingErrorV1::ManifestMissing)?;
    let manifest = open_verified_import_source(&source.absolute_path, NativeFileAccess::Read)
        .map_err(ToolPackageStagingErrorV1::Io)?;
    if manifest.info().is_directory
        || !final_path_within(scan.root_guard.info(), manifest.info())
        || manifest.file().metadata().map_err(ToolPackageStagingErrorV1::Io)?.len()
            != source.byte_length
    {
        return Err(ToolPackageStagingErrorV1::SpecialFileDenied);
    }
    let parsed = parse_tool_package_manifest_with_policy_v1(manifest.into_file(), policy)
        .map_err(ToolPackageStagingErrorV1::ManifestInvalid)?;
    if parsed.source_byte_length() != source.byte_length {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }
    Ok(parsed)
}

pub(crate) fn write_canonical_manifest(
    staging_root: &Path,
    canonical_manifest_bytes: &[u8],
) -> Result<(), ToolPackageStagingErrorV1> {
    let destination = staging_root.join(TOOL_PACKAGE_MANIFEST_NAME);
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(ToolPackageStagingErrorV1::Io)?;
    output.write_all(canonical_manifest_bytes).map_err(ToolPackageStagingErrorV1::Io)?;
    output.flush().map_err(ToolPackageStagingErrorV1::Io)?;
    output.sync_all().map_err(ToolPackageStagingErrorV1::Io)
}

pub(crate) fn verify_runtime_root(
    runtime_root: &Path,
    canonical_manifest_bytes: &[u8],
    canonical_manifest: &CanonicalToolPackageManifestV1,
    policy: ToolPackageImportPolicyV1,
) -> Result<(Vec<ToolPackageFileEntryV1>, u64), ToolPackageStagingErrorV1> {
    canonical_manifest.validate().map_err(ToolPackageStagingErrorV1::RuntimeClosureInvalid)?;
    let expected_manifest_bytes = canonical_json_jcs(canonical_manifest).map_err(|_| {
        ToolPackageStagingErrorV1::RuntimeClosureInvalid(
            ToolPackageManifestContractError::CanonicalManifestBytesMismatch,
        )
    })?;
    if expected_manifest_bytes != canonical_manifest_bytes {
        return Err(ToolPackageStagingErrorV1::RuntimeClosureInvalid(
            ToolPackageManifestContractError::CanonicalManifestBytesMismatch,
        ));
    }
    validate_declared_files(&canonical_manifest.files, policy)?;

    let mut scan = scan_directory(runtime_root, policy)?;
    validate_windows_case_uniqueness(&scan.all_relative_paths)?;
    validate_runtime_root_entry_set(&scan, &canonical_manifest.files)?;
    validate_closed_actual_file_set(&scan.files, &canonical_manifest.files)?;
    scan.files.sort_by(|left, right| {
        left.relative_path.as_str().as_bytes().cmp(right.relative_path.as_str().as_bytes())
    });

    let manifest_source = scan
        .files
        .iter()
        .find(|file| file.manifest)
        .ok_or(ToolPackageStagingErrorV1::ManifestMissing)?;
    let manifest_bytes = read_verified_file(&scan.root_guard, manifest_source, policy)?;
    if manifest_bytes != canonical_manifest_bytes {
        return Err(ToolPackageStagingErrorV1::RuntimeClosureInvalid(
            ToolPackageManifestContractError::CanonicalManifestBytesMismatch,
        ));
    }

    let declarations = canonical_manifest
        .files
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut verified_files = Vec::with_capacity(declarations.len());
    for source in scan.files.iter().filter(|file| !file.manifest) {
        let declared = declarations
            .get(source.relative_path.as_str())
            .copied()
            .ok_or(ToolPackageStagingErrorV1::ClosedFileSetMismatch)?;
        let bytes = read_verified_file(&scan.root_guard, source, policy)?;
        let sha256 = ToolPackageFileSha256V1::sha256(&bytes);
        if source.byte_length != declared.byte_length || sha256 != declared.sha256 {
            return Err(if source.byte_length != declared.byte_length {
                ToolPackageStagingErrorV1::FileLengthMismatch
            } else {
                ToolPackageStagingErrorV1::FileHashMismatch
            });
        }
        verified_files.push(ToolPackageFileEntryV1 {
            path: source.relative_path.clone(),
            byte_length: source.byte_length,
            sha256,
            role: declared.role,
        });
    }
    if verified_files != canonical_manifest.files {
        return Err(ToolPackageStagingErrorV1::RuntimeClosureInvalid(
            ToolPackageManifestContractError::RuntimeClosureMismatch,
        ));
    }
    Ok((verified_files, scan.expanded_bytes))
}

fn validate_runtime_root_entry_set(
    scan: &DirectoryScanV1,
    declared_files: &[ToolPackageFileEntryV1],
) -> Result<(), ToolPackageStagingErrorV1> {
    let mut allowed = BTreeSet::from([TOOL_PACKAGE_MANIFEST_NAME.to_owned()]);
    for file in declared_files {
        allowed.insert(file.path.as_str().to_owned());
        let segments = file.path.as_str().split('/').collect::<Vec<_>>();
        for depth in 1..segments.len() {
            allowed.insert(segments[..depth].join("/"));
        }
    }
    let actual = scan.all_relative_paths.iter().cloned().collect::<BTreeSet<_>>();
    if actual != allowed {
        return Err(ToolPackageStagingErrorV1::ClosedFileSetMismatch);
    }
    Ok(())
}

fn read_verified_file(
    root_guard: &VerifiedFile,
    source: &SourceFileV1,
    policy: ToolPackageImportPolicyV1,
) -> Result<Vec<u8>, ToolPackageStagingErrorV1> {
    let mut input = open_verified_import_source(&source.absolute_path, NativeFileAccess::Read)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidInput {
                ToolPackageStagingErrorV1::SpecialFileDenied
            } else {
                // Name the file: "the path was not found" without the path cannot be
                // acted on, and the scanned entry set and the read can disagree only
                // if the tree moved between them.
                ToolPackageStagingErrorV1::Io(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", source.absolute_path.display()),
                ))
            }
        })?;
    if input.info().is_directory || !final_path_within(root_guard.info(), input.info()) {
        return Err(ToolPackageStagingErrorV1::SpecialFileDenied);
    }
    let current_length = input.file().metadata().map_err(ToolPackageStagingErrorV1::Io)?.len();
    if current_length != source.byte_length {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }
    let maximum = if source.manifest { policy.max_manifest_bytes } else { policy.max_file_bytes };
    let read_limit = maximum.checked_add(1).ok_or(ToolPackageStagingErrorV1::FileSizeExceeded)?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(source.byte_length)
            .map_err(|_| ToolPackageStagingErrorV1::FileSizeExceeded)?,
    );
    input
        .file_mut()
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(ToolPackageStagingErrorV1::Io)?;
    if u64::try_from(bytes.len()).ok() != Some(source.byte_length) {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }
    Ok(bytes)
}

pub(crate) fn validate_declared_files(
    declared_files: &[ToolPackageFileEntryV1],
    policy: ToolPackageImportPolicyV1,
) -> Result<(), ToolPackageStagingErrorV1> {
    validate_closed_file_manifest(declared_files)
        .map_err(ToolPackageStagingErrorV1::InvalidDeclaredFiles)?;
    let mut total = 0_u64;
    for file in declared_files {
        if file.byte_length > policy.max_file_bytes {
            return Err(ToolPackageStagingErrorV1::FileSizeExceeded);
        }
        if depth(file.path.as_str()) > policy.max_relative_depth {
            return Err(ToolPackageStagingErrorV1::PathDepthExceeded);
        }
        total = total
            .checked_add(file.byte_length)
            .ok_or(ToolPackageStagingErrorV1::ExpandedSizeExceeded)?;
    }
    if total > policy.max_expanded_bytes {
        return Err(ToolPackageStagingErrorV1::ExpandedSizeExceeded);
    }
    Ok(())
}

fn scan_directory(
    source_root: &Path,
    policy: ToolPackageImportPolicyV1,
) -> Result<DirectoryScanV1, ToolPackageStagingErrorV1> {
    let root_guard =
        open_verified_import_source(source_root, NativeFileAccess::Metadata).map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidInput {
                ToolPackageStagingErrorV1::InvalidSourceRoot
            } else {
                // The scanned root is what a missing directory is reported against.
                ToolPackageStagingErrorV1::Io(io::Error::new(
                    error.kind(),
                    format!("scan root {}: {error}", source_root.display()),
                ))
            }
        })?;
    if !root_guard.info().is_directory {
        return Err(ToolPackageStagingErrorV1::InvalidSourceRoot);
    }
    let root_info = root_guard.info().clone();
    let mut scan = DirectoryScanV1 {
        root_guard,
        directory_guards: Vec::new(),
        files: Vec::new(),
        all_relative_paths: Vec::new(),
        entry_count: 0,
        expanded_bytes: 0,
    };
    scan_children(&root_info, source_root, &[], policy, &mut scan)?;
    if !scan.files.iter().any(|file| file.manifest) {
        return Err(ToolPackageStagingErrorV1::ManifestMissing);
    }
    Ok(scan)
}

fn scan_children(
    root_info: &NativePathInfo,
    current: &Path,
    parent_segments: &[String],
    policy: ToolPackageImportPolicyV1,
    scan: &mut DirectoryScanV1,
) -> Result<(), ToolPackageStagingErrorV1> {
    let mut children = fs::read_dir(current)
        .map_err(|error| {
            ToolPackageStagingErrorV1::Io(io::Error::new(
                error.kind(),
                format!("scan directory {}: {error}", current.display()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            ToolPackageStagingErrorV1::Io(io::Error::new(
                error.kind(),
                format!("scan directory entries {}: {error}", current.display()),
            ))
        })?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        scan.entry_count =
            scan.entry_count.checked_add(1).ok_or(ToolPackageStagingErrorV1::EntryCountExceeded)?;
        if scan.entry_count > policy.max_entries {
            return Err(ToolPackageStagingErrorV1::EntryCountExceeded);
        }
        let name =
            child.file_name().into_string().map_err(|_| ToolPackageStagingErrorV1::InvalidPath)?;
        let mut segments = parent_segments.to_vec();
        segments.push(name);
        let relative_text = segments.join("/");
        if depth(&relative_text) > policy.max_relative_depth {
            return Err(ToolPackageStagingErrorV1::PathDepthExceeded);
        }
        let relative_path = PackageRelativePath::new(relative_text.clone())
            .map_err(|_| ToolPackageStagingErrorV1::InvalidPath)?;
        scan.all_relative_paths.push(relative_text);

        let guard = open_verified_import_source(&child.path(), NativeFileAccess::Metadata)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::InvalidInput {
                    ToolPackageStagingErrorV1::SpecialFileDenied
                } else {
                    // An enumerated child that cannot be opened is a distinct fault
                    // from an unreadable root, so it names itself.
                    ToolPackageStagingErrorV1::Io(io::Error::new(
                        error.kind(),
                        format!("scanned child {}: {error}", child.path().display()),
                    ))
                }
            })?;
        if !final_path_within(root_info, guard.info()) {
            return Err(ToolPackageStagingErrorV1::InvalidPath);
        }
        if guard.info().is_directory {
            scan_children(root_info, &child.path(), &segments, policy, scan)?;
            scan.directory_guards.push(guard);
            continue;
        }
        if !guard.file().metadata().map_err(ToolPackageStagingErrorV1::Io)?.is_file() {
            return Err(ToolPackageStagingErrorV1::SpecialFileDenied);
        }
        let byte_length = guard.file().metadata().map_err(ToolPackageStagingErrorV1::Io)?.len();
        if byte_length > policy.max_file_bytes {
            return Err(ToolPackageStagingErrorV1::FileSizeExceeded);
        }
        let manifest = relative_path.as_str() == TOOL_PACKAGE_MANIFEST_NAME;
        if manifest && byte_length > policy.max_manifest_bytes {
            return Err(ToolPackageStagingErrorV1::ManifestTooLarge);
        }
        scan.expanded_bytes = scan
            .expanded_bytes
            .checked_add(byte_length)
            .ok_or(ToolPackageStagingErrorV1::ExpandedSizeExceeded)?;
        if scan.expanded_bytes > policy.max_expanded_bytes {
            return Err(ToolPackageStagingErrorV1::ExpandedSizeExceeded);
        }
        scan.files.push(SourceFileV1 {
            relative_path,
            absolute_path: child.path(),
            byte_length,
            manifest,
        });
    }
    Ok(())
}

pub(crate) fn validate_windows_case_uniqueness(
    paths: &[String],
) -> Result<(), ToolPackageStagingErrorV1> {
    for (index, path) in paths.iter().enumerate() {
        for previous in &paths[..index] {
            if windows_ordinal_compare_ignore_case(OsStr::new(path), OsStr::new(previous))
                .map_err(ToolPackageStagingErrorV1::Io)?
                .is_eq()
            {
                return Err(ToolPackageStagingErrorV1::CaseCollision);
            }
        }
    }
    Ok(())
}

fn validate_closed_actual_file_set(
    source_files: &[SourceFileV1],
    declared_files: &[ToolPackageFileEntryV1],
) -> Result<(), ToolPackageStagingErrorV1> {
    let actual = source_files
        .iter()
        .filter(|file| !file.manifest)
        .map(|file| file.relative_path.as_str())
        .collect::<Vec<_>>();
    let declared = declared_files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>();
    if actual.len() != declared.len() || !declared.iter().all(|path| actual.contains(path)) {
        return Err(ToolPackageStagingErrorV1::ClosedFileSetMismatch);
    }
    for source in source_files.iter().filter(|file| !file.manifest) {
        let declared = declared_files
            .iter()
            .find(|entry| entry.path == source.relative_path)
            .ok_or(ToolPackageStagingErrorV1::ClosedFileSetMismatch)?;
        if source.byte_length != declared.byte_length {
            return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
        }
    }
    Ok(())
}

fn copy_verified_source_file(
    root_info: &NativePathInfo,
    source: &SourceFileV1,
    staging_root: &Path,
    declared: Option<&ToolPackageFileEntryV1>,
) -> Result<(), ToolPackageStagingErrorV1> {
    let mut input = open_verified_import_source(&source.absolute_path, NativeFileAccess::Read)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidInput {
                ToolPackageStagingErrorV1::SpecialFileDenied
            } else {
                ToolPackageStagingErrorV1::Io(error)
            }
        })?;
    if input.info().is_directory || !final_path_within(root_info, input.info()) {
        return Err(ToolPackageStagingErrorV1::SpecialFileDenied);
    }
    if input.file().metadata().map_err(ToolPackageStagingErrorV1::Io)?.len() != source.byte_length {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }

    let destination = staging_root.join(source.relative_path.as_str().replace('/', "\\"));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(ToolPackageStagingErrorV1::Io)?;
    }
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&destination)
        .map_err(ToolPackageStagingErrorV1::Io)?;
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = input.file_mut().read(&mut buffer).map_err(ToolPackageStagingErrorV1::Io)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(
                u64::try_from(read).map_err(|_| ToolPackageStagingErrorV1::ExpandedSizeExceeded)?,
            )
            .ok_or(ToolPackageStagingErrorV1::ExpandedSizeExceeded)?;
        if copied > source.byte_length {
            return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read]).map_err(ToolPackageStagingErrorV1::Io)?;
    }
    output.flush().map_err(ToolPackageStagingErrorV1::Io)?;
    output.sync_all().map_err(ToolPackageStagingErrorV1::Io)?;
    if copied != source.byte_length {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }
    let content_hash = ToolPackageFileSha256V1::new(format!("{:x}", hasher.finalize()))
        .map_err(|_| ToolPackageStagingErrorV1::FileHashMismatch)?;
    if declared.is_some_and(|entry| entry.sha256 != content_hash) {
        return Err(ToolPackageStagingErrorV1::FileHashMismatch);
    }
    Ok(())
}

fn depth(path: &str) -> u32 {
    u32::try_from(path.split('/').count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STDIO_MANIFEST: &[u8] =
        include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
    const STDIO_RUNTIME: &[u8] = b"stdio fixture runtime";

    fn write_valid_source(root: &Path) {
        fs::create_dir_all(root.join("bin")).expect("runtime directory");
        fs::write(root.join(TOOL_PACKAGE_MANIFEST_NAME), STDIO_MANIFEST).expect("manifest");
        fs::write(root.join("bin/fixture.exe"), STDIO_RUNTIME).expect("runtime file");
    }

    #[test]
    fn directory_draft_import_preserves_invalid_manifest_for_later_validation() {
        let source = tempfile::tempdir().expect("source");
        fs::create_dir_all(source.path().join("bin")).expect("runtime directory");
        fs::write(source.path().join(TOOL_PACKAGE_MANIFEST_NAME), b"not-json")
            .expect("raw manifest");
        fs::write(source.path().join("bin/fixture.exe"), STDIO_RUNTIME).expect("runtime file");

        let content = import_directory_draft_content_v1(source.path()).expect("draft import");
        assert_eq!(content.raw_manifest(), b"not-json");
        assert_eq!(content.files().len(), 1);
        assert_eq!(content.files()[0].path().as_str(), "bin/fixture.exe");
        assert_eq!(content.files()[0].bytes(), STDIO_RUNTIME);
    }

    #[test]
    fn directory_import_builds_canonical_runtime_root_and_authoritative_closure() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        write_valid_source(source.path());
        fs::create_dir_all(source.path().join("runtime/empty")).expect("directories");

        let staged =
            stage_directory_package_v1(source.path(), staging.path()).expect("stage package");
        assert_eq!(staged.verified_files(), staged.canonical_manifest().files);
        assert_eq!(staged.source_entry_count(), 5);
        assert_eq!(
            staged.source_manifest_byte_length(),
            u64::try_from(STDIO_MANIFEST.len()).expect("manifest length")
        );
        assert!(staged.root().join("bin/fixture.exe").is_file());
        assert!(!staged.root().join("runtime/empty").exists());
        assert_eq!(staged.source_manifest_sha256(), &ContentHash::sha256(STDIO_MANIFEST));
        assert_eq!(
            fs::read(staged.root().join(TOOL_PACKAGE_MANIFEST_NAME)).expect("runtime Manifest"),
            staged.canonical_manifest_bytes()
        );
        assert_ne!(staged.canonical_manifest_bytes(), STDIO_MANIFEST);
        assert_eq!(staged.runtime_closure().files().len(), 1);
        staged.verify_integrity().expect("staged integrity");

        let closure_bytes = staged.runtime_closure().canonical_bytes().expect("closure bytes");
        PackageRuntimeClosureV1::reconcile_stored(&closure_bytes, &staged)
            .expect("stored closure reconciliation");
        let staged_root = staged.root().to_owned();
        drop(staged);
        assert!(!staged_root.exists());
    }

    #[test]
    fn directory_import_rejects_extra_missing_length_and_hash_mismatch() {
        let staging = tempfile::tempdir().expect("staging");

        let extra = tempfile::tempdir().expect("extra source");
        write_valid_source(extra.path());
        fs::write(extra.path().join("extra.bin"), b"undeclared").expect("extra file");
        assert!(matches!(
            stage_directory_package_v1(extra.path(), staging.path()),
            Err(ToolPackageStagingErrorV1::ClosedFileSetMismatch)
        ));

        let missing = tempfile::tempdir().expect("missing source");
        fs::write(missing.path().join(TOOL_PACKAGE_MANIFEST_NAME), STDIO_MANIFEST)
            .expect("manifest");
        assert!(matches!(
            stage_directory_package_v1(missing.path(), staging.path()),
            Err(ToolPackageStagingErrorV1::ClosedFileSetMismatch)
        ));

        let wrong_length = tempfile::tempdir().expect("length source");
        write_valid_source(wrong_length.path());
        fs::write(wrong_length.path().join("bin/fixture.exe"), b"short").expect("short runtime");
        assert!(matches!(
            stage_directory_package_v1(wrong_length.path(), staging.path()),
            Err(ToolPackageStagingErrorV1::FileLengthMismatch)
        ));

        let wrong_hash = tempfile::tempdir().expect("hash source");
        write_valid_source(wrong_hash.path());
        fs::write(wrong_hash.path().join("bin/fixture.exe"), b"xxxxxxxxxxxxxxxxxxxxx")
            .expect("same-length runtime");
        assert!(matches!(
            stage_directory_package_v1(wrong_hash.path(), staging.path()),
            Err(ToolPackageStagingErrorV1::FileHashMismatch)
        ));
        assert_eq!(fs::read_dir(staging.path()).expect("staging parent").count(), 0);
    }

    #[test]
    fn injected_policy_rejects_entry_size_total_and_depth_boundaries() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        write_valid_source(source.path());

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_file_bytes = 1;
        assert!(matches!(
            stage_directory_package_with_policy_v1(source.path(), staging.path(), policy),
            Err(ToolPackageStagingErrorV1::InvalidDeclaredFiles(_))
                | Err(ToolPackageStagingErrorV1::FileSizeExceeded)
        ));

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_entries = 1;
        assert!(matches!(
            stage_directory_package_with_policy_v1(source.path(), staging.path(), policy),
            Err(ToolPackageStagingErrorV1::EntryCountExceeded)
        ));

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_expanded_bytes = 64;
        assert!(matches!(
            stage_directory_package_with_policy_v1(source.path(), staging.path(), policy),
            Err(ToolPackageStagingErrorV1::ExpandedSizeExceeded)
        ));

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_relative_depth = 0;
        assert!(matches!(
            stage_directory_package_with_policy_v1(source.path(), staging.path(), policy),
            Err(ToolPackageStagingErrorV1::PathDepthExceeded)
        ));
    }

    #[test]
    fn manifest_is_required_at_root_and_uses_its_independent_limit() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        assert!(matches!(
            stage_directory_package_v1(source.path(), staging.path()),
            Err(ToolPackageStagingErrorV1::ManifestMissing)
        ));

        fs::write(source.path().join(TOOL_PACKAGE_MANIFEST_NAME), b"1234").expect("manifest");
        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_manifest_bytes = 3;
        assert!(matches!(
            stage_directory_package_with_policy_v1(source.path(), staging.path(), policy),
            Err(ToolPackageStagingErrorV1::ManifestTooLarge)
        ));
    }

    #[test]
    fn runtime_root_mutation_invalidates_staging_evidence_and_reconciliation() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        write_valid_source(source.path());
        let staged =
            stage_directory_package_v1(source.path(), staging.path()).expect("stage package");
        let closure_bytes = staged.runtime_closure().canonical_bytes().expect("closure bytes");

        fs::write(staged.root().join("bin/fixture.exe"), b"xxxxxxxxxxxxxxxxxxxxx")
            .expect("tamper staged runtime");
        assert!(matches!(
            staged.verify_integrity(),
            Err(ToolPackageStagingErrorV1::FileHashMismatch)
        ));
        assert!(matches!(
            staged.fingerprint_evidence(),
            Err(ToolPackageStagingErrorV1::FileHashMismatch)
        ));
        assert!(PackageRuntimeClosureV1::reconcile_stored(&closure_bytes, &staged).is_err());
    }
}
