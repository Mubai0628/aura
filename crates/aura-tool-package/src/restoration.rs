//! Materialize verified V2 content. This is separate from authored Raw V1 import.
use crate::{
    StagedToolPackageV1, ToolPackageStagingErrorV1 as Error,
    manifest_parser::ParsedToolPackageManifestV1,
};
use aura_tool_contract::{
    PackageRelativePath, TOOL_PACKAGE_IMPORT_POLICY_V1, TOOL_PACKAGE_MANIFEST_NAME,
    ToolPackageFileSha256V1,
};
use aura_tool_domain::CanonicalToolPackageManifestV1;
use std::{fs, path::Path};

/// Restores only a closed canonical payload supplied by Core's V2 content store.
/// Does not grant installation/trust. Core must compare the recomputed descriptor
/// with its expected persisted binding before using this runtime root.
pub fn restore_stored_package_v1(
    manifest: &CanonicalToolPackageManifestV1,
    files: &[(PackageRelativePath, Vec<u8>)],
    parent: &Path,
) -> Result<StagedToolPackageV1, Error> {
    let guard = aura_windows_platform::open_verified_tool_output_root(parent).map_err(Error::Io)?;
    if !guard.info().is_directory {
        return Err(Error::InvalidStagingParent);
    }
    let parsed = ParsedToolPackageManifestV1::from_stored_canonical(manifest)
        .map_err(Error::ManifestInvalid)?;
    if manifest.files.len() != files.len() {
        return Err(Error::ClosedFileSetMismatch);
    }
    let mut expanded = parsed.canonical_manifest_bytes().len() as u64;
    for (declared, (path, bytes)) in manifest.files.iter().zip(files) {
        if declared.path != *path {
            return Err(Error::ClosedFileSetMismatch);
        }
        if declared.byte_length != bytes.len() as u64 {
            return Err(Error::FileLengthMismatch);
        }
        if declared.sha256 != ToolPackageFileSha256V1::sha256(bytes) {
            return Err(Error::FileHashMismatch);
        }
        expanded = expanded.checked_add(bytes.len() as u64).ok_or(Error::ExpandedSizeExceeded)?;
    }
    if expanded > TOOL_PACKAGE_IMPORT_POLICY_V1.max_expanded_bytes {
        return Err(Error::ExpandedSizeExceeded);
    }
    let directory = tempfile::Builder::new()
        .prefix("aura-tool-package-")
        .tempdir_in(parent)
        .map_err(Error::Io)?;
    fs::write(directory.path().join(TOOL_PACKAGE_MANIFEST_NAME), parsed.canonical_manifest_bytes())
        .map_err(Error::Io)?;
    for (path, bytes) in files {
        let target = directory.path().join(path.as_str());
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(Error::Io)?;
        }
        fs::write(target, bytes).map_err(Error::Io)?;
    }
    StagedToolPackageV1::from_runtime_root(
        directory,
        parsed,
        (files.len() + 1) as u32,
        expanded,
        TOOL_PACKAGE_IMPORT_POLICY_V1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_restore_preserves_fingerprint_and_rejects_a_changed_or_open_file_set() {
        let root = tempfile::tempdir().expect("owned content");
        let raw =
            include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
        let parsed = crate::parse_tool_package_manifest_v1(raw.as_slice()).expect("Raw import");
        let manifest = parsed.canonical_manifest();
        let file = &manifest.files[0];
        let files = vec![(file.path.clone(), b"stdio fixture runtime".to_vec())];
        let restored =
            restore_stored_package_v1(manifest, &files, root.path()).expect("V2 restore");
        assert_eq!(restored.canonical_manifest(), manifest);
        restored.verify_integrity().expect("fresh closed-root proof");
        assert!(
            crate::parse_tool_package_manifest_v1(restored.canonical_manifest_bytes()).is_err(),
            "restoration must not broaden authored Raw V1 import"
        );
        let mut changed = files.clone();
        changed[0].1[0] ^= 1;
        assert!(matches!(
            restore_stored_package_v1(manifest, &changed, root.path()),
            Err(Error::FileHashMismatch)
        ));
        let mut extra = files.clone();
        extra.push((PackageRelativePath::new("extra.txt").expect("path"), vec![]));
        assert!(matches!(
            restore_stored_package_v1(manifest, &extra, root.path()),
            Err(Error::ClosedFileSetMismatch)
        ));
    }
}
