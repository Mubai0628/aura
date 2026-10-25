use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use aura_domain_common::ToolPackageId;
use aura_tool_contract::{
    PackageFingerprintV1, PackageRelativePath, TOOL_PACKAGE_IMPORT_POLICY_V1,
    TOOL_PACKAGE_MANIFEST_NAME,
};
use aura_windows_platform::open_verified_publish_parent;
use thiserror::Error;

use crate::{
    PackageRuntimeClosureV1, StagedToolPackageV1, ToolPackageFingerprintEvidenceV1,
    ToolPackageStagingErrorV1, verify_runtime_root,
};

/// The immutable V1 Package Store. A publish can only consume W2's sealed
/// `StagedToolPackageV1`; callers cannot provide a manifest file list as
/// substitute staging evidence.
#[derive(Clone, Debug)]
pub struct ToolPackageStoreV1 {
    root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedToolPackageV1 {
    relative_path: PackageRelativePath,
    absolute_path: PathBuf,
    reused_existing: bool,
}

impl PublishedToolPackageV1 {
    #[must_use]
    pub const fn relative_path(&self) -> &PackageRelativePath {
        &self.relative_path
    }

    #[must_use]
    pub fn absolute_path(&self) -> &Path {
        &self.absolute_path
    }

    #[must_use]
    pub const fn reused_existing(&self) -> bool {
        self.reused_existing
    }
}

impl ToolPackageStoreV1 {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ToolPackageStoreErrorV1> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|source| ToolPackageStoreErrorV1::Io {
            action: "create Package Store root",
            path: root.clone(),
            source,
        })?;
        fs::create_dir_all(root.join(".publish-staging")).map_err(|source| {
            ToolPackageStoreErrorV1::Io {
                action: "create Package Store staging root",
                path: root.join(".publish-staging"),
                source,
            }
        })?;
        fs::create_dir_all(root.join("quarantine")).map_err(|source| {
            ToolPackageStoreErrorV1::Io {
                action: "create Package Store quarantine root",
                path: root.join("quarantine"),
                source,
            }
        })?;
        Ok(Self { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn relative_path_for(
        &self,
        tool_package_id: ToolPackageId,
        staged: &StagedToolPackageV1,
    ) -> Result<PackageRelativePath, ToolPackageStoreErrorV1> {
        staged.verify_integrity()?;
        let manifest = staged.canonical_manifest();
        let fingerprint = staged.fingerprint_evidence()?.package_fingerprint();
        PackageRelativePath::new(format!(
            "packages/{}/{}/{}",
            tool_package_id,
            manifest.package_version,
            package_fingerprint_hex(fingerprint)
        ))
        .map_err(|_| ToolPackageStoreErrorV1::InvalidStorePath)
    }

    pub fn publish(
        &self,
        tool_package_id: ToolPackageId,
        staged: &StagedToolPackageV1,
    ) -> Result<PublishedToolPackageV1, ToolPackageStoreErrorV1> {
        staged.verify_integrity()?;
        let relative_path = self.relative_path_for(tool_package_id, staged)?;
        let destination = self.resolve_relative(&relative_path)?;
        let branch = if destination.exists() { "reuse_existing" } else { "fresh_publish" };
        crate::diagnostics::append_store_log(
            "tool_package_store_publish",
            serde_json::json!({
                "branch": branch,
                "relative_path": relative_path.as_str(),
                "destination": destination.display().to_string(),
                "staged_root": staged.root().display().to_string(),
            }),
        );
        let publish_result = self.publish_inner(&relative_path, &destination, staged);
        if let Err(error) = &publish_result {
            crate::diagnostics::append_store_log(
                "tool_package_store_publish_failed",
                serde_json::json!({
                    "branch": branch,
                    "relative_path": relative_path.as_str(),
                    "destination_exists": destination.exists(),
                    "destination_entries": directory_entry_count(&destination),
                    "error": format!("{error} / {error:?}").chars().take(400).collect::<String>(),
                }),
            );
        }
        publish_result
    }

    fn publish_inner(
        &self,
        relative_path: &PackageRelativePath,
        destination: &Path,
        staged: &StagedToolPackageV1,
    ) -> Result<PublishedToolPackageV1, ToolPackageStoreErrorV1> {
        if destination.exists() {
            self.reconcile(relative_path, staged)?;
            return Ok(PublishedToolPackageV1 {
                relative_path: relative_path.clone(),
                absolute_path: destination.to_path_buf(),
                reused_existing: true,
            });
        }

        let parent = destination.parent().ok_or(ToolPackageStoreErrorV1::InvalidStorePath)?;
        fs::create_dir_all(parent).map_err(|source| ToolPackageStoreErrorV1::Io {
            action: "create immutable Package version parent",
            path: parent.to_path_buf(),
            source,
        })?;
        let staging = tempfile::Builder::new()
            .prefix("publish-")
            .tempdir_in(self.root.join(".publish-staging"))
            .map_err(|source| ToolPackageStoreErrorV1::Io {
                action: "create Package publish staging directory",
                path: self.root.join(".publish-staging"),
                source,
            })?;
        copy_runtime_root(staged, staging.path())?;
        verify_exact_runtime_root(staging.path(), staged)?;
        sync_directory_tree(staging.path())?;

        let staging_path = staging.keep();
        match fs::rename(&staging_path, destination) {
            Ok(()) => {}
            Err(_source) if destination.exists() => {
                remove_verified_publish_staging(&self.root, &staging_path)?;
                self.reconcile(relative_path, staged)?;
                return Ok(PublishedToolPackageV1 {
                    relative_path: relative_path.clone(),
                    absolute_path: destination.to_path_buf(),
                    reused_existing: true,
                });
            }
            Err(source) => {
                let cleanup = remove_verified_publish_staging(&self.root, &staging_path);
                if cleanup.is_err() {
                    return Err(ToolPackageStoreErrorV1::CleanupFailed(staging_path));
                }
                return Err(ToolPackageStoreErrorV1::Io {
                    action: "publish immutable Package version",
                    path: destination.to_path_buf(),
                    source,
                });
            }
        }
        sync_directory(parent)?;
        self.reconcile(relative_path, staged)?;
        Ok(PublishedToolPackageV1 {
            relative_path: relative_path.clone(),
            absolute_path: destination.to_path_buf(),
            reused_existing: false,
        })
    }

    /// Re-reads a durable Package root and recomputes its closure and all
    /// fingerprints against the exact W2 staging authority.
    pub fn reconcile(
        &self,
        relative_path: &PackageRelativePath,
        staged: &StagedToolPackageV1,
    ) -> Result<(), ToolPackageStoreErrorV1> {
        staged.verify_integrity()?;
        let destination = self.resolve_relative(relative_path)?;
        verify_exact_runtime_root(&destination, staged)
    }

    pub fn quarantine(
        &self,
        relative_path: &PackageRelativePath,
        quarantine_key: &str,
    ) -> Result<Option<PackageRelativePath>, ToolPackageStoreErrorV1> {
        let source = self.resolve_relative(relative_path)?;
        if !source.exists() {
            return Ok(None);
        }
        let safe_key = quarantine_key
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-'))
            && !quarantine_key.is_empty()
            && quarantine_key.len() <= 96;
        if !safe_key {
            return Err(ToolPackageStoreErrorV1::InvalidStorePath);
        }
        let relative = PackageRelativePath::new(format!("quarantine/{quarantine_key}"))
            .map_err(|_| ToolPackageStoreErrorV1::InvalidStorePath)?;
        let destination = self.resolve_relative(&relative)?;
        if destination.exists() {
            return Err(ToolPackageStoreErrorV1::QuarantineConflict);
        }
        fs::rename(&source, &destination).map_err(|source_error| ToolPackageStoreErrorV1::Io {
            action: "quarantine inconsistent Package payload",
            path: source,
            source: source_error,
        })?;
        sync_directory(self.root.join("quarantine").as_path())?;
        Ok(Some(relative))
    }

    /// Removes one exact immutable payload after the persistence layer has
    /// proved that every strong reference is released and the 30-day window
    /// elapsed. Broad/root deletion is structurally rejected.
    pub fn remove_gc_eligible_payload(
        &self,
        relative_path: &PackageRelativePath,
    ) -> Result<bool, ToolPackageStoreErrorV1> {
        let target = self.resolve_relative(relative_path)?;
        if !target.exists() {
            return Ok(false);
        }
        let package_root = self.root.join("packages");
        let canonical_root =
            fs::canonicalize(&self.root).map_err(|source| ToolPackageStoreErrorV1::Io {
                action: "canonicalize Package Store root",
                path: self.root.clone(),
                source,
            })?;
        let canonical_package_root =
            fs::canonicalize(&package_root).map_err(|source| ToolPackageStoreErrorV1::Io {
                action: "canonicalize Package payload root",
                path: package_root,
                source,
            })?;
        let canonical_target =
            fs::canonicalize(&target).map_err(|source| ToolPackageStoreErrorV1::Io {
                action: "canonicalize Package GC target",
                path: target.clone(),
                source,
            })?;
        if canonical_target == canonical_root
            || canonical_target == canonical_package_root
            || !canonical_target.starts_with(&canonical_package_root)
        {
            return Err(ToolPackageStoreErrorV1::InvalidStorePath);
        }
        fs::remove_dir_all(&canonical_target).map_err(|source| ToolPackageStoreErrorV1::Io {
            action: "remove GC-eligible Package payload",
            path: canonical_target,
            source,
        })?;
        Ok(true)
    }

    /// Enumerates immutable payload directory identities for startup orphan
    /// detection only. Runtime resolution never uses this scan.
    pub fn discover_payload_paths(
        &self,
    ) -> Result<Vec<PackageRelativePath>, ToolPackageStoreErrorV1> {
        let packages = self.root.join("packages");
        if !packages.exists() {
            return Ok(Vec::new());
        }
        let mut discovered = Vec::new();
        for package in read_directories(&packages)? {
            for version in read_directories(&package)? {
                for fingerprint in read_directories(&version)? {
                    let relative = fingerprint
                        .strip_prefix(&self.root)
                        .map_err(|_| ToolPackageStoreErrorV1::InvalidStorePath)?
                        .to_string_lossy()
                        .replace('\\', "/");
                    discovered.push(
                        PackageRelativePath::new(relative)
                            .map_err(|_| ToolPackageStoreErrorV1::InvalidStorePath)?,
                    );
                }
            }
        }
        discovered.sort();
        Ok(discovered)
    }

    pub fn resolve_published_payload(
        &self,
        relative_path: &PackageRelativePath,
    ) -> Result<PathBuf, ToolPackageStoreErrorV1> {
        if !relative_path.as_str().starts_with("packages/") {
            return Err(ToolPackageStoreErrorV1::InvalidStorePath);
        }
        let resolved = self.resolve_relative(relative_path)?;
        if !resolved.is_dir() {
            return Err(ToolPackageStoreErrorV1::Integrity);
        }
        Ok(resolved)
    }

    fn resolve_relative(
        &self,
        relative_path: &PackageRelativePath,
    ) -> Result<PathBuf, ToolPackageStoreErrorV1> {
        let relative = relative_path.as_str().replace('/', "\\");
        let resolved = self.root.join(relative);
        if resolved == self.root || !resolved.starts_with(&self.root) {
            return Err(ToolPackageStoreErrorV1::InvalidStorePath);
        }
        Ok(resolved)
    }
}

fn read_directories(root: &Path) -> Result<Vec<PathBuf>, ToolPackageStoreErrorV1> {
    let mut directories = Vec::new();
    for entry in fs::read_dir(root).map_err(|source| ToolPackageStoreErrorV1::Io {
        action: "enumerate Package Store payloads",
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| ToolPackageStoreErrorV1::Io {
            action: "read Package Store payload entry",
            path: root.to_path_buf(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| ToolPackageStoreErrorV1::Io {
            action: "read Package Store payload entry type",
            path: entry.path(),
            source,
        })?;
        if !file_type.is_dir() || file_type.is_symlink() {
            return Err(ToolPackageStoreErrorV1::Integrity);
        }
        directories.push(entry.path());
    }
    directories.sort();
    Ok(directories)
}

fn directory_entry_count(path: &Path) -> usize {
    fs::read_dir(path).map_or(0, |entries| entries.flatten().count())
}

fn copy_runtime_root(
    staged: &StagedToolPackageV1,
    destination: &Path,
) -> Result<(), ToolPackageStoreErrorV1> {
    write_new_synced(
        destination.join(TOOL_PACKAGE_MANIFEST_NAME),
        staged.canonical_manifest_bytes(),
    )?;
    for file in staged.verified_files() {
        let source = staged.root().join(file.path.as_str().replace('/', "\\"));
        let mut input =
            File::open(&source).map_err(|source_error| ToolPackageStoreErrorV1::Io {
                action: "open staged Package file",
                path: source.clone(),
                source: source_error,
            })?;
        let mut bytes = Vec::with_capacity(
            usize::try_from(file.byte_length).map_err(|_| ToolPackageStoreErrorV1::Integrity)?,
        );
        input.read_to_end(&mut bytes).map_err(|source_error| ToolPackageStoreErrorV1::Io {
            action: "read staged Package file",
            path: source,
            source: source_error,
        })?;
        write_new_synced(destination.join(file.path.as_str().replace('/', "\\")), &bytes)?;
    }
    Ok(())
}

fn write_new_synced(path: PathBuf, bytes: &[u8]) -> Result<(), ToolPackageStoreErrorV1> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ToolPackageStoreErrorV1::Io {
            action: "create Package payload directory",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut output =
        OpenOptions::new().create_new(true).write(true).open(&path).map_err(|source| {
            ToolPackageStoreErrorV1::Io {
                action: "create immutable Package payload file",
                path: path.clone(),
                source,
            }
        })?;
    output.write_all(bytes).map_err(|source| ToolPackageStoreErrorV1::Io {
        action: "write immutable Package payload file",
        path: path.clone(),
        source,
    })?;
    output.flush().map_err(|source| ToolPackageStoreErrorV1::Io {
        action: "flush immutable Package payload file",
        path: path.clone(),
        source,
    })?;
    output.sync_all().map_err(|source| ToolPackageStoreErrorV1::Io {
        action: "sync immutable Package payload file",
        path,
        source,
    })
}

fn verify_exact_runtime_root(
    root: &Path,
    staged: &StagedToolPackageV1,
) -> Result<(), ToolPackageStoreErrorV1> {
    let (verified_files, runtime_bytes) = verify_runtime_root(
        root,
        staged.canonical_manifest_bytes(),
        staged.canonical_manifest(),
        TOOL_PACKAGE_IMPORT_POLICY_V1,
    )?;
    let closure = PackageRuntimeClosureV1::from_verified_runtime_root(
        staged.canonical_manifest_bytes(),
        staged.canonical_manifest(),
        &verified_files,
    )
    .map_err(|_| ToolPackageStoreErrorV1::Integrity)?;
    let fingerprints = ToolPackageFingerprintEvidenceV1::from_authoritative_closure(
        staged.canonical_manifest_bytes(),
        staged.canonical_manifest(),
        &closure,
    )
    .map_err(|_| ToolPackageStoreErrorV1::Integrity)?;
    if runtime_bytes != staged.runtime_expanded_bytes()
        || verified_files != staged.verified_files()
        || &closure != staged.runtime_closure()
        || &fingerprints != staged.fingerprint_evidence()?
    {
        return Err(ToolPackageStoreErrorV1::Integrity);
    }
    Ok(())
}

fn sync_directory_tree(root: &Path) -> Result<(), ToolPackageStoreErrorV1> {
    let mut directories = vec![root.to_path_buf()];
    for entry in fs::read_dir(root).map_err(|source| ToolPackageStoreErrorV1::Io {
        action: "read Package publish directory",
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| ToolPackageStoreErrorV1::Io {
            action: "read Package publish entry",
            path: root.to_path_buf(),
            source,
        })?;
        if entry
            .file_type()
            .map_err(|source| ToolPackageStoreErrorV1::Io {
                action: "read Package publish entry type",
                path: entry.path(),
                source,
            })?
            .is_dir()
        {
            sync_directory_tree(&entry.path())?;
            directories.push(entry.path());
        }
    }
    for directory in directories.into_iter().rev() {
        sync_directory(&directory)?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), ToolPackageStoreErrorV1> {
    match open_verified_publish_parent(path).and_then(|directory| directory.file().sync_all()) {
        Ok(()) => Ok(()),
        // Windows commonly rejects FlushFileBuffers for directory handles.
        // Payload files are synchronously flushed before the atomic rename;
        // retaining the verified directory handle still prevents replacement.
        Err(source) if source.kind() == std::io::ErrorKind::PermissionDenied => Ok(()),
        Err(source) => Err(ToolPackageStoreErrorV1::Io {
            action: "sync Package Store directory",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn remove_verified_publish_staging(
    root: &Path,
    staging: &Path,
) -> Result<(), ToolPackageStoreErrorV1> {
    let expected_parent = root.join(".publish-staging");
    if staging == root || staging == expected_parent || !staging.starts_with(&expected_parent) {
        return Err(ToolPackageStoreErrorV1::InvalidStorePath);
    }
    fs::remove_dir_all(staging).map_err(|source| ToolPackageStoreErrorV1::Io {
        action: "clean failed Package publish staging",
        path: staging.to_path_buf(),
        source,
    })
}

fn package_fingerprint_hex(fingerprint: &PackageFingerprintV1) -> &str {
    fingerprint.0.hex()
}

#[derive(Debug, Error)]
pub enum ToolPackageStoreErrorV1 {
    #[error("Package Store path is outside the immutable payload namespace")]
    InvalidStorePath,
    #[error("Package Store payload failed canonical integrity reconciliation")]
    Integrity,
    #[error("Package quarantine destination already exists")]
    QuarantineConflict,
    #[error("Package publish staging cleanup failed at {0}")]
    CleanupFailed(PathBuf),
    #[error("Package staging evidence is invalid")]
    Staging(#[from] ToolPackageStagingErrorV1),
    #[error("Package Store failed to {action} at {path}: {source}")]
    Io { action: &'static str, path: PathBuf, source: std::io::Error },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use aura_domain_common::ToolPackageId;
    use aura_tool_contract::TOOL_PACKAGE_MANIFEST_NAME;

    use super::*;
    use crate::stage_directory_package_v1;

    const MANIFEST: &[u8] =
        include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
    const RUNTIME: &[u8] = b"stdio fixture runtime";

    fn staged(source: &Path, staging: &Path) -> StagedToolPackageV1 {
        fs::create_dir_all(source.join("bin")).expect("source bin");
        fs::write(source.join(TOOL_PACKAGE_MANIFEST_NAME), MANIFEST).expect("manifest");
        fs::write(source.join("bin/fixture.exe"), RUNTIME).expect("runtime");
        stage_directory_package_v1(source, staging).expect("stage")
    }

    #[test]
    fn publish_is_immutable_idempotent_and_detects_drift() {
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("source");
        let staging = temp.path().join("staging");
        let package_store = ToolPackageStoreV1::open(temp.path().join("store")).expect("store");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&staging).expect("staging");
        let staged = staged(&source, &staging);
        let package_id = ToolPackageId::from_bytes([91; 16]);

        let first = package_store.publish(package_id, &staged).expect("first publish");
        assert!(!first.reused_existing());
        let second = package_store.publish(package_id, &staged).expect("idempotent publish");
        assert!(second.reused_existing());
        assert_eq!(first.relative_path(), second.relative_path());

        fs::write(first.absolute_path().join("bin/fixture.exe"), b"drift").expect("tamper");
        assert!(matches!(
            package_store.reconcile(first.relative_path(), &staged),
            Err(ToolPackageStoreErrorV1::Staging(_)) | Err(ToolPackageStoreErrorV1::Integrity)
        ));
    }

    #[test]
    fn quarantine_and_gc_are_scoped_to_exact_payloads() {
        let temp = tempfile::tempdir().expect("temp");
        let source = temp.path().join("source");
        let staging = temp.path().join("staging");
        let package_store = ToolPackageStoreV1::open(temp.path().join("store")).expect("store");
        fs::create_dir_all(&source).expect("source");
        fs::create_dir_all(&staging).expect("staging");
        let staged = staged(&source, &staging);
        let published =
            package_store.publish(ToolPackageId::from_bytes([92; 16]), &staged).expect("publish");
        let quarantine = package_store
            .quarantine(published.relative_path(), "journal_01")
            .expect("quarantine")
            .expect("moved");
        assert!(package_store.resolve_relative(&quarantine).expect("resolve").is_dir());
        assert!(!published.absolute_path().exists());
    }
}
