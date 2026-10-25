use super::PackageRunOptions;
use anyhow::Result;
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_tool_domain::{
    PackageConformanceStateV1, PackageImportPreviewV1, PackageVersionBindingV1,
};
use aura_tool_package::{PackagePreviewDirectories, StagedToolPackageV1};
use std::sync::Arc;

/// Core retains this single-use object while presenting the concrete preview.
/// It contains no process and cannot be constructed from a wire request. A
/// trusted route still has to verify the user's explicit approval before start.
pub struct PreparedPackageConformance {
    // Drop the payload directory before its enclosing scratch tree.
    pub(super) staged: StagedToolPackageV1,
    pub(super) scratch: tempfile::TempDir,
    pub(super) version: PackageVersionBindingV1,
    pub(super) options: PackageRunOptions,
    pub(super) lease: Arc<super::recovery::OwnerLease>,
    pub(super) previous_state: PackageConformanceStateV1,
}
impl PreparedPackageConformance {
    pub fn version(&self) -> &PackageVersionBindingV1 {
        &self.version
    }
    pub fn settings(&self) -> &[aura_tool_domain::PackageToolSettingsV1] {
        &self.options.settings
    }
    pub fn preview(&self) -> Result<PackageImportPreviewV1> {
        Ok(aura_tool_package::package_import_preview_v1(
            &self.staged,
            self.version.clone(),
            &PackagePreviewDirectories {
                package_root: self.staged.root(),
                workspace_root: self.options.workspace_root.as_deref(),
                directory_claims: &self.options.directory_claims,
            },
            self.previous_state,
        )?)
    }
    /// Bind UI confirmation to the actual runtime directory and configuration,
    /// not merely to the source-directory path or the visible fingerprint prefix.
    pub fn preview_hash(&self) -> Result<CanonicalSha256HashV1> {
        Ok(canonical_sha256(&(self.preview()?, &self.options.settings))?)
    }
    pub fn cancel(self) -> Result<()> {
        drop(self.staged);
        self.scratch.close()?;
        Ok(())
    }
}
