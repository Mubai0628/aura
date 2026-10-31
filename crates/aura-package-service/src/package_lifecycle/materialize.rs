use anyhow::{Result, ensure};
use aura_persistence_v2::packages::LoadedPackageContent;
use aura_tool_package::{StagedToolPackageV1, restore_stored_package_v1};
use std::path::Path;

/// Recreate only the closed, hash-checked V2 payload. The strict stager verifies
/// it again; neither legacy package rows nor arbitrary runtime directories count.
pub fn staged(content: &LoadedPackageContent, root: &Path) -> Result<StagedToolPackageV1> {
    let staged = restore_stored_package_v1(&content.descriptor.manifest, &content.files, root)?;
    ensure!(
        aura_tool_package::package_content_descriptor_v1(&staged)? == content.descriptor,
        "package.materialized_content_changed"
    );
    Ok(staged)
}
