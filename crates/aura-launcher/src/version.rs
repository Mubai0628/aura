//! One version folder checked before it starts: its manifest, and every component's bytes against
//! the hashes the manifest names.
use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use aura_domain_common::{ContentHash, ContentHashAlgorithm};
use aura_runtime_contract::{
    BuildComponentKind, BuildComponentManifest, ReleaseDescriptor, ReleaseIdentity,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct VersionSelection {
    pub version: String,
    pub directory: PathBuf,
    pub desktop_executable: PathBuf,
    pub manifest: BuildComponentManifest,
    pub manifest_json: Value,
    pub descriptor: ReleaseDescriptor,
}

/// Checks `versions/<version>` under `root`. With `expected`, the manifest must be exactly the one
/// it identifies; without, its own bytes identify it.
pub fn resolve_version(
    root: &Path,
    version: &str,
    expected: Option<&ReleaseDescriptor>,
) -> Result<VersionSelection> {
    validate_version_segment(version)?;
    let versions_root = root.join("versions");
    let version_directory = versions_root.join(version);
    let canonical_versions = dunce::canonicalize(&versions_root)
        .with_context(|| format!("missing versions directory: {}", versions_root.display()))?;
    let canonical_version = dunce::canonicalize(&version_directory)
        .with_context(|| format!("missing version: {}", version_directory.display()))?;
    if !canonical_version.starts_with(&canonical_versions) {
        bail!("version escaped the versions directory");
    }

    let manifest_path = canonical_version.join("component-manifest.json");
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: BuildComponentManifest = serde_json::from_slice(&manifest_bytes)
        .context("component-manifest.json is not a valid Aura contract")?;
    let manifest_json = serde_json::from_slice(&manifest_bytes)
        .context("component-manifest.json cannot be represented as JSON")?;
    let descriptor = ReleaseDescriptor {
        bundle_namespace: manifest.bundle_namespace.clone(),
        identity: ReleaseIdentity {
            release_bundle_id: manifest.release_bundle_id,
            component_manifest_hash: ContentHash::new(
                ContentHashAlgorithm::Sha256,
                format!("{:x}", Sha256::digest(&manifest_bytes)),
            )?,
        },
    };
    if expected.is_some_and(|expected| *expected != descriptor) {
        bail!("active release descriptor does not match component-manifest.json");
    }
    if manifest.bundle_namespace.trust_domain().is_none() {
        bail!("unsupported Aura release bundle namespace");
    }
    if manifest.product_version != version {
        bail!(
            "version '{version}' does not match manifest product version '{}'",
            manifest.product_version
        );
    }
    if !manifest.target_triple.eq_ignore_ascii_case("x86_64-pc-windows-msvc") {
        bail!("unsupported release target: {}", manifest.target_triple);
    }

    let mut desktop_executable = None;
    let mut has_core = false;
    for component in &manifest.components {
        let component_path = safe_component_path(&canonical_version, &component.relative_path)?;
        let bytes = fs::read(&component_path)
            .with_context(|| format!("missing release component: {}", component_path.display()))?;
        let actual = match component.content_hash.algorithm {
            ContentHashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(&bytes)),
            ContentHashAlgorithm::Blake3 => blake3::hash(&bytes).to_hex().to_string(),
        };
        if actual != component.content_hash.hex {
            bail!("component hash mismatch: {}", component.component_id);
        }
        match component.kind {
            BuildComponentKind::Desktop => desktop_executable = Some(component_path),
            BuildComponentKind::Core => has_core = true,
            _ => {}
        }
    }
    if !has_core {
        bail!("release bundle does not contain aura-core");
    }
    let desktop_executable =
        desktop_executable.context("release bundle does not contain Aura.exe")?;
    if desktop_executable.file_name() != Some(OsStr::new("Aura.exe")) {
        bail!("desktop component must be named Aura.exe");
    }
    Ok(VersionSelection {
        version: version.to_owned(),
        directory: canonical_version,
        desktop_executable,
        manifest,
        manifest_json,
        descriptor,
    })
}

pub fn validate_version_segment(version: &str) -> Result<()> {
    if version.is_empty()
        || version == "."
        || version == ".."
        || version.starts_with('.')
        || version.contains(['/', '\\'])
        || version.contains(':')
    {
        bail!("version is not a safe directory segment");
    }
    Ok(())
}

fn safe_component_path(version_root: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("component path is not a safe relative path: {relative}");
    }
    let candidate = version_root.join(relative_path);
    let canonical = dunce::canonicalize(&candidate)
        .with_context(|| format!("failed to resolve component path: {}", candidate.display()))?;
    if !canonical.starts_with(version_root) {
        bail!("component path escaped the version: {relative}");
    }
    Ok(canonical)
}
